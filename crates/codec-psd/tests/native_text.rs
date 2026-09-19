use schist_color::{Depth, Rgba};
use schist_core::{Document, Layer, RawBlock, TileCoord};
use schist_text_engine::{Align, StyleRun, TextSpec};
use serde_json::{json, Value};

fn spec(layer: &Layer) -> Value {
    serde_json::from_slice(
        &layer
            .extras
            .iter()
            .find(|b| b.key == *b"PsTx")
            .expect("editable text")
            .data,
    )
    .unwrap()
}
fn block(layer: &Layer, key: &[u8; 4]) -> Vec<u8> {
    layer
        .extras
        .iter()
        .find(|b| b.key == *key)
        .unwrap()
        .data
        .clone()
}
fn document() -> Document {
    let mut doc = Document::new("Native text", 128, 96, Depth::Eight);
    let mut layer = Layer::new_raster("Type");
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, doc.depth)
        .set(0, Rgba::new(0.2, 0.4, 0.6, 1.0));
    let spec = TextSpec {
        text: "A😀 Bé\nNext".into(),
        family: "DejaVu Sans".into(),
        size: 20.0,
        tracking: 2.0,
        align: Align::Center,
        runs: vec![StyleRun {
            start: 6,
            end: 14,
            bold: Some(true),
            ..Default::default()
        }],
        ..Default::default()
    };
    layer.extras.push(RawBlock {
        key: *b"PsTx",
        data: serde_json::to_vec(&json!({
            "spec": spec, "origin": [12, 20], "color": [51, 102, 153, 255]
        }))
        .unwrap(),
    });
    doc.push_layer(layer);
    doc
}

#[test]
fn native_text_exports_utf16_runs_and_remains_editable_without_private_blocks() {
    for psb in [false, true] {
        let mut doc = document();
        // The Unicode byte boundaries above deliberately span an astral glyph.
        let current = spec(&doc.tree.layers[0]);
        let len = current["spec"]["text"].as_str().unwrap().len();
        let mut current = current;
        current["spec"]["runs"][0]["end"] = len.into();
        doc.tree.layers[0].extras[0].data = serde_json::to_vec(&current).unwrap();
        let encoded = schist_codec_psd::write_psd_with(&doc, psb).unwrap();
        let mut native_only = encoded.clone();
        // Simulate an editor stripping unknown private type state. Replacing
        // only the key retains the original native descriptor unchanged.
        for i in 0..native_only.len().saturating_sub(8) {
            if native_only.get(i..i + 8) == Some(b"8BIMPsTx") {
                native_only[i + 4..i + 8].copy_from_slice(b"TEST");
            }
        }
        let reopened = schist_codec_psd::read_psd(&native_only).unwrap();
        let stored = spec(&reopened.tree.layers[0]);
        assert_eq!(stored["spec"]["text"], "A😀 Bé\nNext");
        assert_eq!(stored["color"], json!([51, 102, 153, 255]));
        assert_eq!(stored["spec"]["align"], "Center");
        let parsed: TextSpec = serde_json::from_value(stored["spec"].clone()).unwrap();
        assert!(parsed.style_at(6).bold);
        assert!(!parsed.style_at(0).bold);
        if let Some(directory) = std::env::var_os("SCHIST_INTERCHANGE_ARTIFACT_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join(if psb {
                    "native-text.psb"
                } else {
                    "native-text.psd"
                }),
                &encoded,
            )
            .unwrap();
        }
    }
}

#[test]
fn independent_writer_fixture_imports_and_unchanged_native_bytes_survive() {
    let doc = schist_codec_psd::read_psd(include_bytes!("fixtures/ag-psd-type.psd")).unwrap();
    let layer = &doc.tree.layers[0];
    let stored = spec(layer);
    assert_eq!(stored["spec"]["text"], "A😀 Bé\nNext");
    assert_eq!(stored["spec"]["align"], "Center");
    let parsed: TextSpec = serde_json::from_value(stored["spec"].clone()).unwrap();
    assert!(parsed.style_at(6).bold);
    assert_eq!(parsed.tracking, 2.0);
    let original = block(layer, b"TySh");
    for psb in [false, true] {
        let again =
            schist_codec_psd::read_psd(&schist_codec_psd::write_psd_with(&doc, psb).unwrap())
                .unwrap();
        assert_eq!(block(&again.tree.layers[0], b"TySh"), original);
    }
}

