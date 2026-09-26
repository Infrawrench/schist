//! A local, deterministic sky mask: colour confidence connected to the upper
//! image boundary. It does not share the people/object detection pipeline.
use schist_core::{IntRect, LayerMask, TileCoord, TileMap, TILE_SIZE};
use std::collections::VecDeque;

pub fn sky_mask(pixels: &TileMap, bounds: IntRect, tolerance: f32) -> LayerMask {
    let mut result = LayerMask::new_revealing();
    result.default_value = 0;
    result.bounds = bounds;
    if bounds.is_empty() {
        return result;
    }
    let stride = ((bounds.width().max(bounds.height()) + 1023) / 1024).max(1);
    let w = ((bounds.width() + stride - 1) / stride) as usize;
    let h = ((bounds.height() + stride - 1) / stride) as usize;
    let mut confidence = vec![0.0f32; w * h];
    let mut colors = vec![[0.0; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let p = pixels.pixel(
                (bounds.left + x as i32 * stride + stride / 2).min(bounds.right - 1),
                (bounds.top + y as i32 * stride + stride / 2).min(bounds.bottom - 1),
            );
            let blue = (p.b - p.r).max(0.0) + (p.b - p.g).max(0.0) * 0.5;
            let min = p.r.min(p.g).min(p.b);
            let max = p.r.max(p.g).max(p.b);
            let cloud =
                ((min - 0.55) * 3.0).clamp(0.0, 1.0) * (1.0 - (max - min) * 4.0).clamp(0.0, 1.0);
            confidence[y * w + x] =
                ((blue / (0.10 + (1.0 - tolerance) * 0.20)).max(cloud) * p.a).clamp(0.0, 1.0);
            colors[y * w + x] = [p.r, p.g, p.b];
        }
    }
    let mut seen = vec![false; w * h];
    let mut queue = VecDeque::new();
    for (x, marked) in seen[..w].iter_mut().enumerate() {
        if confidence[x] > 0.45 {
            *marked = true;
            queue.push_back(x);
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= w || ny >= h {
                continue;
            }
            let next = ny * w + nx;
            let difference = (0..3)
                .map(|c| (colors[i][c] - colors[next][c]).abs())
                .fold(0.0, f32::max);
            if !seen[next] && confidence[next] > 0.2 && difference < 0.2 + tolerance * 0.4 {
                seen[next] = true;
                queue.push_back(next);
            }
        }
    }
    let sample = |x: i32, y: i32| -> f32 {
        let (x, y) = (
            x.clamp(0, w as i32 - 1) as usize,
            y.clamp(0, h as i32 - 1) as usize,
        );
        if seen[y * w + x] {
            confidence[y * w + x].min(0.8) / 0.8
        } else {
            0.0
        }
    };
    for c in TileCoord::covering(&bounds) {
        let rect = c.rect();
        let clip = rect.intersect(&bounds);
        let tile = result.tiles.get_mut_or_insert(c);
        for y in clip.top..clip.bottom {
            for x in clip.left..clip.right {
                let fx = (x - bounds.left) as f32 / stride as f32;
                let fy = (y - bounds.top) as f32 / stride as f32;
                let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
                let (dx, dy) = (fx - ix as f32, fy - iy as f32);
                let value = (sample(ix, iy) * (1.0 - dx) + sample(ix + 1, iy) * dx) * (1.0 - dy)
                    + (sample(ix, iy + 1) * (1.0 - dx) + sample(ix + 1, iy + 1) * dx) * dy;
                tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                    (value * 255.0).round() as u8;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sky_is_connected_to_top_and_does_not_select_blue_foreground() {
        let mut pixels = TileMap::new();
        let mut data = vec![0.0; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let c = if y < 12 || (y > 24 && (12..20).contains(&x)) {
                    [0.2, 0.5, 0.9, 1.0]
                } else {
                    [0.2, 0.4, 0.1, 1.0]
                };
                data[(y * 32 + x) * 4..(y * 32 + x + 1) * 4].copy_from_slice(&c);
            }
        }
        schist_core::blit_rgba_f32(
            &mut pixels,
            schist_color::Depth::Eight,
            IntRect::from_size(32, 32),
            &data,
        );
        let mask = sky_mask(&pixels, IntRect::from_size(32, 32), 0.5);
        assert_eq!(mask.value(10, 5), 255);
        assert_eq!(mask.value(10, 18), 0);
        assert_eq!(mask.value(15, 28), 0);
    }
}
