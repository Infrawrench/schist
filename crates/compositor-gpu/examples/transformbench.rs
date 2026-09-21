//! Cmd+T handle-drag costs, including source preparation and tile replacement.
//! Run with `make bench-layer-transform`.
use schist_color::{Depth, Rgba};
use schist_core::{Document, Layer, TileBuf, TileCoord, TILE_PIXELS};
use schist_plugin_api::{EditorState, Modifiers, PointerInput, ToolCtx, ToolPlugin};
use schist_tools_transform::TransformTool;
use std::{hint::black_box, sync::Arc, time::Instant};

fn input(x: f32, y: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    }
}

fn run(size: u32) {
    let mut doc = Document::new("transform benchmark", size, size, Depth::Eight);
    let mut layer = Layer::new_raster("pixels");
    let mut tile = TileBuf::new(Depth::Eight);
    for i in 0..TILE_PIXELS {
        tile.set(i, Rgba::new((i % 256) as f32 / 255.0, 0.4, 0.7, 0.8));
    }
    let tile = Arc::new(tile);
    for coord in TileCoord::covering(&doc.canvas_rect()) {
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .insert(coord, tile.clone());
    }
    doc.push_layer(layer);
    let mut state = EditorState::default();
    let mut tool = TransformTool::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_activate(&mut ctx);
    tool.on_pointer_down(&mut ctx, input(size as f32, size as f32));
    for (label, scale) in [("shrink", 0.35), ("enlarge", 1.5)] {
        let mut times = Vec::new();
        for frame in 0..7 {
            let xy = size as f32 * scale + frame as f32 * 2.0;
            let start = Instant::now();
            tool.on_pointer_move(&mut ctx, input(xy, xy));
            black_box(&ctx.doc.tree.layers[0]);
            if frame > 1 {
                times.push(start.elapsed());
            }
        }
        times.sort();
        println!(
            "{size}x{size} {label}: {:?}/handle move (median)",
            times[times.len() / 2]
        );
    }
}

fn main() {
    // Match the desktop setup: GPU effects installed when an adapter exists.
    if let Ok(gpu) = schist_compositor_gpu::GpuCompositor::new() {
        println!("GPU: {}", gpu.describe());
        schist_fx::set_backend(Arc::new(gpu.fx()));
    }
    run(2048);
    run(4096);
}
