//! Point-sampled, tile-indexed previews for interactive affine transforms.
//! Full-quality commits continue through `transform_tiles`.
use super::*;
use crate::NativeSamples;
use std::sync::Arc;

/// An immutable gesture source. Bounds and tile indexing are prepared once,
/// rather than scanning and flattening every source pixel on each pointer event.
pub struct TransformPreview {
    source: TileMap,
    bounds: IntRect,
    origin: TileCoord,
    cols: usize,
    grid: Vec<Option<Arc<TileBuf>>>,
}

impl TransformPreview {
    pub fn bounds(&self) -> IntRect {
        self.bounds
    }

    pub fn new(source: &TileMap) -> Self {
        let bounds = source.content_bounds();
        let origin = TileCoord::containing(bounds.left, bounds.top);
        let mut cols = 0;
        let mut grid = Vec::new();
        if !bounds.is_empty() {
            let last = TileCoord::containing(bounds.right - 1, bounds.bottom - 1);
            cols = (i64::from(last.tx) - i64::from(origin.tx) + 1) as usize;
            let rows = (i64::from(last.ty) - i64::from(origin.ty) + 1) as usize;
            // Sparse documents can span enormous coordinate ranges. Keep the
            // index bounded and fall back to the source map for those cases.
            if let Some(slots) = cols.checked_mul(rows).filter(|&n| n <= 1 << 20) {
                grid.resize(slots, None);
                for (coord, tile) in source.iter() {
                    let x = i64::from(coord.tx) - i64::from(origin.tx);
                    let y = i64::from(coord.ty) - i64::from(origin.ty);
                    if x >= 0 && y >= 0 && x < cols as i64 && y < rows as i64 {
                        grid[y as usize * cols + x as usize] = Some(tile.clone());
                    }
                }
            }
        }
        Self {
            source: source.clone(),
            bounds,
            origin,
            cols,
            grid,
        }
    }

    /// Nearest source pixel at each destination pixel's centre. Drag previews
    /// intentionally omit edge supersampling and minification box filtering;
    /// neither their pixels nor their clipped extent are used for the commit.
    pub fn render(&self, matrix: &Affine, depth: Depth, clip: IntRect) -> TileMap {
        let mode = self.source.mode();
        let mut out = TileMap::new_in_mode(mode);
        let Some(inv) = matrix.invert() else {
            return out;
        };
        let dst = matrix.transform_bounds(self.bounds).intersect(&clip);
        if dst.is_empty() {
            return out;
        }
        let coords: Vec<_> = TileCoord::covering(&dst).collect();
        let tiles: Vec<_> = coords
            .into_par_iter()
            .filter_map(|coord| {
                let rect = coord.rect();
                let clip = rect.intersect(&dst);
                let mut tile = TileBuf::new_in_mode(depth, mode);
                let mut any = false;
                for y in clip.top..clip.bottom {
                    for x in clip.left..clip.right {
                        let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
                        if !self.bounds.contains(sx, sy) {
                            continue;
                        }
                        let source_coord = TileCoord::containing(sx, sy);
                        let source = if self.grid.is_empty() {
                            self.source.get(source_coord)
                        } else {
                            let x = (source_coord.tx - self.origin.tx) as usize;
                            let y = (source_coord.ty - self.origin.ty) as usize;
                            self.grid[y * self.cols + x].as_ref()
                        };
                        let Some(source) = source else { continue };
                        let from = (sy.rem_euclid(TILE_SIZE) * TILE_SIZE + sx.rem_euclid(TILE_SIZE))
                            as usize;
                        let to = ((y - rect.top) * TILE_SIZE + x - rect.left) as usize;
                        any |= copy_pixel(source, &mut tile, from, to);
                    }
                }
                any.then(|| (coord, Arc::new(tile)))
            })
            .collect();
        for (coord, tile) in tiles {
            out.insert(coord, tile);
        }
        out
    }
}

#[inline]
fn copy_samples<T: Copy + PartialOrd + Default>(
    src: &[T],
    dst: &mut [T],
    from: usize,
    to: usize,
    channels: usize,
) -> bool {
    let from = from * channels;
    let to = to * channels;
    if src[from + channels - 1] > T::default() {
        dst[to..to + channels].copy_from_slice(&src[from..from + channels]);
        true
    } else {
        false
    }
}

