use schist_codec_psd::{read_psd, write_psd};
use schist_color::{Depth, Rgba};
use schist_core::smart_source::{SmartSource, SourceIdentity};
use schist_core::{Document, Layer, SmartObject, TileCoord};

#[test]
fn nested_editable_layers_missing_link_and_placement_survive_save_and_recovery() {
    let mut nested = Document::new("source", 3, 4, Depth::Sixteen);
    nested.push_layer(Layer::new_raster("editable foreground"));
    nested.push_layer(Layer::new_raster("editable background"));
    let metadata = SmartSource {
        identity: SourceIdentity {
            version: 1,
            id: "instance-family".into(),
            linked_path: Some("/does-not-exist/art.psd".into()),
            origin: [-1, 3],
            linked_stamp: None,
        },
        document: write_psd(&nested).unwrap(),
    };
    let mut parent = Document::new("parent", 16, 16, Depth::Sixteen);
    let mut layer = Layer::new_raster("instance");
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::Sixteen)
        .set(0, Rgba::WHITE);
    let mut smart = SmartObject::wrap(layer.as_raster().unwrap().tiles.clone(), "source");
    smart.transform.tx = 7.0;
    layer.smart = Some(Box::new(smart));
    layer.extras = metadata.blocks(&layer).unwrap();
    parent.push_layer(layer);
    // Recovery uses the same PSD encoding. Merely reopening must never read a link.
    let reopened = read_psd(&write_psd(&parent).unwrap()).unwrap();
    let layer = &reopened.tree.layers[0];
    let source = SmartSource::read(layer).unwrap().unwrap();
    assert_eq!(source, metadata);
    assert_eq!(layer.smart.as_ref().unwrap().transform.tx, 7.0);
    let contents = read_psd(&source.document).unwrap();
    assert_eq!(contents.tree.layers.len(), 2);
    assert_eq!(contents.tree.layers[0].name, "editable foreground");
    assert_eq!(contents.depth, Depth::Sixteen);
}

#[test]
fn native_ink_separations_and_fully_transparent_smart_sources_survive() {
    use schist_color::{ColorMode, NativePixel};
    use schist_core::TileMap;
    let mut doc = Document::new("native", 2, 2, Depth::Sixteen);
    doc.mode = ColorMode::Cmyk;
    let mut source = TileMap::new_in_mode(ColorMode::Cmyk);
    let ink = NativePixel {
        mode: ColorMode::Cmyk,
        color: [0.0, 0.0, 0.0, 0.8],
        alpha: 1.0,
    };
    source
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::ThirtyTwo)
        .set_native_pixel(0, ink);
    let mut layer = Layer::new_raster("K only");
    layer.smart = Some(Box::new(SmartObject::wrap(source, "inks")));
    doc.push_layer(layer);
    let mut empty = Layer::new_raster("empty editable source");
    empty.smart = Some(Box::new(SmartObject::wrap(TileMap::new(), "empty")));
    doc.push_layer(empty);
    let reopened = read_psd(&write_psd(&doc).unwrap()).unwrap();
    assert_eq!(
        reopened.tree.layers[0]
            .smart
            .as_ref()
            .unwrap()
            .source
            .native_pixel(0, 0),
        ink
    );
    assert!(reopened.tree.layers[1].smart.is_some());
}
