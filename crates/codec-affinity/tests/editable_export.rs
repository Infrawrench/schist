//! Native structure assertions supplement (and do not replace) readback checks.
use schist_codec_affinity::{
    graph::{self, tag, Graph, Value},
    read_affinity, write_affinity, Archive,
};
use schist_color::{Depth, Rgba};
use schist_core::{Anchor, Document, Layer, RawBlock, SubPath, VectorPath, VectorShape};
use schist_text_engine::{StyleRun, TextSpec};

fn graph(bytes: &[u8]) -> Graph {
    let archive = Archive::parse(bytes).unwrap();
    graph::parse(&archive.extract(archive.head("doc.dat").unwrap()).unwrap()).unwrap()
}
fn text_doc(wrap: bool) -> Document {
    let mut doc = Document::new("editable", 256, 256, Depth::Eight);
    let mut layer = Layer::new_raster("Type");
    let spec = TextSpec {
        text: "Native type".into(),
        family: schist_text_engine::default_family(),
        size: 20.0,
        wrap_width: wrap.then_some(150.0),
        runs: vec![StyleRun {
            start: 7,
            end: 11,
            bold: Some(true),
            color: Some([240, 50, 20, 255]),
            ..Default::default()
        }],
        ..Default::default()
    };
    layer.extras.push(RawBlock {
        key: *b"PsTx",
        data: serde_json::to_vec(
            &serde_json::json!({"spec": spec, "origin": [12, 20], "color": [20, 80, 220, 255]}),
        )
        .unwrap(),
    });
    doc.push_layer(layer);
    doc
}
fn saved_artifact(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("SCHIST_INTERCHANGE_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
    }
}

