//! Reconstruct the CMS's fused lattice, preserving its byte-quantized weights.
//!
//! moxcms 0.8 uses a 33^3 lattice (17^4 for CMYK input) for smooth LUT
//! profiles. Its public executor samples quantized positions near each node.
//! Inverting those small, separable interpolation systems recovers the actual
//! lattice instead of baking an additional approximation on top of the CMS.
//! Profiles requiring the CMS's unfused evaluator stay on the CPU.
use moxcms::{
    ColorProfile, DataColorSpace, LutStore, LutWarehouse, RenderingIntent, ToneReprCurve,
    TransformExecutor,
};
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource};
static SHADER: ComputeShader = ComputeShader::new("icc-clut", include_str!("gpu_lut.wgsl"));

pub(super) struct Lut {
    input: usize,
    output: usize,
    grid: usize,
    values: Vec<f32>,
}

fn smooth(curve: &[u16], max_step: u16) -> bool {
    curve
        .windows(2)
        .all(|p| p[1] > p[0] && p[1] - p[0] <= max_step)
}
fn curves_safe(lut: &LutWarehouse) -> bool {
    match lut {
        LutWarehouse::Multidimensional(lut) => lut
            .a_curves
            .iter()
            .chain(&lut.b_curves)
            .chain(&lut.m_curves)
            .all(|c| match c {
                ToneReprCurve::Parametric(_) => true,
                ToneReprCurve::Lut(c) => smooth(c, 2100),
            }),
        LutWarehouse::Lut(lut) => {
            // These are the channel windows checked by this CMS version's
            // fusion analysis. Use stricter monotonicity, rejecting plateaus.
            [
                (lut.num_input_channels as usize, &lut.input_table),
                (lut.num_output_channels as usize, &lut.output_table),
            ]
            .into_iter()
            .all(|(count, table)| {
                let (values, step) = match table {
                    LutStore::Store8(v) => (v.iter().map(|&v| v as u16).collect::<Vec<_>>(), 16),
                    LutStore::Store16(v) => (v.clone(), 2100),
                };
                (0..count).all(|c| {
                    values
                        .get(c * count..(c + 1) * count)
                        .is_some_and(|v| smooth(v, step))
                })
            })
        }
    }
}
fn selected(p: &ColorProfile, intent: RenderingIntent, reverse: bool) -> Option<&LutWarehouse> {
    match (reverse, intent) {
        (false, RenderingIntent::Perceptual) => p.lut_a_to_b_perceptual.as_ref(),
        (false, RenderingIntent::Saturation) => p.lut_a_to_b_saturation.as_ref(),
        (false, _) => p.lut_a_to_b_colorimetric.as_ref(),
        (true, RenderingIntent::Perceptual) => p.lut_b_to_a_perceptual.as_ref(),
        (true, RenderingIntent::Saturation) => p.lut_b_to_a_saturation.as_ref(),
        (true, _) => p.lut_b_to_a_colorimetric.as_ref(),
    }
}

