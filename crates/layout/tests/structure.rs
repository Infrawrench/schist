use schist_layout::{authoring, structure, *};

fn document() -> LayoutDocument {
    let mut doc = LayoutDocument::new(
        (0..6)
            .map(|i| Page::new(format!("page {i}"), 200.0, 300.0))
            .collect(),
    );
    let mut history = History::default();
    for page in 0..6 {
        authoring::rectangle(
            &mut doc,
            &mut history,
            page,
            Rect::new(10.0, 10.0, 30.0, 40.0),
            authoring::Paint::none(),
        );
    }
    doc.parents.push(ParentPage {
        name: "parent".into(),
        sheets: Vec::new(),
        placements: [0, 2, 5]
            .into_iter()
            .map(|page| schist_layout::parents::ParentPlacement {
                page,
                sheet: page % 2,
                transform: schist_layout::affine::Affine::translate(page as f32, 3.0),
                visible: page != 2,
            })
            .collect(),
        applied_to: vec![0, 2, 5],
        based_on: None,
        hidden: false,
        objects: vec![ParentObject {
            object: doc.objects[0].clone(),
            overridden_on: vec![0, 5],
        }],
    });
    for page in [0, 2, 5] {
        doc.pages[page].master = Some(0);
    }
    doc
}

fn reversible(before: &LayoutDocument, after: &mut LayoutDocument, history: &mut History) {
    let expected = after.clone();
    assert_eq!(history.undo_depth(), 1);
    for _ in 0..5 {
        assert!(history.undo(after));
        assert_eq!(after, before);
        assert!(!history.can_undo());
        assert!(history.redo(after));
        assert_eq!(*after, expected);
        assert!(!history.can_redo());
    }
}

#[test]
fn every_page_permutation_keeps_its_objects_parents_and_overrides() {
    let before = document();
    for from in 0..6 {
        for to in 0..6 {
            let mut doc = before.clone();
            let mut history = History::default();
            assert_eq!(
                structure::move_page(&mut doc, &mut history, from, to),
                from != to
            );
            if from == to {
                continue;
            }
            for object in &doc.objects {
                let old = before.object(object.id).unwrap();
                assert_eq!(doc.pages[object.page].name, before.pages[old.page].name);
                assert_eq!(doc.pages[object.page].master, before.pages[old.page].master);
            }
            for (new, old) in doc.parents[0].objects[0]
                .overridden_on
                .iter()
                .zip(&before.parents[0].objects[0].overridden_on)
            {
                assert_eq!(doc.pages[*new].name, before.pages[*old].name);
            }
            for (new, old) in doc.parents[0]
                .placements
                .iter()
                .zip(&before.parents[0].placements)
            {
                assert_eq!(doc.pages[new.page].name, before.pages[old.page].name);
                assert_eq!(
                    (new.sheet, new.transform, new.visible),
                    (old.sheet, old.transform, old.visible)
                );
            }
            assert_eq!(
                doc.spreads, before.spreads,
                "page order changes within fixed slots"
            );
            reversible(&before, &mut doc, &mut history);
        }
    }
}

#[test]
fn removing_any_page_removes_only_its_objects_and_undo_restores_every_reference() {
    let before = document();
    for index in 0..6 {
        let mut doc = before.clone();
        let mut history = History::default();
        assert!(structure::remove_page(&mut doc, &mut history, index));
        assert_eq!(doc.objects.len(), before.objects.len() - 1);
        assert!(doc
            .objects
            .iter()
            .all(|object| before.object(object.id).unwrap().page != index));
        assert_eq!(doc.object_layers.len(), doc.objects.len());
        assert_eq!(
            doc.spreads.iter().map(|s| s.pages.len()).sum::<usize>(),
            doc.pages.len()
        );
        reversible(&before, &mut doc, &mut history);
    }
    let mut one = blank_a4();
    assert!(!structure::remove_page(
        &mut one,
        &mut History::default(),
        0
    ));
}

#[test]
fn inserting_anywhere_and_assigning_a_parent_are_individually_reversible() {
    let before = document();
    for index in 0..6 {
        let mut doc = before.clone();
        let mut history = History::default();
        assert!(structure::add_page(
            &mut doc,
            &mut history,
            index,
            Page::a4()
        ));
        reversible(&before, &mut doc, &mut history);
    }
    let mut doc = before.clone();
    let mut history = History::default();
    assert!(structure::set_parent(&mut doc, &mut history, 1, Some(0)));
    assert_eq!(doc.pages[1].master, Some(0));
    assert!(doc.parents[0].applied_to.contains(&1));
    reversible(&before, &mut doc, &mut history);
}

