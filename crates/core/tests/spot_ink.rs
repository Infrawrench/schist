use schist_color::Depth;
use schist_core::{Document, InkChannel, InkPreview, IntRect, SelectOp, StrokeEdit, TileCoord};

#[test]
fn metadata_fill_selection_undo_and_delete_are_independent_of_layers() {
    let mut doc = Document::new("plates", 2, 1, Depth::ThirtyTwo);
    let channel = InkChannel::spot("Varnish".into(), [0.1, 0.4, 0.8]);
    let id = channel.info.id;
    let mut edit = doc.begin_edit("create");
    edit.change_ink_channels(|c| c.push(channel));
    edit.commit();
    doc.mark_saved();
    doc.selection
        .apply_shape(IntRect::from_size(2, 1), SelectOp::Replace, |x, _| {
            if x == 0 {
                128
            } else {
                0
            }
        });
    let mut edit = doc.begin_edit("fill");
    assert!(edit.fill_ink(id, 1.0));
    edit.commit();
    assert!((doc.ink_channels[0].pixels.value(0, 0) - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(doc.ink_channels[0].pixels.value(1, 0), 0.0);
    assert!(doc.tree.layers.is_empty());
    doc.undo();
    assert!(!doc.dirty);
    assert_eq!(doc.ink_channels[0].pixels.value(0, 0), 0.0);
    doc.redo();
    let mut edit = doc.begin_edit("rename and delete");
    edit.change_ink_channels(|c| {
        c[0].info.name = "New name".into();
        c.clear();
    });
    edit.commit();
    assert!(doc.ink_channels.is_empty());
    doc.undo();
    assert_eq!(doc.ink_channels[0].info.name, "Varnish");
    assert!(doc.ink_channels[0].pixels.value(0, 0) > 0.5);
}

#[test]
fn cancelled_stroke_restores_absent_tiles_and_committed_stroke_is_one_undo() {
    let mut doc = Document::new("plates", 1, 1, Depth::Eight);
    let channel = InkChannel::spot("Ink".into(), [1.0, 0.0, 0.0]);
    let id = channel.info.id;
    doc.ink_channels.push(channel);
    let coord = TileCoord::containing(0, 0);
    let mut stroke = StrokeEdit::new("stroke");
    stroke.writable_ink_tile(&mut doc, id, coord).unwrap()[0] = 0.75;
    stroke.cancel(&mut doc);
    assert!(doc.ink_channels[0].pixels.0.is_empty());
    let mut stroke = StrokeEdit::new("stroke");
    stroke.writable_ink_tile(&mut doc, id, coord).unwrap()[0] = 0.5;
    stroke.writable_ink_tile(&mut doc, id, coord).unwrap()[0] = 0.75;
    assert!(stroke.commit(&mut doc));
    doc.undo();
    assert!(doc.ink_channels[0].pixels.0.is_empty());
    doc.redo();
    assert_eq!(doc.ink_channels[0].pixels.value(0, 0), 0.75);
}

#[test]
fn transparent_inks_multiply_process_and_other_plates_while_solidity_is_display_only() {
    let mut cyan = InkChannel::spot("Cyan spot".into(), [0.0, 1.0, 1.0]);
    let mut yellow = InkChannel::spot("Yellow spot".into(), [1.0, 1.0, 0.0]);
    cyan.pixels.set(0, 0, 1.0);
    yellow.pixels.set(0, 0, 1.0);
    let mut channels = vec![cyan, yellow];
    let rect = IntRect::from_size(1, 1);
    let mut process = [128, 192, 255, 255];
    schist_core::ink::preview_rgba8(&channels, InkPreview::Overprint, rect, &mut process);
    assert_eq!(process, [0, 192, 0, 255]);
    channels[1].info.solidity = 1.0;
    let mut opaque = [128, 192, 255, 255];
    schist_core::ink::preview_rgba8(&channels, InkPreview::Overprint, rect, &mut opaque);
    assert_eq!(opaque, [255, 255, 0, 255]);
    channels[1].info.visible = false;
    let mut hidden = [128, 192, 255, 255];
    schist_core::ink::preview_rgba8(&channels, InkPreview::Overprint, rect, &mut hidden);
    assert_eq!(hidden, [0, 192, 255, 255]);
    let mut plate = [128, 192, 255, 255];
    schist_core::ink::preview_rgba8(
        &channels,
        InkPreview::Separation(channels[1].info.id),
        rect,
        &mut plate,
    );
    assert_eq!(
        plate,
        [0, 0, 0, 255],
        "hidden ink still has its unchanged printable separation"
    );
    let mut original = [128, 192, 255, 255];
    schist_core::ink::preview_rgba8(&channels, InkPreview::Process, rect, &mut original);
    assert_eq!(original, [128, 192, 255, 255]);
}

#[test]
fn crop_and_resample_do_not_alias_undo_snapshots() {
    let mut doc = Document::new("plates", 3, 1, Depth::Sixteen);
    let mut channel = InkChannel::spot("Ink".into(), [0.0; 3]);
    channel.pixels.set(1, 0, 0.5);
    channel.pixels.set(2, 0, 1.0);
    doc.ink_channels.push(channel);
    let mut edit = doc.begin_edit("crop");
    edit.remap_ink(
        IntRect::from_size(2, 1),
        |x, y| ((x + 1) as f32, y as f32),
        false,
    );
    edit.set_canvas_size(2, 1);
    edit.commit();
    assert_eq!(doc.ink_channels[0].pixels.value(0, 0), 0.5);
    let mut edit = doc.begin_edit("size");
    edit.resize_ink(4, 1);
    edit.set_canvas_size(4, 1);
    edit.commit();
    assert_eq!(doc.ink_channels[0].pixels.value(1, 0), 0.625);
    doc.undo();
    doc.undo();
    assert_eq!(doc.width, 3);
    assert_eq!(doc.ink_channels[0].pixels.value(0, 0), 0.0);
    assert_eq!(doc.ink_channels[0].pixels.value(2, 0), 1.0);
}