impl Lut {
    pub fn compile(
        src: &ColorProfile,
        dst: &ColorProfile,
        intent: RenderingIntent,
        executor: &dyn TransformExecutor<f32>,
        src_stride: usize,
        dst_stride: usize,
    ) -> Option<Self> {
        let channels = |p: &ColorProfile| match p.color_space {
            DataColorSpace::Rgb | DataColorSpace::Lab => Some(3usize),
            DataColorSpace::Cmyk => Some(4),
            _ => None,
        };
        let (input, output) = (channels(src)?, channels(dst)?);
        if input == 4 && output == 4 {
            return None;
        }
        let source = selected(src, intent, false);
        let dest = selected(dst, intent, true);
        if source.is_none() && dest.is_none()
            || source.is_some_and(|l| !curves_safe(l))
            || dest.is_some_and(|l| !curves_safe(l))
        {
            return None;
        }
        let grid: usize = if input == 4 { 17 } else { 33 };
        let count = grid.pow(input as u32);
        let mut samples = vec![1.0; count * src_stride];
        for i in 0..count {
            let mut index = i;
            for c in (0..input).rev() {
                samples[i * src_stride + c] = (index % grid) as f32 / (grid - 1) as f32;
                index /= grid;
            }
        }
        let mut transformed = vec![0.0; count * dst_stride];
        executor.transform(&samples, &mut transformed).ok()?;
        let mut values: Vec<f32> = transformed
            .chunks_exact(dst_stride)
            .flat_map(|p| p[..output].iter().copied())
            .collect();
        // Each sampled coordinate blends a node with at most one neighbour.
        // The matrix is strictly diagonally dominant; solve in f64 once.
        let mut lower = vec![0.0; grid];
        let mut diagonal = vec![0.0; grid];
        let mut upper = vec![0.0; grid];
        for i in 0..grid {
            let position = (((i as f32 / (grid - 1) as f32) * 255.0).round() / 255.0
                * (grid - 1) as f32) as f64;
            let left = (position.floor() as usize).min(grid - 1);
            let fraction = position - left as f64;
            for (j, weight) in [(left, 1.0 - fraction), ((left + 1).min(grid - 1), fraction)] {
                if j == i {
                    diagonal[i] += weight;
                } else if j < i {
                    lower[i] += weight;
                } else {
                    upper[i] += weight;
                }
            }
        }
        for axis in 0..input {
            let inner = grid.pow((input - axis - 1) as u32) * output;
            let outer = count * output / (grid * inner);
            for block in 0..outer {
                for channel in 0..inner {
                    let at = |i: usize| (block * grid + i) * inner + channel;
                    let mut rhs = (0..grid).map(|i| values[at(i)] as f64).collect::<Vec<_>>();
                    let mut diag = diagonal.clone();
                    for i in 1..grid {
                        let m = lower[i] / diag[i - 1];
                        diag[i] -= m * upper[i - 1];
                        rhs[i] -= m * rhs[i - 1];
                    }
                    rhs[grid - 1] /= diag[grid - 1];
                    for i in (0..grid - 1).rev() {
                        rhs[i] = (rhs[i] - upper[i] * rhs[i + 1]) / diag[i];
                    }
                    for i in 0..grid {
                        values[at(i)] = rhs[i] as f32;
                    }
                }
            }
        }
        if values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let lut = Self {
            input,
            output,
            grid,
            values,
        };
        if !lut.values.iter().all(|v| v.is_finite()) {
            return None;
        }
        // Guard the lattice contract against CMS/version/profile route changes.
        let mut probes = vec![1.0; 4096 * src_stride];
        let mut seed = 0x982734u32;
        for p in probes.chunks_exact_mut(src_stride) {
            for c in &mut p[..input] {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *c = (seed >> 24) as f32 / 255.0;
            }
        }
        let mut expected = vec![0.0; 4096 * dst_stride];
        executor.transform(&probes, &mut expected).ok()?;
        for (p, expected) in probes
            .chunks_exact(src_stride)
            .zip(expected.chunks_exact(dst_stride))
        {
            let actual = lut.sample(p);
            if actual
                .iter()
                .zip(expected)
                .any(|(a, b)| (a - b).abs() > 3e-5)
            {
                return None;
            }
        }
        Some(lut)
    }

    fn sample(&self, p: &[f32]) -> Vec<f32> {
        let coords = p[..self.input]
            .iter()
            .map(|v| (v * 255.0).round().clamp(0.0, 255.0) / 255.0 * (self.grid - 1) as f32)
            .collect::<Vec<_>>();
        let mut out = vec![0.0; self.output];
        for corner in 0..1 << self.input {
            let mut index = 0;
            let mut weight = 1.0;
            for (axis, &v) in coords.iter().enumerate() {
                let high = corner & (1 << axis) != 0;
                index =
                    index * self.grid + (v.floor() as usize + usize::from(high)).min(self.grid - 1);
                weight *= if high {
                    v - v.floor()
                } else {
                    1.0 - (v - v.floor())
                };
            }
            for (c, out) in out.iter_mut().enumerate() {
                *out += self.values[index * self.output + c] * weight;
            }
        }
        out
    }

    pub fn program(
        &self,
        count: usize,
        src_stride: usize,
        dst_stride: usize,
        preserve_alpha: bool,
        clamp: bool,
    ) -> ComputeProgram {
        let mut program = ComputeProgram {
            buffers: vec![self.values.clone()],
            steps: vec![],
            result: ComputeSource::Input(0),
            work: count.saturating_mul(128),
        };
        program.result = program.push(
            &SHADER,
            ComputeSource::Input(0),
            ComputeSource::Input(1),
            vec![
                self.input as f32,
                self.output as f32,
                self.grid as f32,
                src_stride as f32,
                dst_stride as f32,
                u8::from(preserve_alpha) as f32,
                u8::from(clamp) as f32,
            ],
            count * dst_stride,
            [count as u32, 1, dst_stride as u32],
        );
        program
    }
}
