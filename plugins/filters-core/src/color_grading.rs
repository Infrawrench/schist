//! Camera Raw's colour grading wheels.
//!
//! Lightroom's Color Grading panel tints the shadows, midtones and
//! highlights separately, plus the whole image, with a hue, a saturation
//! (how strong the tint is) and a luminance shift for each, and two
//! controls shaping the regions: Blending (how far they overlap) and
//! Balance (where the split between shadows and highlights falls). This is
//! our own construction of that idea, not Adobe's algorithm:
//!
//! - a pixel's region weights come from its luma, raised to
//!   `2^(-balance/100)` so a positive balance widens the highlights, and
//!   split at a third and two thirds by smoothsteps whose width grows
//!   with Blending; the midtone weight is what the other two leave;
//! - each wheel's tint is its hue at full saturation with the luma taken
//!   out, so tinting shifts colour without brightening, scaled by the
//!   wheel's saturation; the weighted tints are added together;
//! - each luminance shift lifts towards white or lowers towards black in
//!   proportion to the headroom left, so it cannot clip.
//!
//! Every operation is per pixel, which is also what lets a document's
//! grading be baked into a 3D LUT.

use crate::util::luma;
use schist_fx::{ComputeEntry, ComputeShader};
use schist_plugin_api::FilterValues;

/// The tint a fully saturated wheel adds, as a fraction of full scale.
const TINT_STRENGTH: f32 = 0.3;
/// How far a full luminance shift moves towards white or black.
const LUM_STRENGTH: f32 = 0.35;

pub(crate) static GRADING: ComputeShader = ComputeShader {
    name: "color_grading",
    source: include_str!("shaders/color_grading.wgsl"),
    entry: ComputeEntry::Rgba,
};

/// Filter value keys for each wheel (shadows, midtones, highlights,
/// global): hue, saturation, luminance.
pub const WHEEL_KEYS: [[&str; 3]; 4] = [
    [
        "grade_shadows_hue",
        "grade_shadows_sat",
        "grade_shadows_lum",
    ],
    [
        "grade_midtones_hue",
        "grade_midtones_sat",
        "grade_midtones_lum",
    ],
    [
        "grade_highlights_hue",
        "grade_highlights_sat",
        "grade_highlights_lum",
    ],
    ["grade_global_hue", "grade_global_sat", "grade_global_lum"],
];
pub const BLENDING_KEY: &str = "grade_blending";
pub const BALANCE_KEY: &str = "grade_balance";

/// Whether a filter value belongs to the grading wheels.
pub fn is_grading_key(key: &str) -> bool {
    key.starts_with("grade_")
}

/// The wheels compiled for a run over pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Grading {
    /// Half the width of each region boundary, in luma.
    edge: f32,
    /// Exponent applied to luma before splitting it into regions.
    exponent: f32,
    /// Per wheel (shadows, midtones, highlights, global): RGB tint.
    tints: [[f32; 3]; 4],
    /// Per wheel: luminance shift, -1..=1 scaled by `LUM_STRENGTH`.
    lums: [f32; 4],
}

impl Grading {
    pub fn from_values(v: &FilterValues) -> Grading {
        let mut tints = [[0.0; 3]; 4];
        let mut lums = [0.0; 4];
        for (i, [hue, sat, lum]) in WHEEL_KEYS.iter().enumerate() {
            let strength = (v.get(sat) / 100.0).clamp(0.0, 1.0) * TINT_STRENGTH;
            tints[i] = tint(v.get(hue)).map(|c| c * strength);
            lums[i] = (v.get(lum) / 100.0).clamp(-1.0, 1.0) * LUM_STRENGTH;
        }
        let blending = (v.get(BLENDING_KEY) / 100.0).clamp(0.0, 1.0);
        let balance = (v.get(BALANCE_KEY) / 100.0).clamp(-1.0, 1.0);
        Grading {
            edge: 0.02 + blending / 3.0,
            exponent: (-balance).exp2(),
            tints,
            lums,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.lums.iter().all(|l| *l == 0.0) && self.tints.iter().flatten().all(|c| *c == 0.0)
    }

    /// Shadow, midtone, highlight and global weights for a luma.
    fn weights(&self, l: f32) -> [f32; 4] {
        let t = l.clamp(0.0, 1.0).powf(self.exponent);
        let mut shadows = 1.0 - smoothstep(1.0 / 3.0 - self.edge, 1.0 / 3.0 + self.edge, t);
        let mut highlights = smoothstep(2.0 / 3.0 - self.edge, 2.0 / 3.0 + self.edge, t);
        // Wide blending lets the two ends overlap in the middle; share
        // rather than double the effect there.
        let sum = shadows + highlights;
        if sum > 1.0 {
            shadows /= sum;
            highlights /= sum;
        }
        [shadows, 1.0 - shadows - highlights, highlights, 1.0]
    }

    pub fn apply(&self, p: &mut [f32]) {
        let w = self.weights(luma(p));
        let mut lum = 0.0;
        let mut shift = [0.0f32; 3];
        for ((weight, l), tint) in w.iter().zip(self.lums).zip(self.tints) {
            lum += weight * l;
            for (s, t) in shift.iter_mut().zip(tint) {
                *s += weight * t;
            }
        }
        for c in 0..3 {
            let v = p[c];
            let lifted = if lum >= 0.0 {
                v + lum * (1.0 - v)
            } else {
                v + lum * v
            };
            p[c] = (lifted + shift[c]).clamp(0.0, 1.0);
        }
    }

    pub fn apply_rgba(&self, px: &mut [f32]) {
        if self.is_identity() {
            return;
        }
        for p in px.as_chunks_mut::<4>().0.iter_mut() {
            self.apply(p);
        }
    }

    /// Arguments for `color_grading.wgsl`, in the order it reads them.
    pub fn coeffs(&self) -> Vec<f32> {
        let mut out = vec![self.edge, self.exponent];
        out.extend(self.tints.iter().flatten());
        out.extend(self.lums);
        out
    }
}

/// A hue's colour at full saturation with its luma removed: the direction
/// a tint pushes in, leaving brightness where it was.
fn tint(hue: f32) -> [f32; 3] {
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let rgb = match h as u32 {
        0 => [1.0, x, 0.0],
        1 => [x, 1.0, 0.0],
        2 => [0.0, 1.0, x],
        3 => [0.0, x, 1.0],
        4 => [x, 0.0, 1.0],
        _ => [1.0, 0.0, x],
    };
    let l = luma(&rgb);
    rgb.map(|c| c - l)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&'static str, f32)]) -> FilterValues {
        let mut v = FilterValues::default();
        v.set(BLENDING_KEY, 50.0);
        for (k, value) in pairs {
            v.set(k, *value);
        }
        v
    }

