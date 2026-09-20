use schist_color::{Depth, Rgba};
use schist_core::{filter_stack::*, *};

fn filtered_document() -> Document {
    let mut doc = Document::new("native filter export", 4, 3, Depth::Sixteen);
    let mut layer = Layer::new_raster("Editable blur");
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
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
        .set(0, Rgba::new(0.8, 0.3, 0.1, 1.0));
    doc.push_layer(layer);
    doc
}

#[test]
fn native_export_embeds_source_and_reopens_without_private_filter_blocks() {
    for psb in [false, true] {
        let doc = filtered_document();
        let bytes = schist_codec_psd::write_psd_with(&doc, psb).unwrap();
        if let Some(dir) = std::env::var_os("SCHIST_NATIVE_PSD_EXPORT_DIR") {
            let path = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(if psb {
                    "native-filters.psb"
                } else {
                    "native-filters.psd"
                }),
                &bytes,
            )
            .unwrap();
        }
        let mut reopened = schist_codec_psd::read_psd(&bytes).unwrap();
        assert!(reopened.tree.layers[0]
            .extras
            .iter()
            .any(|b| b.key == *b"SoLd"));
        assert!(reopened
            .preserved_layer_info
            .iter()
            .any(|b| b.key == *b"lnk2"));
        // Hide private block signatures in the on-disk file to exercise the
        // public native interchange path, without allowing the writer to bake.
        let mut native_only = bytes.clone();
        for at in 0..native_only.len().saturating_sub(8) {
            if &native_only[at..at + 4] == b"8BIM"
                && matches!(&native_only[at + 4..at + 8], b"ScFs" | b"ScFo" | b"ScFi")
            {
                native_only[at + 4..at + 8].copy_from_slice(b"Test");
            }
        }
        let native = schist_codec_psd::read_psd(&native_only).unwrap();
        let stack = FilterStack::read(&native.tree.layers[0])
            .unwrap()
            .expect("native recipe imported");
        assert_eq!(stack.effects[0].values["radius"], 2.75);
        assert!(
            (read_source(&native.tree.layers[0]).unwrap().pixel(0, 0).r - 0.12345).abs() < 0.0001
        );
        assert!(
            native.tree.layers[0]
                .as_raster()
                .unwrap()
                .tiles
                .pixel(0, 0)
                .r
                > 0.79
        );

        // Subsequent parameter edits regenerate owned SoLd and reuse the source.
        let link_count = reopened.preserved_layer_info.len();
        let layer = &mut reopened.tree.layers[0];
        let source = read_source(layer).unwrap();
        let mut stack = FilterStack::read(layer).unwrap().unwrap();
        stack.effects[0].values.insert("radius".into(), 7.5);
        layer.extras = stack.blocks(layer, &source).unwrap();
        let again =
            schist_codec_psd::read_psd(&schist_codec_psd::write_psd_with(&reopened, psb).unwrap())
                .unwrap();
        assert_eq!(again.preserved_layer_info.len(), link_count);
        let block = again.tree.layers[0]
            .extras
            .iter()
            .find(|b| b.key == *b"SoLd")
            .unwrap();
        let desc = schist_psd_descriptor::parse(&block.data[12..]).unwrap();
        let fx = desc.get("filterFX").unwrap().as_object().unwrap();
        let item = fx.get("filterFXList").unwrap().as_list().unwrap()[0]
            .as_object()
            .unwrap();
        assert_eq!(
            item.get("Fltr")
                .unwrap()
                .as_object()
                .unwrap()
                .number("Rds "),
            Some(7.5)
        );
    }
}

