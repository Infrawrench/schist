//! Sampling raster tiles during an integer-translation preview.

use schist_core::{TileCoord, TileMap, TILE_SIZE};

/// Decode into a zeroed RGBA buffer. A translated output tile overlaps at
/// most four source tiles: look each one up once and copy its intersection,
/// instead of doing a hash lookup and coordinate division for every pixel.
pub(super) fn decode(tiles: &TileMap, coord: TileCoord, offset: (i32, i32), out: &mut [f32]) {
    if offset == (0, 0) {
        if let Some(tile) = tiles.get(coord) {
            tile.decode_f32(out);
        }
        return;
    }
    // Subtraction can leave the i32 pixel domain at extreme drag offsets.
    let size = TILE_SIZE as i64;
    let x = coord.tx as i64 * size - offset.0 as i64;
    let y = coord.ty as i64 * size - offset.1 as i64;
    let rx = x.rem_euclid(size) as usize;
    let ry = y.rem_euclid(size) as usize;
    let size = TILE_SIZE as usize;
    let xs = [(rx, 0, size - rx), (0, size - rx, rx)];
    let ys = [(ry, 0, size - ry), (0, size - ry, ry)];
    for (qy, &(sy, oy, h)) in ys.iter().enumerate() {
        for (qx, &(sx, ox, w)) in xs.iter().enumerate() {
            if w == 0 || h == 0 {
                continue;
            }
            let tx = x.div_euclid(TILE_SIZE as i64) + qx as i64;
            let ty = y.div_euclid(TILE_SIZE as i64) + qy as i64;
            let domain = i32::MIN as i64 / TILE_SIZE as i64..=i32::MAX as i64 / TILE_SIZE as i64;
            if !domain.contains(&tx) || !domain.contains(&ty) {
                continue;
            }
            let Some(tile) = tiles.get(TileCoord {
                tx: tx as i32,
                ty: ty as i32,
            }) else {
                continue;
            };
            for row in 0..h {
                let src = (sy + row) * size + sx;
                let dst = ((oy + row) * size + ox) * 4;
                for (col, rgba) in out[dst..dst + w * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .enumerate()
                {
                    let p = tile.get(src + col);
                    if p.a <= 0.0 {
                        continue;
                    }
                    rgba.copy_from_slice(&[p.r, p.g, p.b, p.a]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_color::{ColorMode, Depth, Rgba};
    use schist_core::{TileBuf, TILE_PIXELS};
    use std::sync::Arc;

    fn assert_matches_pixel_sampling(tiles: &TileMap, coord: TileCoord, offset: (i32, i32)) {
        let mut actual = vec![0.0; TILE_PIXELS * 4];
        decode(tiles, coord, offset, &mut actual);
        for (i, rgba) in actual.as_chunks::<4>().0.iter().enumerate() {
            let x = coord.tx as i64 * TILE_SIZE as i64 + (i % TILE_SIZE as usize) as i64
                - offset.0 as i64;
            let y = coord.ty as i64 * TILE_SIZE as i64 + (i / TILE_SIZE as usize) as i64
                - offset.1 as i64;
            let p = match (i32::try_from(x), i32::try_from(y)) {
                (Ok(x), Ok(y)) => tiles.pixel(x, y),
                _ => Rgba::TRANSPARENT,
            };
            let expected = if offset != (0, 0) && p.a <= 0.0 {
                [0.0; 4]
            } else {
                [p.r, p.g, p.b, p.a]
            };
            assert_eq!(
                *rgba, expected,
                "coord={coord:?}, offset={offset:?}, pixel={i}"
            );
        }
    }

    #[test]
    fn shifted_tiles_match_point_sampling_across_depths_modes_and_sparse_edges() {
        for mode in [ColorMode::Rgb, ColorMode::Cmyk, ColorMode::Lab] {
            for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
                let mut tiles = TileMap::new();
                for ty in -1..=1 {
                    for tx in -1..=1 {
                        if tx == 1 && ty == 0 {
                            continue; // a missing quadrant must stay transparent
                        }
                        let mut tile = TileBuf::new_in_mode(depth, mode);
                        for i in 0..TILE_PIXELS {
                            tile.set(
                                i,
                                Rgba::new(
                                    (i % 256) as f32 / 255.0,
                                    (i / 256) as f32 / 255.0,
                                    (tx + ty + 2) as f32 / 4.0,
                                    (i % 3) as f32 / 2.0,
                                ),
                            );
                        }
                        tiles.insert(TileCoord { tx, ty }, Arc::new(tile));
                    }
                }
                for offset in [
                    (0, 0),
                    (1, -1),
                    (-1, 1),
                    (53, 87),
                    (-255, -255),
                    (256, -256),
                    (0, 512),
                    (512, 0),
                ] {
                    for coord in [TileCoord { tx: 0, ty: 0 }, TileCoord { tx: -1, ty: 1 }] {
                        assert_matches_pixel_sampling(&tiles, coord, offset);
                    }
                }
            }
        }
    }

    #[test]
    fn extreme_offsets_do_not_wrap_to_unrelated_tiles() {
        let mut tiles = TileMap::new();
        let mut tile = TileBuf::new(Depth::Eight);
        for i in 0..TILE_PIXELS {
            tile.set(i, Rgba::new(1.0, 0.0, 0.0, 1.0));
        }
        let tile = Arc::new(tile);
        for tx in [i32::MIN / TILE_SIZE, i32::MAX / TILE_SIZE, 0] {
            tiles.insert(TileCoord { tx, ty: 0 }, tile.clone());
        }
        for coord in [TileCoord { tx: 0, ty: 0 }, TileCoord { tx: -1, ty: 0 }] {
            for offset in [(i32::MIN, 0), (i32::MAX, 0), (0, i32::MIN), (0, i32::MAX)] {
                assert_matches_pixel_sampling(&tiles, coord, offset);
            }
        }
    }
}
