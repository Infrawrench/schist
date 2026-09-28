//! Row-contiguous FP32 depthwise convolution for restoration networks.
//! Keep the original tap order; SIMD runs across pixels, never the reduction.
use tract_linalg::multithread::par_chunks_mut;
use tract_onnx::tract_core::{
    internal::*,
    ops::{
        cnn::{Conv, KernelFormat, PaddingSpec},
        nn::DataFormat,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct RowDepthwiseConv {
    shapes: [Vec<usize>; 4],
    channels: usize,
    height: usize,
    width: usize,
    out_height: usize,
    out_width: usize,
    kernel: usize,
    pad: usize,
}

impl RowDepthwiseConv {
    fn new(conv: &Conv, shapes: [Vec<usize>; 4]) -> Option<Self> {
        if conv.q_params.is_some()
            || conv.kernel_fmt != KernelFormat::OIHW
            || conv.pool_spec.strides().as_ref() != [1, 1]
            || conv.pool_spec.dilations().as_ref() != [1, 1]
        {
            return None;
        }
        let (batch, channels, height, width) =
            match (conv.pool_spec.data_format, shapes[0].as_slice()) {
                (DataFormat::NCHW, &[b, c, h, w]) => (b, c, h, w),
                (DataFormat::CHW, &[c, h, w]) => (1, c, h, w),
                _ => return None,
            };
        let &[kernel, kw] = conv.pool_spec.kernel_shape.as_slice() else {
            return None;
        };
        if !matches!(kernel, 3 | 5)
            || kernel != kw
            || conv.group != channels
            || conv.input_channels() != channels
            || conv.output_channels() != channels
            || shapes[1] != [channels, 1, kernel, kernel]
            || !(shapes[2].is_empty() || shapes[2] == [1] || shapes[2] == [channels])
        {
            return None;
        }
        let pad = match &conv.pool_spec.padding {
            PaddingSpec::Valid => 0,
            PaddingSpec::SameUpper | PaddingSpec::SameLower => kernel / 2,
            PaddingSpec::Explicit(a, b) if a.as_slice() == [0, 0] && b.as_slice() == [0, 0] => 0,
            PaddingSpec::Explicit(a, b)
                if a.as_slice() == [kernel / 2; 2] && b.as_slice() == [kernel / 2; 2] =>
            {
                kernel / 2
            }
            _ => return None,
        };
        let out_height = height.checked_add(2 * pad)?.checked_sub(kernel - 1)?;
        let out_width = width.checked_add(2 * pad)?.checked_sub(kernel - 1)?;
        let expected = if shapes[0].len() == 4 {
            vec![batch, channels, out_height, out_width]
        } else {
            vec![channels, out_height, out_width]
        };
        if shapes[3] != expected {
            return None;
        }
        for shape in &shapes {
            let size =
                shape.iter().try_fold(
                    1usize,
                    |n, &d| {
                        if d == 0 {
                            None
                        } else {
                            n.checked_mul(d)
                        }
                    },
                )?;
            if size > isize::MAX as usize / std::mem::size_of::<f32>() {
                return None;
            }
        }
        Some(Self {
            shapes,
            channels,
            height,
            width,
            out_height,
            out_width,
            kernel,
            pad,
        })
    }

    fn rows(&self, input: &[f32], weights: &[f32], bias: &[f32], first: usize, output: &mut [f32]) {
        for (row_index, row) in output.chunks_exact_mut(self.out_width).enumerate() {
            let index = first + row_index;
            let plane = index / self.out_height;
            let y = index % self.out_height;
            let channel = plane % self.channels;
            row.fill(bias[if bias.len() == 1 { 0 } else { channel }]);
            let source =
                &input[plane * self.height * self.width..(plane + 1) * self.height * self.width];
            let filter = &weights
                [channel * self.kernel * self.kernel..(channel + 1) * self.kernel * self.kernel];
            for ky in 0..self.kernel {
                let Some(sy) = (y + ky).checked_sub(self.pad).filter(|&v| v < self.height) else {
                    continue;
                };
                for kx in 0..self.kernel {
                    let left = self.pad.saturating_sub(kx);
                    let right = self
                        .out_width
                        .min((self.width + self.pad).saturating_sub(kx));
                    if left >= right {
                        continue;
                    }
                    let start = sy * self.width + left + kx - self.pad;
                    let weight = filter[ky * self.kernel + kx];
                    for (out, &value) in row[left..right]
                        .iter_mut()
                        .zip(&source[start..start + right - left])
                    {
                        *out += value * weight;
                    }
                }
            }
        }
    }
}

impl Op for RowDepthwiseConv {
    fn name(&self) -> StaticName {
        "RowDepthwiseConv".into()
    }
    op_as_typed_op!();
}
impl TypedOp for RowDepthwiseConv {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(inputs.len() == 3, "depthwise input count changed");
        for (input, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                input.datum_type == DatumType::F32
                    && input.shape.as_concrete() == Some(shape.as_slice()),
                "depthwise input fact changed"
            );
        }
        Ok(tvec!(f32::fact(&self.shapes[3])))
    }
    as_op!();
}
impl EvalOp for RowDepthwiseConv {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(inputs.len() == 3, "depthwise input count changed");
        for (input, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                input.datum_type() == DatumType::F32 && input.shape() == shape,
                "depthwise input shape or type changed"
            );
        }
        let input = inputs[0].to_plain_array_view::<f32>()?;
        let weights = inputs[1].to_plain_array_view::<f32>()?;
        let bias = inputs[2].to_plain_array_view::<f32>()?;
        let input = input.as_slice().context("non-contiguous depthwise input")?;
        let weights = weights
            .as_slice()
            .context("non-contiguous depthwise weights")?;
        let bias = bias.as_slice().context("non-contiguous depthwise bias")?;
        let mut output = Tensor::zero::<f32>(&self.shapes[3])?;
        {
            let mut view = output.to_plain_array_view_mut::<f32>()?;
            let values = view
                .as_slice_mut()
                .context("non-contiguous depthwise output")?;
            par_chunks_mut(values, self.out_width, values.len(), |first, chunk| {
                self.rows(input, weights, bias, first, chunk);
                Ok(())
            })?;
        }
        Ok(tvec!(output.into()))
    }
}

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<usize> {
    let mut count = 0;
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(conv) = node.op_as::<Conv>() else {
            continue;
        };
        if node.inputs.len() != 3 || node.outputs.len() != 1 {
            continue;
        }
        let facts = node
            .inputs
            .iter()
            .map(|&i| model.outlet_fact(i))
            .collect::<TractResult<Vec<_>>>()?;
        let facts = [facts[0], facts[1], facts[2], &node.outputs[0].fact];
        if facts
            .iter()
            .any(|f| f.datum_type != DatumType::F32 || f.shape.as_concrete().is_none())
        {
            continue;
        }
        let shapes = facts.map(|f| f.shape.as_concrete().unwrap().to_vec());
        if let Some(plan) = RowDepthwiseConv::new(conv, shapes) {
            model.node_mut(id).op = Box::new(plan);
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tract_onnx::tract_core::ops::cnn::PoolSpec;

    fn conv(channels: usize, kernel: usize, same: bool) -> Conv {
        Conv::new(
            PoolSpec {
                data_format: DataFormat::NCHW,
                kernel_shape: tvec!(kernel, kernel),
                padding: if same {
                    PaddingSpec::SameUpper
                } else {
                    PaddingSpec::Valid
                },
                dilations: None,
                strides: None,
                input_channels: channels,
                output_channels: channels,
            },
            KernelFormat::OIHW,
            channels,
            None,
        )
    }
    fn tensor(shape: &[usize], phase: usize) -> TractResult<TValue> {
        Ok(Tensor::from_shape(
            shape,
            &(0..shape.iter().product())
                .map(|i| ((i * 17 + phase) % 101) as f32 / 50. - 1.)
                .collect::<Vec<_>>(),
        )?
        .into())
    }
    fn case(
        batch: usize,
        c: usize,
        h: usize,
        w: usize,
        kernel: usize,
        same: bool,
        scalar: bool,
    ) -> TractResult<()> {
        let op = conv(c, kernel, same);
        let shapes = [
            vec![batch, c, h, w],
            vec![c, 1, kernel, kernel],
            if scalar { vec![] } else { vec![c] },
            vec![
                batch,
                c,
                if same { h } else { h - kernel + 1 },
                if same { w } else { w - kernel + 1 },
            ],
        ];
        let inputs = tvec!(
            tensor(&shapes[0], 0)?,
            tensor(&shapes[1], 7)?,
            tensor(&shapes[2], 13)?
        );
        let plan = RowDepthwiseConv::new(&op, shapes.clone()).unwrap();
        let mut model = TypedModel::default();
        let image = model.add_source("image", f32::fact(&shapes[0]))?;
        let weights = model.add_const("weights", inputs[1].clone().into_tensor())?;
        let bias = model.add_const("bias", inputs[2].clone().into_tensor())?;
        let out = model.wire_node("conv", op, &[image, weights, bias])?;
        model.select_output_outlets(&out)?;
        let original = model.clone().into_optimized()?.into_runnable()?;
        let expected = original.run(tvec!(inputs[0].clone()))?;
        let actual = plan.eval(inputs.clone())?;
        let e = expected[0].to_plain_array_view::<f32>()?;
        let a = actual[0].to_plain_array_view::<f32>()?;
        assert_eq!(e.shape(), a.shape());
        for (&a, &e) in a.iter().zip(e.iter()) {
            assert!((a - e).abs() < 1e-5 * (1. + e.abs()), "{a} != {e}");
        }
        model.declutter()?;
        assert_eq!(optimize(&mut model)?, 1);
        let optimized = model
            .into_optimized()?
            .into_runnable()?
            .run(tvec!(inputs[0].clone()))?;
        assert_eq!(optimized[0].to_plain_array_view::<f32>()?, a);
        // CHW has the same plane geometry, but no batch dimension.
        if batch == 1 {
            let mut chw = conv(c, kernel, same);
            chw.pool_spec.data_format = DataFormat::CHW;
            let mut shapes = shapes;
            shapes[0].remove(0);
            shapes[3].remove(0);
            let chw = RowDepthwiseConv::new(&chw, shapes.clone()).unwrap();
            let mut image = inputs[0].clone().into_tensor();
            image.set_shape(&shapes[0])?;
            let out = chw.eval(tvec!(image.into(), inputs[1].clone(), inputs[2].clone()))?;
            assert_eq!(
                out[0].to_plain_array_view::<f32>()?.as_slice(),
                a.as_slice()
            );
        }
        Ok(())
    }
    #[test]
    fn depthwise_matches_tract_for_batches_edges_thin_images_and_tails() -> TractResult<()> {
        for kernel in [3, 5] {
            for (b, c, h, w) in [(1, 3, 1, 1), (2, 5, 2, 3), (1, 7, 9, 17), (2, 2, 11, 31)] {
                for scalar in [false, true] {
                    case(b, c, h, w, kernel, true, scalar)?;
                }
            }
            case(2, 3, 13, 29, kernel, false, false)?;
            case(1, 3, kernel, kernel, kernel, false, true)?;
        }
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_depthwise_preserves_every_output_bit() -> TractResult<()> {
        use tract_linalg::multithread::{multithread_tract_scope, Executor};
        let shapes = [
            vec![2, 7, 127, 129],
            vec![7, 1, 3, 3],
            vec![7],
            vec![2, 7, 127, 129],
        ];
        let op = RowDepthwiseConv::new(&conv(7, 3, true), shapes.clone()).unwrap();
        let inputs = tvec!(
            tensor(&shapes[0], 0)?,
            tensor(&shapes[1], 3)?,
            tensor(&shapes[2], 7)?
        );
        let serial = multithread_tract_scope(Executor::SingleThread, || op.eval(inputs.clone()))?;
        let parallel = multithread_tract_scope(Executor::multithread(3), || op.eval(inputs))?;
        let bits = |v: &TValue| -> TractResult<Vec<u32>> {
            Ok(v.to_plain_array_view::<f32>()?
                .iter()
                .map(|v| v.to_bits())
                .collect())
        };
        assert_eq!(bits(&serial[0])?, bits(&parallel[0])?);
        Ok(())
    }
    #[test]
    fn unsupported_depthwise_geometry_and_changed_inputs_are_rejected() -> TractResult<()> {
        let shapes = [
            vec![1, 3, 7, 9],
            vec![3, 1, 3, 3],
            vec![3],
            vec![1, 3, 7, 9],
        ];
        for option in 0..8 {
            let mut op = conv(3, 3, true);
            match option {
                0 => op.group = 1,
                1 => op.q_params = Some(DatumType::I32),
                2 => op.pool_spec.data_format = DataFormat::NHWC,
                3 => op.pool_spec.strides = Some(tvec!(2, 1)),
                4 => op.pool_spec.dilations = Some(tvec!(1, 2)),
                5 => op.pool_spec.padding = PaddingSpec::Explicit(tvec!(0, 1), tvec!(2, 1)),
                6 => op.pool_spec.kernel_shape = tvec!(3, 5),
                _ => op.kernel_fmt = KernelFormat::HWIO,
            }
            assert!(RowDepthwiseConv::new(&op, shapes.clone()).is_none());
        }
        for i in 0..4 {
            let mut invalid = shapes.clone();
            invalid[i][0] = 0;
            assert!(RowDepthwiseConv::new(&conv(3, 3, true), invalid).is_none());
        }
        let mut invalid = shapes.clone();
        invalid[0][2] = usize::MAX;
        invalid[3][2] = usize::MAX;
        assert!(RowDepthwiseConv::new(&conv(3, 3, true), invalid).is_none());
        let op = RowDepthwiseConv::new(&conv(3, 3, true), shapes.clone()).unwrap();
        assert!(op.eval(tvec!()).is_err());
        for image in [
            tensor(&[1, 3, 9, 7], 0)?,
            Tensor::zero::<i32>(&shapes[0])?.into(),
        ] {
            assert!(op
                .eval(tvec!(image, tensor(&shapes[1], 0)?, tensor(&shapes[2], 0)?))
                .is_err());
        }
        Ok(())
    }
}
