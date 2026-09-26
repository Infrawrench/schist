//! Bounded spatial bands for large, same-padded 3×3 decoder convolutions.
//! Keep the model's float32 filters, bias and sampling geometry unchanged.
use crate::accelerate_matrix::cblas_sgemm;
use tract_onnx::tract_core::{
    internal::*,
    ops::{
        cnn::{Conv, KernelFormat, PaddingSpec},
        nn::DataFormat,
    },
};

const SCRATCH_FLOATS: usize = 4 * 1024 * 1024;

fn volume(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |n, &d| if d == 0 { None } else { n.checked_mul(d) })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BandedConv {
    shapes: [Vec<usize>; 4],
    batch: usize,
    ic: usize,
    oc: usize,
    height: usize,
    width: usize,
    spatial: usize,
    reduction: usize,
    band: usize,
}

impl BandedConv {
    fn new(conv: &Conv, shapes: [Vec<usize>; 4]) -> Option<Self> {
        if conv.q_params.is_some()
            || conv.group != 1
            || conv.kernel_fmt != KernelFormat::OIHW
            || conv.pool_spec.data_format != DataFormat::NCHW
            || conv.pool_spec.kernel_shape.as_slice() != [3, 3]
            || conv.pool_spec.strides().as_ref() != [1, 1]
            || conv.pool_spec.dilations().as_ref() != [1, 1]
        {
            return None;
        }
        let lengths = shapes.each_ref().map(|s| volume(s));
        if lengths.iter().any(Option::is_none) {
            return None;
        }
        let &[batch, ic, height, width] = shapes[0].as_slice() else {
            return None;
        };
        let &[ob, oc, oh, ow] = shapes[3].as_slice() else {
            return None;
        };
        if [ob, oh, ow] != [batch, height, width]
            || shapes[1] != [oc, ic, 3, 3]
            || conv.input_channels() != ic
            || conv.output_channels() != oc
            || shapes[2].len() > 1
            || !matches!(lengths[2], Some(n) if n == 1 || n == oc)
        {
            return None;
        }
        let same_padding = match &conv.pool_spec.padding {
            PaddingSpec::SameUpper | PaddingSpec::SameLower => true,
            PaddingSpec::Explicit(before, after) => {
                before.as_slice() == [1, 1] && after.as_slice() == [1, 1]
            }
            _ => false,
        };
        if !same_padding {
            return None;
        }
        let spatial = height.checked_mul(width)?;
        let reduction = ic.checked_mul(9)?;
        if [oc, spatial, reduction]
            .iter()
            .any(|&n| n > i32::MAX as usize)
        {
            return None;
        }
        // Narrow-output layers need longer bands to amortize BLAS calls;
        // wider outputs favor smaller working sets. Keep whole source rows
        // together when the scratch bound allows it.
        let limit = if oc >= 32 { 1024 } else { 4096 };
        let mut band = spatial.min(limit).min(SCRATCH_FLOATS / reduction);
        if band >= width {
            band -= band % width;
        }
        if band == 0 {
            return None;
        }
        Some(Self {
            shapes,
            batch,
            ic,
            oc,
            height,
            width,
            spatial,
            reduction,
            band,
        })
    }

    /// Pack K×N in filter order. Copy contiguous row interiors; only image
    /// borders contribute zeros. Every scratch element is written each time.
    fn pack(&self, input: &[f32], start: usize, count: usize, packed: &mut [f32]) {
        assert_eq!(input.len(), self.ic * self.spatial);
        assert_eq!(packed.len(), self.reduction * count);
        for (k, destination) in packed.chunks_exact_mut(count).enumerate() {
            let channel = k / 9;
            let ky = k % 9 / 3;
            let kx = k % 3;
            let mut done = 0;
            while done < count {
                let position = start + done;
                let y = position / self.width;
                let x = position % self.width;
                let n = (count - done).min(self.width - x);
                let row = &mut destination[done..done + n];
                let sy = (y + ky).checked_sub(1).filter(|&v| v < self.height);
                let left = x.max(usize::from(kx == 0));
                let right = (x + n).min(self.width - usize::from(kx == 2));
                if let Some(sy) = sy.filter(|_| left < right) {
                    row[..left - x].fill(0.);
                    row[right - x..].fill(0.);
                    let source = channel * self.spatial + sy * self.width + left + kx - 1;
                    row[left - x..right - x].copy_from_slice(&input[source..source + right - left]);
                } else {
                    row.fill(0.);
                }
                done += n;
            }
        }
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
        let Some(plan) = BandedConv::new(conv, shapes) else {
            continue;
        };
        // Target the large decoder stages, retaining tract's small/depthwise
        // kernels and the backbone's smaller residual convolutions.
        if plan.spatial < 65_536 || !(16..=64).contains(&plan.oc) {
            continue;
        }
        model.node_mut(id).op = Box::new(plan);
        count += 1;
    }
    Ok(count)
}

