//! Foreground color estimation removes spill from translucent edges without
//! changing the matte. Two box-filter passes follow Forte, ICIP 2021
//! (Approximate Fast Foreground Colour Estimation). See the Python counterpart
//! for provenance. Context is discarded so processing blocks have no seams.

use anyhow::{bail, Result};

const BLOCK: usize = 256;
const RADII: [usize; 2] = [45, 3];
const HALO: usize = 48;

fn fractional(alpha: f32) -> bool {
    let coverage = alpha * 255.0;
    coverage > 0.5 && coverage < 254.5
}

pub fn clean_foreground(
    rgb: &[f32],
    alpha: &[f32],
    width: usize,
    height: usize,
) -> Result<Vec<f32>> {
    clean_foreground_cancellable(rgb, alpha, width, height, || false)
}

pub fn clean_foreground_cancellable(
    rgb: &[f32],
    alpha: &[f32],
    width: usize,
    height: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<f32>> {
    let count = width.checked_mul(height);
    if count.is_none_or(|n| n == 0 || n > 16_777_216 || alpha.len() != n)
        || count.and_then(|n| n.checked_mul(3)) != Some(rgb.len())
        || rgb
            .iter()
            .chain(alpha)
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        bail!("invalid foreground color input");
    }
    let mut result = rgb.to_vec();
    for top in (0..height).step_by(BLOCK) {
        for left in (0..width).step_by(BLOCK) {
            if cancelled() {
                bail!("foreground color estimation cancelled");
            }
            let bottom = (top + BLOCK).min(height);
            let right = (left + BLOCK).min(width);
            if !(top..bottom).any(|y| {
                (left..right).any(|x| {
                    let a = alpha[y * width + x];
                    fractional(a)
                })
            }) {
                continue;
            }
            let x0 = left.saturating_sub(HALO);
            let y0 = top.saturating_sub(HALO);
            let x1 = (right + HALO).min(width);
            let y1 = (bottom + HALO).min(height);
            let (w, h) = (x1 - x0, y1 - y0);
            let mut a = Vec::with_capacity(w * h);
            let mut colors = (0..3)
                .map(|_| Vec::with_capacity(w * h))
                .collect::<Vec<_>>();
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * width + x;
                    a.push(alpha[i]);
                    for c in 0..3 {
                        colors[c].push(rgb[i * 3 + c]);
                    }
                }
            }
            let mut f = colors.clone();
            let mut b = colors.clone();
            for radius in RADII {
                let mass = box_mean(&a, w, h, radius);
                for c in 0..3 {
                    let fa: Vec<_> = f[c].iter().zip(&a).map(|(v, a)| v * a).collect();
                    let ba: Vec<_> = b[c].iter().zip(&a).map(|(v, a)| v * (1.0 - a)).collect();
                    let fm = box_mean(&fa, w, h, radius);
                    let bm = box_mean(&ba, w, h, radius);
                    for i in 0..a.len() {
                        let mass = mass[i].clamp(0.0, 1.0);
                        let fg = fm[i] / mass.max(1e-5);
                        let bg = bm[i] / (1.0 - mass).max(1e-5);
                        f[c][i] = (fg + a[i] * (colors[c][i] - a[i] * fg - (1.0 - a[i]) * bg))
                            .clamp(0.0, 1.0);
                        b[c][i] = bg;
                    }
                }
            }
            for y in top..bottom {
                for x in left..right {
                    let dst = y * width + x;
                    if fractional(alpha[dst]) {
                        let src = (y - y0) * w + x - x0;
                        for c in 0..3 {
                            result[dst * 3 + c] = f[c][src];
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

/// Replicated border samples; f64 running sums prevent long-row drift while
/// storing f32 intermediates, matching scipy.ndimage.uniform_filter.
fn box_mean(input: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let mut temp = vec![0.0; input.len()];
    let mut out = vec![0.0; input.len()];
    let divisor = (2 * radius + 1) as f64;
    // SciPy traverses axis 0 before axis 1.
    for x in 0..w {
        let mut sum = 0.0;
        for j in -(radius as isize)..=radius as isize {
            sum += input[j.clamp(0, h as isize - 1) as usize * w + x] as f64;
        }
        for y in 0..h {
            temp[y * w + x] = (sum / divisor) as f32;
            sum -= input[y.saturating_sub(radius) * w + x] as f64;
            sum += input[(y + radius + 1).min(h - 1) * w + x] as f64;
        }
    }
    for y in 0..h {
        let mut sum = 0.0;
        for j in -(radius as isize)..=radius as isize {
            sum += temp[y * w + j.clamp(0, w as isize - 1) as usize] as f64;
        }
        for x in 0..w {
            out[y * w + x] = (sum / divisor) as f32;
            sum -= temp[y * w + x.saturating_sub(radius)] as f64;
            sum += temp[y * w + (x + radius + 1).min(w - 1)] as f64;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_estimation_removes_spill_without_changing_opaque_colors() {
        let (w, h) = (601, 7);
        let mut alpha = Vec::new();
        let mut rgb = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let a = ((x as f32 - 247.0) / 18.0).clamp(0.0, 1.0);
                alpha.push(a);
                rgb.extend([0.8 * a, 1.0 - a, 0.3 * a]);
            }
        }
        let output = clean_foreground(&rgb, &alpha, w, h).unwrap();
        let mut old_error = 0.0;
        let mut new_error = 0.0;
        for i in 0..alpha.len() {
            if alpha[i] == 0.0 || alpha[i] == 1.0 {
                assert_eq!(&output[i * 3..i * 3 + 3], &rgb[i * 3..i * 3 + 3]);
            } else {
                for (c, target) in [0.8, 0.0, 0.3].into_iter().enumerate() {
                    old_error += alpha[i] * (rgb[i * 3 + c] - target).abs();
                    new_error += alpha[i] * (output[i * 3 + c] - target).abs();
                }
            }
        }
        assert!(new_error < old_error * 0.6, "{new_error} vs {old_error}");
        // Shift the same edge across the processing boundary.
        let shifted_rgb = &rgb[3..w * 3];
        let shifted = clean_foreground(shifted_rgb, &alpha[1..w], w - 1, 1).unwrap();
        for x in 230..280 {
            for c in 0..3 {
                assert!((shifted[(x - 1) * 3 + c] - output[x * 3 + c]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn foreground_estimation_rejects_invalid_inputs_and_cancels() {
        assert!(clean_foreground(&[], &[], 0, 1).is_err());
        assert!(clean_foreground(&[0.0; 3], &[f32::NAN], 1, 1).is_err());
        assert!(clean_foreground(&[f32::INFINITY; 3], &[0.5], 1, 1).is_err());
        assert!(clean_foreground(&[0.0; 3], &[0.5], usize::MAX, 2).is_err());
        assert!(clean_foreground_cancellable(&[0.5; 3], &[0.5], 1, 1, || true).is_err());
        for a in [0.0, 0.0001, 0.9999, 1.0] {
            let rgb = [0.2, 0.7, 0.4, 0.8, 0.1, 0.6];
            assert_eq!(clean_foreground(&rgb, &[a; 2], 2, 1).unwrap(), rgb);
        }
    }
}
