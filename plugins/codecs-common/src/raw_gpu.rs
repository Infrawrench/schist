use schist_fx::{ComputeProgram, ComputeShader, ComputeSource as Source};
use std::sync::OnceLock;
static HISTOGRAM: ComputeShader =
    ComputeShader::workgroup("raw-exposure-histogram", include_str!("raw_histogram.wgsl"));
static ENCODE: ComputeShader =
    ComputeShader::new("raw-exposure-encode", include_str!("raw_encode.wgsl"));
static CURVE: OnceLock<Vec<f32>> = OnceLock::new();

pub(super) fn program(count: usize, exposure: f32) -> Option<ComputeProgram> {
    if count == 0 || count > 16_777_216 {
        return None;
    }
    let groups = count.div_ceil(4096);
    let curve = CURVE.get_or_init(|| {
        (0..=u16::MAX)
            .map(|i| super::srgb_encode(i as f32 / u16::MAX as f32))
            .collect()
    });
    let mut p = ComputeProgram {
        buffers: vec![],
        steps: vec![],
        result: Source::Input(0),
        work: count.saturating_mul(32),
    };
    let hist = p.push(
        &HISTOGRAM,
        Source::Input(0),
        Source::Input(0),
        vec![count as f32],
        groups * 4096,
        [count as u32, 1, 4],
    );
    p.steps.last_mut()?.invocations = groups;
    let sum = p.push(
        &ENCODE,
        hist,
        hist,
        vec![0.0, groups as f32],
        4096,
        [4096, 1, 1],
    );
    let gain = if exposure.is_finite() {
        2.0f32.powf(exposure.clamp(-5.0, 5.0))
    } else {
        1.0
    };
    let selected = p.push(
        &ENCODE,
        sum,
        sum,
        vec![1.0, count as f32, gain],
        1,
        [1, 1, 1],
    );
    // The curve is immutable and lives in the same cached input as other model data.
    let mut args = vec![2.0];
    args.extend(curve);
    p.result = p.push(
        &ENCODE,
        Source::Input(0),
        selected,
        args,
        count * 4,
        [count as u32, 1, 4],
    );
    Some(p)
}
