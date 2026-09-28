//! Reuse fixed resize plans and specialize their contiguous rows.
//! Keep tract's coordinates, coefficients, axis order and accumulation order.
use std::sync::Arc;
use tract_linalg::multithread::par_chunks_mut;
use tract_onnx::tract_core::{
    internal::*,
    ops::nn::resize::{self, AxisPlan, Interpolator, Resize},
};

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    optimize_impl(model, false)
}

/// Also plan restoration's cubic/nearest resizes, retaining tract's taps and
/// axis order. Independent output rows share the model's bounded executor.
pub(super) fn optimize_restoration(model: &mut TypedModel) -> TractResult<()> {
    optimize_impl(model, true)
}

fn optimize_impl(model: &mut TypedModel, parallel: bool) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(op) = node.op_as::<Resize>() else {
            continue;
        };
        let input = model.outlet_fact(node.inputs[0])?;
        let (Some(shape), Some(output)) = (
            input.shape.as_concrete(),
            node.outputs[0].fact.shape.as_concrete(),
        ) else {
            continue;
        };
        if input.datum_type != f32::datum_type()
            || (!parallel && op.interpolator != Interpolator::Linear)
            || shape.contains(&0)
            || output.contains(&0)
            || shape.len() != output.len()
            || node.inputs[1..]
                .iter()
                .any(|&i| model.outlet_fact(i).map_or(true, |f| f.konst.is_none()))
        {
            continue;
        }
        let scale_tensor = op
            .optional_scales_input
            .and_then(|ix| model.outlet_fact(node.inputs[ix]).ok())
            .and_then(|f| f.konst.as_deref())
            .filter(|s| s.len() == shape.len());
        let scales: Vec<f32> = if let Some(s) = scale_tensor {
            s.to_plain_array_view::<f32>()?.iter().copied().collect()
        } else {
            output
                .iter()
                .zip(shape)
                .map(|(&o, &i)| o as f32 / i as f32)
                .collect()
        };
        let mut current = shape.to_vec();
        let mut steps = Vec::new();
        for (axis, scale) in scales.into_iter().enumerate() {
            if current[axis] == output[axis] && scale == 1.0 {
                continue;
            }
            let plan = op.plan_axis(scale, current[axis], output[axis]);
            let mut next = current.clone();
            next[axis] = output[axis];
            steps.push(Step {
                axis,
                input: current,
                output: next.clone(),
                plan,
            });
            current = next;
        }
        model.node_mut(id).op = Box::new(PlannedResize {
            input: shape.to_vec(),
            steps: Arc::new(steps),
            output: node.outputs[0].fact.clone(),
            parallel,
        });
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Step {
    axis: usize,
    input: Vec<usize>,
    output: Vec<usize>,
    plan: AxisPlan,
}

#[derive(Clone, Debug)]
struct PlannedResize {
    input: Vec<usize>,
    steps: Arc<Vec<Step>>,
    output: TypedFact,
    parallel: bool,
}
impl PartialEq for PlannedResize {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.steps, &other.steps) && self.parallel == other.parallel
    }
}
impl Eq for PlannedResize {}
impl Op for PlannedResize {
    fn name(&self) -> StaticName {
        "PlannedResize".into()
    }
    op_as_typed_op!();
}
impl EvalOp for PlannedResize {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(
            !inputs.is_empty() && inputs[0].shape() == self.input,
            "resize input shape changed"
        );
        let mut data = inputs[0].clone();
        for step in self.steps.iter() {
            let input = data.to_plain_array_view::<f32>()?;
            let input = input.as_slice().context("non-contiguous resize input")?;
            let mut result = Tensor::zero::<f32>(&step.output)?;
            {
                let mut output = result.to_plain_array_view_mut::<f32>()?;
                let output = output.as_slice_mut().unwrap();
                if self.parallel {
                    resample_rows(input, output, step)?;
                } else if step.input[step.axis + 1..].iter().product::<usize>() == 1
                    && step.plan.window == 2
                    && !step.plan.extrapolated.contains(&true)
                {
                    contiguous_rows(input, output, step.input[step.axis], &step.plan);
                } else {
                    resize::resample_axis(input, &step.input, step.axis, &step.plan, 0., output);
                }
            }
            data = result.into();
        }
        Ok(tvec!(data))
    }
}
impl TypedOp for PlannedResize {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(self.output.clone()))
    }
    as_op!();
}

