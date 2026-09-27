//! Antialiased triangle resampling for foreground-model inputs.
//!
//! Direct bilinear point sampling aliases high-resolution camera textures.
//! Widening the triangle on reduction averages the whole source footprint.

use rayon::prelude::*;

fn taps(source: usize, target: usize) -> Vec<Vec<(usize, f64)>> {
    let scale = source as f64 / target as f64;
    let radius = scale.max(1.0);
    (0..target)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let start = ((center - radius + 0.5) as isize).max(0) as usize;
            let end = ((center + radius + 0.5) as usize).min(source);
            let mut row: Vec<_> = (start..end)
                .map(|j| {
                    (
                        j,
                        (1.0 - ((j as f64 + 0.5 - center) / radius).abs()).max(0.0),
                    )
                })
                .collect();
            let sum: f64 = row.iter().map(|v| v.1).sum();
            for (_, weight) in &mut row {
                *weight /= sum;
            }
            row
        })
        .collect()
}

pub(crate) fn rgb_triangle(
    rgb: &[f32],
    width: usize,
    height: usize,
    out_width: usize,
    out_height: usize,
) -> Vec<f32> {
    let xs = taps(width, out_width);
    let ys = taps(height, out_height);
    let mut horizontal = vec![0.0f32; out_width * height * 3];
    horizontal
        .par_chunks_mut(out_width * 3)
        .with_min_len(height.div_ceil(4))
        .enumerate()
        .for_each(|(y, row)| {
            for (pixel, taps) in row.as_chunks_mut::<3>().0.iter_mut().zip(&xs) {
                let mut sum = [0.0f64; 3];
                for &(ix, weight) in taps {
                    let source = &rgb[(y * width + ix) * 3..];
                    for c in 0..3 {
                        sum[c] += source[c] as f64 * weight;
                    }
                }
                *pixel = sum.map(|v| v as f32);
            }
        });
    let mut result = vec![0.0f32; out_width * out_height * 3];
    result
        .par_chunks_mut(out_width * 3)
        .with_min_len(out_height.div_ceil(4))
        .enumerate()
        .for_each(|(y, row)| {
            for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let mut sum = [0.0f64; 3];
                for &(iy, weight) in &ys[y] {
                    let source = &horizontal[(iy * out_width + x) * 3..];
                    for c in 0..3 {
                        sum[c] += source[c] as f64 * weight;
                    }
                }
                *pixel = sum.map(|v| v as f32);
            }
        });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_rgb_passes_preserve_scalar_channel_sums() {
        for (w, h, ow, oh) in [
            (1, 1, 9, 7),
            (37, 29, 11, 13),
            (17, 11, 43, 31),
            (127, 97, 13, 9),
        ] {
            let rgb: Vec<_> = (0..w * h * 3)
                .map(|i| (i * 31 % 251) as f32 / 250.)
                .collect();
            let xs = taps(w, ow);
            let ys = taps(h, oh);
            let mut horizontal = vec![0.0f32; ow * h * 3];
            for y in 0..h {
                for (x, taps) in xs.iter().enumerate() {
                    for c in 0..3 {
                        horizontal[(y * ow + x) * 3 + c] =
                            taps.iter()
                                .map(|&(ix, weight)| rgb[(y * w + ix) * 3 + c] as f64 * weight)
                                .sum::<f64>() as f32;
                    }
                }
            }
            let mut expected = vec![0.0f32; ow * oh * 3];
            for (y, taps) in ys.iter().enumerate() {
                for x in 0..ow {
                    for c in 0..3 {
                        expected[(y * ow + x) * 3 + c] = taps
                            .iter()
                            .map(|&(iy, weight)| horizontal[(iy * ow + x) * 3 + c] as f64 * weight)
                            .sum::<f64>()
                            as f32;
                    }
                }
            }
            assert_eq!(rgb_triangle(&rgb, w, h, ow, oh), expected);
        }
    }

    #[test]
    fn reduction_averages_texture_instead_of_aliasing_it() {
        let pixels: Vec<f32> = (0..60 * 60)
            .flat_map(|i| [((i + i / 60) % 2) as f32; 3])
            .collect();
        let small = rgb_triangle(&pixels, 60, 60, 5, 5);
        assert!(small.iter().all(|v| (v - 0.5).abs() < 0.001));
        assert_eq!(
            rgb_triangle(&[0.2, 0.4, 0.8], 1, 1, 9, 7),
            [0.2, 0.4, 0.8].repeat(63)
        );
    }

    #[test]
    fn triangle_matches_pillow_float_reference() {
        let input: Vec<f32> = (0..37 * 29 * 3)
            .map(|i| ((i * 31 % 251) as f32) / 250.0)
            .collect();
        let actual = rgb_triangle(&input, 37, 29, 11, 13);
        let bytes = include_bytes!("../tests/fixtures/foreground-resize.f32");
        for (value, bytes) in actual.iter().zip(bytes.as_chunks::<4>().0) {
            let expected = f32::from_le_bytes(*bytes);
            assert!((value - expected).abs() < 2e-6, "{value} != {expected}");
        }
        assert_eq!(actual.len() * 4, bytes.len());
    }
}
