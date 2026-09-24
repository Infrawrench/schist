//! Glow falloff conventions. Affinity's intensity boosts a blurred matte;
//! Photoshop's spread grows it first, with range controlling the falloff.
use super::*;
use schist_core::style::GlowFalloff;
use std::collections::VecDeque;

/// Approximate Photoshop's softer falloff with a grown matte and Gaussian.
/// The saved composite of the text-effects regression gives sigma ~11 px
/// for size 38 / spread 21%, or ~0.375 of the remaining soft width.
pub(super) fn parameters(g: &GlowStyle) -> (usize, f32) {
    let size = g.size.max(0.0);
    let spread = size * g.spread.clamp(0.0, 1.0);
    (
        spread.round() as usize,
        (size - spread) * 0.375 * 3.0f32.sqrt(),
    )
}

pub(super) fn prepare(a: &mut [f32], w: usize, h: usize, rect: IntRect, g: &GlowStyle) {
    match g.falloff {
        GlowFalloff::Gaussian => {
            match g.technique {
                Technique::Softer => blur::gaussian_alpha(a, w, h, g.size),
                Technique::Precise => precise_grow(a, w, h, g.size),
            }
            apply_spread(a, g.spread);
        }
        GlowFalloff::Photoshop { range, noise } => {
            match g.technique {
                Technique::Softer => {
                    let (radius, blur) = parameters(g);
                    grow(a, w, h, radius);
                    blur::gaussian_alpha(a, w, h, blur);
                }
                Technique::Precise => precise_grow(a, w, h, g.size),
            }
            let range = range.clamp(0.01, 1.0);
            for (i, v) in a.iter_mut().enumerate() {
                let x = rect.left + (i % w) as i32;
                let y = rect.top + (i / w) as i32;
                *v = (*v / range).clamp(0.0, 1.0) * (1.0 - noise.clamp(0.0, 1.0) * grain(x, y));
            }
        }
    }
}

/// Separable maximum with a sliding queue: O(pixels), even for large spreads.
fn grow(a: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let mut tmp = vec![0.0; a.len()];
    max_pass(a, &mut tmp, w, h, radius, false);
    max_pass(&tmp, a, w, h, radius, true);
}

fn max_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, radius: usize, vertical: bool) {
    let (lines, len, stride, step) = if vertical { (w, h, w, 1) } else { (h, w, 1, w) };
    let radius = radius.min(len);
    let mut queue = VecDeque::<usize>::new();
    for line in 0..lines {
        queue.clear();
        let base = line * step;
        let mut next = 0;
        for i in 0..len {
            let end = (i + radius + 1).min(len);
            while next < end {
                while queue
                    .back()
                    .is_some_and(|&j| src[base + j * stride] <= src[base + next * stride])
                {
                    queue.pop_back();
                }
                queue.push_back(next);
                next += 1;
            }
            while queue.front().is_some_and(|&j| j < i.saturating_sub(radius)) {
                queue.pop_front();
            }
            dst[base + i * stride] = src[base + queue[0] * stride];
        }
    }
}

fn grain(x: i32, y: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16777216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_preserves_partial_coverage_and_handles_plane_edges() {
        let mut alpha = vec![0.0; 15];
        alpha[5] = 0.3;
        alpha[7] = 0.8;
        grow(&mut alpha, 5, 3, 1);
        assert_eq!(alpha, [0.3, 0.8, 0.8, 0.8, 0.0].repeat(3));

        let mut alpha = vec![0.25];
        grow(&mut alpha, 1, 1, 100);
        assert_eq!(alpha, [0.25]);
    }
}
