//! Run the Remove tool over a photograph, the way a stroke on the canvas
//! does, and write the result -- for looking at, which is the only real
//! test of a fill.
//!
//! ```sh
//! cargo run --release -p schist-tools-retouch --example remove_demo -- \
//!     in.ppm out.ppm <brush size> x,y x,y x,y ...
//! ```
//!
//! Binary PPM (P6) in and out; convert with any image tool. The points
//! are one stroke, in image pixels.

use schist_color::{Depth, Rgba};
use schist_core::{Document, Layer, TileCoord, TILE_SIZE};
use schist_plugin_api::{EditorState, Modifiers, PointerInput, ToolCtx, ToolPlugin};
use schist_tools_retouch::RemoveTool;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() >= 4,
        "usage: remove_demo in.ppm out.ppm <size> x,y [x,y ...]"
    );
    let bytes = std::fs::read(&args[0]).expect("read input");
    let header: Vec<&[u8]> = bytes
        .split(|b| b.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .take(4)
        .collect();
    assert_eq!(header[0], b"P6", "binary PPM only");
    let parse = |s: &[u8]| std::str::from_utf8(s).unwrap().parse::<usize>().unwrap();
    let (w, h) = (parse(header[1]), parse(header[2]));
    let pixels = &bytes[bytes.len() - w * h * 3..];

    let mut doc = Document::new("demo", w as u32, h as u32, Depth::Eight);
    let layer = Layer::new_raster("photo");
    let id = layer.id;
    doc.push_layer(layer);
    {
        let tiles = &mut doc
            .tree
            .find_mut(id)
            .unwrap()
            .as_raster_mut()
            .unwrap()
            .tiles;
        for y in 0..h {
            for x in 0..w {
                let p = &pixels[(y * w + x) * 3..];
                let coord = TileCoord::containing(x as i32, y as i32);
                let r = coord.rect();
                tiles.get_mut_or_insert(coord, Depth::Eight).set(
                    ((y as i32 - r.top) * TILE_SIZE + (x as i32 - r.left)) as usize,
                    Rgba::new(
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                        1.0,
                    ),
                );
            }
        }
    }
    doc.active_layer = Some(id);

    let mut state = EditorState {
        brush_size: args[2].parse().expect("brush size"),
        ..EditorState::default()
    };
    let points: Vec<(f32, f32)> = args[3..]
        .iter()
        .map(|p| {
            let (x, y) = p.split_once(',').expect("x,y");
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect();
    let input = |(x, y): (f32, f32)| PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    let mut tool = RemoveTool::default_tool();
    let started = std::time::Instant::now();
    {
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(points[0]));
        for &p in &points[1..] {
            tool.on_pointer_move(&mut ctx, input(p));
        }
        tool.on_pointer_up(&mut ctx, input(points[points.len() - 1]));
    }
    eprintln!("removed in {:.2}s", started.elapsed().as_secs_f32());

    let tiles = &doc.tree.find(id).unwrap().as_raster().unwrap().tiles;
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            let [r, g, b, _] = tiles.pixel(x as i32, y as i32).to_u8();
            out.extend([r, g, b]);
        }
    }
    std::fs::write(&args[1], out).expect("write output");
}