#[test]
fn independent_ag_psd_fixture_imports_source_order_parameters_and_disabled_state() {
    let bytes = include_bytes!("fixtures/native-smart-filters-ag-psd.psd");
    let doc = schist_codec_psd::read_psd(bytes).unwrap();
    let layer = &doc.tree.layers[0];
    let stack = FilterStack::read(layer)
        .unwrap()
        .expect("independently authored native filters");
    assert_eq!(stack.region, IntRect::new(2, 1, 5, 3));
    assert_eq!(stack.effects[0].id, "filter.gaussian_blur");
    assert_eq!(stack.effects[0].values["radius"], 2.25);
    assert!(stack.effects[0].enabled);
    assert_eq!(stack.effects[1].id, "filter.unsharp_mask");
    assert_eq!(stack.effects[1].values["amount"], 125.0);
    assert_eq!(stack.effects[1].values["threshold"], 7.0);
    assert!(!stack.effects[1].enabled);
    assert_eq!(stack.effects[2].id, "filter.motion_blur");
    assert_eq!(stack.effects[2].values["distance"], 12.0);
    assert_eq!(stack.effects[2].values["angle"], 30.0);
    assert_eq!(stack.effects[3].id, "filter.median");
    assert_eq!(stack.effects[3].values["radius"], 2.0);
    assert_eq!(stack.effects[4].id, "filter.high_pass");
    assert_eq!(stack.effects[4].values["radius"], 4.5);
    let source = read_source(layer).unwrap();
    assert_eq!(source.pixel(2, 1), Rgba::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(source.pixel(3, 1), Rgba::new(0.0, 1.0, 0.0, 1.0));
    assert_ne!(
        source.pixel(2, 1),
        layer.as_raster().unwrap().tiles.pixel(2, 1)
    );
}

#[test]
fn bake_drops_owned_native_placement_and_unsupported_stacks_stay_private() {
    let doc = filtered_document();
    let mut reopened =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    reopened.tree.layers[0].extras = without_stack(&reopened.tree.layers[0].extras);
    let baked =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&reopened).unwrap()).unwrap();
    assert!(!baked.tree.layers[0]
        .extras
        .iter()
        .any(|b| matches!(&b.key, b"SoLd" | b"SoLE" | b"PlLd" | b"ScFi")));
    assert!(!has_stack(&baked.tree.layers[0]));

    let mut unsupported = filtered_document();
    let layer = &mut unsupported.tree.layers[0];
    let source = read_source(layer).unwrap();
    let mut stack = FilterStack::read(layer).unwrap().unwrap();
    stack.effects[0].id = "filter.custom_external".into();
    layer.extras = stack.blocks(layer, &source).unwrap();
    let saved =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&unsupported).unwrap()).unwrap();
    assert_eq!(
        FilterStack::read(&saved.tree.layers[0]).unwrap(),
        Some(stack)
    );
    assert!(!saved.tree.layers[0]
        .extras
        .iter()
        .any(|b| b.key == *b"SoLd"));
    assert!(!saved.preserved_layer_info.iter().any(|b| b.key == *b"lnk2"));
}

#[test]
fn unknown_original_smart_objects_are_preserved_without_becoming_owned() {
    let mut doc = filtered_document();
    let original = RawBlock {
        key: *b"SoLd",
        data: b"unsupported native descriptor".to_vec(),
    };
    doc.tree.layers[0].extras.push(original.clone());
    let doc = schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    assert!(doc.tree.layers[0].extras.contains(&original));
    assert!(!doc.tree.layers[0].extras.iter().any(|b| b.key == *b"ScFi"));
}

#[test]
fn changed_sources_get_distinct_links_without_hiding_previous_objects() {
    let doc = filtered_document();
    let mut reopened =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    let original_link = reopened
        .preserved_layer_info
        .iter()
        .find(|b| b.key == *b"lnk2")
        .unwrap()
        .data
        .clone();
    let mut second = reopened.tree.layers[0].clone();
    second.id = LayerId::next();
    second.name = "Second immutable source".into();
    let mut source = read_source(&second).unwrap();
    source
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
        .set(0, Rgba::new(0.9, 0.1, 0.2, 1.0));
    let stack = FilterStack::read(&second).unwrap().unwrap();
    // blocks deliberately reuses the source for parameter edits. Source replacement
    // removes ScFo before storing the new immutable source.
    second.extras.retain(|b| b.key != SOURCE_KEY);
    second.extras = stack.blocks(&second, &source).unwrap();
    reopened.push_layer(second);
    let result =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&reopened).unwrap()).unwrap();
    let links: Vec<_> = result
        .preserved_layer_info
        .iter()
        .filter(|b| b.key == *b"lnk2")
        .collect();
    assert_eq!(links.len(), 1);
    assert!(links[0].data.starts_with(&original_link));
    assert!(links[0].data.len() > original_link.len());
    let ids: Vec<_> = result
        .tree
        .layers
        .iter()
        .map(|layer| {
            let block = layer.extras.iter().find(|b| b.key == *b"SoLd").unwrap();
            schist_psd_descriptor::parse(&block.data[12..])
                .unwrap()
                .get("Idnt")
                .unwrap()
                .as_text()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn profile_and_resolution_changes_cannot_reuse_stale_embedded_sources() {
    let doc = filtered_document();
    let mut reopened =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    let original = reopened.tree.layers[0]
        .extras
        .iter()
        .find(|b| b.key == *b"SoLd")
        .unwrap();
    let original_id = schist_psd_descriptor::parse(&original.data[12..])
        .unwrap()
        .get("Idnt")
        .unwrap()
        .as_text()
        .unwrap()
        .to_owned();
    reopened.resolution_dpi = 300.0;
    reopened.icc_profile = Some(b"independent profile bytes".to_vec());
    let result =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&reopened).unwrap()).unwrap();
    let placed = result.tree.layers[0]
        .extras
        .iter()
        .find(|b| b.key == *b"SoLd")
        .unwrap();
    let descriptor = schist_psd_descriptor::parse(&placed.data[12..]).unwrap();
    assert_ne!(
        descriptor.get("Idnt").unwrap().as_text().unwrap(),
        original_id
    );
    assert_eq!(descriptor.number("Rslt"), Some(300.0));
}

