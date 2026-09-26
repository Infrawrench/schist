//! Native-resolution trimap matting. ViT's context is nonlocal: discard the
//! outer context and blend overlapping predictions, rather than joining tiles.

use crate::Model;
use anyhow::{bail, Context, Result};

const SIDE: usize = 768;
const HALO: usize = 128;
const WINDOW: usize = 512;
const STRIDE: usize = 384;
const RADIUS: usize = 4;
const CORE_RADIUS: usize = 16;
const CORE_EXPANSION: usize = 12;

pub(crate) fn refine(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<f32>> {
    if model.channels() != 4 || model.spec.input.dims() != (SIDE, SIDE) {
        bail!("invalid detail matting model");
    }
    let core_model = crate::get("matting").context("opaque core model unavailable")?;
    let opaque_hint =
        crate::matting::refine_local(&core_model, rgb, coarse, width, height, &mut cancelled)?;
    drop(core_model);
    refine_seeded_with(
        rgb,
        coarse,
        width,
        height,
        Some(&opaque_hint),
        cancelled,
        |planes| {
            let output = model.run_planes(&planes.iter().map(Vec::as_slice).collect::<Vec<_>>())?;
            let view = output[0].to_plain_array_view::<f32>()?;
            if view.shape() != [1, 1, SIDE, SIDE] {
                bail!("unexpected detail matting output shape");
            }
            Ok(view
                .as_slice()
                .context("non-contiguous detail matte")?
                .to_vec())
        },
    )
}

// Admit only broad opaque interiors from the local refiner. Its narrow edge
// clumps never become locked foreground. The detector's known background wins.
fn seed_opaque(
    tri: &mut [u8],
    hint: &[f32],
    width: usize,
    height: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<()> {
    let mut horizontal = vec![false; hint.len()];
    let mut core = vec![false; hint.len()];
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let row = &hint[y * width..(y + 1) * width];
            horizontal[y * width + x] = row
                [x.saturating_sub(CORE_RADIUS)..=(x + CORE_RADIUS).min(width - 1)]
                .iter()
                .all(|&a| a > 0.98);
        }
    }
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let i = y * width + x;
            if (y.saturating_sub(CORE_RADIUS)..=(y + CORE_RADIUS).min(height - 1))
                .all(|sy| horizontal[sy * width + x])
            {
                core[i] = true;
            }
        }
    }
    // Expansion smaller than erosion stays within the original opaque hint,
    // keeping four pixels of unknown boundary for the detail model to solve.
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            horizontal[y * width + x] = core[y * width + x.saturating_sub(CORE_EXPANSION)
                ..=y * width + (x + CORE_EXPANSION).min(width - 1)]
                .contains(&true);
        }
    }
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let i = y * width + x;
            if tri[i] != 0
                && (y.saturating_sub(CORE_EXPANSION)..=(y + CORE_EXPANSION).min(height - 1))
                    .any(|sy| horizontal[sy * width + x])
            {
                tri[i] = 2;
            }
        }
    }
    Ok(())
}

// 0 = known background, 1 = unknown, 2 = known foreground. Shrink confidence
// regions in both axes, including around small gaps and at the image boundary.
fn trimap(
    coarse: &[f32],
    width: usize,
    height: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>> {
    let mut horizontal = vec![[0.0f32; 2]; coarse.len()];
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let mut low = 1.0f32;
            let mut high = 0.0f32;
            for sx in x.saturating_sub(RADIUS)..=(x + RADIUS).min(width - 1) {
                low = low.min(coarse[y * width + sx]);
                high = high.max(coarse[y * width + sx]);
            }
            horizontal[y * width + x] = [low, high];
        }
    }
    let mut result = vec![1; coarse.len()];
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let mut low = 1.0f32;
            let mut high = 0.0f32;
            for sy in y.saturating_sub(RADIUS)..=(y + RADIUS).min(height - 1) {
                let range = horizontal[sy * width + x];
                low = low.min(range[0]);
                high = high.max(range[1]);
            }
            result[y * width + x] = if high < 0.02 {
                0
            } else if low > 0.98 {
                2
            } else {
                1
            };
        }
    }
    Ok(result)
}

