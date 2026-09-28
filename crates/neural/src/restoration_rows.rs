//! Contiguous channel reductions and reflected rows for restoration.
//! The model, FP32 arithmetic and border coordinates stay unchanged.
use tract_linalg::multithread::par_chunks_mut;
use tract_onnx::tract_core::{
    internal::*,
    ops::{
        array::{Pad, PadMode},
        nn::{Reduce, Reducer},
    },
};

fn volume(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |n, &d| if d == 0 { None } else { n.checked_mul(d) })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReflectRows {
    input: Vec<usize>,
    output: Vec<usize>,
    top: usize,
    left: usize,
}

impl ReflectRows {
    fn new(op: &Pad, input: &TypedFact) -> Option<Self> {
        let shape = input.shape.as_concrete()?;
        if input.datum_type != DatumType::F32
            || op.mode != PadMode::Reflect
            || shape.len() < 2
            || shape.len() != op.pads.len()
            || op.pads[..shape.len() - 2].iter().any(|&p| p != (0, 0))
            || op
                .pads
                .iter()
                .zip(shape)
                .any(|(&(a, b), &d)| a >= d || b >= d)
        {
            return None;
        }
        volume(shape)?;
        let output: Vec<_> = shape
            .iter()
            .zip(&op.pads)
            .map(|(&d, &(a, b))| d.checked_add(a)?.checked_add(b))
            .collect::<Option<_>>()?;
        volume(&output)?;
        Some(Self {
            input: shape.to_vec(),
            output,
            top: op.pads[shape.len() - 2].0,
            left: op.pads[shape.len() - 1].0,
        })
    }
}

impl Op for ReflectRows {
    fn name(&self) -> StaticName {
        "ReflectRows".into()
    }
    op_as_typed_op!();
}
impl TypedOp for ReflectRows {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type == DatumType::F32
                && inputs[0].shape.as_concrete() == Some(self.input.as_slice()),
            "reflection input fact changed"
        );
        Ok(tvec!(f32::fact(&self.output)))
    }
    as_op!();
}
impl EvalOp for ReflectRows {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let input = args_1!(inputs);
        ensure!(
            input.shape() == self.input,
            "reflection input shape changed"
        );
        let view = input.to_plain_array_view::<f32>()?;
        let source = view.as_slice().context("non-contiguous reflection input")?;
        let rank = self.input.len();
        let (height, width) = (self.input[rank - 2], self.input[rank - 1]);
        let (out_height, out_width) = (self.output[rank - 2], self.output[rank - 1]);
        let mut output = Tensor::zero::<f32>(&self.output)?;
        {
            let mut view = output.to_plain_array_view_mut::<f32>()?;
            let values = view.as_slice_mut().unwrap();
            par_chunks_mut(values, out_width, values.len(), |first, chunk| {
                for (row, dst) in chunk.chunks_exact_mut(out_width).enumerate() {
                    let plane = (first + row) / out_height;
                    let y = reflect((first + row) % out_height, self.top, height);
                    let offset = (plane * height + y) * width;
                    let src = &source[offset..offset + width];
                    dst[self.left..self.left + width].copy_from_slice(src);
                    for (x, out) in dst[..self.left].iter_mut().enumerate() {
                        *out = src[self.left - x];
                    }
                    for (x, out) in dst[self.left + width..].iter_mut().enumerate() {
                        *out = src[width - 2 - x];
                    }
                }
                Ok(())
            })?;
        }
        Ok(tvec!(output.into()))
    }
}