#[test]
fn native_affine_corners_keep_original_source_dimensions_and_filter_values() {
    for raster_placement in [false, true] {
        let mut doc = filtered_document();
        let matrix = Affine {
            a: 1.0,
            b: 0.5,
            c: -0.25,
            d: 2.0,
            tx: 7.0,
            ty: -3.0,
        };
        let layer = &mut doc.tree.layers[0];
        if raster_placement {
            // Version 2 is the separate raster-stack placement extension. This
            // checks its on-disk contract without requiring that PR's core API.
            let block = layer
                .extras
                .iter_mut()
                .find(|b| b.key == STACK_KEY)
                .unwrap();
            let mut data: serde_json::Value = serde_json::from_slice(&block.data).unwrap();
            data["version"] = 2.into();
            data["placement"] = serde_json::json!({ "matrix": {
                "a": matrix.a, "b": matrix.b, "c": matrix.c, "d": matrix.d, "tx": matrix.tx, "ty": matrix.ty,
            }, "filter": "Bicubic" });
            block.data = serde_json::to_vec(&data).unwrap();
        } else {
            let mut smart = SmartObject::wrap(read_source(layer).unwrap(), "native source");
            smart.transform = matrix;
            layer.smart = Some(Box::new(smart));
        }
        let bytes = schist_codec_psd::write_psd(&doc).unwrap();
        let result = schist_codec_psd::read_psd(&bytes).unwrap();
        let native = result.tree.layers[0]
            .extras
            .iter()
            .find(|b| b.key == *b"SoLd")
            .unwrap();
        let descriptor = schist_psd_descriptor::parse(&native.data[12..]).unwrap();
        let transform: Vec<_> = descriptor
            .get("Trnf")
            .unwrap()
            .as_list()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(transform, [7.0, -3.0, 11.0, -1.0, 10.25, 5.0, 6.25, 3.0]);
        assert_eq!(
            descriptor
                .get("Sz  ")
                .unwrap()
                .as_object()
                .unwrap()
                .number("Wdth"),
            Some(4.0)
        );
        let item = descriptor
            .get("filterFX")
            .unwrap()
            .as_object()
            .unwrap()
            .get("filterFXList")
            .unwrap()
            .as_list()
            .unwrap()[0]
            .as_object()
            .unwrap();
        assert_eq!(
            item.get("Fltr")
                .unwrap()
                .as_object()
                .unwrap()
                .number("Rds "),
            Some(2.75)
        );
        if let Some(dir) = std::env::var_os("SCHIST_NATIVE_PSD_EXPORT_DIR") {
            let path = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(if raster_placement {
                    "raster-affine-filters.psd"
                } else {
                    "smart-affine-filters.psd"
                }),
                bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn additional_native_mappings_and_adjustable_sharpen_preserve_parameters() {
    let mut doc = filtered_document();
    let layer = &mut doc.tree.layers[0];
    let source = read_source(layer).unwrap();
    let mut stack = FilterStack::read(layer).unwrap().unwrap();
    for (id, values) in [
        (
            "filter.motion_blur",
            vec![("distance", 12.0), ("angle", 30.0)],
        ),
        ("filter.median", vec![("radius", 2.0)]),
        ("filter.high_pass", vec![("radius", 4.5)]),
        ("filter.sharpen", vec![("amount", 175.0)]),
    ] {
        stack.effects.push(FilterEffect {
            id: id.into(),
            enabled: true,
            values: values
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            foreground: [0.0, 0.0, 0.0, 1.0],
            background: [1.0; 4],
        });
    }
    layer.extras = stack.blocks(layer, &source).unwrap();
    let bytes = schist_codec_psd::write_psd(&doc).unwrap();
    let result = schist_codec_psd::read_psd(&bytes).unwrap();
    let native = result.tree.layers[0]
        .extras
        .iter()
        .find(|b| b.key == *b"SoLd")
        .unwrap();
    let descriptor = schist_psd_descriptor::parse(&native.data[12..]).unwrap();
    let items = descriptor
        .get("filterFX")
        .unwrap()
        .as_object()
        .unwrap()
        .get("filterFXList")
        .unwrap()
        .as_list()
        .unwrap();
    assert_eq!(items.len(), 5);
    let sharpen = items[0]
        .as_object()
        .unwrap()
        .get("Fltr")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(sharpen.class, "UnsM");
    assert_eq!(sharpen.number("Amnt"), Some(175.0));
    assert_eq!(sharpen.number("Rds "), Some(1.0));
    assert_eq!(sharpen.number("Thsh"), Some(0.0));
    if let Some(dir) = std::env::var_os("SCHIST_NATIVE_PSD_EXPORT_DIR") {
        let path = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("more-native-filters.psd"), bytes).unwrap();
    }
}

#[test]
fn native_source_with_different_profile_stays_opaque() {
    let bytes = include_bytes!("fixtures/native-smart-filters-ag-psd.psd");
    let mut doc = schist_codec_psd::read_psd(bytes).unwrap();
    doc.tree.layers[0]
        .extras
        .retain(|b| !matches!(&b.key, b"ScFs" | b"ScFo" | b"ScFi"));
    doc.icc_profile = Some(b"different parent RGB profile".to_vec());
    let result = schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    assert!(!has_stack(&result.tree.layers[0]));
    assert!(result.tree.layers[0]
        .extras
        .iter()
        .any(|b| b.key == *b"SoLd"));
}
