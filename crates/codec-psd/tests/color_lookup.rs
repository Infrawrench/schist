//! Color Lookup layers (`clrL`) and Camera Raw colour grading (`ScCg`)
//! through PSD and PSB.

use schist_adjustments::lut::Lut3d;
use schist_adjustments::{ColorLookup, LutInput, LutTable, Params};
use schist_codec_psd::{read_psd, write_psd_with};
use schist_color::Depth;
use schist_core::{AdjustmentData, AdjustmentKind, Document, Layer, LayerKind, RawBlock};

fn lookup() -> ColorLookup {
    let mut cube = Lut3d::identity(9);
    for v in &mut cube.table {
        *v = [v[0] * 0.8 + 0.1, v[1], 1.0 - v[2]];
    }
    ColorLookup {
        name: "Teal.cube".into(),
        input: LutInput::Document,
        table: Some(LutTable::from_cube("Teal", &cube)),
    }
}

fn lookup_layer(data: AdjustmentData) -> Layer {
    let mut layer = Layer::new_raster("Color Lookup");
    layer.kind = LayerKind::Adjustment(data);
    layer
}

fn adjustment(doc: &Document) -> &AdjustmentData {
    match &doc.tree.layers[0].kind {
        LayerKind::Adjustment(data) => data,
        other => panic!("came back as {other:?}"),
    }
}

#[test]
fn a_color_lookup_made_in_schist_survives_psd_and_psb() {
    let params = Params::ColorLookup(lookup());
    for psb in [false, true] {
        let mut doc = Document::new("t", 16, 16, Depth::Eight);
        doc.push_layer(lookup_layer(AdjustmentData {
            kind: AdjustmentKind::ColorLookup,
            raw: Vec::new(),
            params_json: Some(serde_json::to_string(&params).unwrap()),
        }));
        let bytes = write_psd_with(&doc, psb).unwrap();
        assert!(bytes.windows(4).any(|w| w == b"clrL"));
        let back = read_psd(&bytes).unwrap();
        let data = adjustment(&back);
        assert_eq!(data.kind, AdjustmentKind::ColorLookup);
        assert_eq!(schist_adjustments::resolve(data), params, "psb={psb}");
    }
}

#[test]
fn an_untouched_imported_block_is_written_back_verbatim() {
    // A block as another application might write it: our fields plus one
    // we do not model. Re-encoding would drop the unknown field.
    let mut b = schist_psd_descriptor::Builder::new("null");
    b.enumerated("lookupType", "colorLookupType", "3DLUT")
        .text("Nm  ", "Teal.cube")
        .bool("Dthr", true)
        .enumerated("LUTFormat", "LUTFormatType", "LUTFormatCUBE")
        .raw("LUT3DFileData", lookup().table.unwrap().source())
        .integer("xtra", 7);
    let mut raw = 1u16.to_be_bytes().to_vec();
    raw.extend_from_slice(&b.finish_versioned());

    let imported = || {
        let mut layer = lookup_layer(AdjustmentData {
            kind: AdjustmentKind::ColorLookup,
            raw: raw.clone(),
            params_json: None,
        });
        layer.extras.push(RawBlock {
            key: *b"clrL",
            data: raw.clone(),
        });
        layer
    };
    let parsed = schist_adjustments::parse_psd(AdjustmentKind::ColorLookup, &raw);
    assert!(matches!(parsed, Params::ColorLookup(_)));

    // Opened and saved without edits.
    let mut doc = Document::new("t", 16, 16, Depth::Eight);
    doc.push_layer(imported());
    let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    assert_eq!(adjustment(&back).raw, raw);

    // The dialog was committed without changing anything: still verbatim.
    let mut doc = Document::new("t", 16, 16, Depth::Eight);
    let mut layer = imported();
    if let LayerKind::Adjustment(data) = &mut layer.kind {
        data.raw.clear();
        data.params_json = Some(serde_json::to_string(&parsed).unwrap());
    }
    doc.push_layer(layer);
    let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    assert_eq!(adjustment(&back).raw, raw);

    // A real edit regenerates the block, once.
    let mut edited = lookup();
    edited.input = LutInput::Linear;
    let edited = Params::ColorLookup(edited);
    let mut doc = Document::new("t", 16, 16, Depth::Eight);
    let mut layer = imported();
    if let LayerKind::Adjustment(data) = &mut layer.kind {
        data.raw.clear();
        data.params_json = Some(serde_json::to_string(&edited).unwrap());
    }
    doc.push_layer(layer);
    let bytes = write_psd_with(&doc, false).unwrap();
    assert_eq!(bytes.windows(4).filter(|w| *w == b"clrL").count(), 1);
    let back = read_psd(&bytes).unwrap();
    assert_ne!(adjustment(&back).raw, raw);
}

#[test]
fn unrenderable_lookups_are_preserved() {
    // An abstract-profile lookup renders as nothing but keeps its bytes.
    let mut b = schist_psd_descriptor::Builder::new("null");
    b.enumerated("lookupType", "colorLookupType", "abstractProfile")
        .raw("profile", b"an ICC profile");
    let mut raw = 1u16.to_be_bytes().to_vec();
    raw.extend_from_slice(&b.finish_versioned());
    let mut doc = Document::new("t", 16, 16, Depth::Eight);
    let mut layer = lookup_layer(AdjustmentData {
        kind: AdjustmentKind::ColorLookup,
        raw: raw.clone(),
        params_json: None,
    });
    layer.extras.push(RawBlock {
        key: *b"clrL",
        data: raw.clone(),
    });
    doc.push_layer(layer);
    let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    let data = adjustment(&back);
    assert_eq!(data.raw, raw);
    assert_eq!(schist_adjustments::resolve(data), Params::Unsupported);
}

#[test]
fn colour_grading_rides_beside_the_raw_block() {
    use schist_core::raw::GradeWheel;
    let mut settings = schist_core::RawSettings {
        exposure: 0.5,
        ..Default::default()
    };
    settings.grading.highlights = GradeWheel {
        hue: 40.0,
        saturation: 30.0,
        luminance: 10.0,
    };
    settings.grading.balance = -25.0;
    for psb in [false, true] {
        let mut doc = Document::new("t", 16, 16, Depth::Eight);
        let mut layer = Layer::new_raster("capture");
        layer.raw = Some(Box::new(schist_core::RawDevelopment {
            source: std::sync::Arc::from(&b"camera bytes"[..]),
            settings,
            masks: Vec::new(),
        }));
        doc.push_layer(layer);
        let bytes = write_psd_with(&doc, psb).unwrap();
        let back = read_psd(&bytes).unwrap();
        let layer = &back.tree.layers[0];
        assert_eq!(layer.raw.as_ref().unwrap().settings, settings);
        assert!(
            !layer.extras.iter().any(|b| b.key == *b"ScCg"),
            "the block is regenerated, not kept twice"
        );
        // Saving again does not duplicate it.
        let again = write_psd_with(&back, psb).unwrap();
        assert_eq!(again.windows(4).filter(|w| *w == b"ScCg").count(), 1);
    }
}