fn reflect(x: usize, before: usize, len: usize) -> usize {
    if x < before {
        before - x
    } else if x - before < len {
        x - before
    } else {
        len - 2 - (x - before - len)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChannelMaximum {
    input: Vec<usize>,
}

impl ChannelMaximum {
    fn new(op: &Reduce, fact: &TypedFact) -> Option<Self> {
        let shape = fact.shape.as_concrete()?;
        if fact.datum_type != DatumType::F32
            || op.reducer != Reducer::Max
            || shape.len() != 4
            || op.axes.as_slice() != [1]
            || volume(shape)? < 32768
            || shape[1] < 2
            || shape[2] * shape[3] < 2
        {
            return None;
        }
        Some(Self {
            input: shape.to_vec(),
        })
    }
    fn output_shape(&self) -> Vec<usize> {
        let mut shape = self.input.clone();
        shape[1] = 1;
        shape
    }
}
impl Op for ChannelMaximum {
    fn name(&self) -> StaticName {
        "ChannelMaximum".into()
    }
    op_as_typed_op!();
}
impl TypedOp for ChannelMaximum {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type == DatumType::F32
                && inputs[0].shape.as_concrete() == Some(self.input.as_slice()),
            "maximum input fact changed"
        );
        Ok(tvec!(f32::fact(self.output_shape())))
    }
    as_op!();
}
impl EvalOp for ChannelMaximum {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let input = args_1!(inputs);
        ensure!(input.shape() == self.input, "maximum input shape changed");
        let view = input.to_plain_array_view::<f32>()?;
        let source = view.as_slice().context("non-contiguous maximum input")?;
        let (channels, height, width) = (self.input[1], self.input[2], self.input[3]);
        let spatial = height * width;
        let mut output = Tensor::zero::<f32>(&self.output_shape())?;
        {
            let mut view = output.to_plain_array_view_mut::<f32>()?;
            let values = view.as_slice_mut().unwrap();
            par_chunks_mut(values, width, source.len(), |first, chunk| {
                for (row, dst) in chunk.chunks_exact_mut(width).enumerate() {
                    let batch = (first + row) / height;
                    let y = (first + row) % height;
                    dst.fill(f32::MIN);
                    for channel in 0..channels {
                        let offset = (batch * channels + channel) * spatial + y * width;
                        for (out, &value) in dst.iter_mut().zip(&source[offset..offset + width]) {
                            // Match tract's strided reduction comparison and
                            // channel order, including NaNs, equal signed zeros
                            // and the finite MIN initial accumulator.
                            *out = if *out > value { *out } else { value };
                        }
                    }
                }
                Ok(())
            })?;
        }
        Ok(tvec!(output.into()))
    }
}

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        if node.inputs.len() != 1 || node.outputs.len() != 1 {
            continue;
        }
        let fact = model.outlet_fact(node.inputs[0])?;
        let replacement: Option<Box<dyn TypedOp>> = if let Some(op) = node.op_as::<Pad>() {
            ReflectRows::new(op, fact).map(|op| Box::new(op) as _)
        } else if let Some(op) = node.op_as::<Reduce>() {
            ChannelMaximum::new(op, fact).map(|op| Box::new(op) as _)
        } else {
            None
        };
        if let Some(op) = replacement {
            model.node_mut(id).op = op;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bits(value: &TValue) -> TractResult<Vec<u32>> {
        Ok(value
            .to_plain_array_view::<f32>()?
            .iter()
            .map(|v| v.to_bits())
            .collect())
    }

    #[test]
    fn reflected_rows_match_upstream_bitwise_at_borders_and_degenerate_axes() -> TractResult<()> {
        for shape in [
            vec![1, 1],
            vec![2, 3],
            vec![3, 4, 5],
            vec![2, 3, 5, 7],
            vec![2, 1, 3, 2, 4],
        ] {
            let rank = shape.len();
            let values = [
                0.,
                -0.,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::from_bits(0x7fc01234),
                0.37,
            ];
            let input = Tensor::from_shape(
                &shape,
                &(0..volume(&shape).unwrap())
                    .map(|i| values[i % values.len()])
                    .collect::<Vec<_>>(),
            )?;
            for top in 0..shape[rank - 2] {
                for left in 0..shape[rank - 1] {
                    let mut pads = vec![(0, 0); rank];
                    pads[rank - 2] = (top, shape[rank - 2] - 1 - top);
                    pads[rank - 1] = (left, shape[rank - 1] - 1 - left);
                    let op = Pad {
                        pads,
                        mode: PadMode::Reflect,
                    };
                    let fast = ReflectRows::new(&op, &f32::fact(&shape)).unwrap();
                    let expected = op.eval(tvec!(input.clone().into()))?;
                    let actual = fast.eval(tvec!(input.clone().into()))?;
                    assert_eq!(actual[0].shape(), expected[0].shape());
                    assert_eq!(bits(&actual[0])?, bits(&expected[0])?);
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reflected_parallel_rows_cross_planes_without_changing_bits() -> TractResult<()> {
        use tract_linalg::multithread::{multithread_tract_scope, Executor};
        let shape = [2, 3, 129, 97];
        let op = Pad {
            pads: vec![(0, 0), (0, 0), (2, 1), (1, 2)],
            mode: PadMode::Reflect,
        };
        let fast = ReflectRows::new(&op, &f32::fact(shape)).unwrap();
        let input = Tensor::from_shape(
            &shape,
            &(0..volume(&shape).unwrap())
                .map(|i| i as f32 / 31.)
                .collect::<Vec<_>>(),
        )?;
        let expected = op.eval(tvec!(input.clone().into()))?;
        let actual =
            multithread_tract_scope(Executor::multithread(3), || fast.eval(tvec!(input.into())))?;
        assert_eq!(bits(&actual[0])?, bits(&expected[0])?);
        Ok(())
    }

    #[test]
    fn channel_maximum_matches_upstream_for_finite_nonfinite_and_signed_zero() -> TractResult<()> {
        let shape = [2, 3, 127, 129];
        let op = Reduce {
            axes: tvec!(1),
            reducer: Reducer::Max,
        };
        let fast = ChannelMaximum::new(&op, &f32::fact(shape)).unwrap();
        for special in [
            0.,
            -0.,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::from_bits(0x7fc01234),
        ] {
            for uniform in [true, false] {
                let mut data: Vec<_> = (0..volume(&shape).unwrap())
                    .map(|i| {
                        if uniform {
                            if special == 0. && i % 2 != 0 {
                                -special
                            } else {
                                special
                            }
                        } else {
                            (i % 197) as f32 - 100.
                        }
                    })
                    .collect();
                for i in [
                    0,
                    1,
                    15,
                    16,
                    255,
                    127 * 129 + 9,
                    2 * 127 * 129 + 11,
                    data.len() - 1,
                ] {
                    data[i] = special;
                }
                let input = Tensor::from_shape(&shape, &data)?;
                let expected = op.eval(tvec!(input.clone().into()))?;
                let inputs = tvec!(input.into());
                #[cfg(not(target_arch = "wasm32"))]
                let actual = tract_linalg::multithread::multithread_tract_scope(
                    tract_linalg::multithread::Executor::multithread(3),
                    || fast.eval(inputs),
                )?;
                #[cfg(target_arch = "wasm32")]
                let actual = fast.eval(inputs)?;
                assert_eq!(actual[0].shape(), expected[0].shape());
                assert_eq!(bits(&actual[0])?, bits(&expected[0])?);
            }
        }
        Ok(())
    }

    #[test]
    fn row_rewrites_reject_unsupported_geometry_and_changed_inputs() -> TractResult<()> {
        let mut op = Pad {
            pads: vec![(0, 0), (0, 0), (1, 1), (1, 1)],
            mode: PadMode::Reflect,
        };
        for fact in [
            f32::fact([1, 3, 1, 4]),
            f32::fact([1, 0, 3, 4]),
            i32::fact([1, 3, 3, 4]),
            f32::fact([i64::MAX as usize, 3, 3, 4]),
        ] {
            assert!(ReflectRows::new(&op, &fact).is_none());
        }
        let fast = ReflectRows::new(&op, &f32::fact([1, 3, 3, 4])).unwrap();
        assert!(fast.eval(tvec!()).is_err());
        assert!(fast
            .eval(tvec!(Tensor::zero::<i32>(&[1, 3, 3, 4])?.into()))
            .is_err());
        assert!(fast
            .eval(tvec!(Tensor::zero::<f32>(&[1, 3, 4, 3])?.into()))
            .is_err());
        op.pads[1] = (1, 0);
        assert!(ReflectRows::new(&op, &f32::fact([1, 3, 3, 4])).is_none());
        for axes in [
            tvec!(1, 2),
            tvec!(3, 2, 1),
            tvec!(1, 2, 3, 4),
            tvec!(1, 2, 2, 3),
        ] {
            assert!(ChannelMaximum::new(
                &Reduce {
                    axes,
                    reducer: Reducer::Max
                },
                &f32::fact([1, 3, 128, 128])
            )
            .is_none());
        }
        let op = Reduce {
            axes: tvec!(1),
            reducer: Reducer::Max,
        };
        assert!(ChannelMaximum::new(&op, &f32::fact([0, 3, 128, 128])).is_none());
        for shape in [
            vec![1, 1, 256, 256],
            vec![1, 32768, 1, 1],
            vec![3, 128, 128],
        ] {
            assert!(ChannelMaximum::new(&op, &f32::fact(shape)).is_none());
        }
        assert!(ChannelMaximum::new(&op, &i32::fact([1, 3, 128, 128])).is_none());
        assert!(ChannelMaximum::new(&op, &f32::fact([i64::MAX as usize, 3, 128, 128])).is_none());
        assert!(ChannelMaximum::new(
            &Reduce {
                reducer: Reducer::Sum,
                ..op.clone()
            },
            &f32::fact([1, 3, 128, 128])
        )
        .is_none());
        let fast = ChannelMaximum::new(&op, &f32::fact([1, 3, 128, 128])).unwrap();
        assert!(fast.eval(tvec!()).is_err());
        assert!(fast
            .eval(tvec!(Tensor::zero::<i32>(&[1, 3, 128, 128])?.into()))
            .is_err());
        assert!(fast
            .eval(tvec!(Tensor::zero::<f32>(&[1, 3, 128, 129])?.into()))
            .is_err());
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn run_full_plan(
        plan: &std::sync::Arc<TypedSimplePlan>,
        data: TValue,
    ) -> TractResult<TVec<TValue>> {
        crate::cpu_threads::run("anti-smudge", || plan.run(tvec!(data)))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "full shipped graph and paired timing; make profile-anti-smudge-rows"]
    fn anti_smudge_image_rows_match_previous_cpu() -> TractResult<()> {
        use std::time::Instant;
        use tract_onnx::prelude::*;
        let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(bytes.as_ref()))?
            .with_input_fact(0, f32::fact([1, 3, 2048, 2048]).into())?
            .into_typed()?;
        let before = crate::prepare_cpu_plan_with_options(
            "anti-smudge",
            model.clone(),
            false,
            crate::CpuOptimizations {
                restoration_kernels: true,
                tiled_attention: true,
                image_rows: false,
            },
        )?;
        let after = crate::prepare_cpu_plan_with_options(
            "anti-smudge",
            model,
            false,
            crate::CpuOptimizations::ALL,
        )?;
        assert_eq!(
            after
                .model()
                .nodes()
                .iter()
                .filter(|n| n.op_is::<ReflectRows>())
                .count(),
            16
        );
        assert_eq!(
            after
                .model()
                .nodes()
                .iter()
                .filter(|n| n.op_is::<ChannelMaximum>())
                .count(),
            1
        );
        assert_eq!(
            after
                .model()
                .nodes()
                .iter()
                .filter(|n| n.op().name() == "PlannedResize")
                .count(),
            8
        );
        let data: TValue = Tensor::from_shape(
            &[1, 3, 2048, 2048],
            &(0..3 * 2048 * 2048)
                .map(|i| (i * 17 % 101) as f32 / 100.)
                .collect::<Vec<_>>(),
        )?
        .into();
        let mut expected: Option<TVec<TValue>> = None;
        let mut times = [Vec::new(), Vec::new()];
        let mut max_difference = 0.0f32;
        for round in 0..4 {
            for slot in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                let output = run_full_plan(if slot == 0 { &before } else { &after }, data.clone())?;
                let seconds = start.elapsed().as_secs_f64();
                eprintln!(
                    "image rows round={round} optimized={}: {seconds:.3}s",
                    slot == 1
                );
                if round != 0 {
                    times[slot].push(seconds);
                }
                if let Some(reference) = &expected {
                    let reference = reference[0].to_plain_array_view::<f32>()?;
                    let actual = output[0].to_plain_array_view::<f32>()?;
                    assert_eq!(actual.shape(), reference.shape());
                    for (&a, &b) in actual.iter().zip(reference.iter()) {
                        let difference = (a - b).abs();
                        assert!(
                            a.is_finite() && b.is_finite() && difference < 5e-5,
                            "restoration output {a} != {b}"
                        );
                        max_difference = max_difference.max(difference);
                    }
                } else {
                    expected = Some(output);
                }
            }
        }
        let scene: TValue = Tensor::from_shape(
            &[1, 3, 2048, 2048],
            &(0..3 * 2048 * 2048)
                .map(|i| {
                    let c = i / (2048 * 2048);
                    let y = i / 2048 % 2048;
                    let x = i % 2048;
                    if x % 227 < 8 && y % 193 < 8 {
                        return 1.;
                    }
                    let dx = x as f32 - 641.;
                    let dy = y as f32 - 719.;
                    (0.02
                        + 0.07 * c as f32
                        + x as f32 / 8192.
                        + y as f32 / 16384.
                        + 18. / (dx.hypot(dy) + 50.)
                        + (x % 13) as f32 / 1000.)
                        .min(1.)
                })
                .collect::<Vec<_>>(),
        )?
        .into();
        let reference = run_full_plan(&before, scene.clone())?;
        let actual = run_full_plan(&after, scene)?;
        let reference = reference[0].to_plain_array_view::<f32>()?;
        let actual = actual[0].to_plain_array_view::<f32>()?;
        for (&a, &b) in actual.iter().zip(reference.iter()) {
            let difference = (a - b).abs();
            assert!(
                a.is_finite() && b.is_finite() && difference < 5e-5,
                "spatial scene output {a} != {b}"
            );
            max_difference = max_difference.max(difference);
        }
        for values in &mut times {
            values.sort_by(f64::total_cmp);
        }
        eprintln!("Image rows paired medians (3 warm runs each): {:.3}s -> {:.3}s; max absolute difference {max_difference}",times[0][1],times[1][1]);
        Ok(())
    }
}
