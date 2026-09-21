//! CPU canvas-drag throughput. Run with `make bench-layer-drag`.
//! Measures compositing only; thumbnail refresh and viewport presentation
//! are additional costs in the editor.

// Keep the sampling microbenchmark on the actual private implementation.
#[path = "../src/shifted.rs"]
mod shifted;

use schist_color::{Depth, Rgba};
use schist_compositor::composite_tile_cpu;
use schist_core::{Document, Layer, TileBuf, TileCoord, TileMap, TILE_PIXELS, TILE_SIZE};
use std::{hint::black_box, sync::Arc, time::Instant};

fn main() {
    let mut doc = Document::new("drag benchmark", 4096, 4096, Depth::Eight);
    for alpha in [1.0, 0.6] {
        let mut layer = Layer::new_raster("pixels");
        let mut tile = TileBuf::new(Depth::Eight);
        for i in 0..TILE_PIXELS {
            tile.set(i, Rgba::new((i % 256) as f32 / 255.0, 0.4, 0.7, alpha));
        }
        let tile = Arc::new(tile);
        for ty in 0..16 {
            for tx in 0..16 {
                layer
                    .as_raster_mut()
                    .unwrap()
                    .tiles
                    .insert(TileCoord { tx, ty }, tile.clone());
            }
        }
        doc.push_layer(layer);
    }
    let coord = TileCoord { tx: 4, ty: 4 };
    for offset in [(0, 0), (1, -1), (53, 87), (256, -256)] {
        doc.tree.layers[1].render_offset = offset;
        for _ in 0..8 {
            black_box(composite_tile_cpu(&doc, coord));
        }
        let mut times = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            for _ in 0..64 {
                black_box(composite_tile_cpu(black_box(&doc), coord));
            }
            times.push(start.elapsed() / 64);
        }
        times.sort();
        println!("offset {offset:?}: {:?}/tile (median)", times[2]);
    }

    // Alternate old/new sampling in one process, independent of blending,
    // allocations and thread-pool scheduling. Do not impose timing asserts.
    let tiles = &doc.tree.layers[1].as_raster().unwrap().tiles;
    let offset = (53, 87);
    let mut out = vec![0.0; TILE_PIXELS * 4];
    let samplers = [point_sample, shifted::decode];
    let mut times = [Vec::new(), Vec::new()];
    for round in 0..10 {
        for which in [round % 2, 1 - round % 2] {
            let start = Instant::now();
            for _ in 0..64 {
                out.fill(0.0);
                samplers[which](black_box(tiles), coord, offset, black_box(&mut out));
                black_box(&out);
            }
            times[which].push(start.elapsed() / 64);
        }
    }
    for samples in &mut times {
        samples.sort();
    }
    println!(
        "shifted sampling: {:?} -> {:?}/tile ({:.2}x faster)",
        times[0][5],
        times[1][5],
        times[0][5].as_secs_f64() / times[1][5].as_secs_f64()
    );
}

/// The original drag sampler, retained only as a benchmark baseline.
fn point_sample(tiles: &TileMap, coord: TileCoord, offset: (i32, i32), out: &mut [f32]) {
    let rect = coord.rect();
    for i in 0..TILE_PIXELS {
        let x = rect.left + i as i32 % TILE_SIZE - offset.0;
        let y = rect.top + i as i32 / TILE_SIZE - offset.1;
        let p = tiles.pixel(x, y);
        if p.a <= 0.0 {
            continue;
        }
        out[i * 4..i * 4 + 4].copy_from_slice(&[p.r, p.g, p.b, p.a]);
    }
}
