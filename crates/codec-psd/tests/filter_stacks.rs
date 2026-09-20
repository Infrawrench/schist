use schist_color::{Depth, Rgba};
use schist_core::{filter_stack::*, *};

#[test]
fn filter_stack_psd_and_psb_preserve_source_recipe_and_visible_cache() {
    for psb in [false, true] {
        let mut doc = Document::new("filters", 3, 2, Depth::Sixteen);
        let mut layer = Layer::new_raster("Filtered");
        let mut source = TileMap::new();
        source
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
            .set(0, Rgba::new(0.12345, 0.45678, 0.89123, 1.0));
        let mut stack = FilterStack::new(doc.canvas_rect());
        stack.effects.push(FilterEffect {
            id: "filter.gaussian_blur".into(),
            enabled: true,
            values: [("radius".into(), 2.75)].into(),
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
        });
        layer.extras = stack.blocks(&layer, &source).unwrap();
        // The public raster must stay visibly different from the immutable source.
        layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
            .set(0, Rgba::new(0.8, 0.3, 0.1, 1.0));
        layer.smart = Some(Box::new(SmartObject::wrap(
            layer.as_raster().unwrap().tiles.clone(),
            "filtered source",
        )));
        doc.push_layer(layer);
        let bytes = schist_codec_psd::write_psd_with(&doc, psb).unwrap();
        let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
        let layer = &reopened.tree.layers[0];
        assert_eq!(FilterStack::read(layer).unwrap(), Some(stack));
        let restored = read_source(layer).unwrap();
        assert_eq!(
            restored.get(TileCoord { tx: 0, ty: 0 }),
            source.get(TileCoord { tx: 0, ty: 0 })
        );
        assert!(layer.as_raster().unwrap().tiles.pixel(0, 0).r > 0.79);
        assert!(layer.smart.is_some());
        let again =
            schist_codec_psd::read_psd(&schist_codec_psd::write_psd_with(&reopened, psb).unwrap())
                .unwrap();
        assert_eq!(again.tree.layers[0].extras, layer.extras);
    }
}

#[test]
fn filter_stack_placed_raster_psd_psb_reopens_and_can_transform_without_resampling_twice() {
    for psb in [false, true] {
        let mut doc = Document::new("placed", 8, 8, Depth::Sixteen);
        let mut layer = Layer::new_raster("source");
        let mut source = TileMap::new();
        source
            .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
            .set(0, Rgba::new(0.12345, 0.45678, 0.89123, 1.0));
        layer.as_raster_mut().unwrap().tiles = source.clone();
        layer.extras = FilterStack::new(doc.canvas_rect())
            .blocks(&layer, &source)
            .unwrap();
        let id = doc.push_layer(layer);
        let mut edit = doc.begin_edit("scale down");
        edit.transform_layer(
            id,
            &Affine::scale(0.25, 0.25),
            Filter::Bicubic,
            IntRect::from_size(8, 8),
        );
        edit.commit();
        let extras = doc.tree.find(id).unwrap().extras.clone();
        let mut reopened =
            schist_codec_psd::read_psd(&schist_codec_psd::write_psd_with(&doc, psb).unwrap())
                .unwrap();
        let layer = &reopened.tree.layers[0];
        assert_eq!(layer.extras, extras);
        assert!(layer.extras.iter().any(|b| b.key == CACHE_KEY));
        let id = layer.id;
        let mut edit = reopened.begin_edit("scale back up");
        edit.transform_layer(
            id,
            &Affine::scale(4.0, 4.0),
            Filter::Bicubic,
            IntRect::from_size(8, 8),
        );
        edit.commit();
        let layer = reopened.tree.find(id).unwrap();
        assert_eq!(
            layer
                .as_raster()
                .unwrap()
                .tiles
                .get(TileCoord { tx: 0, ty: 0 }),
            source.get(TileCoord { tx: 0, ty: 0 })
        );
        assert_eq!(
            FilterStack::read(layer)
                .unwrap()
                .unwrap()
                .placement
                .unwrap()
                .matrix,
            Affine::IDENTITY
        );
    }
}
