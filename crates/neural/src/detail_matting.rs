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

// Stop once the last central window covers the image boundary. Advancing all
// the way to `length` adds a mostly padded tile when the preceding window
// already covers the edge, without extending the image's coverage.
fn tile_starts(length: usize) -> impl Iterator<Item = usize> {
    (0..=length.saturating_sub(WINDOW).div_ceil(STRIDE)).map(|i| i * STRIDE)
}

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
    let confident: Vec<bool> = hint.iter().map(|&a| a > 0.98).collect();
    let core = binary_window(&confident, width, height, CORE_RADIUS, true, cancelled)?;
    drop(confident);
    // Expansion smaller than erosion stays within the original opaque hint,
    // keeping four pixels of unknown boundary for the detail model to solve.
    let expanded = binary_window(&core, width, height, CORE_EXPANSION, false, cancelled)?;
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        for x in 0..width {
            let i = y * width + x;
            if tri[i] != 0 && expanded[i] {
                tri[i] = 2;
            }
        }
    }
    Ok(())
}

// A running count gives exactly the same clipped all/any box windows as the
// scalar morphology, in O(pixels) rather than O(pixels * radius). Each row
// remains a cancellation boundary, including the vertical pass.
fn binary_window(
    source: &[bool],
    width: usize,
    height: usize,
    radius: usize,
    erode: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<bool>> {
    let mut horizontal = vec![false; source.len()];
    let decide = |count, length| if erode { count == length } else { count > 0 };
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        let row = &source[y * width..(y + 1) * width];
        let mut count: usize = row[..(radius + 1).min(width)]
            .iter()
            .map(|&v| usize::from(v))
            .sum();
        for x in 0..width {
            let length = (x + radius + 1).min(width) - x.saturating_sub(radius);
            horizontal[y * width + x] = decide(count, length);
            if x >= radius {
                count -= usize::from(row[x - radius]);
            }
            if x + radius + 1 < width {
                count += usize::from(row[x + radius + 1]);
            }
        }
    }
    let mut counts = vec![0usize; width];
    for row in horizontal
        .chunks_exact(width)
        .take((radius + 1).min(height))
    {
        for (count, &v) in counts.iter_mut().zip(row) {
            *count += usize::from(v);
        }
    }
    let mut result = vec![false; source.len()];
    for y in 0..height {
        if cancelled() {
            bail!("matting cancelled");
        }
        let length = (y + radius + 1).min(height) - y.saturating_sub(radius);
        for x in 0..width {
            result[y * width + x] = decide(counts[x], length);
        }
        if y >= radius {
            let row = &horizontal[(y - radius) * width..(y - radius + 1) * width];
            for (count, &v) in counts.iter_mut().zip(row) {
                *count -= usize::from(v);
            }
        }
        if y + radius + 1 < height {
            let row = &horizontal[(y + radius + 1) * width..(y + radius + 2) * width];
            for (count, &v) in counts.iter_mut().zip(row) {
                *count += usize::from(v);
            }
        }
    }
    Ok(result)
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
    for top in tile_starts(height) {
        let bh = WINDOW.min(height - top);
        for left in tile_starts(width) {
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

    #[test]
    fn running_binary_windows_match_scalar_boxes_at_clipped_edges() {
        for (w, h) in [(1, 1), (1, 19), (23, 1), (7, 13), (65, 47)] {
            for pattern in 0..3 {
                let input: Vec<bool> = (0..w * h)
                    .map(|i| match pattern {
                        0 => false,
                        1 => true,
                        _ => (i * 37 + i / w * 19) % 101 > 11,
                    })
                    .collect();
                for radius in [0, 1, 4, 12, 16, 128] {
                    for erode in [false, true] {
                        let actual =
                            binary_window(&input, w, h, radius, erode, &mut || false).unwrap();
                        for y in 0..h {
                            for x in 0..w {
                                let mut values = (y.saturating_sub(radius)
                                    ..=(y + radius).min(h - 1))
                                    .flat_map(|sy| {
                                        (x.saturating_sub(radius)..=(x + radius).min(w - 1))
                                            .map(move |sx| sy * w + sx)
                                    })
                                    .map(|i| input[i]);
                                let expected = if erode {
                                    values.all(|v| v)
                                } else {
                                    values.any(|v| v)
                                };
                                assert_eq!(
                                    actual[y * w + x],
                                    expected,
                                    "{w}x{h} ({x},{y}) radius {radius} erode {erode}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(binary_window(&[true], 1, 1, 4, true, &mut || true).is_err());
    }

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
    fn terminal_windows_cover_edges_without_redundant_padded_tiles() {
        for length in [1, 128, 384, 511, 512, 513, 768, 896, 897, 1280, 2048, 3088] {
            let starts: Vec<_> = tile_starts(length).collect();
            assert_eq!(starts[0], 0);
            let last = *starts.last().unwrap();
            assert!(last < length && last + WINDOW >= length);
            if starts.len() > 1 {
                assert!(starts[starts.len() - 2] + WINDOW < length);
            }
            let (w, h) = (length, 1);
            let expected: Vec<_> = (0..w).map(|x| (x % 251) as f32 / 250.).collect();
            let rgb: Vec<_> = expected.iter().flat_map(|&v| [v, v, v]).collect();
            let mut calls = 0;
            let actual = refine_with(
                &rgb,
                &vec![0.5; w],
                w,
                h,
                || false,
                |planes| {
                    calls += 1;
                    Ok(planes[0].clone())
                },
            )
            .unwrap();
            assert_eq!(calls, starts.len());
            assert!(actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 2e-7));
        }
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
