//! Interactive canvas costs, including uploads and readbacks.
//! Run with `make bench-canvas`; no timing assertions.
use schist_color::{Depth, Rgba};
use schist_compositor::{viewport::ViewportParams, Compositor, CpuCompositor};
use schist_compositor_gpu::GpuCompositor;
use schist_core::{Document, IntRect, Layer, TileBuf, TileCoord, TILE_PIXELS};
use std::{hint::black_box, sync::Arc, time::Instant};

fn measure(mut frame: impl FnMut(usize)) -> std::time::Duration {
    for i in 0..3 {
        frame(i);
    }
    let mut times = Vec::new();
    for i in 3..14 {
        let start = Instant::now();
        frame(i);
        times.push(start.elapsed());
    }
    times.sort();
    times[times.len() / 2]
}

fn run(backend: &dyn Compositor, doc: &mut Document) {
    let coords: Vec<_> = TileCoord::covering(&IntRect::new(0, 0, 2048, 1536)).collect();
    let drag = measure(|frame| {
        doc.tree.layers[1].render_offset = (53 + frame as i32, 87);
        black_box(backend.tiles_rgba8(doc, &coords));
    });
    println!(
        "{} drag composite, {} tiles: {drag:?}",
        backend.name(),
        coords.len()
    );
    let grid: Vec<_> = backend
        .tiles_rgba8(doc, &coords)
        .into_iter()
        .map(|tile| Some(Arc::new(tile)))
        .collect();
    for zoom in [1.0, 0.5] {
        let mut params = ViewportParams {
            width: 1920,
            height: 1080,
            origin: (0.0, 0.0),
            zoom,
            scale_factor: 1.0,
            rotation: 0.0,
            canvas: doc.canvas_rect(),
            grid_origin: (0, 0),
            grid_cols: 8,
            grid_rows: 6,
            surround: 0x343434,
        };
        let pan = measure(|frame| {
            params.origin = (-(frame as f32), -8.0);
            black_box(backend.viewport(&params, &grid).unwrap_or_else(|| {
                schist_compositor::viewport::render_viewport_cpu(&params, &grid)
            }));
        });
        println!("{} warm pan at {zoom}x, 1920x1080: {pan:?}", backend.name());
    }
}

fn main() {
    let mut doc = Document::new("canvas benchmark", 4096, 4096, Depth::Eight);
    for alpha in [1.0, 0.6] {
        let mut layer = Layer::new_raster("pixels");
        for ty in 0..16 {
            for tx in 0..16 {
                let mut tile = TileBuf::new(Depth::Eight);
                for i in 0..TILE_PIXELS {
                    tile.set(
                        i,
                        Rgba::new(
                            (i % 256) as f32 / 255.0,
                            tx as f32 / 16.0,
                            ty as f32 / 16.0,
                            alpha,
                        ),
                    );
                }
                layer
                    .as_raster_mut()
                    .unwrap()
                    .tiles
                    .insert(TileCoord { tx, ty }, Arc::new(tile));
            }
        }
        doc.push_layer(layer);
    }
    run(&CpuCompositor, &mut doc);
    match GpuCompositor::new() {
        Ok(gpu) => {
            println!("GPU: {}", gpu.describe());
            run(&gpu, &mut doc);
        }
        Err(error) => println!("GPU unavailable: {error}"),
    }
}