fn contiguous_rows(input: &[f32], output: &mut [f32], len_in: usize, plan: &AxisPlan) {
    let len_out = plan.extrapolated.len();
    for (src, dst) in input
        .chunks_exact(len_in)
        .zip(output.chunks_exact_mut(len_out))
    {
        for ((out, &[i0, i1]), &[w0, w1]) in dst
            .iter_mut()
            .zip(plan.indices.as_chunks::<2>().0)
            .zip(plan.weights.as_chunks::<2>().0)
        {
            let mut value = 0.;
            if w0 != 0. {
                value += w0 * src[i0];
            }
            if w1 != 0. {
                value += w1 * src[i1];
            }
            *out = value;
        }
    }
}

fn resample_rows(input: &[f32], output: &mut [f32], step: &Step) -> TractResult<()> {
    let inner: usize = step.input[step.axis + 1..].iter().product();
    let len_in = step.input[step.axis];
    let len_out = step.output[step.axis];
    let plan = &step.plan;
    // Width resampling gathers two/four taps per pixel. Const-sized tap loops
    // avoid the generic resampler's per-pixel slice setup and zero fill.
    if inner == 1 && !plan.extrapolated.contains(&true) {
        return par_chunks_mut(output, len_out, output.len(), |first, chunk| {
            let source = &input[first * len_in..][..chunk.len() / len_out * len_in];
            match plan.window {
                2 => sample_width::<2>(source, chunk, len_in, plan),
                4 => sample_width::<4>(source, chunk, len_in, plan),
                _ => resize::resample_axis(
                    source,
                    &[source.len() / len_in, len_in],
                    1,
                    plan,
                    0.,
                    chunk,
                ),
            }
            Ok(())
        });
    }
    // Height/outer-axis resampling combines contiguous inner rows. Keep the
    // same tap order, including skipped zero weights and +0 initialization.
    par_chunks_mut(output, inner, output.len(), |first, chunk| {
        for (row, dst) in chunk.chunks_exact_mut(inner).enumerate() {
            let outer = (first + row) / len_out;
            let x = (first + row) % len_out;
            dst.fill(0.);
            if plan.extrapolated[x] {
                continue;
            }
            for tap in 0..plan.window {
                let weight = plan.weights[x * plan.window + tap];
                if weight == 0. {
                    continue;
                }
                let index = plan.indices[x * plan.window + tap];
                let offset = (outer * len_in + index) * inner;
                for (out, &value) in dst.iter_mut().zip(&input[offset..offset + inner]) {
                    *out += weight * value;
                }
            }
        }
        Ok(())
    })
}

