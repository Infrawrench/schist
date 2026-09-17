//! Exact clipped-percentile automatic corrections, shared by native and web hosts.
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, FilterOperation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AutoMode {
    Tone,
    Contrast,
    Color,
}

static HISTOGRAM: ComputeShader =
    ComputeShader::workgroup("auto-radix-histogram", include_str!("auto_histogram.wgsl"));
static SELECT: ComputeShader =
    ComputeShader::new("auto-radix-select", include_str!("auto_select.wgsl"));
static MEAN: ComputeShader = ComputeShader::workgroup(
    "auto-clipped-mean",
    concat!(
        include_str!("auto_common.wgsl"),
        include_str!("auto_mean.wgsl")
    ),
);
static FINISH: ComputeShader = ComputeShader::new(
    "auto-coefficients",
    concat!(
        include_str!("auto_common.wgsl"),
        include_str!("auto_finish.wgsl")
    ),
);
static APPLY: ComputeShader = ComputeShader::new("auto-apply", include_str!("auto_apply.wgsl"));

pub fn operation(mode: AutoMode) -> FilterOperation {
    FilterOperation::Program {
        build,
        params: vec![mode as u8 as f32],
        work_per_pixel: 128,
    }
}

pub fn program(pixels: usize, mode: AutoMode) -> Option<ComputeProgram> {
    build(pixels, 1, &[mode as u8 as f32])
}

fn build(w: usize, h: usize, args: &[f32]) -> Option<ComputeProgram> {
    let count = w.checked_mul(h)?;
    if count == 0 || count > 16_777_216 || args.len() != 1 || !(0.0..=2.0).contains(&args[0]) {
        return None;
    }
    let blocks = count.div_ceil(4096);
    let input = ComputeSource::Input(0);
    let mut p = ComputeProgram {
        buffers: vec![],
        steps: vec![],
        result: input,
        work: count.saturating_mul(128),
    };
    let mut selected = input;
    for pass in 0..4 {
        let histogram = p.push(
            &HISTOGRAM,
            input,
            selected,
            vec![pass as f32],
            blocks * 1536,
            [count as u32, 1, 4],
        );
        p.steps.last_mut()?.invocations = blocks;
        selected = p.push(
            &SELECT,
            histogram,
            selected,
            vec![pass as f32],
            24,
            [blocks as u32, 1, 1],
        );
        p.steps.last_mut()?.invocations = 6;
    }
    let means = if args[0] == 2.0 {
        let result = p.push(
            &MEAN,
            input,
            selected,
            args.to_vec(),
            blocks * 4,
            [count as u32, 1, 4],
        );
        p.steps.last_mut()?.invocations = blocks;
        result
    } else {
        input
    };
    let coefficients = p.push(
        &FINISH,
        means,
        selected,
        args.to_vec(),
        9,
        [blocks as u32, 1, 1],
    );
    p.steps.last_mut()?.invocations = 3;
    p.result = p.push(
        &APPLY,
        input,
        coefficients,
        vec![],
        count * 4,
        [count as u32, 1, 4],
    );
    p.steps.last_mut()?.invocations = count;
    Some(p)
}

/// Returns false when there are no visible pixels to correct.
pub fn apply(pixels: &mut [f32], mode: AutoMode) -> bool {
    if !pixels.len().is_multiple_of(4) || !pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0.0) {
        return false;
    }
    if let Some(program) = program(pixels.len() / 4, mode) {
        if let Some(out) = schist_fx::try_compute(pixels, &program) {
            pixels.copy_from_slice(&out);
            return true;
        }
    }
    apply_cpu(pixels, mode)
}

/// CPU reference preserves exact order statistics, including signed zero.
pub fn apply_cpu(pixels: &mut [f32], mode: AutoMode) -> bool {
    // Photoshop clips half a percent off each end so a handful of
    // stray pixels cannot flatten the whole stretch.
    const CLIP: f32 = 0.005;
    let mut lo = [1.0f32; 3];
    let mut hi = [0.0f32; 3];
    for ch in 0..3 {
        let mut vals: Vec<f32> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0.0)
            .map(|p| p[ch])
            .collect();
        if vals.is_empty() {
            return false;
        }
        vals.sort_by(|a, b| a.total_cmp(b));
        let n = vals.len();
        lo[ch] = vals[((n as f32 * CLIP) as usize).min(n - 1)];
        hi[ch] = vals[((n as f32 * (1.0 - CLIP)) as usize).min(n - 1)];
    }
    if mode == AutoMode::Contrast {
        // One stretch for all three channels keeps the colour cast.
        let l = lo[0].min(lo[1]).min(lo[2]);
        let h = hi[0].max(hi[1]).max(hi[2]);
        lo = [l; 3];
        hi = [h; 3];
    }
    // Auto Color additionally pulls each channel's midtone to neutral
    // grey, which is the only thing distinguishing it from Auto Tone.
    // This used to be `v.powf(1.0)`, the identity, so the two menu
    // items produced byte-identical results.
    let mut gamma = [1.0f32; 3];
    if mode == AutoMode::Color {
        let mut sum = [0.0f64; 3];
        let mut n = 0u64;
        for p in pixels.as_chunks::<4>().0 {
            if p[3] <= 0.0 {
                continue;
            }
            for ch in 0..3 {
                let span = (hi[ch] - lo[ch]).max(1e-4);
                sum[ch] += f64::from(((p[ch] - lo[ch]) / span).clamp(0.0, 1.0));
            }
            n += 1;
        }
        if n > 0 {
            for ch in 0..3 {
                let mean = (sum[ch] / n as f64) as f32;
                // Solve mean^gamma = 0.5 for gamma, so the channel's
                // midtone lands on neutral grey. Clamped so a nearly
                // black or white channel cannot explode.
                if mean > 1e-3 && mean < 1.0 - 1e-3 {
                    gamma[ch] = (0.5f32.ln() / mean.ln()).clamp(0.2, 5.0);
                }
            }
        }
    }
    for p in pixels.as_chunks_mut::<4>().0 {
        if p[3] <= 0.0 {
            continue;
        }
        for ch in 0..3 {
            let span = (hi[ch] - lo[ch]).max(1e-4);
            let mut v = ((p[ch] - lo[ch]) / span).clamp(0.0, 1.0);
            if gamma[ch] != 1.0 {
                v = v.powf(gamma[ch]);
            }
            p[ch] = v;
        }
    }
    true
}