    fn graded(g: &Grading, rgb: [f32; 3]) -> [f32; 3] {
        let mut p = [rgb[0], rgb[1], rgb[2], 1.0];
        g.apply(&mut p);
        [p[0], p[1], p[2]]
    }

    #[test]
    fn neutral_wheels_change_nothing() {
        let g = Grading::from_values(&values(&[(BALANCE_KEY, 40.0)]));
        assert!(g.is_identity());
        for rgb in [[0.1, 0.5, 0.9], [0.0; 3], [1.0; 3]] {
            assert_eq!(graded(&g, rgb), rgb);
        }
    }

    #[test]
    fn shadow_tint_stays_in_the_shadows() {
        // Blue shadows (hue 240).
        let g = Grading::from_values(&values(&[
            ("grade_shadows_hue", 240.0),
            ("grade_shadows_sat", 100.0),
        ]));
        let dark = graded(&g, [0.1; 3]);
        assert!(dark[2] > dark[0] + 0.1, "shadow not blued: {dark:?}");
        let light = graded(&g, [0.9; 3]);
        assert!(
            (light[2] - light[0]).abs() < 1e-3,
            "highlight tinted: {light:?}"
        );
        // Tinting moves colour, not brightness.
        assert!((luma(&dark) - 0.1).abs() < 0.03, "{dark:?}");
    }

    #[test]
    fn global_tint_reaches_everything() {
        let g = Grading::from_values(&values(&[
            ("grade_global_hue", 30.0),
            ("grade_global_sat", 60.0),
        ]));
        for v in [0.05, 0.5, 0.95] {
            let out = graded(&g, [v; 3]);
            assert!(out[0] > out[2], "{v}: {out:?}");
        }
    }

    #[test]
    fn luminance_lifts_without_clipping() {
        let g = Grading::from_values(&values(&[("grade_highlights_lum", 100.0)]));
        let out = graded(&g, [0.95; 3]);
        assert!(out[0] > 0.95 && out[0] <= 1.0, "{out:?}");
        let low = graded(&g, [0.05; 3]);
        assert!((low[0] - 0.05).abs() < 1e-3, "shadows moved: {low:?}");
    }

    #[test]
    fn balance_moves_the_split() {
        let at = |balance: f32| {
            let g = Grading::from_values(&values(&[
                ("grade_highlights_hue", 0.0),
                ("grade_highlights_sat", 100.0),
                (BALANCE_KEY, balance),
            ]));
            let out = graded(&g, [0.5; 3]);
            out[0] - out[2]
        };
        assert!(at(100.0) > at(0.0) && at(0.0) > at(-100.0));
    }

    #[test]
    fn blending_widens_the_regions() {
        let reach = |blending: f32| {
            let g = Grading::from_values(&values(&[
                ("grade_shadows_hue", 120.0),
                ("grade_shadows_sat", 100.0),
                (BLENDING_KEY, blending),
            ]));
            let out = graded(&g, [0.55; 3]);
            out[1] - out[0]
        };
        assert!(reach(100.0) > reach(0.0) + 0.01);
        assert!(reach(0.0).abs() < 1e-4);
    }

    #[test]
    fn weights_partition_the_tones() {
        for blending in [0.0, 50.0, 100.0] {
            let g = Grading::from_values(&values(&[(BLENDING_KEY, blending)]));
            for i in 0..=20 {
                let w = g.weights(i as f32 / 20.0);
                assert!(w[..3].iter().all(|x| *x >= -1e-6));
                assert!((w[0] + w[1] + w[2] - 1.0).abs() < 1e-5);
            }
        }
    }
}