fn sample_width<const N: usize>(input: &[f32], output: &mut [f32], len_in: usize, plan: &AxisPlan) {
    for (src, dst) in input
        .chunks_exact(len_in)
        .zip(output.chunks_exact_mut(plan.extrapolated.len()))
    {
        for ((out, indices), weights) in dst
            .iter_mut()
            .zip(plan.indices.as_chunks::<N>().0)
            .zip(plan.weights.as_chunks::<N>().0)
        {
            let mut value = 0.;
            for tap in 0..N {
                if weights[tap] != 0. {
                    value += weights[tap] * src[indices[tap]];
                }
            }
            *out = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resize::{CoordTransformer, Nearest};

    fn check_resize(
        shape: &[usize],
        target: &[usize],
        coord: CoordTransformer,
        interpolator: Interpolator,
        nearest: Nearest,
        use_scales: bool,
        special: bool,
    ) -> TractResult<()> {
        let input = Tensor::from_shape(
            shape,
            &(0..shape.iter().product())
                .map(|i| {
                    if special {
                        [
                            0.,
                            -0.,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            f32::from_bits(0x7fc01234),
                            0.7,
                        ][i % 6]
                    } else {
                        (i as f32 - 35.) / 13.
                    }
                })
                .collect::<Vec<_>>(),
        )?;
        let op = Resize {
            coord_transformer: coord,
            interpolator: interpolator.clone(),
            nearest,
            optional_scales_input: use_scales.then_some(1),
            optional_sizes_input: (!use_scales).then_some(1),
        };
        let aux = if use_scales {
            Tensor::from_shape(
                &[shape.len()],
                &target
                    .iter()
                    .zip(shape)
                    .map(|(&o, &i)| o as f32 / i as f32)
                    .collect::<Vec<_>>(),
            )?
        } else {
            Tensor::from_shape(
                &[shape.len()],
                &target.iter().map(|&v| v as i64).collect::<Vec<_>>(),
            )?
        };
        let expected = op.eval(tvec!(input.clone().into(), aux.clone().into()))?;
        for parallel in [false, true] {
            if !parallel && interpolator != Interpolator::Linear {
                continue;
            }
            let mut model = TypedModel::default();
            let data = model.add_source("input", f32::fact(shape))?;
            let aux = model.add_const("size", aux.clone())?;
            let out = model.wire_node("resize", op.clone(), &[data, aux])?;
            model.select_output_outlets(&out)?;
            optimize_impl(&mut model, parallel)?;
            assert!(model.node(out[0].node).op_is::<PlannedResize>());
            let actual = model.into_runnable()?.run(tvec!(input.clone().into()))?;
            assert_eq!(actual[0].shape(), expected[0].shape());
            for (a, b) in actual[0]
                .to_plain_array_view::<f32>()?
                .iter()
                .zip(expected[0].to_plain_array_view::<f32>()?.iter())
            {
                assert!(
                    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
                    "resize {a:?} != {b:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn planned_resize_matches_upstream_for_coordinates_sizes_and_scales() -> TractResult<()> {
        for coord in [
            CoordTransformer::HalfPixel,
            CoordTransformer::AlignCorners,
            CoordTransformer::Asymmetric,
            CoordTransformer::PytorchHalfPixel,
            CoordTransformer::HalfPixelSymmetric,
            CoordTransformer::TfHalfPixelForNn,
        ] {
            for target in [
                [1, 2, 11, 17],
                [1, 2, 3, 4],
                [1, 2, 1, 1],
                [1, 2, 5, 7],
                [2, 3, 6, 8],
            ] {
                for use_scales in [false, true] {
                    for interpolator in [
                        Interpolator::Linear,
                        Interpolator::Nearest,
                        Interpolator::Cubic,
                    ] {
                        for nearest in [Nearest::Floor, Nearest::RoundPreferCeil] {
                            check_resize(
                                &[1, 2, 5, 7],
                                &target,
                                coord.clone(),
                                interpolator.clone(),
                                nearest,
                                use_scales,
                                false,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn planned_resize_preserves_zero_weights_and_nonfinite_values() -> TractResult<()> {
        for interpolator in [
            Interpolator::Linear,
            Interpolator::Nearest,
            Interpolator::Cubic,
        ] {
            check_resize(
                &[2, 1, 5, 7],
                &[3, 2, 8, 11],
                CoordTransformer::HalfPixel,
                interpolator,
                Nearest::Floor,
                false,
                true,
            )?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn planned_parallel_resize_matches_upstream_across_planes() -> TractResult<()> {
        use tract_linalg::multithread::{multithread_tract_scope, Executor};
        multithread_tract_scope(Executor::multithread(3), || {
            for interpolator in [
                Interpolator::Linear,
                Interpolator::Nearest,
                Interpolator::Cubic,
            ] {
                check_resize(
                    &[2, 3, 127, 129],
                    &[3, 4, 151, 171],
                    CoordTransformer::HalfPixel,
                    interpolator,
                    Nearest::Floor,
                    false,
                    false,
                )?;
            }
            Ok(())
        })
    }
}
