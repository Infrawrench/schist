//! Generate small spot-bearing files for an independent PSD reader.
use schist_color::{Depth, Rgba};
use schist_core::{Document, InkChannel, Layer, TileCoord};
fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir).unwrap();
    for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
        for psb in [false, true] {
            let mut doc = Document::new("separations", 3, 1, depth);
            let id = doc.push_layer(Layer::new_raster("Process"));
            let mut edit = doc.begin_edit("seed");
            let tile = edit.writable_tile(id, TileCoord::containing(0, 0)).unwrap();
            for i in 0..3 {
                tile.set(i, Rgba::new(0.25, 0.5, 0.75, 0.5));
            }
            edit.commit();
            let mut ink = InkChannel::spot("Cyan — 特別".into(), [0.0, 1.0, 1.0]);
            ink.info.id = 42;
            ink.info.solidity = 0.35;
            ink.pixels.set(1, 0, 0.75);
            ink.pixels.set(2, 0, 1.0);
            doc.ink_channels.push(ink);
            let path = format!(
                "{dir}/spot-{}.{}",
                depth.bytes_per_channel() * 8,
                if psb { "psb" } else { "psd" }
            );
            std::fs::write(path, schist_codec_psd::write_psd_with(&doc, psb).unwrap()).unwrap();
        }
    }
}
