mod common;
use common::{Psd, Res, L};
use schist_codec_psd::{read_psd, write_psd_with};
use schist_color::{ColorMode, Depth, NativePixel};
use schist_core::{IntRect, TileCoord};

fn sample(value: f32, depth: Depth) -> Vec<u8> {
    match depth {
        Depth::Eight => vec![schist_color::f32_to_u8(value)],
        Depth::Sixteen => schist_color::f32_to_u16(value).to_be_bytes().to_vec(),
        Depth::ThirtyTwo => value.to_be_bytes().to_vec(),
    }
}

#[test]
fn native_import_edit_undo_and_save_reopen_at_every_depth_and_compression() {
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
            for compression in 0..4 {
                let mut fixture = Psd::rgb8(3, 1);
                fixture.mode = if mode == ColorMode::Cmyk { 4 } else { 9 };
                fixture.depth = (depth.bytes_per_channel() * 8) as u16;
                fixture.channels = (mode.channels() + 1) as u16;
                fixture.negative_count = true;
                let mut planes = Vec::new();
                // These separations cannot be recovered by RGB -> CMYK.
                for c in 0..mode.channels() {
                    let v = [0.125, 0.375, 0.75, 0.5][c];
                    planes.push((
                        c as i16,
                        sample(if mode == ColorMode::Cmyk { 1.0 - v } else { v }, depth).repeat(3),
                    ));
                }
                let alpha: Vec<_> = [0.5, 0.0, 1.0]
                    .into_iter()
                    .flat_map(|v| sample(v, depth))
                    .collect();
                planes.push((-1, alpha));
                fixture.layers.push(L {
                    rect: (0, 0, 1, 3),
                    raw_planes: Some(planes),
                    rle: compression == 1,
                    zip: compression >= 2,
                    predict: compression == 3,
                    ..L::default()
                });
                // Profile is opaque metadata to the codec; invalid/unknown
                // profiles must survive just as valid device profiles do.
                let profile = b"native profile bytes preserved verbatim".to_vec();
                fixture.resources.push(Res {
                    id: 1039,
                    name: Vec::new(),
                    data: profile.clone(),
                });
                let mut doc = read_psd(&fixture.build()).unwrap();
                let id = doc.tree.layers[0].id;
                let pixels = |doc: &schist_core::Document| {
                    (0..3)
                        .map(|x| {
                            doc.tree.layers[0]
                                .as_raster()
                                .unwrap()
                                .tiles
                                .native_pixel(x, 0)
                        })
                        .collect::<Vec<_>>()
                };
                let before = pixels(&doc);
                assert_eq!(before[0].mode, mode);
                for (c, expected) in [0.125, 0.375, 0.75, 0.5]
                    .into_iter()
                    .take(mode.channels())
                    .enumerate()
                {
                    assert!((before[0].color[c] - expected).abs() < 0.003);
                }
                assert_eq!(before[1].alpha, 0.0);
                let channel = mode.channels() - 1;
                let mut edit = doc.begin_edit("native channel");
                edit.fill_native_channel(id, channel, 0.875);
                edit.commit();
                let after = pixels(&doc);
                for p in &after {
                    assert!((p.color[channel] - 0.875).abs() < 0.003);
                }
                for (a, b) in after.iter().zip(&before) {
                    assert_eq!(a.alpha, b.alpha);
                    assert_eq!(&a.color[..channel], &b.color[..channel]);
                }
                doc.undo();
                assert_eq!(pixels(&doc), before);
                doc.redo();
                assert_eq!(pixels(&doc), after);
                for psb in [false, true] {
                    let reopened = read_psd(&write_psd_with(&doc, psb).unwrap()).unwrap();
                    assert_eq!(reopened.mode, mode);
                    assert_eq!(reopened.depth, depth);
                    assert_eq!(pixels(&reopened), after);
                    assert_eq!(reopened.icc_profile.as_deref(), Some(profile.as_slice()));
                }
                let merged =
                    schist_compositor::composite_native_region(&doc, IntRect::from_size(3, 1));
                assert_eq!(merged[0], after[0]);
                let tile = doc.tree.layers[0]
                    .as_raster()
                    .unwrap()
                    .tiles
                    .get(TileCoord::containing(0, 0))
                    .unwrap();
                let unchanged = tile.native_pixel(0);
                assert_eq!(unchanged, after[0]);
            }
        }
    }
}

#[test]
fn equal_rgb_does_not_collapse_distinct_separations() {
    let mut doc = schist_core::Document::new("separations", 2, 1, Depth::ThirtyTwo);
    doc.mode = ColorMode::Cmyk;
    let id = doc.push_layer(schist_core::Layer::new_raster("inks"));
    let mut edit = doc.begin_edit("seed");
    let tile = edit.writable_tile(id, TileCoord::containing(0, 0)).unwrap();
    let key = NativePixel {
        mode: ColorMode::Cmyk,
        color: [0.0, 0.0, 0.0, 0.5],
        alpha: 1.0,
    };
    let process = NativePixel {
        color: [0.5, 0.5, 0.5, 0.0],
        ..key
    };
    assert_eq!(key.to_rgba(), process.to_rgba());
    tile.set_native_pixel(0, key);
    tile.set_native_pixel(1, process);
    edit.commit();
    let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    let tiles = &back.tree.layers[0].as_raster().unwrap().tiles;
    assert_eq!(tiles.native_pixel(0, 0), key);
    assert_eq!(tiles.native_pixel(1, 0), process);
}

#[test]
fn transparent_native_channels_survive_pruning_translation_and_save() {
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        let mut doc = schist_core::Document::new("hidden samples", 3, 1, Depth::ThirtyTwo);
        doc.mode = mode;
        let id = doc.push_layer(schist_core::Layer::new_raster("transparent"));
        let mut edit = doc.begin_edit("native");
        let p = NativePixel {
            mode,
            color: [
                0.25,
                0.5,
                0.75,
                if mode == ColorMode::Cmyk { 0.5 } else { 0.0 },
            ],
            alpha: 0.0,
        };
        edit.writable_tile(id, TileCoord::containing(0, 0))
            .unwrap()
            .set_native_pixel(0, p);
        edit.commit();
        let raster = doc.tree.layers[0].as_raster_mut().unwrap();
        raster.tiles.prune_blank();
        raster.tiles = raster.tiles.translated(1, 0, Depth::ThirtyTwo);
        let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
        assert_eq!(
            back.tree.layers[0]
                .as_raster()
                .unwrap()
                .tiles
                .native_pixel(1, 0),
            p
        );
    }
}
