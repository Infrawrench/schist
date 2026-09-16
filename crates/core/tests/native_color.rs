use schist_color::{ColorMode, Depth, NativePixel, Rgba};
use schist_core::{Affine, Document, Filter, IntRect, Layer, TileCoord, TileMap};

fn document(mode: ColorMode, depth: Depth) -> Document {
    let mut doc = Document::new("native", 2, 1, depth);
    doc.mode = mode;
    let id = doc.push_layer(Layer::new_raster("inks"));
    let mut edit = doc.begin_edit("seed");
    let tile = edit.writable_tile(id, TileCoord::containing(0, 0)).unwrap();
    tile.set_native_pixel(
        0,
        NativePixel {
            mode,
            color: [
                0.25,
                0.5,
                0.75,
                if mode == ColorMode::Cmyk { 0.5 } else { 0.0 },
            ],
            alpha: 0.5,
        },
    );
    edit.commit();
    doc
}

fn pixel(doc: &Document) -> NativePixel {
    doc.tree.layers[0]
        .as_raster()
        .unwrap()
        .tiles
        .native_pixel(0, 0)
}

#[test]
fn independent_channels_alpha_undo_redo_cancel_and_mode_conversion() {
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
            let mut doc = document(mode, depth);
            let id = doc.active_layer.unwrap();
            let before = pixel(&doc);
            let snapshot = doc.tree.layers[0].as_raster().unwrap().tiles.clone();
            for channel in 0..mode.channels() {
                let mut edit = doc.begin_edit("channel");
                assert!(edit.fill_native_channel(id, channel, 1.0));
                edit.commit();
                let edited = pixel(&doc);
                assert_eq!(edited.color[channel], 1.0);
                for c in 0..mode.channels() {
                    if c != channel {
                        assert_eq!(edited.color[c], before.color[c]);
                    }
                }
                assert_eq!(edited.alpha, before.alpha);
                assert_eq!(
                    snapshot.native_pixel(0, 0),
                    before,
                    "COW snapshots retain native samples"
                );
                doc.undo();
                assert_eq!(pixel(&doc), before);
                doc.redo();
                assert_eq!(pixel(&doc), edited);
                doc.undo();
            }
            let mut edit = doc.begin_edit("cancel");
            edit.fill_native_channel(id, 0, 0.0);
            edit.cancel();
            assert_eq!(pixel(&doc), before);
            let mut edit = doc.begin_edit("RGB");
            edit.set_color_mode(ColorMode::Rgb);
            edit.commit();
            assert_eq!(pixel(&doc).mode, ColorMode::Rgb);
            doc.undo();
            assert_eq!(doc.mode, mode);
            assert_eq!(pixel(&doc), before);
        }
    }
}

#[test]
fn rgb_identity_and_alpha_only_writes_keep_native_samples() {
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        let mut doc = document(mode, Depth::ThirtyTwo);
        let id = doc.active_layer.unwrap();
        let before = pixel(&doc);
        let mut edit = doc.begin_edit("RGB compatibility");
        let tile = edit.writable_tile(id, TileCoord::containing(0, 0)).unwrap();
        let rgb = tile.get(0);
        tile.set(0, rgb);
        assert_eq!(tile.native_pixel(0), before);
        tile.set(0, Rgba { a: 0.25, ..rgb });
        assert_eq!(tile.native_pixel(0).color, before.color);
        assert_eq!(tile.native_pixel(0).alpha, 0.25);
        edit.commit();
        doc.undo();
        assert_eq!(pixel(&doc), before);
    }
}

#[test]
fn translation_and_resampling_keep_four_inks() {
    let doc = document(ColorMode::Cmyk, Depth::ThirtyTwo);
    let src = &doc.tree.layers[0].as_raster().unwrap().tiles;
    let before = src.native_pixel(0, 0);
    for (dx, dy) in [(1, -1), (256, 256)] {
        let moved = src.translated(dx, dy, Depth::ThirtyTwo);
        assert_eq!(moved.native_pixel(dx, dy), before);
    }
    for filter in [Filter::Nearest, Filter::Bilinear, Filter::Bicubic] {
        let resized = schist_core::resample::transform_tiles(
            src,
            &Affine::scale(2.0, 2.0),
            Depth::ThirtyTwo,
            filter,
            IntRect::from_size(4, 2),
        );
        assert_eq!(resized.native_pixel(0, 0), before);
    }
    let empty = TileMap::new_in_mode(ColorMode::Cmyk);
    assert_eq!(empty.native_pixel(0, 0).mode, ColorMode::Cmyk);
}
