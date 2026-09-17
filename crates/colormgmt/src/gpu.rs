//! Matrix/TRC RGB profiles, including sRGB/linear CICP transfer metadata.
//! CLUT profiles and other CICP transfers retain the CMS executor.
use moxcms::{ColorProfile, DataColorSpace, ToneReprCurve};
use schist_fx::{ComputeProgram, ComputeShader};
static SHADER: ComputeShader = ComputeShader {
    name: "icc-matrix-trc",
    source: include_str!("gpu.wgsl"),
};

pub(super) fn coefficients(src: &ColorProfile, dst: &ColorProfile) -> Option<Vec<f32>> {
    for p in [src, dst] {
        if p.color_space != DataColorSpace::Rgb
            || p.pcs != DataColorSpace::Xyz
            || p.cicp.as_ref().is_some_and(|c| {
                !matches!(
                    c.transfer_characteristics,
                    moxcms::TransferCharacteristics::Srgb | moxcms::TransferCharacteristics::Linear
                )
            })
            || p.lut_a_to_b_perceptual.is_some()
            || p.lut_a_to_b_colorimetric.is_some()
            || p.lut_a_to_b_saturation.is_some()
            || p.lut_b_to_a_perceptual.is_some()
            || p.lut_b_to_a_colorimetric.is_some()
            || p.lut_b_to_a_saturation.is_some()
        {
            return None;
        }
    }
    let mut out: Vec<f32> = src
        .transform_matrix(dst)
        .v
        .into_iter()
        .flatten()
        .map(|v| v as f32)
        .collect();
    out.resize(15, 0.0);
    let curves = [
        src.red_trc.as_ref()?,
        src.green_trc.as_ref()?,
        src.blue_trc.as_ref()?,
        dst.red_trc.as_ref()?,
        dst.green_trc.as_ref()?,
        dst.blue_trc.as_ref()?,
    ];
    for (i, curve) in curves.into_iter().enumerate() {
        out[9 + i] = out.len() as f32;
        let profile = if i < 3 { src } else { dst };
        let evaluator = match profile.cicp.as_ref() {
            Some(c) if i < 3 => {
                ToneReprCurve::make_cicp_linear_evaluator(c.transfer_characteristics).ok()?
            }
            Some(c) => ToneReprCurve::make_cicp_gamma_evaluator(c.transfer_characteristics).ok()?,
            None if i < 3 => curve.make_linear_evaluator().ok()?,
            None => curve.make_gamma_evaluator().ok()?,
        };
        // Match the CMS float executor's table domains, including its input truncation.
        let len = if i < 3 { 16384 } else { 32768 };
        out.extend_from_slice(&[len as f32]);
        out.extend((0..len).map(|n| {
            evaluator
                .evaluate_value(n as f32 / (len - 1) as f32)
                .clamp(0.0, 1.0)
        }));
    }
    out.iter().all(|v| v.is_finite()).then_some(out)
}
pub(super) fn program(params: &[f32], len: usize) -> ComputeProgram {
    let mut p = ComputeProgram::single(
        &SHADER,
        params.to_vec(),
        len,
        [(len / 4) as u32, 1, 4],
        len.saturating_mul(24),
    );
    p.steps[0].invocations = len / 4;
    p
}
