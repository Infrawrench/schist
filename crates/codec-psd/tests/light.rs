//! Native settings only, extracted from the reported PSD; no artwork.
mod common;

use schist_adjustments::{Light, Params};
use schist_codec_psd::{read_psd, write_psd};
use schist_color::Depth;
use schist_core::{AdjustmentData, AdjustmentKind, Document, Layer, LayerKind};

const LIGHT1: &[u8] = include_bytes!("fixtures/photoshop-light-1.cged");
const LIGHT2: &[u8] = include_bytes!("fixtures/photoshop-light-2.cged");

fn fixture(raw: &[u8], reverse: bool) -> Document {
    let mut psd = common::Psd::rgb8(4, 4);
    psd.layers.push(common::L::solid(
        "background",
        (0, 0, 4, 4),
        [80, 128, 200, 255],
    ));
    let mut layer = common::L::default();
    layer.extra_blocks = vec![(*b"brit", vec![0; 8]), (*b"CgEd", raw.to_vec())];
    if reverse {
        layer.extra_blocks.reverse();
    }
    psd.layers.push(layer);
    read_psd(&psd.build()).unwrap()
}

fn data(doc: &Document) -> &AdjustmentData {
    match &doc.tree.layers[1].kind {
        LayerKind::Adjustment(data) => data,
        _ => panic!("Light was imported as pixels"),
    }
}

fn params_for(data: &AdjustmentData) -> Params {
    data.params_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_else(|| schist_adjustments::parse_psd(data.kind, &data.raw))
}

#[test]
fn native_controls_override_zero_legacy_block_in_either_order() {
    for reverse in [false, true] {
        for (raw, expected) in [
            (
                LIGHT1,
                Light {
                    exposure: 1.14,
                    contrast: -31.0,
                    ..Light::default()
                },
            ),
            (
                LIGHT2,
                Light {
                    exposure: -0.47,
                    contrast: -97.0,
                    whites: 37.0,
                    blacks: -68.0,
                    shadows: 95.0,
                    highlights: 0.0,
                },
            ),
        ] {
            let doc = fixture(raw, reverse);
            assert_eq!(data(&doc).kind, AdjustmentKind::Light);
            assert_eq!(params_for(data(&doc)), Params::Light(expected));
            let out = schist_compositor::composite_region_rgba8(&doc, doc.canvas_rect());
            assert_ne!(&out[..3], &[80, 128, 200], "Light was a no-op");
            let back = read_psd(&write_psd(&doc).unwrap()).unwrap();
            assert_eq!(data(&back).raw, raw, "untouched native settings changed");
            assert_eq!(
                schist_compositor::composite_region_rgba8(&back, back.canvas_rect()),
                out
            );
        }
    }
}

#[test]
fn editing_and_saving_replaces_native_controls_without_duplicates() {
    let mut doc = fixture(LIGHT2, false);
    let mut params = params_for(data(&doc));
    for (key, value) in [
        ("exposure", 2.5),
        ("contrast", 20.0),
        ("highlights", -12.0),
        ("shadows", 17.0),
        ("whites", 8.0),
        ("blacks", -24.0),
    ] {
        params.set_param(key, value);
    }
    if let LayerKind::Adjustment(a) = &mut doc.tree.layers[1].kind {
        a.params_json = Some(serde_json::to_string(&params).unwrap());
        a.raw.clear(); // The editor discards superseded raw parameters on commit.
    }
    for _ in 0..2 {
        doc = read_psd(&write_psd(&doc).unwrap()).unwrap();
        assert_eq!(params_for(data(&doc)), params);
        for key in [b"brit", b"CgEd"] {
            assert_eq!(
                doc.tree.layers[1]
                    .extras
                    .iter()
                    .filter(|b| &b.key == key)
                    .count(),
                1
            );
        }
    }
}

#[test]
fn committing_unchanged_settings_retains_the_original_descriptor() {
    let mut doc = fixture(LIGHT2, false);
    let params = params_for(data(&doc));
    if let LayerKind::Adjustment(a) = &mut doc.tree.layers[1].kind {
        a.params_json = Some(serde_json::to_string(&params).unwrap());
        a.raw.clear();
    }
    let back = read_psd(&write_psd(&doc).unwrap()).unwrap();
    assert_eq!(data(&back).raw, LIGHT2);
}

#[test]
fn light_without_preserved_blocks_writes_both_native_blocks() {
    let mut doc = Document::new("Light", 4, 4, Depth::Eight);
    let params = Params::Light(Light {
        exposure: 0.75,
        ..Light::default()
    });
    let mut layer = Layer::new_raster("Light");
    layer.kind = LayerKind::Adjustment(AdjustmentData {
        kind: AdjustmentKind::Light,
        raw: vec![],
        params_json: Some(serde_json::to_string(&params).unwrap()),
    });
    doc.tree.layers.push(layer);
    let back = read_psd(&write_psd(&doc).unwrap()).unwrap();
    let LayerKind::Adjustment(a) = &back.tree.layers[0].kind else {
        panic!("lost adjustment")
    };
    assert_eq!(params_for(a), params);
    assert!(back.tree.layers[0].extras.iter().any(|b| b.key == *b"brit"));
}

#[test]
fn unrelated_or_invalid_cged_does_not_replace_legacy_adjustments() {
    let mut unrelated = LIGHT1.to_vec();
    let mode = unrelated
        .windows(19)
        .position(|w| w == b"brightnessModeLight")
        .unwrap();
    unrelated[mode + 18] = b'?';
    let mut nonfinite = LIGHT1.to_vec();
    let value = nonfinite.windows(4).position(|w| w == b"doub").unwrap() + 4;
    nonfinite[value..value + 8].copy_from_slice(&f64::NAN.to_be_bytes());
    for raw in [&unrelated[..], &nonfinite, &LIGHT1[..20]] {
        assert!(Light::parse(raw).is_none());
        let doc = fixture(raw, false);
        assert_eq!(data(&doc).kind, AdjustmentKind::BrightnessContrast);
        let back = read_psd(&write_psd(&doc).unwrap()).unwrap();
        assert!(back.tree.layers[1]
            .extras
            .iter()
            .any(|b| b.key == *b"CgEd" && b.data == raw));
    }
}