#[test]
fn new_artistic_and_frame_text_have_native_story_font_and_color_runs() {
    for wrap in [false, true] {
        let doc = text_doc(wrap);
        let (bytes, report) = write_affinity(&doc, None).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let g = graph(&bytes);
        let node = g
            .nodes
            .iter()
            .find(|n| n.type_tag() == tag(if wrap { b"TxtF" } else { b"TxtA" }))
            .expect("native text object");
        let story = g.child(node, b"StSt").unwrap();
        let blocks = g.children(story, b"Blok");
        let glyphs = g.child(blocks[0], b"Glyp").unwrap();
        assert_eq!(
            glyphs.field(b"Utf8"),
            Some(&Value::Str("Native type\0".into()))
        );
        let attrs = g.child(blocks[0], b"GAtt").unwrap();
        let runs = g.children(attrs, b"Runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].field(b"Indx"), Some(&Value::I32(7)));
        assert_eq!(runs[1].field(b"Indx"), Some(&Value::I32(12)));
        assert!(
            g.child(node, b"Flow").is_some(),
            "native frame flow must survive graph cycles"
        );
        let (again, report) = read_affinity(&bytes).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let stored: serde_json::Value = serde_json::from_slice(
            &again.tree.layers[0]
                .extras
                .iter()
                .find(|b| b.key == *b"PsTx")
                .unwrap()
                .data,
        )
        .unwrap();
        let spec: TextSpec = serde_json::from_value(stored["spec"].clone()).unwrap();
        assert_eq!(spec.text, "Native type");
        assert!(spec.style_at(8).bold);
        assert_eq!(spec.style_at(8).color, Some([240, 50, 20, 255]));
        assert_eq!(stored["origin"], serde_json::json!([12, 20]));
        assert_eq!(spec.wrap_width, wrap.then_some(150.0));
        saved_artifact(
            if wrap {
                "native-frame.af"
            } else {
                "native-artistic.af"
            },
            &bytes,
        );
    }
}

#[test]
fn curves_keep_cubic_handles_closed_state_fill_and_stroke() {
    for closed in [false, true] {
        let mut doc = Document::new("curve", 128, 128, Depth::Eight);
        let mut layer = Layer::new_raster("Curve");
        let mut path = VectorPath::new("Curve");
        path.subpaths.push(SubPath {
            closed,
            anchors: vec![
                Anchor::smooth(10.0, 25.0, 5.0, -8.0),
                Anchor::smooth(80.0, 40.0, 6.0, 10.0),
                Anchor::corner(40.0, 95.0),
            ],
        });
        if !closed {
            path.subpaths[0].anchors[0].handle_in = (0.0, 0.0);
            path.subpaths[0].anchors[2].handle_out = (0.0, 0.0);
        }
        let mut shape = VectorShape::new(path.clone(), Rgba::from_u8(20, 80, 220, 255));
        shape.stroke = Some((Rgba::from_u8(220, 50, 20, 255), 3.0));
        layer.shape = Some(Box::new(shape));
        doc.push_layer(layer);
        let (bytes, report) = write_affinity(&doc, None).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let g = graph(&bytes);
        assert!(g.nodes.iter().any(|n| n.type_tag() == tag(b"PCrv")));
        let (again, report) = read_affinity(&bytes).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let shape = again.tree.layers[0].shape.as_ref().unwrap();
        assert_eq!(shape.path, path);
        assert_eq!(shape.fill.to_u8(), [20, 80, 220, 255]);
        assert_eq!(shape.stroke.unwrap().0.to_u8(), [220, 50, 20, 255]);
        assert_eq!(shape.stroke.unwrap().1, 3.0);
        saved_artifact(
            if closed {
                "native-closed.af"
            } else {
                "native-open.af"
            },
            &bytes,
        );
    }
}

#[test]
fn source_shape_stays_parametric_until_geometry_is_edited() {
    let (mut doc, _) = read_affinity(include_bytes!(
        "../../../fixtures/affinity-probe/shp_star_curved.af"
    ))
    .unwrap();
    let index = doc
        .tree
        .layers
        .iter()
        .position(|l| l.shape.is_some())
        .unwrap();
    let (native, _) = write_affinity(&doc, None).unwrap();
    assert!(graph(&native)
        .nodes
        .iter()
        .any(|n| n.type_tag() == tag(b"ShpN")));
    doc.tree.layers[index].shape.as_mut().unwrap().path.subpaths[0].anchors[0]
        .point
        .0 += 3.0;
    let (edited, _) = write_affinity(&doc, None).unwrap();
    let g = graph(&edited);
    assert!(g.nodes.iter().any(|n| n.type_tag() == tag(b"PCrv")));
    assert!(!g.nodes.iter().any(|n| n.type_tag() == tag(b"ShpN")));
}

#[test]
fn rotated_source_text_keeps_native_transform_without_false_local_editability() {
    let source = include_bytes!("../../../fixtures/affinity-probe/text_rotated.af");
    let (doc, _) = read_affinity(source).unwrap();
    let layer = doc
        .tree
        .layers
        .iter()
        .find(|l| l.extras.iter().any(|b| b.key == *b"AfNt"))
        .unwrap();
    assert!(!layer.extras.iter().any(|b| b.key == *b"PsTx"));
    let (bytes, report) = write_affinity(&doc, None).unwrap();
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let before = graph(source);
    let after = graph(&bytes);
    let before = before
        .nodes
        .iter()
        .find(|n| n.type_tag() == tag(b"TxtF"))
        .unwrap();
    let after = after
        .nodes
        .iter()
        .find(|n| n.type_tag() == tag(b"TxtF"))
        .unwrap();
    assert_eq!(before.field(b"Xfrm"), after.field(b"Xfrm"));
}

#[test]
fn pixel_edits_do_not_resurrect_stale_native_typography() {
    let (mut doc, _) = read_affinity(include_bytes!(
        "../../../fixtures/affinity-probe/text_rotated.af"
    ))
    .unwrap();
    let layer = doc
        .tree
        .layers
        .iter_mut()
        .find(|l| l.extras.iter().any(|b| b.key == *b"AfNt"))
        .unwrap();
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(schist_core::TileCoord::containing(0, 0), Depth::Eight)
        .set(0, Rgba::WHITE);
    let (bytes, report) = write_affinity(&doc, None).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert!(!graph(&bytes)
        .nodes
        .iter()
        .any(|n| n.type_tag() == tag(b"TxtF")));
    let (again, _) = read_affinity(&bytes).unwrap();
    assert_eq!(
        again
            .tree
            .layers
            .last()
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .pixel(0, 0),
        Rgba::WHITE
    );
}

#[test]
fn unverified_astral_run_units_use_the_existing_raster_fallback() {
    let mut doc = text_doc(false);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&doc.tree.layers[0].extras[0].data).unwrap();
    stored["spec"]["text"] = "A😀".into();
    stored["spec"]["runs"] = serde_json::json!([]);
    doc.tree.layers[0].extras[0].data = serde_json::to_vec(&stored).unwrap();
    let (bytes, report) = write_affinity(&doc, None).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert!(!graph(&bytes)
        .nodes
        .iter()
        .any(|n| matches!(n.type_tag().to_be_bytes(), [b'T', b'x', b't', b'A' | b'F'])));
}
