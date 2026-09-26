//! Reuse fixed resize plans and specialize their contiguous, two-tap rows.
//! Keep tract's coordinates, coefficients, axis order and accumulation order.
use std::sync::Arc;
use tract_onnx::tract_core::{
    internal::*,
    ops::nn::resize::{self, AxisPlan, Interpolator, Resize},
};

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
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
            || op.interpolator != Interpolator::Linear
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
}
impl PartialEq for PlannedResize {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.steps, &other.steps)
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
            inputs[0].shape() == self.input,
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
                if step.input[step.axis + 1..].iter().product::<usize>() == 1
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

#[cfg(test)]
mod tests {
    use super::*;
    use resize::{CoordTransformer, Nearest};

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
            for target in [[1, 2, 11, 17], [1, 2, 3, 4], [1, 2, 1, 1], [1, 2, 5, 7]] {
                for use_scales in [false, true] {
                    let shape = [1, 2, 5, 7];
                    let input = Tensor::from_shape(
                        &shape,
                        &(0..70).map(|i| (i as f32 - 35.) / 13.).collect::<Vec<_>>(),
                    )?;
                    let op = Resize {
                        coord_transformer: coord.clone(),
                        interpolator: Interpolator::Linear,
                        nearest: Nearest::Floor,
                        optional_scales_input: use_scales.then_some(1),
                        optional_sizes_input: (!use_scales).then_some(1),
                    };
                    let aux = if use_scales {
                        Tensor::from_shape(
                            &[4],
                            &target
                                .iter()
                                .zip(shape)
                                .map(|(&o, i)| o as f32 / i as f32)
                                .collect::<Vec<_>>(),
                        )?
                    } else {
                        Tensor::from_shape(&[4], &target.map(|v| v as i64))?
                    };
                    let expected = op.eval(tvec!(input.clone().into(), aux.clone().into()))?;
                    let mut model = TypedModel::default();
                    let data = model.add_source("input", f32::fact(shape))?;
                    let aux = model.add_const("size", aux)?;
                    let out = model.wire_node("resize", op, &[data, aux])?;
                    model.select_output_outlets(&out)?;
                    optimize(&mut model)?;
                    assert!(model.node(out[0].node).op_is::<PlannedResize>());
                    let actual = model.into_runnable()?.run(tvec!(input.into()))?;
                    assert_eq!(
                        actual[0].to_plain_array_view::<f32>()?,
                        expected[0].to_plain_array_view::<f32>()?
                    );
                }
            }
        }
        Ok(())
    }
}
