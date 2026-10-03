//! Camera Raw.
//!
//! Photoshop's Camera Raw is a whole raw-development pipeline; this is its
//! Basic and Detail panels applied to already-developed pixels, which is
//! also what "Filter ▸ Camera Raw Filter" does to a normal layer.
//!
//! The order matters and is the same one Adobe uses: white balance, then
//! exposure and the tone controls, then presence (clarity, vibrance,
//! saturation), then colour grading, then detail (sharpening, noise
//! reduction), then the vignette last so it darkens the finished image.

use crate::util::{at, gaussian_rgba, luma, put};
use crate::{param, simple_filter};
use schist_i18n::t;
use schist_plugin_api::{FilterParam, FilterPlugin, FilterValues};
use std::sync::Arc;

/// Smooth weighting of a value's membership of a tonal band.
///
/// Highlights and Shadows have to act on their own end of the range and
/// fade out before they reach the other, or they just become Exposure.
fn band(l: f32, centre: f32, width: f32) -> f32 {
    let t = ((l - centre) / width).clamp(-1.0, 1.0);
    let s = 1.0 - t * t;
    s * s
}

simple_filter!(
    CameraRaw,
    "filter.camera_raw",
    t("filter.camera_raw.name"),
    "Camera Raw",
    [
        param(
            "temperature",
            t("filter.camera_raw.param.temperature"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "tint",
            t("filter.camera_raw.param.tint"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param("exposure", t("common.exposure"), -5.0, 5.0, 0.0, " EV"),
        param("contrast", t("common.contrast"), -100.0, 100.0, 0.0, ""),
        param(
            "highlights",
            t("filter.camera_raw.param.highlights"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "shadows",
            t("filter.camera_raw.param.shadows"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "whites",
            t("filter.camera_raw.param.whites"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "blacks",
            t("filter.camera_raw.param.blacks"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "clarity",
            t("filter.camera_raw.param.clarity"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "dehaze",
            t("filter.camera_raw.param.dehaze"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "vibrance",
            t("filter.camera_raw.param.vibrance"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param("saturation", t("common.saturation"), -100.0, 100.0, 0.0, ""),
        param(
            "sharpening",
            t("filter.camera_raw.param.sharpening"),
            0.0,
            150.0,
            0.0,
            ""
        ),
        param(
            "noise",
            t("filter.camera_raw.param.noise"),
            0.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "vignette",
            t("filter.camera_raw.param.vignette"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        // Color grading. The dialog draws the hue and saturation of each
        // region as a wheel rather than as these sliders.
        param(
            "grade_shadows_hue",
            t("filter.camera_raw.param.grade_shadows_hue"),
            0.0,
            360.0,
            0.0,
            "°"
        ),
        param(
            "grade_shadows_sat",
            t("filter.camera_raw.param.grade_shadows_sat"),
            0.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_shadows_lum",
            t("filter.camera_raw.param.grade_shadows_lum"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_midtones_hue",
            t("filter.camera_raw.param.grade_midtones_hue"),
            0.0,
            360.0,
            0.0,
            "°"
        ),
        param(
            "grade_midtones_sat",
            t("filter.camera_raw.param.grade_midtones_sat"),
            0.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_midtones_lum",
            t("filter.camera_raw.param.grade_midtones_lum"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_highlights_hue",
            t("filter.camera_raw.param.grade_highlights_hue"),
            0.0,
            360.0,
            0.0,
            "°"
        ),
        param(
            "grade_highlights_sat",
            t("filter.camera_raw.param.grade_highlights_sat"),
            0.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_highlights_lum",
            t("filter.camera_raw.param.grade_highlights_lum"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_global_hue",
            t("filter.camera_raw.param.grade_global_hue"),
            0.0,
            360.0,
            0.0,
            "°"
        ),
        param(
            "grade_global_sat",
            t("filter.camera_raw.param.grade_global_sat"),
            0.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_global_lum",
            t("filter.camera_raw.param.grade_global_lum"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
        param(
            "grade_blending",
            t("filter.camera_raw.param.grade_blending"),
            0.0,
            100.0,
            50.0,
            ""
        ),
        param(
            "grade_balance",
            t("filter.camera_raw.param.grade_balance"),
            -100.0,
            100.0,
            0.0,
            ""
        ),
    ],
    develop
);

/// The whole Camera Raw pipeline on the CPU, in Adobe's order.
fn develop(px: &mut [f32], w: usize, h: usize, v: &FilterValues) {
    // ---- white balance ----
    let temp = v.get("temperature") / 100.0;
    let tint = v.get("tint") / 100.0;
    if temp != 0.0 || tint != 0.0 {
        for p in px.as_chunks_mut::<4>().0.iter_mut() {
            // Warmer lifts red and drops blue; tint trades green
            // against magenta, which is the other axis of the
            // correction.
            p[0] = (p[0] * (1.0 + temp * 0.35)).clamp(0.0, 1.0);
            p[2] = (p[2] * (1.0 - temp * 0.35)).clamp(0.0, 1.0);
            p[1] = (p[1] * (1.0 - tint * 0.25)).clamp(0.0, 1.0);
            p[0] = (p[0] * (1.0 + tint * 0.12)).clamp(0.0, 1.0);
            p[2] = (p[2] * (1.0 + tint * 0.12)).clamp(0.0, 1.0);
        }
    }

    // ---- exposure and tone ----
    let exposure = 2f32.powf(v.get("exposure"));
    let contrast = v.get("contrast") / 100.0;
    let highlights = v.get("highlights") / 100.0;
    let shadows = v.get("shadows") / 100.0;
    let whites = v.get("whites") / 100.0;
    let blacks = v.get("blacks") / 100.0;
    for p in px.as_chunks_mut::<4>().0.iter_mut() {
        for c in p.iter_mut().take(3) {
            *c = (*c * exposure).clamp(0.0, 1.0);
        }
        let l = luma(p);
        // Each control is a gain applied through its own band, so
        // Shadows leaves the highlights where they are and vice versa.
        let mut gain = 0.0;
        if highlights != 0.0 {
            gain += highlights * 0.5 * band(l, 0.8, 0.45);
        }
        if shadows != 0.0 {
            gain += shadows * 0.5 * band(l, 0.2, 0.45);
        }
        if whites != 0.0 {
            gain += whites * 0.35 * band(l, 1.0, 0.4);
        }
        if blacks != 0.0 {
            gain += blacks * 0.35 * band(l, 0.0, 0.4);
        }
        if gain != 0.0 {
            for c in p.iter_mut().take(3) {
                *c = (*c + gain * (1.0 - *c).max(0.05)).clamp(0.0, 1.0);
            }
        }
        if contrast != 0.0 {
            // S-curve about mid grey.
            let k = 1.0 + contrast;
            for c in p.iter_mut().take(3) {
                *c = ((*c - 0.5) * k + 0.5).clamp(0.0, 1.0);
            }
        }
    }

    // ---- clarity and dehaze: local contrast at two scales ----
    let clarity = v.get("clarity") / 100.0;
    let dehaze = v.get("dehaze") / 100.0;
    for (amount, radius) in [(clarity, 12.0f32), (dehaze, 48.0f32)] {
        if amount == 0.0 {
            continue;
        }
        let mut low = px.to_vec();
        gaussian_rgba(&mut low, w, h, radius);
        for (p, l) in px
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(low.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                // Midtone-weighted so clarity does not blow the
                // highlights or crush the shadows.
                let weight = band(l[c], 0.5, 0.75);
                p[c] = (p[c] + (p[c] - l[c]) * amount * 1.5 * weight).clamp(0.0, 1.0);
            }
        }
    }

    // ---- presence ----
    let vibrance = v.get("vibrance") / 100.0;
    let saturation = v.get("saturation") / 100.0;
    if vibrance != 0.0 || saturation != 0.0 {
        for p in px.as_chunks_mut::<4>().0.iter_mut() {
            let l = luma(p);
            let max = p[0].max(p[1]).max(p[2]);
            let min = p[0].min(p[1]).min(p[2]);
            let sat = max - min;
            // Vibrance leans on the least saturated pixels.
            let amount = saturation + vibrance * (1.0 - sat);
            let k = 1.0 + amount;
            for c in p.iter_mut().take(3) {
                *c = (l + (*c - l) * k).clamp(0.0, 1.0);
            }
        }
    }

    // ---- colour grading, last of the colour controls ----
    crate::color_grading::Grading::from_values(v).apply_rgba(px);

    // ---- detail ----
    let noise = v.get("noise") / 100.0;
    if noise > 0.0 {
        // Edge-preserving average, so grain goes and detail stays.
        let src = px.to_vec();
        let t = (0.06 + 0.12 * (1.0 - noise)).max(1e-3);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let centre = at(&src, w, h, x, y);
                let mut acc = [0.0f32; 4];
                let mut wsum = 0.0;
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let q = at(&src, w, h, x + dx, y + dy);
                        let d = (q[0] - centre[0])
                            .abs()
                            .max((q[1] - centre[1]).abs())
                            .max((q[2] - centre[2]).abs());
                        let k = (1.0 - d / t).max(0.0);
                        for c in 0..4 {
                            acc[c] += q[c] * k;
                        }
                        wsum += k;
                    }
                }
                if wsum > 0.0 {
                    let mut out = centre;
                    for c in 0..3 {
                        out[c] = centre[c] + (acc[c] / wsum - centre[c]) * noise;
                    }
                    put(px, w, x as usize, y as usize, out);
                }
            }
        }
    }
    let sharpening = v.get("sharpening") / 100.0;
    if sharpening > 0.0 {
        let mut low = px.to_vec();
        gaussian_rgba(&mut low, w, h, 1.0);
        for (p, l) in px
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(low.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                p[c] = (p[c] + (p[c] - l[c]) * sharpening).clamp(0.0, 1.0);
            }
        }
    }

    // ---- vignette, last so it darkens the finished image ----
    let vignette = v.get("vignette") / 100.0;
    if vignette != 0.0 {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let max_r = cx.hypot(cy).max(1.0);
        for y in 0..h {
            for x in 0..w {
                let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / max_r;
                // Flat in the middle, falling off towards the corners.
                let falloff = (d * d * d).clamp(0.0, 1.0);
                let k = 1.0 - vignette * falloff;
                let i = (y * w + x) * 4;
                for c in 0..3 {
                    px[i + c] = (px[i + c] * k).clamp(0.0, 1.0);
                }
            }
        }
    }
}

/// The keys [`apply_local`] reads. Exposure is in EV; the rest run
/// -100..=100, with sharpness below zero softening.
pub const LOCAL_KEYS: [&str; 10] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "clarity",
    "dehaze",
    "saturation",
    "sharpness",
];

/// Local sharpness as an unsharp-mask radius and amount. A negative amount
/// moves each pixel towards the blur instead of away from it, which is how
/// a local brush softens skin or a background.
pub(crate) fn local_sharpness(sharpness: f32) -> (f32, f32) {
    if sharpness > 0.0 {
        (1.0, sharpness / 100.0 * 1.5)
    } else {
        (2.0, (sharpness / 100.0).max(-1.0))
    }
}

/// Apply Camera Raw's controls locally: the adjusted picture is blended
/// over `px` by `coverage`, one sample in 0..=1 per pixel.
///
/// The adjustment is the global filter's own pipeline restricted to
/// [`LOCAL_KEYS`] (no whites/blacks, vibrance, noise reduction or
/// vignette, which Lightroom's local panel does not have either). It runs
/// on the GPU when one is available; otherwise only over the bounding box
/// of the coverage plus the widest blur's reach.
pub fn apply_local(px: &mut [f32], w: usize, h: usize, v: &FilterValues, coverage: &[f32]) {
    if w == 0 || h == 0 || px.len() != w * h * 4 || coverage.len() != w * h {
        return;
    }
    if LOCAL_KEYS.iter().all(|k| v.get(k) == 0.0) {
        return;
    }
    if schist_fx::backend().compute_available(w.saturating_mul(h).saturating_mul(160)) {
        let operation = local_operation(v, Arc::from(coverage));
        if crate::gpu::apply_operation(&operation, px, w, h) {
            return;
        }
    }
    apply_local_cpu(px, w, h, v, coverage);
}

/// The GPU form of [`apply_local`], for hosts that run operations
/// asynchronously (the browser).
pub fn local_operation(v: &FilterValues, coverage: Arc<[f32]>) -> schist_fx::FilterOperation {
    crate::gpu_extra::camera_raw_local(v, coverage)
}

/// [`apply_local`] on the CPU only, which the GPU graph is checked against.
pub fn apply_local_cpu(px: &mut [f32], w: usize, h: usize, v: &FilterValues, coverage: &[f32]) {
    if w == 0 || h == 0 || px.len() != w * h * 4 || coverage.len() != w * h {
        return;
    }
    // The bounding box of everything the mask touches.
    let (mut left, mut top, mut right, mut bottom) = (w, h, 0, 0);
    for (y, row) in coverage.chunks(w).enumerate() {
        if let Some(first) = row.iter().position(|c| *c > 0.0) {
            let last = row.iter().rposition(|c| *c > 0.0).unwrap_or(first);
            left = left.min(first);
            right = right.max(last + 1);
            top = top.min(y);
            bottom = y + 1;
        }
    }
    if left >= right || top >= bottom {
        return;
    }
    // Pixels just outside the mask still feed the blurs of the ones inside.
    let reach = if v.get("dehaze") != 0.0 {
        48.0
    } else if v.get("clarity") != 0.0 {
        12.0
    } else if v.get("sharpness") != 0.0 {
        2.0
    } else {
        0.0
    };
    let margin = (reach * 3.0f32).ceil() as usize;
    let (cl, ct) = (left.saturating_sub(margin), top.saturating_sub(margin));
    let (cr, cb) = ((right + margin).min(w), (bottom + margin).min(h));
    let (cw, ch) = (cr - cl, cb - ct);
    let mut crop = Vec::with_capacity(cw * ch * 4);
    for y in ct..cb {
        crop.extend_from_slice(&px[(y * w + cl) * 4..(y * w + cr) * 4]);
    }

    let mut global = FilterValues::default();
    for key in LOCAL_KEYS {
        if key != "sharpness" {
            global.set(key, v.get(key));
        }
    }
    develop(&mut crop, cw, ch, &global);
    let (radius, amount) = local_sharpness(v.get("sharpness"));
    if amount != 0.0 {
        let mut low = crop.clone();
        gaussian_rgba(&mut low, cw, ch, radius);
        for (p, l) in crop
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(low.as_chunks::<4>().0.iter())
        {
            for c in 0..3 {
                p[c] = (p[c] + (p[c] - l[c]) * amount).clamp(0.0, 1.0);
            }
        }
    }

    for y in top..bottom {
        for x in left..right {
            let k = coverage[y * w + x].clamp(0.0, 1.0);
            if k <= 0.0 {
                continue;
            }
            let i = (y * w + x) * 4;
            let j = ((y - ct) * cw + (x - cl)) * 4;
            for c in 0..3 {
                px[i + c] += (crop[j + c] - px[i + c]) * k;
            }
        }
    }
}

pub fn register(registry: &mut schist_plugin_api::PluginRegistry) {
    registry.register_filter(Box::new(CameraRaw));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
                [0.2 + 0.5 * x, 0.3 + 0.3 * y, 0.4, 1.0]
            })
            .collect()
    }

    #[test]
    fn local_adjustment_follows_the_coverage() {
        let (w, h) = (64, 32);
        let original = picture(w, h);
        let mut coverage = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w / 2 {
                coverage[y * w + x] = 1.0;
            }
        }
        coverage[5 * w + 40] = 0.5;
        let mut v = FilterValues::default();
        v.set("exposure", 1.0);
        let mut px = original.clone();
        apply_local_cpu(&mut px, w, h, &v, &coverage);
        let at = |buf: &[f32], x: usize, y: usize| buf[(y * w + x) * 4 + 1];
        assert!(at(&px, 10, 10) > at(&original, 10, 10) * 1.9);
        assert_eq!(at(&px, 50, 10), at(&original, 50, 10), "uncovered");
        let half = at(&px, 40, 5);
        let (lo, hi) = (at(&original, 40, 5), (at(&original, 40, 5) * 2.0).min(1.0));
        assert!(
            (half - (lo + hi) / 2.0).abs() < 1e-5,
            "partial coverage blends"
        );
        assert_eq!(px[3], 1.0, "alpha untouched");
    }

    #[test]
    fn negative_sharpness_softens() {
        let (w, h) = (32, 32);
        let mut px: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = if (i % w + i / w) % 2 == 0 { 0.8 } else { 0.2 };
                [v, v, v, 1.0]
            })
            .collect();
        let before = px[(16 * w + 16) * 4];
        let mut v = FilterValues::default();
        v.set("sharpness", -100.0);
        apply_local_cpu(&mut px, w, h, &v, &vec![1.0; w * h]);
        let after = px[(16 * w + 16) * 4];
        assert!((after - 0.5).abs() < (before - 0.5).abs() * 0.5, "{after}");
    }

    #[test]
    fn neutral_or_empty_masks_change_nothing() {
        let (w, h) = (16, 16);
        let original = picture(w, h);
        let mut px = original.clone();
        let mut v = FilterValues::default();
        apply_local(&mut px, w, h, &v, &vec![1.0; w * h]);
        assert_eq!(px, original);
        v.set("contrast", 50.0);
        apply_local(&mut px, w, h, &v, &vec![0.0; w * h]);
        assert_eq!(px, original);
    }
}
