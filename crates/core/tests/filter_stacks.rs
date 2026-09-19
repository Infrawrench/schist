use schist_color::{ColorMode, Depth, NativePixel, Rgba};
use schist_core::{filter_stack::*, *};

fn sample(mode: ColorMode, depth: Depth) -> TileMap {
    let mut source = TileMap::new_in_mode(mode);
    let tile = source.get_mut_or_insert_mode(TileCoord { tx: -1, ty: 0 }, depth, mode);
    let mut pixel = NativePixel::transparent(mode);
    pixel.color = [0.1234567, 0.314159, 0.99997, 0.3456];
    pixel.alpha = 0.876543;
    tile.set_native_pixel(12, pixel);
    let tile = source.get_mut_or_insert_mode(TileCoord { tx: 0, ty: 0 }, depth, mode);
    tile.set_native_pixel(0, pixel);
    source
}
fn layered() -> (Document, LayerId) {
    let mut doc = Document::new("stack", 4, 4, Depth::Sixteen);
    let mut layer = Layer::new_raster("source");
    let source = sample(ColorMode::Rgb, doc.depth);
    layer.as_raster_mut().unwrap().tiles = source.clone();
    let mut stack = FilterStack::new(IntRect::from_size(4, 4));
    stack.effects.push(FilterEffect {
        id: "test".into(),
        enabled: true,
        values: Default::default(),
        foreground: [0.0, 0.0, 0.0, 1.0],
        background: [1.0; 4],
    });
    layer.extras = stack.blocks(&layer, &source).unwrap();
    let id = doc.push_layer(layer);
    (doc, id)
}
#[test]
fn filter_stack_source_roundtrips_native_tiles_and_hidden_samples() {
    for mode in [ColorMode::Rgb, ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
            let source = sample(mode, depth);
            let reopened = decode_source(&encode_source(&source).unwrap()).unwrap();
            assert_eq!(reopened.mode(), mode);
            assert_eq!(source.len(), reopened.len());
            for (coord, tile) in source.iter() {
                assert_eq!(Some(tile), reopened.get(*coord));
            }
        }
    }
}
#[test]
fn filter_stack_destructive_edit_undo_restores_pixels_and_recipe() {
    let (mut doc, id) = layered();
    let before = doc.tree.find(id).unwrap().clone();
    let mut edit = doc.begin_edit("paint");
    edit.writable_tile(id, TileCoord { tx: 0, ty: 0 })
        .unwrap()
        .set(0, Rgba::WHITE);
    edit.commit();
    assert!(!has_stack(doc.tree.find(id).unwrap()));
    assert!(doc.undo().is_some());
    assert_eq!(doc.tree.find(id).unwrap().extras, before.extras);
    assert_eq!(
        doc.tree
            .find(id)
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .pixel(0, 0),
        before.as_raster().unwrap().tiles.pixel(0, 0)
    );
    assert!(doc.redo().is_some());
    assert!(!has_stack(doc.tree.find(id).unwrap()));
}
#[test]
fn filter_stack_stroke_cancel_and_undo_restore_recipe() {
    let (mut doc, id) = layered();
    let original = doc.tree.find(id).unwrap().extras.clone();
    let mut stroke = StrokeEdit::new("stroke");
    stroke
        .writable_tile(&mut doc, id, TileCoord { tx: 0, ty: 0 })
        .unwrap()
        .set(0, Rgba::BLACK);
    assert!(!has_stack(doc.tree.find(id).unwrap()));
    stroke.cancel(&mut doc);
    assert_eq!(doc.tree.find(id).unwrap().extras, original);
    let mut stroke = StrokeEdit::new("stroke");
    stroke
        .writable_tile(&mut doc, id, TileCoord { tx: 0, ty: 0 })
        .unwrap()
        .set(0, Rgba::BLACK);
    stroke.commit(&mut doc);
    doc.undo();
    assert_eq!(doc.tree.find(id).unwrap().extras, original);
}
#[test]
fn filter_stack_move_rasterize_and_mode_conversion_bake_with_undo() {
    for operation in 0..3 {
        let (mut doc, id) = layered();
        doc.tree.find_mut(id).unwrap().smart = Some(Box::new(SmartObject::wrap(
            sample(ColorMode::Rgb, doc.depth),
            "smart",
        )));
        let original = doc.tree.find(id).unwrap().extras.clone();
        let mut edit = doc.begin_edit("bake operation");
        match operation {
            0 => edit.translate_layer(id, 1, 1),
            1 => edit.set_smart_object(id, None),
            _ => edit.set_color_mode(ColorMode::Cmyk),
        }
        edit.commit();
        assert!(!has_stack(doc.tree.find(id).unwrap()));
        doc.undo();
        assert_eq!(doc.tree.find(id).unwrap().extras, original);
    }
}
#[test]
fn filter_stack_cross_document_mode_conversion_bakes_and_same_mode_keeps_stack() {
    let (doc, id) = layered();
    let layer = doc.tree.find(id).unwrap().clone();
    let mut rgb = Document::new("rgb", 4, 4, Depth::Sixteen);
    rgb.push_layer(layer.clone());
    assert!(has_stack(&rgb.tree.layers[0]));
    let mut cmyk = Document::new("cmyk", 4, 4, Depth::Sixteen);
    cmyk.mode = ColorMode::Cmyk;
    cmyk.push_layer(layer);
    assert!(!has_stack(&cmyk.tree.layers[0]));
}
#[test]
fn filter_stack_rejects_corruption_and_pathological_regions() {
    let source = encode_source(&sample(ColorMode::Rgb, Depth::Eight)).unwrap();
    assert!(decode_source(&source[..source.len() / 2]).is_err());
    for region in [
        IntRect::new(i32::MAX - 2, 0, i32::MAX, 1),
        IntRect::from_size(1, 32_000_000),
        IntRect::EMPTY,
    ] {
        assert!(FilterStack::new(region).validate().is_err());
    }
    let mut stack = FilterStack::new(IntRect::from_size(4, 4));
    stack.version = 2;
    assert!(stack.validate().is_err());
}
#[test]
fn filter_stack_history_counts_source_bytes() {
    let mut history = History::new();
    history.byte_limit = 16;
    for i in 0..3 {
        history.push(Edit {
            name: i.to_string(),
            ops: vec![EditOp::LayerExtrasSet {
                layer: LayerId(1),
                before: vec![],
                after: vec![RawBlock {
                    key: SOURCE_KEY,
                    data: vec![1; 20],
                }],
            }],
        });
    }
    assert_eq!(history.pop_undo().unwrap().name, "2");
    assert!(history.pop_undo().is_none());
}

#[test]
fn filter_stack_float_source_preserves_hdr_and_nan_payload_bits() {
    let mut source = TileMap::new();
    let coord = TileCoord { tx: 0, ty: 0 };
    if let TileBuf::F32(samples) = source.get_mut_or_insert(coord, Depth::ThirtyTwo) {
        samples[..4].copy_from_slice(&[12.5, -0.0, f32::from_bits(0x7fc12345), 0.0]);
    }
    let restored = decode_source(&encode_source(&source).unwrap()).unwrap();
    let TileBuf::F32(samples) = restored.get(coord).unwrap().as_ref() else {
        panic!("float source")
    };
    assert_eq!(
        samples[..4].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        vec![
            12.5f32.to_bits(),
            (-0.0f32).to_bits(),
            0x7fc12345,
            0.0f32.to_bits()
        ]
    );
}