#[test]
fn editing_an_imported_layer_replaces_stale_native_text() {
    let mut doc = schist_codec_psd::read_psd(include_bytes!("fixtures/ag-psd-type.psd")).unwrap();
    doc.preserved_layer_info.push(RawBlock {
        key: *b"Txt2",
        data: b"original global type cache".to_vec(),
    });
    let mut stored = spec(&doc.tree.layers[0]);
    stored["spec"]["text"] = "Replacement".into();
    stored["spec"]["runs"] = json!([]);
    let layer = &mut doc.tree.layers[0];
    let original = block(layer, b"TySh");
    layer
        .extras
        .iter_mut()
        .find(|b| b.key == *b"PsTx")
        .unwrap()
        .data = serde_json::to_vec(&stored).unwrap();
    let encoded = schist_codec_psd::write_psd(&doc).unwrap();
    let mut reopened = schist_codec_psd::read_psd(&encoded).unwrap();
    assert!(!reopened
        .preserved_layer_info
        .iter()
        .any(|b| b.key == *b"Txt2"));
    assert_eq!(
        reopened
            .preserved_layer_info
            .iter()
            .find(|b| b.key == *b"ScT2")
            .unwrap()
            .data,
        b"original global type cache"
    );
    assert_ne!(block(&reopened.tree.layers[0], b"TySh"), original);
    reopened.tree.layers[0].extras.retain(|b| b.key != *b"PsTx");
    let independent =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&reopened).unwrap()).unwrap();
    assert_eq!(
        spec(&independent.tree.layers[0])["spec"]["text"],
        "Replacement"
    );
}

#[test]
fn centered_wrapped_text_and_mixed_sizes_keep_geometry_without_private_state() {
    let mut doc = document();
    let mut stored = spec(&doc.tree.layers[0]);
    stored["spec"]["wrap_width"] = 250.0.into();
    stored["spec"]["runs"][0]["size"] = 32.0.into();
    stored["spec"]["line_height"] = 1.5.into();
    doc.tree.layers[0].extras[0].data = serde_json::to_vec(&stored).unwrap();
    let mut bytes = schist_codec_psd::write_psd(&doc).unwrap();
    for at in 0..bytes.len().saturating_sub(8) {
        if bytes.get(at..at + 8) == Some(b"8BIMPsTx") {
            bytes[at + 4..at + 8].copy_from_slice(b"TEST");
        }
    }
    let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
    let imported = spec(&reopened.tree.layers[0]);
    assert_eq!(imported["origin"], stored["origin"]);
    assert_eq!(imported["spec"]["wrap_width"], stored["spec"]["wrap_width"]);
    assert!((imported["spec"]["line_height"].as_f64().unwrap() - 1.5).abs() < 0.001);
    let imported: TextSpec = serde_json::from_value(imported["spec"].clone()).unwrap();
    assert_eq!(imported.style_at(6).size, 32.0);
}

#[test]
fn unrepresentable_affine_text_is_kept_native_without_false_editability() {
    let doc =
        schist_codec_psd::read_psd(include_bytes!("fixtures/ag-psd-type-rotated.psd")).unwrap();
    assert!(!doc.tree.layers[0].extras.iter().any(|b| b.key == *b"PsTx"));
    let original = block(&doc.tree.layers[0], b"TySh");
    let again = schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    assert_eq!(block(&again.tree.layers[0], b"TySh"), original);
}

#[test]
fn unsupported_new_type_settings_do_not_emit_a_misleading_native_layer() {
    let mut doc = document();
    let mut stored = spec(&doc.tree.layers[0]);
    stored["spec"]["features"] = json!([{"tag":"ss01", "value":1}]);
    doc.tree.layers[0].extras[0].data = serde_json::to_vec(&stored).unwrap();
    let again = schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    assert!(!again.tree.layers[0]
        .extras
        .iter()
        .any(|b| b.key == *b"TySh"));
    assert_eq!(spec(&again.tree.layers[0]), stored);
}

#[test]
fn native_ligature_flag_follows_the_actual_schist_layout_default_and_override() {
    for enabled in [false, true] {
        let mut doc = document();
        let mut stored = spec(&doc.tree.layers[0]);
        if enabled {
            stored["spec"]["features"] = json!([{"tag":"liga", "value":1}]);
        }
        doc.tree.layers[0].extras[0].data = serde_json::to_vec(&stored).unwrap();
        let mut bytes = schist_codec_psd::write_psd(&doc).unwrap();
        for at in 0..bytes.len().saturating_sub(8) {
            if bytes.get(at..at + 8) == Some(b"8BIMPsTx") {
                bytes[at + 4..at + 8].copy_from_slice(b"TEST");
            }
        }
        let reopened = schist_codec_psd::read_psd(&bytes).unwrap();
        let imported: TextSpec =
            serde_json::from_value(spec(&reopened.tree.layers[0])["spec"].clone()).unwrap();
        assert_eq!(imported.feature("liga", false), enabled);
    }
}

#[test]
fn independent_automatic_leading_is_measured_and_overset_boxes_remain_native() {
    let doc = schist_codec_psd::read_psd(include_bytes!("fixtures/ag-psd-type-auto-leading.psd"))
        .unwrap();
    let stored = spec(&doc.tree.layers[0]);
    let imported: TextSpec = serde_json::from_value(stored["spec"].clone()).unwrap();
    let metrics = schist_text_engine::measure(&imported).unwrap();
    assert!((metrics.line_advance - 48.0).abs() < 0.01);
    let overset =
        schist_codec_psd::read_psd(include_bytes!("fixtures/ag-psd-type-overset.psd")).unwrap();
    assert!(!overset.tree.layers[0]
        .extras
        .iter()
        .any(|b| b.key == *b"PsTx"));
    let native = block(&overset.tree.layers[0], b"TySh");
    let again =
        schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&overset).unwrap()).unwrap();
    assert_eq!(block(&again.tree.layers[0], b"TySh"), native);
}
