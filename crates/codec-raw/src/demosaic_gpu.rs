use super::{wrap, CfaPeriod, Quality, DIRECTION_EPSILON, PAD};
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};
static SHADER: ComputeShader =
    ComputeShader::new("raw-demosaic", include_str!("demosaic_gpu.wgsl"));

pub(super) fn run(
    data: &[f32],
    period: &CfaPeriod,
    w: usize,
    h: usize,
    bayer: bool,
    quality: Quality,
) -> Option<Vec<f32>> {
    if !schist_fx::backend().compute_available(w.saturating_mul(h).saturating_mul(100)) {
        return None;
    }
    let pw = w.checked_add(PAD * 2)?;
    let mut out = vec![0.0; w * h * 3];
    // Bound both the source and three-channel output; preserve the full halo.
    let rows = (4_000_000usize / pw).saturating_sub(PAD * 2).max(1).min(h);
    for top in (0..h).step_by(rows) {
        let height = rows.min(h - top);
        let ph = height + PAD * 2;
        // Only upload the contiguous sensor rows required by this band. The
        // shader extends the mosaic by CFA periods without a host padded copy.
        let (mut first, mut last) = (h, 0);
        for y in top..top + ph {
            let row = wrap(y as isize - PAD as isize, h, period.ch);
            first = first.min(row);
            last = last.max(row);
        }
        let input = &data[first * w..(last + 1) * w];
        let p = band_program(period, w, h, top, height, first, bayer, quality)?;
        let band = schist_fx::try_compute(input, &p)?;
        out[top * w * 3..(top + height) * w * 3].copy_from_slice(&band);
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn band_program(
    period: &CfaPeriod,
    w: usize,
    h: usize,
    top: usize,
    height: usize,
    first: usize,
    bayer: bool,
    quality: Quality,
) -> Option<ComputeProgram> {
    static PREPARE: ComputeShader =
        ComputeShader::new("raw-mosaic-padding", include_str!("demosaic_prepare.wgsl"));
    let pw = w.checked_add(PAD * 2)?;
    let ph = height.checked_add(PAD * 2)?;
    let n = pw.checked_mul(ph)?;
    let mut args = vec![
        0.0,
        pw as f32,
        ph as f32,
        DIRECTION_EPSILON,
        period.cw as f32,
        period.ch as f32,
        top as f32,
    ];
    args.extend(period.period.iter().map(|&c| c as f32));
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
    let source = p.push(
        &PREPARE,
        ComputeSource::Input(0),
        ComputeSource::Input(0),
        vec![
            w as f32,
            h as f32,
            top as f32,
            first as f32,
            period.cw as f32,
            period.ch as f32,
        ],
        n,
        [pw as u32, ph as u32, 1],
    );
    for &mode in modes {
        let plane = matches!(mode, 1 | 3 | 4);
        let mut params = args.clone();
        params[0] = mode as f32;
        let auxiliary = if p.steps.len() == 1 {
            source
        } else {
            ComputeSource::Step(p.steps.len() - 1)
        };
        p.steps.push(ComputeStep {
            shader: SHADER,
            source,
            auxiliary,
            params,
            output_len: if plane { n } else { w * height * 3 },
            invocations: if plane { n } else { w * height },
            shape: [w as u32, height as u32, 3],
        });
    }
    p.result = ComputeSource::Step(p.steps.len() - 1);

    Some(p)
}