impl Op for BandedConv {
    fn name(&self) -> StaticName {
        "BandedAccelerateConv".into()
    }
    op_as_typed_op!();
}
impl TypedOp for BandedConv {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(f32::fact(&self.shapes[3])))
    }
    as_op!();
}
impl EvalOp for BandedConv {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(inputs.len() == 3, "convolution input count changed");
        for (input, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                input.datum_type() == DatumType::F32 && input.shape() == shape,
                "convolution input shape or type changed"
            );
        }
        let input = inputs[0].to_plain_array_view::<f32>()?;
        let input = input
            .as_slice()
            .context("non-contiguous convolution input")?;
        let weights = inputs[1].to_plain_array_view::<f32>()?;
        let weights = weights
            .as_slice()
            .context("non-contiguous convolution weights")?;
        let bias = inputs[2].to_plain_array_view::<f32>()?;
        let bias = bias.as_slice().context("non-contiguous convolution bias")?;
        let mut scratch = vec![0.; self.reduction * self.band];
        let mut output = Tensor::zero::<f32>(&self.shapes[3])?;
        {
            let mut view = output.to_plain_array_view_mut::<f32>()?;
            let values = view
                .as_slice_mut()
                .context("non-contiguous convolution output")?;
            for batch in 0..self.batch {
                let source =
                    &input[batch * self.ic * self.spatial..(batch + 1) * self.ic * self.spatial];
                let destination = &mut values
                    [batch * self.oc * self.spatial..(batch + 1) * self.oc * self.spatial];
                for (channel, row) in destination.chunks_exact_mut(self.spatial).enumerate() {
                    row.fill(bias[if bias.len() == 1 { 0 } else { channel }]);
                }
                for start in (0..self.spatial).step_by(self.band) {
                    let n = self.band.min(self.spatial - start);
                    let packed = &mut scratch[..self.reduction * n];
                    self.pack(source, start, n, packed);
                    // SAFETY: checked shapes prove weights=OC×K, packed=K×N,
                    // and output=OC×spatial. N<=spatial-start, so the strided
                    // output view stays in this batch. All BLAS dimensions and
                    // leading strides fit positive i32. Inputs are immutable;
                    // fully initialized scratch and output are disjoint, and
                    // the call is synchronous. beta=1 adds the original bias.
                    unsafe {
                        cblas_sgemm(
                            101,
                            111,
                            111,
                            self.oc as i32,
                            n as i32,
                            self.reduction as i32,
                            1.,
                            weights.as_ptr(),
                            self.reduction as i32,
                            packed.as_ptr(),
                            n as i32,
                            1.,
                            destination.as_mut_ptr().add(start),
                            self.spatial as i32,
                        );
                    }
                }
            }
        }
        Ok(tvec!(output.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tract_onnx::tract_core::ops::cnn::PoolSpec;

    fn conv(ic: usize, oc: usize) -> Conv {
        Conv::new(
            PoolSpec {
                data_format: DataFormat::NCHW,
                kernel_shape: tvec!(3, 3),
                padding: PaddingSpec::SameUpper,
                dilations: None,
                strides: None,
                input_channels: ic,
                output_channels: oc,
            },
            KernelFormat::OIHW,
            1,
            None,
        )
    }
    fn shapes(b: usize, ic: usize, oc: usize, h: usize, w: usize, scalar: bool) -> [Vec<usize>; 4] {
        [
            vec![b, ic, h, w],
            vec![oc, ic, 3, 3],
            if scalar { vec![] } else { vec![oc] },
            vec![b, oc, h, w],
        ]
    }
    fn tensor(shape: &[usize], phase: f32) -> TractResult<TValue> {
        let data: Vec<f32> = (0..volume(shape).unwrap())
            .map(|i| ((i as f32 * 0.31) + phase).sin() * 0.1)
            .collect();
        Ok(Tensor::from_shape(shape, &data)?.into())
    }
    fn close(actual: &TValue, expected: &TValue) -> TractResult<()> {
        let a = actual.to_plain_array_view::<f32>()?;
        let b = expected.to_plain_array_view::<f32>()?;
        assert_eq!(a.shape(), b.shape());
        for (&a, &b) in a.iter().zip(b.iter()) {
            assert!(
                a.is_finite() && (a - b).abs() < 3e-5 * (1. + b.abs()),
                "{a} != {b}"
            );
        }
        Ok(())
    }

    #[test]
    fn banded_convolution_matches_scalar_reference_across_edges_batches_and_tails(
    ) -> TractResult<()> {
        for (b, ic, oc, h, w, scalar) in [
            (2, 3, 5, 1, 1, false),
            (1, 2, 3, 5, 2, true),
            (2, 4, 7, 9, 17, false),
            (1, 2, 4, 3, 29, true),
        ] {
            let shapes = shapes(b, ic, oc, h, w, scalar);
            let mut plan = BandedConv::new(&conv(ic, oc), shapes.clone()).unwrap();
            plan.band = plan.band.min(11); // Force row crossings and incomplete final bands.
            let inputs = tvec!(
                tensor(&shapes[0], 0.)?,
                tensor(&shapes[1], 0.3)?,
                tensor(&shapes[2], 0.8)?
            );
            let a = inputs[0].to_plain_array_view::<f32>()?;
            let k = inputs[1].to_plain_array_view::<f32>()?;
            let bias = inputs[2].to_plain_array_view::<f32>()?;
            let bias = bias.as_slice().unwrap();
            let mut reference = vec![0.; b * oc * h * w];
            for batch in 0..b {
                for co in 0..oc {
                    for y in 0..h {
                        for x in 0..w {
                            let mut sum = bias[if scalar { 0 } else { co }];
                            for ci in 0..ic {
                                for ky in 0..3 {
                                    for kx in 0..3 {
                                        let sy = y as isize + ky as isize - 1;
                                        let sx = x as isize + kx as isize - 1;
                                        if sy >= 0 && sx >= 0 && sy < h as isize && sx < w as isize
                                        {
                                            sum += a[[batch, ci, sy as usize, sx as usize]]
                                                * k[[co, ci, ky, kx]];
                                        }
                                    }
                                }
                            }
                            reference[((batch * oc + co) * h + y) * w + x] = sum;
                        }
                    }
                }
            }
            let expected: TValue = Tensor::from_shape(&shapes[3], &reference)?.into();
            close(&plan.eval(inputs)?[0], &expected)?;
        }
        Ok(())
    }

    #[test]
    fn convolution_packing_preserves_bits_and_rewrites_only_supported_geometry() -> TractResult<()>
    {
        let shape = shapes(1, 2, 3, 3, 5, false);
        let plan = BandedConv::new(&conv(2, 3), shape.clone()).unwrap();
        let special = [
            0, 0x80000000, 0x7fc01234, 0x7f800000, 0xff800000, 0x3e000000,
        ];
        let input: Vec<f32> = (0..30)
            .map(|i| f32::from_bits(special[i % special.len()]))
            .collect();
        for (start, n) in [(0, 7), (7, 7), (14, 1)] {
            let mut packed = vec![f32::from_bits(0xdeadbeef); 18 * n];
            plan.pack(&input, start, n, &mut packed);
            for ci in 0..2 {
                for ky in 0..3 {
                    for kx in 0..3 {
                        for j in 0..n {
                            let y = ((start + j) / 5) as isize + ky as isize - 1;
                            let x = ((start + j) % 5) as isize + kx as isize - 1;
                            let expected = if (0..3).contains(&y) && (0..5).contains(&x) {
                                input[ci * 15 + y as usize * 5 + x as usize].to_bits()
                            } else {
                                0
                            };
                            assert_eq!(
                                packed[((ci * 3 + ky) * 3 + kx) * n + j].to_bits(),
                                expected
                            );
                        }
                    }
                }
            }
        }
        for choice in 0..8 {
            let mut op = conv(2, 3);
            match choice {
                0 => op.group = 2,
                1 => op.q_params = Some(DatumType::I32),
                2 => op.pool_spec.data_format = DataFormat::NHWC,
                3 => op.pool_spec.strides = Some(tvec!(2, 1)),
                4 => op.pool_spec.dilations = Some(tvec!(1, 2)),
                5 => op.pool_spec.padding = PaddingSpec::Explicit(tvec!(1, 0), tvec!(1, 2)),
                6 => {
                    op.pool_spec.padding = PaddingSpec::Explicit(tvec!(usize::MAX, 1), tvec!(1, 1))
                }
                _ => op.pool_spec.kernel_shape = tvec!(1, 1),
            }
            assert!(BandedConv::new(&op, shape.clone()).is_none());
        }
        for invalid in [
            shapes(1, 2, 3, 0, 5, false),
            shapes(1, 2, 3, usize::MAX, 5, false),
            shapes(1, SCRATCH_FLOATS, 3, 1, 1, false),
        ] {
            assert!(BandedConv::new(&conv(invalid[0][1], 3), invalid).is_none());
        }
        let large = BandedConv::new(&conv(576, 256), shapes(1, 576, 256, 96, 96, false)).unwrap();
        assert!(large.band * large.reduction <= SCRATCH_FLOATS);
        assert!(plan
            .eval(tvec!(
                tensor(&[1, 2, 5, 3], 0.)?,
                tensor(&shape[1], 0.)?,
                tensor(&shape[2], 0.)?
            ))
            .is_err());
        assert!(plan.eval(tvec!()).is_err());
        Ok(())
    }

    fn graph(ic: usize, oc: usize, h: usize, w: usize) -> TractResult<TypedModel> {
        let shape = shapes(1, ic, oc, h, w, false);
        let mut model = TypedModel::default();
        let input = model.add_source("input", f32::fact(&shape[0]))?;
        let weights = model.add_const("weights", tensor(&shape[1], 0.3)?.into_tensor())?;
        let bias = model.add_const("bias", tensor(&shape[2], 0.8)?.into_tensor())?;
        let output = model.wire_node("conv", conv(ic, oc), &[input, weights, bias])?;
        model.select_output_outlets(&output)?;
        Ok(model)
    }

    #[test]
    fn banded_convolution_graph_matches_optimized_tract_with_constant_weights() -> TractResult<()> {
        for (h, w, oc, replaced) in [
            (256, 257, 17, true),
            (127, 129, 17, false),
            (256, 257, 128, false),
        ] {
            let original = graph(3, oc, h, w)?;
            let mut accelerated = original.clone();
            accelerated.declutter()?;
            assert_eq!(optimize(&mut accelerated)?, usize::from(replaced));
            let original = original.into_optimized()?.into_runnable()?;
            let accelerated = accelerated.into_optimized()?.into_runnable()?;
            let input = tvec!(tensor(&[1, 3, h, w], 0.)?);
            close(
                &accelerated.run(input.clone())?[0],
                &original.run(input)?[0],
            )?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "development timing; make profile-decoder-convolutions"]
    fn profile_decoder_convolutions() -> TractResult<()> {
        use std::time::Instant;
        for (ic, oc, side) in [
            (576, 256, 96),
            (352, 128, 192),
            (176, 64, 384),
            (68, 32, 768),
            (32, 16, 768),
        ] {
            let original = graph(ic, oc, side, side)?;
            let mut accelerated = original.clone();
            accelerated.declutter()?;
            if optimize(&mut accelerated)? == 0 {
                eprintln!("decoder {ic}->{oc} {side}px: retained tract");
                continue;
            }
            let mut original = original.into_optimized()?;
            // Include the existing fast padding pass in the comparison.
            crate::pad_copy::optimize(&mut original)?;
            let original = original.into_runnable()?;
            let accelerated = accelerated.into_optimized()?.into_runnable()?;
            let input = tvec!(tensor(&[1, ic, side, side], 0.)?);
            close(
                &accelerated.run(input.clone())?[0],
                &original.run(input.clone())?[0],
            )?;
            let mut times = [Vec::new(), Vec::new()];
            for round in 0..8 {
                for slot in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let start = Instant::now();
                    let plan = if slot == 0 { &original } else { &accelerated };
                    std::hint::black_box(plan.run(input.clone())?);
                    times[slot].push(start.elapsed().as_secs_f64());
                }
            }
            for values in &mut times {
                values.sort_by(f64::total_cmp);
            }
            let median = |v: &[f64]| (v[3] + v[4]) * 0.5 * 1000.;
            eprintln!(
                "decoder {ic}->{oc} {side}px: tract {:.3}ms, banded {:.3}ms",
                median(&times[0]),
                median(&times[1])
            );
        }
        Ok(())
    }
}
