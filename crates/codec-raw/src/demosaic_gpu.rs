use super::{Mosaic, Quality, PAD};
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};
static SHADER: ComputeShader = ComputeShader {
    name: "raw-demosaic",
    source: include_str!("demosaic_gpu.wgsl"),
};

pub(super) fn run(
    m: &Mosaic,
    w: usize,
    h: usize,
    bayer: bool,
    quality: Quality,
) -> Option<Vec<f32>> {
    if !schist_fx::backend().compute_available(w.saturating_mul(h).saturating_mul(100)) {
        return None;
    }
    let mut out = vec![0.0; w * h * 3];
    // Bound both the source and three-channel output; preserve the full halo.
    let rows = (4_000_000usize / m.pw)
        .saturating_sub(PAD * 2)
        .max(1)
        .min(h);
    for top in (0..h).step_by(rows) {
        let height = rows.min(h - top);
        let ph = height + PAD * 2;
        let n = m.pw * ph;
        let data = &m.plane[top * m.pw..(top + ph) * m.pw];
        let mut args = vec![0.0, m.pw as f32, ph as f32, 0.0, 0.0, 0.0];
        args.extend(
            m.codes[top * m.pw..(top + ph) * m.pw]
                .iter()
                .map(|&c| c as f32),
        );
        let modes: &[u32] = match (bayer, quality) {
            (true, Quality::Fast) => &[0],
            (true, Quality::Best) => &[1, 2],
            (false, Quality::Fast) => &[6],
            (false, Quality::Best) => &[3, 4, 5],
        };
        let mut p = ComputeProgram {
            buffers: vec![],
            steps: vec![],
            result: ComputeSource::Input(0),
            // The tail band belongs to the same already eligible image job.
            work: w.saturating_mul(h).saturating_mul(100),
        };
        for &mode in modes {
            let plane = matches!(mode, 1 | 3 | 4);
            let mut params = args.clone();
            params[0] = mode as f32;
            let auxiliary = if p.steps.is_empty() {
                ComputeSource::Input(0)
            } else {
                ComputeSource::Step(p.steps.len() - 1)
            };
            p.steps.push(ComputeStep {
                shader: &SHADER,
                source: ComputeSource::Input(0),
                auxiliary,
                params,
                output_len: if plane { n } else { w * height * 3 },
                invocations: if plane { n } else { w * height },
                shape: [w as u32, height as u32, 3],
            });
        }
        p.result = ComputeSource::Step(p.steps.len() - 1);
        let band = schist_fx::try_compute(data, &p)?;
        out[top * w * 3..(top + height) * w * 3].copy_from_slice(&band);
    }
    Some(out)
}