fn refine_seeded_with(
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
    opaque_hint: Option<&[f32]>,
    mut cancelled: impl FnMut() -> bool,
    mut predict: impl FnMut(&[Vec<f32>]) -> Result<Vec<f32>>,
) -> Result<Vec<f32>> {
    let n = width
        .checked_mul(height)
        .context("matting dimensions overflow")?;
    if n == 0 || n > 16_777_216 || n.checked_mul(3) != Some(rgb.len()) || coarse.len() != n {
        bail!("invalid detail matting dimensions");
    }
    if rgb.iter().chain(coarse).any(|v| !v.is_finite()) {
        bail!("non-finite detail matting input");
    }
    let mut tri = trimap(coarse, width, height, &mut cancelled)?;
    if let Some(hint) = opaque_hint {
        if hint.len() != n || hint.iter().any(|a| !a.is_finite()) {
            bail!("invalid opaque matting hint");
        }
        seed_opaque(&mut tri, hint, width, height, &mut cancelled)?;
    }
    let ramp: Vec<f32> = (0..WINDOW)
        .map(|x| {
            let x = x as f32 + 0.5;
            (x.min(WINDOW as f32 - x) / (WINDOW - STRIDE) as f32).min(1.0)
        })
        .collect();
    let mut result = vec![0.0f32; n];
    let mut weights = vec![0.0f32; n];
    let mut planes = vec![vec![0.0f32; SIDE * SIDE]; 4];
    for top in (0..height).step_by(STRIDE) {
        let bh = WINDOW.min(height - top);
        for left in (0..width).step_by(STRIDE) {
            if cancelled() {
                bail!("matting cancelled");
            }
            let bw = WINDOW.min(width - left);
            let unknown =
                (top..top + bh).any(|y| tri[y * width + left..y * width + left + bw].contains(&1));
            let output = if unknown {
                for y in 0..SIDE {
                    let sy = (top + y).saturating_sub(HALO).min(height - 1);
                    for x in 0..SIDE {
                        let sx = (left + x).saturating_sub(HALO).min(width - 1);
                        let src = sy * width + sx;
                        let dst = y * SIDE + x;
                        for c in 0..3 {
                            planes[c][dst] = rgb[src * 3 + c].clamp(0.0, 1.0);
                        }
                        planes[3][dst] = tri[src] as f32 * 0.5;
                    }
                }
                let output = predict(&planes)?;
                if output.len() != SIDE * SIDE || output.iter().any(|a| !a.is_finite()) {
                    bail!("invalid detail matting output");
                }
                Some(output)
            } else {
                None
            };
            for y in 0..bh {
                for x in 0..bw {
                    let i = (top + y) * width + left + x;
                    let value = output.as_ref().map_or(tri[i] as f32 * 0.5, |a| {
                        a[(y + HALO) * SIDE + x + HALO].clamp(0.0, 1.0)
                    });
                    let weight = ramp[y] * ramp[x];
                    result[i] += value * weight;
                    weights[i] += weight;
                }
            }
        }
    }
    for i in 0..n {
        result[i] = if tri[i] == 1 {
            result[i] / weights[i]
        } else {
            tri[i] as f32 * 0.5
        };
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refine_with(
        rgb: &[f32],
        coarse: &[f32],
        width: usize,
        height: usize,
        cancelled: impl FnMut() -> bool,
        predict: impl FnMut(&[Vec<f32>]) -> Result<Vec<f32>>,
    ) -> Result<Vec<f32>> {
        refine_seeded_with(rgb, coarse, width, height, None, cancelled, predict)
    }

    #[test]
    fn opaque_cores_keep_sleeves_without_locking_thin_edge_clumps() {
        let (w, h) = (121, 81);
        let mut hint = vec![0.0; w * h];
        for y in 5..76 {
            for x in 60..115 {
                hint[y * w + x] = 1.0;
            }
        }
        for y in 20..28 {
            for x in 10..50 {
                hint[y * w + x] = 1.0;
            }
        }
        let mut tri = vec![1; w * h];
        tri[40 * w + 90] = 0;
        seed_opaque(&mut tri, &hint, w, h, &mut || false).unwrap();
        assert_eq!(tri[40 * w + 86], 2);
        assert_eq!(tri[24 * w + 30], 1);
        assert_eq!(tri[40 * w + 90], 0);
        assert!(tri.iter().zip(hint).all(|(&t, a)| t != 2 || a > 0.98));
    }

    #[test]
    fn trimap_protects_known_regions_and_leaves_strands_unknown() {
        let mut a = vec![0.0; 41 * 17];
        for y in 0..17 {
            for x in 20..41 {
                a[y * 41 + x] = 1.0;
            }
            a[y * 41 + 8] = 0.6;
        }
        let t = trimap(&a, 41, 17, &mut || false).unwrap();
        assert_eq!(t[8 * 41], 0);
        assert_eq!(t[8 * 41 + 8], 1);
        assert_eq!(t[8 * 41 + 16], 1);
        assert_eq!(t[8 * 41 + 24], 2);
    }

    #[test]
    fn known_pixels_are_exact_and_do_not_require_inference() {
        for a in [0.0, 1.0] {
            let coarse = vec![a; 901 * 13];
            let result = refine_with(
                &vec![0.3; coarse.len() * 3],
                &coarse,
                901,
                13,
                || false,
                |_| panic!("known window invoked model"),
            )
            .unwrap();
            assert_eq!(result, coarse);
        }
    }

    #[test]
    fn windows_reconstruct_soft_strands_at_seams_and_partial_edges() {
        let (w, h) = (901, 27);
        let expected: Vec<f32> = (0..w * h)
            .map(|i| ((i % w) as f32 * 0.1).sin() * 0.4 + 0.5)
            .collect();
        let rgb: Vec<f32> = expected.iter().flat_map(|a| [*a, 0.2, 0.4]).collect();
        let result = refine_with(
            &rgb,
            &vec![0.5; w * h],
            w,
            h,
            || false,
            |planes| Ok(planes[0].clone()),
        )
        .unwrap();
        assert!(result
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 2e-7));
    }

    #[test]
    fn context_disagreement_is_blended_without_hard_tile_joins() {
        let (w, h) = (901, 1);
        let mut call = 0;
        let result = refine_with(
            &vec![0.5; w * 3],
            &vec![0.5; w],
            w,
            h,
            || false,
            |_| {
                let value = (call % 2) as f32;
                call += 1;
                Ok(vec![value; SIDE * SIDE])
            },
        )
        .unwrap();
        assert!(result.windows(2).all(|p| (p[1] - p[0]).abs() < 0.008));
        assert!(result[440] > 0.3 && result[440] < 0.7);
    }

    #[test]
    fn invalid_inputs_outputs_and_cancellation_do_not_produce_mattes() {
        assert!(refine_with(&[], &[], 0, 0, || false, |_| unreachable!()).is_err());
        assert!(refine_with(
            &[0.0; 3],
            &[0.5],
            usize::MAX,
            2,
            || false,
            |_| unreachable!()
        )
        .is_err());
        assert!(refine_with(&[f32::NAN; 3], &[0.5], 1, 1, || false, |_| unreachable!()).is_err());
        assert!(refine_with(&[0.0; 3], &[0.5], 1, 1, || true, |_| unreachable!()).is_err());
        for output in [vec![], vec![f32::NAN; SIDE * SIDE]] {
            assert!(
                refine_with(&[0.0; 3], &[0.5], 1, 1, || false, |_| Ok(output.clone())).is_err()
            );
        }
    }
}