#[test]
fn layers_control_paint_order_visibility_and_editing_and_round_trip_through_undo() {
    let mut doc = document();
    let mut history = History::default();
    let id = structure::add_layer(&mut doc, &mut history, "Top".into()).unwrap();
    let object = doc.objects[0].id;
    assert!(structure::move_objects_to_layer(
        &mut doc,
        &mut history,
        &[object],
        id
    ));
    let before = doc.clone();
    history.clear();
    assert!(structure::change_layer(
        &mut doc,
        &mut history,
        id,
        |layer| layer.visible = false
    ));
    assert!(!doc.page_objects(0).iter().any(|o| o.id == object));
    reversible(&before, &mut doc, &mut history);
    history.clear();
    let before = doc.clone();
    assert!(structure::change_layer(
        &mut doc,
        &mut history,
        id,
        |layer| layer.locked = true
    ));
    let locked = doc.clone();
    assert!(!authoring::delete(&mut doc, &mut history, object));
    assert_eq!(doc, locked);
    reversible(&before, &mut doc, &mut history);
}

#[test]
fn an_invalid_batch_rolls_back_its_successful_prefix() {
    let mut doc = document();
    let before = doc.clone();
    let mut history = History::default();
    let page = snapshot_page(&doc.pages[0]);
    let mut changed = page.clone();
    changed.hidden = true;
    assert!(!history.apply(
        &mut doc,
        LayoutEdit::Batch {
            edits: vec![
                LayoutEdit::PageChanged {
                    index: 0,
                    before: page.clone(),
                    after: changed
                },
                LayoutEdit::PageChanged {
                    index: 99,
                    before: page.clone(),
                    after: page
                },
            ]
        }
    ));
    assert_eq!(doc, before);
    assert_eq!(history.undo_depth(), 0);
}

#[test]
fn renaming_a_style_updates_all_references_in_one_step_without_touching_the_other_kind() {
    use schist_layout::properties::rename_style;
    for paragraph in [true, false] {
        let mut doc = document();
        doc.stories = (0..12)
            .map(|_| {
                let mut story = Story::from_text("Body copy", "Default");
                story.apply_style(0, 4, "Default");
                story
            })
            .collect();
        let before = doc.clone();
        let mut history = History::default();
        assert!(rename_style(
            &mut doc,
            &mut history,
            paragraph,
            "Default",
            "Renamed"
        ));
        if paragraph {
            assert!(doc.styles.paragraph("Default").is_none());
            assert!(doc.styles.character("Default").is_some());
            assert_eq!(
                doc.styles.paragraph("Body").unwrap().based_on.as_deref(),
                Some("Renamed")
            );
        } else {
            assert!(doc.styles.character("Default").is_none());
            assert!(doc.styles.paragraph("Default").is_some());
            assert!(doc.stories.iter().all(|s| s.ranges[0].style == "Renamed"));
        }
        reversible(&before, &mut doc, &mut history);
    }
}

#[test]
fn creating_or_duplicating_frames_leaves_no_story_or_layer_entry_after_undo() {
    for text in [false, true] {
        let mut doc = blank_a4();
        let before = doc.clone();
        let mut history = History::default();
        let bounds = Rect::new(1.0, 2.0, 30.0, 40.0);
        let id = if text {
            authoring::text_frame(&mut doc, &mut history, 0, bounds)
                .unwrap()
                .object
        } else {
            authoring::rectangle(&mut doc, &mut history, 0, bounds, authoring::Paint::none())
                .unwrap()
        };
        reversible(&before, &mut doc, &mut history);
        history.clear();
        let before = doc.clone();
        assert!(authoring::duplicate(&mut doc, &mut history, id).is_some());
        reversible(&before, &mut doc, &mut history);
    }
}

#[test]
fn paragraph_style_application_is_one_step_for_any_number_of_stories() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let mut ids = Vec::new();
    for _ in 0..12 {
        let frame =
            authoring::text_frame(&mut doc, &mut history, 0, Rect::new(1.0, 2.0, 30.0, 40.0))
                .unwrap();
        authoring::set_text(&mut doc, &mut history, frame.story, "hello");
        ids.push(frame.object);
    }
    let before = doc.clone();
    history.clear();
    assert!(authoring::set_paragraph_style(
        &mut doc,
        &mut history,
        &ids,
        "Default"
    ));
    reversible(&before, &mut doc, &mut history);
}

#[test]
fn layer_drag_inserts_at_every_boundary_without_permuting_other_layers() {
    let mut before = document();
    for index in 0..5 {
        structure::add_layer(
            &mut before,
            &mut History::default(),
            format!("Layer {index}"),
        );
    }
    for &id in &before.layers {
        for target in before.layers.iter().copied().map(Some).chain([None]) {
            let mut doc = before.clone();
            let mut history = History::default();
            let changed = structure::place_layer(&mut doc, &mut history, id, target);
            if target == Some(id) {
                assert!(!changed);
            } else {
                let remaining: Vec<_> = before
                    .layers
                    .iter()
                    .copied()
                    .filter(|other| *other != id)
                    .collect();
                assert_eq!(
                    doc.layers
                        .iter()
                        .copied()
                        .filter(|other| *other != id)
                        .collect::<Vec<_>>(),
                    remaining
                );
                let index = doc.layers.iter().position(|other| *other == id).unwrap();
                assert_eq!(doc.layers.get(index + 1).copied(), target);
            }
            if changed {
                reversible(&before, &mut doc, &mut history);
            } else {
                assert_eq!(doc, before);
                assert_eq!(history.undo_depth(), 0);
            }
        }
    }
}

