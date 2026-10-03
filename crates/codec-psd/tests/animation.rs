//! Frame animations survive PSD and PSB saves in Schist's `ScAn` block.

use schist_codec_psd::{read_psd, write_psd_with};
use schist_color::{Depth, Rgba};
use schist_core::animation::{self, LoopCount};
use schist_core::{blit_rgba8, Document, IntRect, Layer, LayerPath, PreservedResource, RawBlock};

fn solid(name: &str, rect: IntRect, color: Rgba) -> Layer {
    let mut layer = Layer::new_raster(name);
    let px = color.to_u8();
    let rgba: Vec<u8> = (0..rect.width() * rect.height())
        .flat_map(|_| px)
        .collect();
    blit_rgba8(
        &mut layer.as_raster_mut().unwrap().tiles,
        Depth::Eight,
        rect,
        &rgba,
    );
    layer
}

fn animated() -> Document {
    let mut doc = Document::new("anim", 40, 30, Depth::Eight);
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let blue = Rgba::new(0.0, 0.0, 1.0, 1.0);
    doc.push_layer(solid("Background", IntRect::from_size(40, 30), Rgba::WHITE));
    let mut group = Layer::new_group("Sprites");
    if let schist_core::LayerKind::Group(g) = &mut group.kind {
        g.children.push(solid("Red", IntRect::from_xywh(0, 0, 8, 8), red));
        g.children.push(solid("Blue", IntRect::from_xywh(10, 10, 8, 8), blue));
    }
    doc.push_layer(group);
    animation::make_frames_from_layers(&mut doc, "make").unwrap();
    animation::duplicate(&mut doc, "new").unwrap();
    let red_id = doc.tree.iter().find(|l| l.name == "Red").unwrap().id;
    animation::set_offset(&mut doc, red_id, (4, 2), "offset").unwrap();
    animation::set_delay(&mut doc, Some(0), 250, "delay").unwrap();
    animation::set_loop(&mut doc, LoopCount::Times(3), "loop").unwrap();
    animation::set_new_layers_visible(&mut doc, false, "rule").unwrap();
    let mut edit = doc.begin_edit("hide blue");
    let blue_id = edit.doc().tree.iter().find(|l| l.name == "Blue").unwrap().id;
    edit.change_props(blue_id, |l| l.opacity = 0.5);
    edit.commit();
    doc
}

/// Each frame's (name, visible, opacity, offset), so two documents with
/// different layer ids can be compared.
fn summary(doc: &Document) -> Vec<(u32, Vec<(String, bool, u32, (i32, i32))>)> {
    let t = doc.timeline.as_ref().unwrap();
    (0..t.frames.len())
        .map(|i| {
            (
                t.frames[i].delay_ms,
                doc.tree
                    .iter()
                    .map(|l| {
                        let s = t.resolve(i, l);
                        (
                            l.name.clone(),
                            s.visible,
                            (s.opacity * 255.0).round() as u32,
                            s.offset,
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn timeline_round_trips_through_psd_and_psb() {
    let doc = animated();
    for psb in [false, true] {
        let bytes = write_psd_with(&doc, psb).unwrap();
        let back = read_psd(&bytes).unwrap();
        let t = back.timeline.as_ref().expect("timeline read back");
        let original = doc.timeline.as_ref().unwrap();
        assert_eq!(t.frames.len(), 3);
        assert_eq!(t.current, original.current);
        assert_eq!(t.loop_count, LoopCount::Times(3));
        assert!(!t.new_layers_visible);
        assert_eq!(summary(&back), summary(&doc), "psb={psb}");
        // The current frame's offset is on the layer again.
        let red = back.tree.iter().find(|l| l.name == "Red").unwrap();
        assert_eq!(red.render_offset, (4, 2));
        // The block is consumed rather than kept twice.
        assert!(back.preserved_layer_info.iter().all(|b| b.key != *b"ScAn"));
        // And a second save writes the same block again.
        let again = write_psd_with(&back, psb).unwrap();
        assert_eq!(summary(&read_psd(&again).unwrap()), summary(&doc));
    }
}

#[test]
fn layers_renamed_or_reordered_elsewhere_match_by_name_or_drop() {
    let doc = animated();
    let bytes = write_psd_with(&doc, false).unwrap();
    // Simulate another editor having inserted a layer at the bottom and
    // renamed another, keeping Schist's block as an unknown one.
    let pos = bytes.windows(4).position(|w| w == b"ScAn").unwrap();
    let len = u32::from_be_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
    let block = RawBlock {
        key: *b"ScAn",
        data: bytes[pos + 8..pos + 8 + len].to_vec(),
    };
    let mut back = read_psd(&bytes).unwrap();
    back.timeline = None;
    let mut edit = back.begin_edit("insert");
    edit.insert_layer(LayerPath(vec![0]), Layer::new_raster("Inserted"));
    edit.commit();
    let blue = back.tree.iter().find(|l| l.name == "Blue").unwrap().id;
    back.tree.find_mut(blue).unwrap().name = "Renamed".into();
    back.preserved_layer_info.push(block);
    let reread = read_psd(&write_psd_with(&back, false).unwrap()).unwrap();
    let t = reread.timeline.as_ref().unwrap();
    assert_eq!(t.frames.len(), 3);
    let red = reread.tree.iter().find(|l| l.name == "Red").unwrap();
    assert_eq!(t.frames[1].state(red.id).unwrap().offset, (4, 2));
    let renamed = reread.tree.iter().find(|l| l.name == "Renamed").unwrap();
    assert!(
        t.frames.iter().all(|f| f.state(renamed.id).is_none()),
        "an unmatched name is dropped, not guessed"
    );
}

#[test]
fn unknown_versions_and_photoshop_animation_resources_are_preserved_verbatim() {
    let mut doc = Document::new("plain", 8, 8, Depth::Eight);
    doc.push_layer(Layer::new_raster("a"));
    let future = RawBlock {
        key: *b"ScAn",
        data: br#"{"version":99}"#.to_vec(),
    };
    doc.preserved_layer_info.push(future.clone());
    // An opaque stand-in for Photoshop's own animation resource.
    let resource = PreservedResource {
        id: 4000,
        name: vec![0, 0],
        data: b"8BIMAnDs-opaque-payload".to_vec(),
    };
    doc.preserved_resources.push(resource.clone());
    let back = read_psd(&write_psd_with(&doc, false).unwrap()).unwrap();
    assert!(back.timeline.is_none());
    assert!(back.preserved_layer_info.contains(&future));
    let kept = back.preserved_resources.iter().find(|r| r.id == 4000).unwrap();
    assert_eq!(kept.data, resource.data);
}
