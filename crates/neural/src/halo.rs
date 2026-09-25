//! Conservative cleanup of compact radial glow left by a restoration model.
//! Fits a smooth background and a Gaussian around detected light cores. No
//! scene coordinates are supplied; ambiguous fits and border sources are skipped.

use std::f64::consts::PI;

const RADIUS: usize = 36;

#[derive(Clone)]
struct Sample {
    x: f64,
    y: f64,
    r2: f64,
    rgb: [f64; 3],
}

struct Fit {
    beta: [[f64; 3]; 4],
    error: f64,
}

struct Glow {
    x: f64,
    y: f64,
    radius: f64,
    sigma: f64,
    amplitude: [f64; 3],
    improvement: f64,
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn basis(p: &Sample, sigma: Option<f64>) -> [f64; 4] {
    [
        1.0,
        p.x / RADIUS as f64,
        p.y / RADIUS as f64,
        sigma.map_or(0.0, |s| (-p.r2 / (2.0 * s * s)).exp()),
    ]
}

fn predict(beta: &[[f64; 3]; 4], f: &[f64; 4], c: usize) -> f64 {
    (0..4).map(|i| beta[i][c] * f[i]).sum()
}

// Solve all three colour channels together, with partial pivoting. A singular
// fit means insufficient background information and is simply rejected.
fn solve(mut matrix: [[f64; 7]; 4], n: usize) -> Option<[[f64; 3]; 4]> {
    for i in 0..n {
        let pivot = (i..n).max_by(|&a, &b| matrix[a][i].abs().total_cmp(&matrix[b][i].abs()))?;
        matrix.swap(i, pivot);
        let divisor = matrix[i][i];
        if !divisor.is_finite() || divisor.abs() < 1e-10 {
            return None;
        }
        for j in i..7 {
            matrix[i][j] /= divisor;
        }
        for row in 0..n {
            if row == i {
                continue;
            }
            let scale = matrix[row][i];
            for j in i..7 {
                matrix[row][j] -= scale * matrix[i][j];
            }
        }
    }
    let mut result = [[0.0; 3]; 4];
    for i in 0..n {
        result[i].copy_from_slice(&matrix[i][4..7]);
    }
    result
        .iter()
        .flatten()
        .all(|v| v.is_finite())
        .then_some(result)
}

fn fit(samples: &[Sample], sigma: Option<f64>) -> Option<Fit> {
    let n = if sigma.is_some() { 4 } else { 3 };
    let features: Vec<_> = samples.iter().map(|p| basis(p, sigma)).collect();
    let mut weights = vec![1.0; samples.len()];
    let mut beta = [[0.0; 3]; 4];
    let mut error = 0.0;
    for _ in 0..5 {
        let mut matrix = [[0.0; 7]; 4];
        for ((p, f), &weight) in samples.iter().zip(&features).zip(&weights) {
            for i in 0..n {
                for j in 0..n {
                    matrix[i][j] += weight * f[i] * f[j];
                }
                for c in 0..3 {
                    matrix[i][4 + c] += weight * f[i] * p.rgb[c];
                }
            }
        }
        beta = solve(matrix, n)?;
        error = 0.0;
        for ((p, f), weight) in samples.iter().zip(&features).zip(&mut weights) {
            let mut square = 0.0;
            for c in 0..3 {
                let residual = p.rgb[c] - predict(&beta, f, c);
                square += residual * residual;
                error += residual.abs().min(0.08);
            }
            *weight = (0.02 / ((square / 3.0).sqrt() + 1e-6)).min(1.0);
        }
    }
    Some(Fit {
        beta,
        error: error / (samples.len() * 3) as f64,
    })
}

fn candidate(rgb: &[f32], width: usize, x: f64, y: f64, area: usize) -> Option<Glow> {
    let radius = 2.0_f64.max((area as f64 / PI).sqrt() + 1.0);
    let mut samples = Vec::new();
    for iy in y as usize - RADIUS..=y as usize + RADIUS {
        for ix in x as usize - RADIUS..=x as usize + RADIUS {
            let (dx, dy) = (ix as f64 - x, iy as f64 - y);
            let r2 = dx * dx + dy * dy;
            let at = (iy * width + ix) * 3;
            let pixel = [rgb[at] as f64, rgb[at + 1] as f64, rgb[at + 2] as f64];
            if r2 > (radius + 1.0).powi(2)
                && r2 < (RADIUS * RADIUS) as f64
                && pixel.iter().all(|&v| v < 0.62)
            {
                samples.push(Sample {
                    x: dx,
                    y: dy,
                    r2,
                    rgb: pixel,
                });
            }
        }
    }
    if samples.len() < 200 {
        return None;
    }
    let baseline = fit(&samples, None)?;
    let mut best: Option<(f64, Glow)> = None;
    for sigma in [3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 13.0, 17.0] {
        if sigma < radius * 0.7 {
            continue;
        }
        let Some(fitted) = fit(&samples, Some(sigma)) else {
            continue;
        };
        let amplitude = fitted.beta[3];
        let channel = (0..3).max_by(|&a, &b| amplitude[a].total_cmp(&amplitude[b]))?;
        if amplitude[channel] < 0.025
            || amplitude[channel] > 0.5
            || amplitude.iter().any(|&v| v < -0.06)
        {
            continue;
        }
        let improvement = (baseline.error - fitted.error) / baseline.error.max(1e-6);
        if improvement < 0.15 {
            continue;
        }
        let mut sectors: [Vec<f64>; 8] = std::array::from_fn(|_| Vec::new());
        for p in &samples {
            if p.r2 >= (sigma * 1.8).powi(2) {
                continue;
            }
            let sector = ((p.y.atan2(p.x) + PI) / (2.0 * PI) * 8.0) as usize;
            if sector < 8 {
                sectors[sector]
                    .push(p.rgb[channel] - predict(&fitted.beta, &basis(p, None), channel));
            }
        }
        let mut radial = 0;
        for values in &mut sectors {
            if values.len() <= 2 {
                continue;
            }
            values.sort_unstable_by(f64::total_cmp);
            let median = (values[(values.len() - 1) / 2] + values[values.len() / 2]) * 0.5;
            if median > 0.008 {
                radial += 1;
            }
        }
        if radial < 6 {
            continue;
        }
        if best.as_ref().is_none_or(|(error, _)| fitted.error < *error) {
            best = Some((
                fitted.error,
                Glow {
                    x,
                    y,
                    radius,
                    sigma,
                    amplitude,
                    improvement,
                },
            ));
        }
    }
    best.map(|(_, glow)| glow)
}

pub(crate) fn clean_up(rgb: &mut [f32], width: usize, height: usize) {
    if width <= RADIUS * 2 || height <= RADIUS * 2 {
        return;
    }
    let bright: Vec<_> = rgb
        .chunks_exact(3)
        .map(|p| p.iter().any(|&v| v > 0.665))
        .collect();
    let mut seen = vec![false; width * height];
    let mut candidates = Vec::new();
    for seed in 0..bright.len() {
        if !bright[seed] || seen[seed] {
            continue;
        }
        seen[seed] = true;
        let mut stack = vec![seed];
        let (mut area, mut sum_x, mut sum_y) = (0, 0, 0);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (width, 0, height, 0);
        while let Some(at) = stack.pop() {
            let (x, y) = (at % width, at / width);
            area += 1;
            sum_x += x;
            sum_y += y;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            for next in [
                x.checked_sub(1).map(|_| at - 1),
                (x + 1 < width).then_some(at + 1),
                y.checked_sub(1).map(|_| at - width),
                (y + 1 < height).then_some(at + width),
            ]
            .into_iter()
            .flatten()
            {
                if bright[next] && !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        let (bw, bh) = (max_x - min_x + 1, max_y - min_y + 1);
        if !(2..=120).contains(&area)
            || bw.max(bh) > 20
            || bw.max(bh) as f64 > 2.5 * bw.min(bh) as f64
        {
            continue;
        }
        let (x, y) = (sum_x as f64 / area as f64, sum_y as f64 / area as f64);
        if x < RADIUS as f64
            || y < RADIUS as f64
            || x as usize + RADIUS >= width
            || y as usize + RADIUS >= height
        {
            continue;
        }
        if let Some(glow) = candidate(rgb, width, x, y, area) {
            candidates.push(glow);
        }
    }
    candidates.sort_by(|a, b| b.improvement.total_cmp(&a.improvement));
    let mut accepted: Vec<Glow> = Vec::new();
    let mut correction = vec![0.0_f64; rgb.len()];
    for glow in candidates {
        if accepted
            .iter()
            .any(|g| (g.x - glow.x).powi(2) + (g.y - glow.y).powi(2) < (RADIUS * RADIUS) as f64)
        {
            continue;
        }
        // Four standard deviations avoids a visible rectangular cutoff.
        let support = (glow.sigma * 4.0).ceil() as usize;
        for y in
            (glow.y as usize).saturating_sub(support)..=(glow.y as usize + support).min(height - 1)
        {
            for x in (glow.x as usize).saturating_sub(support)
                ..=(glow.x as usize + support).min(width - 1)
            {
                let r2 = (x as f64 - glow.x).powi(2) + (y as f64 - glow.y).powi(2);
                let at = (y * width + x) * 3;
                let peak = rgb[at..at + 3].iter().copied().fold(0.0_f32, f32::max) as f64;
                let protected = smooth((peak - 0.65) / 0.06);
                let core_gate = smooth((r2.sqrt() - glow.radius * 0.85) / (glow.radius * 0.55));
                let weight =
                    (-r2 / (2.0 * glow.sigma * glow.sigma)).exp() * (1.0 - protected) * core_gate;
                for c in 0..3 {
                    correction[at + c] += weight * glow.amplitude[c].max(0.0);
                }
            }
        }
        accepted.push(glow);
    }
    for (pixel, delta) in rgb.iter_mut().zip(correction) {
        *pixel = (*pixel as f64 - delta).clamp(0.0, 1.0) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(halo: bool) -> Vec<f32> {
        let mut rgb = vec![0.0; 128 * 128 * 3];
        for y in 0..128 {
            for x in 0..128 {
                let r2 = (x as f64 - 64.0).powi(2) + (y as f64 - 64.0).powi(2);
                for c in 0..3 {
                    let bg = 0.17 + x as f64 * 0.0002 + y as f64 * 0.0001;
                    let glow = if halo {
                        [0.25, 0.16, 0.02][c] * (-r2 / 200.0).exp()
                    } else {
                        0.0
                    };
                    rgb[(y * 128 + x) * 3 + c] = if r2 <= 9.0 { 0.8 } else { (bg + glow) as f32 };
                }
            }
        }
        rgb
    }

    #[test]
    fn removes_radial_glow_preserving_core_and_background() {
        let mut image = scene(true);
        let clean = scene(false);
        let before = image.clone();
        clean_up(&mut image, 128, 128);
        assert_eq!(&image[(64 * 128 + 64) * 3..][..3], &[0.8; 3]);
        let mut old_error = 0.0;
        let mut new_error = 0.0;
        for y in 0..128 {
            for x in 0..128 {
                let r2 = (x as f64 - 64.0).powi(2) + (y as f64 - 64.0).powi(2);
                if (100.0..625.0).contains(&r2) {
                    for c in 0..3 {
                        let at = (y * 128 + x) * 3 + c;
                        old_error += (before[at] - clean[at]).abs();
                        new_error += (image[at] - clean[at]).abs();
                    }
                }
            }
        }
        assert!(
            new_error < old_error * 0.1,
            "before={old_error}, after={new_error}"
        );
        assert_eq!(image[0], before[0]);
        assert!(image
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    }

    #[test]
    fn clean_light_gradient_and_thin_images_are_unchanged() {
        let mut image = scene(false);
        let before = image.clone();
        clean_up(&mut image, 128, 128);
        assert_eq!(image, before);
        let mut thin = vec![0.7; 128 * 3];
        clean_up(&mut thin, 1, 128);
        assert_eq!(thin, vec![0.7; 128 * 3]);
    }
}
