//! Reproducible stroke workload, also callable from Node after wasm-bindgen.
use schist_color::{Depth, Rgba};
use schist_core::{Document, Layer};
use schist_plugin_api::{EditorState, PointerInput, ToolCtx};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn paint_benchmark(diameter: f32) -> u32 {
    let mut doc = Document::new("benchmark", 1024, 1024, Depth::Eight);
    doc.push_layer(Layer::new_raster("paint"));
    let mut state = EditorState {
        brush_size: diameter,
        brush_hardness: 0.7,
        tool_opacity: 0.5,
        foreground: Rgba::new(0.2, 0.4, 0.8, 1.0),
        ..Default::default()
    };
    let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
    let input = |i: u32| PointerInput {
        x: 150.0 + i as f32 * 6.0,
        y: 512.0 + (i as f32 * 0.1).sin() * 200.0,
        pressure: 0.5 + (i % 20) as f32 / 40.0,
        modifiers: Default::default(),
    };
    for _ in 0..4 {
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(0));
        for i in 1..120 {
            tool.on_pointer_move(&mut ctx, input(i));
            ctx.doc.take_damage();
        }
        tool.on_pointer_up(&mut ctx, input(120));
    }
    let raster = doc.tree.layers[0].as_raster().unwrap();
    // Include every pixel in a stable order so the benchmark also catches
    // accidental changes to stroke output across optimizations.
    let mut checksum = 0u32;
    for y in 0..1024 {
        for x in 0..1024 {
            for v in raster.tiles.pixel(x, y).to_u8() {
                checksum = checksum.wrapping_mul(31).wrapping_add(v as u32);
            }
        }
    }
    checksum
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    for diameter in [32.0, 128.0, 300.0] {
        let started = std::time::Instant::now();
        let checksum = paint_benchmark(diameter);
        println!("{diameter}px: {:?}, checksum {checksum}", started.elapsed());
    }
}