#[inline]
fn copy_pixel(src: &TileBuf, dst: &mut TileBuf, from: usize, to: usize) -> bool {
    match (src, &mut *dst) {
        (TileBuf::U8(s), TileBuf::U8(d)) => return copy_samples(s, d, from, to, 4),
        (TileBuf::U16(s), TileBuf::U16(d)) => return copy_samples(s, d, from, to, 4),
        (TileBuf::F32(s), TileBuf::F32(d)) => return copy_samples(s, d, from, to, 4),
        (TileBuf::Native(s), TileBuf::Native(d)) if s.mode == d.mode => {
            let n = s.mode.channels() + 1;
            match (&s.samples, &mut d.samples) {
                (NativeSamples::U8(s), NativeSamples::U8(d)) => {
                    return copy_samples(s, d, from, to, n)
                }
                (NativeSamples::U16(s), NativeSamples::U16(d)) => {
                    return copy_samples(s, d, from, to, n)
                }
                (NativeSamples::F32(s), NativeSamples::F32(d)) => {
                    return copy_samples(s, d, from, to, n)
                }
                _ => {}
            }
        }
        _ => {}
    }
    let pixel = src.native_pixel(from).converted(dst.mode());
    if pixel.alpha > 0.0 {
        dst.set_native_pixel(to, pixel);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_color::{ColorMode, NativePixel};

    #[test]
    fn previews_match_point_sampling_across_modes_depths_and_transforms() {
        let clip = IntRect::new(-12, -12, 110, 100);
        for mode in [ColorMode::Rgb, ColorMode::Cmyk, ColorMode::Lab] {
            for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
                let mut source = TileMap::new_in_mode(mode);
                for rect in [IntRect::new(-4, -3, 5, 6), IntRect::new(253, 254, 260, 261)] {
                    for y in rect.top..rect.bottom {
                        for x in rect.left..rect.right {
                            let pixel = NativePixel {
                                mode,
                                color: [0.17, 0.41, 0.73, 0.29],
                                alpha: if (x + y) % 3 == 0 { 0.0 } else { 0.63 },
                            };
                            source
                                .get_mut_or_insert(TileCoord::containing(x, y), depth)
                                .set_native_pixel(
                                    (y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE))
                                        as usize,
                                    pixel,
                                );
                        }
                    }
                }
                let preview = TransformPreview::new(&source);
                for matrix in [
                    Affine::IDENTITY,
                    Affine::scale(0.25, 0.3),
                    Affine::scale(-1.4, 2.0).around(4.0, 2.0),
                    Affine::rotate(0.4).then(&Affine::translate(3.25, -1.75)),
                    Affine::translate(-250.0, -250.0),
                ] {
                    let output = preview.render(&matrix, depth, clip);
                    let inv = matrix.invert().unwrap();
                    assert_eq!(output.mode(), mode);
                    for y in clip.top..clip.bottom {
                        for x in clip.left..clip.right {
                            let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
                            let expected =
                                source.native_pixel(sx.floor() as i32, sy.floor() as i32);
                            let actual = output.native_pixel(x, y);
                            if expected.alpha > 0.0 {
                                assert_eq!(
                                    actual, expected,
                                    "{mode:?} {depth:?} {matrix:?} at {x},{y}"
                                );
                            } else {
                                assert_eq!(actual.alpha, 0.0);
                            }
                        }
                    }
                    assert!(output.native_pixel(clip.right, clip.top).alpha == 0.0);
                }
            }
        }
    }

    #[test]
    fn source_snapshot_survives_edits_and_supports_mixed_depths() {
        let mut source = TileMap::new();
        let coord = TileCoord { tx: 0, ty: 0 };
        source
            .get_mut_or_insert(coord, Depth::ThirtyTwo)
            .set(0, Rgba::new(0.2, 0.4, 0.6, 1.0));
        let snapshot = TransformPreview::new(&source);
        source
            .get_mut_or_insert(coord, Depth::ThirtyTwo)
            .set(0, Rgba::WHITE);
        let output = snapshot.render(&Affine::IDENTITY, Depth::Eight, IntRect::new(0, 0, 8, 8));
        assert_eq!(output.pixel(0, 0).to_u8(), [51, 102, 153, 255]);
    }

    #[test]
    fn empty_singular_and_widely_separated_sources_are_bounded() {
        let clip = IntRect::new(0, 0, 8, 8);
        assert!(TransformPreview::new(&TileMap::new())
            .render(&Affine::IDENTITY, Depth::Eight, clip)
            .is_empty());
        let mut source = TileMap::new();
        for tx in [0, 2_000_000] {
            source
                .get_mut_or_insert(TileCoord { tx, ty: 0 }, Depth::Eight)
                .set(0, Rgba::WHITE);
        }
        let preview = TransformPreview::new(&source);
        assert!(preview.grid.is_empty());
        assert_eq!(
            preview
                .render(&Affine::IDENTITY, Depth::Eight, clip)
                .pixel(0, 0),
            Rgba::WHITE
        );
        assert!(preview
            .render(&Affine::scale(0.0, 1.0), Depth::Eight, clip)
            .is_empty());
    }
}
