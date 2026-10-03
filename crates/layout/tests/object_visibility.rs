use schist_layout::{
    authoring, compose, snapshot_object, structure, History, LayoutEdit, Rect, Story,
};

#[test]
fn visibility_is_one_undo_step_and_never_changes_source_flow_or_opacity() {
    for opacity in [0.0, 0.35, 1.0] {
        let mut doc = schist_layout::blank_a4();
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 120.0, 80.0),
        )
        .unwrap();
        doc.objects[0].transparency = opacity;
        *doc.story_mut(frame.story) =
            Story::from_text("Hidden text retains its flow. ".repeat(30), "Body");
        let flow = compose::compose_story(&doc, frame.story);
        let before = doc.clone();
        let mut history = History::default();
        assert!(structure::set_object_hidden(
            &mut doc,
            &mut history,
            frame.object,
            true
        ));
        assert!(!structure::set_object_hidden(
            &mut doc,
            &mut history,
            frame.object,
            true
        ));
        assert!(doc.page_objects(0).is_empty());
        assert!(doc.page_artwork(0, doc.pages[0].bleed_rect()).is_empty());
        assert_eq!(compose::compose_story(&doc, frame.story), flow);
        assert_eq!(doc.objects[0].transparency, opacity);
        let hidden = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(!history.can_undo());
        assert!(history.redo(&mut doc));
        assert_eq!(doc, hidden);
        // Remove/restore snapshots must not make hidden artwork visible.
        assert!(history.apply(
            &mut doc,
            LayoutEdit::RemovedObject {
                index: 0,
                object: snapshot_object(&hidden.objects[0])
            }
        ));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, hidden);
        doc.objects[0].locked = true;
        assert!(!structure::set_object_hidden(
            &mut doc,
            &mut history,
            frame.object,
            false
        ));
    }
}

#[test]
fn old_objects_and_snapshots_default_to_visible() {
    let mut doc = schist_layout::blank_a4();
    authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 120.0, 80.0),
    )
    .unwrap();
    let object = &doc.objects[0];
    let mut json = serde_json::to_value(object).unwrap();
    json.as_object_mut().unwrap().remove("hidden");
    assert_eq!(
        serde_json::from_value::<schist_layout::PlacedObject>(json).unwrap(),
        *object
    );
    let snapshot = snapshot_object(object);
    let mut json = serde_json::to_value(&snapshot).unwrap();
    json.as_object_mut().unwrap().remove("hidden");
    assert_eq!(
        serde_json::from_value::<schist_layout::ObjectSnapshot>(json).unwrap(),
        snapshot
    );
}
