//! Full-resolution alpha refinement. The detector supplies semantics; this
//! trained, local network refines its boundary using the original RGB pixels.
//! Tile context is discarded, not blended, so edges do not develop grid seams.

use crate::{Input, Model};
use anyhow::{bail, Context, Result};

/// Refine an unthresholded foreground probability map into a soft alpha matte.
/// RGB is interleaved, straight (not premultiplied), and in 0..=1.
pub fn refine_alpha(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
) -> Result<Vec<f32>> {
    refine_alpha_cancellable(model, rgb, coarse, width, height, || false)
}

/// As [`refine_alpha`], checking cancellation before each bounded tile.
pub fn refine_alpha_cancellable(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
    cancelled: impl FnMut() -> bool,
) -> Result<Vec<f32>> {
    if model.spec.id == "detail-matting" {
        return crate::detail_matting::refine(model, rgb, coarse, width, height, cancelled);
    }
    refine_local(model, rgb, coarse, width, height, cancelled)
}

// The detail refiner uses this model only for opaque interior seeds. Keep that
// call separate from dispatch so generic cancellation does not recurse.
pub(crate) fn refine_local(
    model: &Model,
    rgb: &[f32],
    coarse: &[f32],
    width: usize,
    height: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<f32>> {
    let count = width
        .checked_mul(height)
        .context("image dimensions overflow")?;
    if count == 0
        || count > 16_777_216
        || count.checked_mul(3) != Some(rgb.len())
        || coarse.len() != count
    {
        bail!("invalid matting dimensions or buffer lengths");
    }
    if rgb.iter().chain(coarse).any(|v| !v.is_finite()) {
        bail!("non-finite matting input");
    }
    let Input::Tiles {
        size,
        overlap,
        scale: 1,
    } = model.spec.input
    else {
        bail!("matting requires a tiled, same-size model");
    };
    if model.channels() != 4 || overlap < 11 || overlap * 2 >= size {
        bail!("invalid matting channels or tile context");
    }
    let stride = size - overlap * 2;
    let mut result = vec![0.0; count];
    let mut planes = vec![vec![0.0; size * size]; 4];
    for top in (0..height).step_by(stride) {
        for left in (0..width).step_by(stride) {
            if cancelled() {
                bail!("matting cancelled");
            }
            for y in 0..size {
                let sy = (top as isize + y as isize - overlap as isize)
                    .clamp(0, height as isize - 1) as usize;
                for x in 0..size {
                    let sx = (left as isize + x as isize - overlap as isize)
                        .clamp(0, width as isize - 1) as usize;
                    let src = sy * width + sx;
                    let dst = y * size + x;
                    for c in 0..3 {
                        planes[c][dst] = rgb[src * 3 + c].clamp(0.0, 1.0);
                    }
                    planes[3][dst] = coarse[src].clamp(0.0, 1.0);
                }
            }
            // Entirely confident tiles do not need an edge model. Inspect the
            // halo too, so adjacent tiles make compatible decisions at edges.
            let low = planes[3].iter().copied().fold(1.0f32, f32::min);
            let high = planes[3].iter().copied().fold(0.0f32, f32::max);
            let output = if high <= 0.001 || low >= 0.999 {
                planes[3].clone()
            } else {
                let refs: Vec<_> = planes.iter().map(Vec::as_slice).collect();
                let outputs = model.run_planes(&refs)?;
                let view = outputs[0].to_plain_array_view::<f32>()?;
                if view.shape() != [1, 1, size, size] {
                    bail!("unexpected matting output shape {:?}", view.shape());
                }
                let flat = view.as_slice().context("non-contiguous matte")?;
                if flat.iter().any(|v| !v.is_finite()) {
                    bail!("non-finite matting output");
                }
                flat.to_vec()
            };
            for y in 0..stride.min(height - top) {
                for x in 0..stride.min(width - left) {
                    result[(top + y) * width + left + x] =
                        output[(y + overlap) * size + x + overlap].clamp(0.0, 1.0);
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matting_matches_the_python_training_runtime() {
        let model = crate::get("matting").unwrap();
        let mut planes = vec![vec![0.0; 128 * 128]; 4];
        for y in 0..128 {
            for x in 0..128 {
                let i = y * 128 + x;
                planes[0][i] = x as f32 / 127.0;
                planes[1][i] = y as f32 / 127.0;
                planes[2][i] = ((x + y) % 17) as f32 / 16.0;
                planes[3][i] = ((x as f32 - y as f32 + 8.0) / 16.0).clamp(0.0, 1.0);
            }
        }
        let result = model
            .run_planes(&planes.iter().map(Vec::as_slice).collect::<Vec<_>>())
            .unwrap();
        let view = result[0].to_plain_array_view::<f32>().unwrap();
        let actual = view.as_slice().unwrap();
        let expected = include_bytes!("../tests/fixtures/matting-reference.f32");
        assert_eq!(expected.len(), actual.len() * 4);
        let error = actual
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .map(|(got, bytes)| (got - f32::from_le_bytes(*bytes)).abs())
            .fold(0.0f32, f32::max);
        assert!(error < 1e-5, "Python/Rust alpha error {error}");
    }

    #[test]
    fn matting_rejects_bad_buffers_and_nonfinite_values() {
        let model = crate::get("matting").unwrap();
        assert!(refine_alpha(&model, &[], &[], 0, 1).is_err());
        assert!(refine_alpha(&model, &[0.0; 3], &[0.5], usize::MAX, 2).is_err());
        assert!(refine_alpha(&model, &[f32::NAN; 3], &[0.5], 1, 1).is_err());
        assert!(refine_alpha(&model, &[0.0; 3], &[f32::INFINITY], 1, 1).is_err());
        assert!(refine_alpha_cancellable(&model, &[0.5; 3], &[0.5], 1, 1, || true).is_err());
    }

    #[test]
    fn matting_keeps_confident_pixels_and_handles_partial_tiles() {
        let model = crate::get("matting").unwrap();
        for (w, h) in [(1, 1), (7, 13), (193, 97)] {
            for alpha in [0.0, 1.0] {
                let coarse = vec![alpha; w * h];
                let result = refine_alpha(&model, &vec![0.5; w * h * 3], &coarse, w, h).unwrap();
                assert_eq!(result, coarse);
            }
        }
    }

    #[test]
    fn matting_does_not_punch_holes_in_certain_interiors_near_edges() {
        let model = crate::get("matting").unwrap();
        let (w, h) = (193, 97);
        let mut rgb = vec![0.0; w * h * 3];
        let mut coarse = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                coarse[i] = ((x as f32 - 85.0) / 20.0).clamp(0.0, 1.0);
                rgb[i * 3..i * 3 + 3].copy_from_slice(&[
                    ((x * 17 + y * 13) % 101) as f32 / 100.0,
                    ((x * 7 + y * 29) % 71) as f32 / 70.0,
                    ((x * 31 + y * 3) % 41) as f32 / 40.0,
                ]);
            }
        }
        let result = refine_alpha(&model, &rgb, &coarse, w, h).unwrap();
        for (before, after) in coarse.iter().zip(result) {
            if *before == 0.0 || *before == 1.0 {
                assert_eq!(*before, after, "certain alpha changed in an edge tile");
            }
        }
    }

    #[test]
    fn matting_tile_boundary_matches_a_single_context_window() {
        let model = crate::get("matting").unwrap();
        let (w, h) = (193, 128);
        let mut rgb = vec![0.0; w * h * 3];
        let mut coarse = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let value =
                    ((x as f32 - 96.0 + (y as f32 * 0.2).sin() * 5.0) / 12.0 + 0.5).clamp(0.0, 1.0);
                coarse[i] = value;
                rgb[i * 3..i * 3 + 3].copy_from_slice(&[value, 0.4, 1.0 - value]);
            }
        }
        let tiled = refine_alpha(&model, &rgb, &coarse, w, h).unwrap();
        let mut planes = vec![vec![0.0; 128 * 128]; 4];
        for y in 0..128 {
            for x in 0..128 {
                let i = y * w + x + 32;
                for c in 0..3 {
                    planes[c][y * 128 + x] = rgb[i * 3 + c];
                }
                planes[3][y * 128 + x] = coarse[i];
            }
        }
        let outputs = model
            .run_planes(&planes.iter().map(Vec::as_slice).collect::<Vec<_>>())
            .unwrap();
        let view = outputs[0].to_plain_array_view::<f32>().unwrap();
        let flat = view.as_slice().unwrap();
        for y in 20..108 {
            for x in 85..108 {
                assert!(
                    (tiled[y * w + x] - flat[y * 128 + x - 32]).abs() < 1e-5,
                    "seam at {x},{y}"
                );
            }
        }
    }
}