#[test]
fn layer_moves_insert_for_any_distance_and_invalid_targets_are_inert() {
    let mut before = document();
    for index in 0..5 {
        structure::add_layer(
            &mut before,
            &mut History::default(),
            format!("Layer {index}"),
        );
    }
    for from in 0..before.layers.len() {
        for to in 0..before.layers.len() {
            let mut doc = before.clone();
            let mut history = History::default();
            let mut expected = before.layers.clone();
            let id = expected.remove(from);
            expected.insert(to, id);
            assert_eq!(
                structure::move_layer(&mut doc, &mut history, id, to as isize - from as isize),
                from != to
            );
            assert_eq!(doc.layers, expected);
            if from != to {
                reversible(&before, &mut doc, &mut history);
            }
        }
    }
    let mut doc = before.clone();
    let mut history = History::default();
    for (id, target) in [(LayerId(999), None), (doc.layers[0], Some(LayerId(999)))] {
        assert!(!structure::place_layer(&mut doc, &mut history, id, target));
    }
    assert_eq!(doc, before);
    assert_eq!(history.undo_depth(), 0);
}

#[test]
fn dragging_any_size_selection_to_a_layer_is_one_edit_and_preserves_objects() {
    let mut before = document();
    let target =
        structure::add_layer(&mut before, &mut History::default(), "Target".into()).unwrap();
    for count in 1..=before.objects.len() {
        let mut doc = before.clone();
        let mut history = History::default();
        let ids: Vec<_> = doc
            .objects
            .iter()
            .take(count)
            .map(|object| object.id)
            .collect();
        assert!(structure::move_objects_to_layer(
            &mut doc,
            &mut history,
            &ids,
            target
        ));
        assert_eq!(doc.objects, before.objects);
        for object in &doc.objects {
            assert_eq!(
                doc.object_layer(object.id),
                if ids.contains(&object.id) {
                    target
                } else {
                    before.object_layer(object.id)
                }
            );
        }
        reversible(&before, &mut doc, &mut history);
        let after = doc.clone();
        assert!(!structure::move_objects_to_layer(
            &mut doc,
            &mut history,
            &ids,
            target
        ));
        assert_eq!(doc, after);
        assert_eq!(history.undo_depth(), 1);
    }
}

#[test]
fn a_layer_drop_rejects_the_whole_selection_if_any_member_or_destination_is_invalid() {
    for invalid in 0..4 {
        let mut doc = document();
        let target =
            structure::add_layer(&mut doc, &mut History::default(), "Target".into()).unwrap();
        let mut ids: Vec<_> = doc.objects.iter().map(|object| object.id).collect();
        let destination = match invalid {
            0 => {
                doc.objects.last_mut().unwrap().locked = true;
                target
            }
            1 => {
                ids.push(ObjectId(999));
                target
            }
            2 => {
                doc.layer_properties
                    .iter_mut()
                    .find(|layer| layer.id == target)
                    .unwrap()
                    .locked = true;
                target
            }
            _ => LayerId(999),
        };
        let before = doc.clone();
        let mut history = History::default();
        assert!(!structure::move_objects_to_layer(
            &mut doc,
            &mut history,
            &ids,
            destination
        ));
        assert_eq!(doc, before);
        assert_eq!(history.undo_depth(), 0);
    }
}

#[test]
fn changing_layer_order_preserves_implicit_membership_and_same_layer_drops_do_nothing() {
    let mut before = document();
    before.object_layers.clear();
    before.parents[0].objects[0].object.id = ObjectId(999);
    let original = before.layers[0];
    let mut ids: Vec<_> = before.objects.iter().map(|object| object.id).collect();
    let mut doc = before.clone();
    let mut history = History::default();
    assert!(!structure::move_objects_to_layer(
        &mut doc,
        &mut history,
        &ids,
        original
    ));
    assert_eq!(doc, before);
    assert_eq!(history.undo_depth(), 0);
    ids.push(ObjectId(999));
    structure::add_layer(&mut doc, &mut history, "New top".into()).unwrap();
    assert!(ids.iter().all(|id| doc.object_layer(*id) == original));
    reversible(&before, &mut doc, &mut history);
    doc.object_layers.clear();
    let before = doc.clone();
    let implicit_layer = doc.layers[0];
    history.clear();
    assert!(structure::place_layer(
        &mut doc,
        &mut history,
        implicit_layer,
        None
    ));
    assert!(ids.iter().all(|id| doc.object_layer(*id) == implicit_layer));
    reversible(&before, &mut doc, &mut history);
}
