//! Undo and redo against a real document.
//!
//! The tests here are round trips: apply an edit, undo it, and require
//! the document to be byte-identical to what it was. An inverse that is
//! nearly right is worse than no undo at all, because it is very hard to
//! notice and very expensive to find.

use schist_layout::edit::{
    snapshot_character_style, snapshot_ink, snapshot_object, snapshot_page, snapshot_settings,
    snapshot_spread, snapshot_story,
};
use schist_layout::geometry::{mm, Insets, Rect, Spread};
use schist_layout::ink::Ink;
use schist_layout::model::{
    blank_a4, FrameOverflow, LayoutDocument, LayoutObject, ObjectId, ParentPage, PlacedObject,
    StoryId,
};
use schist_layout::story::{Point as StoryPoint, Story};
use schist_layout::{GridSettings, History, LayoutEdit, Page};

fn doc_with_three_pages() -> LayoutDocument {
    let mut doc = blank_a4();
    doc.pages = vec![
        Page::letter(),
        Page::a4(),
        Page::new("Wide", mm(300.0), mm(200.0)),
    ];
    doc.spreads = vec![Spread::single(0), Spread::single(1), Spread::single(2)];
    doc
}

fn text_object(page: usize, name: &str) -> PlacedObject {
    PlacedObject {
        appearance: Default::default(),
        id: ObjectId::next(),
        page,
        bounds: Rect::new(mm(20.0), mm(20.0), mm(50.0), mm(30.0)),
        object: LayoutObject::TextFrame {
            text_path: None,
            story: StoryId(0),
            columns: 1,
            gutter: 0.0,
            insets: Insets::ZERO,
            overflow: FrameOverflow::Thread,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: name.into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    }
}

/// Apply, undo, and require the document back exactly as it was.
fn round_trip(doc: &mut LayoutDocument, edit: LayoutEdit) {
    let before = doc.clone();
    let mut history = History::default();
    assert!(history.apply(doc, edit), "the edit did not apply");
    assert_ne!(*doc, before, "the edit changed nothing");
    assert!(history.undo(doc), "undo failed");
    assert_eq!(*doc, before, "undo did not restore the document");
    assert!(history.redo(doc), "redo failed");
    assert_ne!(*doc, before, "redo changed nothing");
    assert!(history.undo(doc), "a second undo failed");
    assert_eq!(*doc, before, "a second undo did not restore the document");
}

#[test]
fn adding_an_object_round_trips() {
    let mut doc = blank_a4();
    let object = text_object(0, "Body");
    let edit = LayoutEdit::AddedObject {
        index: 0,
        object: snapshot_object(&object),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn removing_an_object_round_trips() {
    let mut doc = blank_a4();
    doc.add_object(text_object(0, "Body"));
    let edit = LayoutEdit::RemovedObject {
        index: 0,
        object: snapshot_object(&doc.objects[0]),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn moving_an_object_round_trips() {
    let mut doc = blank_a4();
    let mut object = text_object(0, "Body");
    object.bounds = Rect::new(1.0, 2.0, 3.0, 4.0);
    let id = doc.add_object(object);
    let mut moved = doc.objects[0].clone();
    moved.bounds = Rect::new(40.0, 50.0, 60.0, 70.0);
    let edit = LayoutEdit::ObjectChanged {
        id: id.0,
        before: snapshot_object(&doc.objects[0]),
        after: snapshot_object(&moved),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn editing_a_story_round_trips() {
    let mut doc = blank_a4();
    let mut story = Story::new();
    story.push_paragraph("The quick brown fox.", "Body");
    story.apply_style(4, 9, "Italic");
    let id = doc.add_story(story);
    let mut edited = doc.story(id).unwrap().clone();
    edited.points[0] = StoryPoint::Paragraph {
        text: "The quick brown dog jumps over the lazy dog.".into(),
        style: "Body".into(),
    };
    let edit = LayoutEdit::StoryChanged {
        id: id.0,
        before: snapshot_story(doc.story(id).unwrap()),
        after: snapshot_story(&edited),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn a_page_change_round_trips() {
    let mut doc = doc_with_three_pages();
    let mut after = doc.pages[1].clone();
    after.bleed = (mm(3.0)).into();
    after.margins = Insets::uniform(mm(10.0));
    let edit = LayoutEdit::PageChanged {
        index: 1,
        before: snapshot_page(&doc.pages[1]),
        after: snapshot_page(&after),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn adding_a_page_round_trips() {
    let mut doc = doc_with_three_pages();
    let edit = LayoutEdit::AddedPage {
        index: 1,
        page: snapshot_page(&Page::a4()),
        spreads: doc.spreads.iter().map(snapshot_spread).collect(),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn removing_a_page_round_trips_and_renumbers() {
    let mut doc = doc_with_three_pages();
    // An object on the last page, so the renumbering can be seen.
    doc.add_object(text_object(2, "On the last page"));
    let edit = LayoutEdit::RemovedPage {
        index: 0,
        page: snapshot_page(&doc.pages[0]),
        spreads: doc.spreads.iter().map(snapshot_spread).collect(),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn a_page_move_round_trips() {
    let mut doc = doc_with_three_pages();
    let edit = LayoutEdit::PageMoved { from: 0, to: 2 };
    round_trip(&mut doc, edit);
}

#[test]
fn a_style_change_round_trips() {
    let mut doc = blank_a4();
    let mut body = doc.styles.paragraph("Body").unwrap().clone();
    body.point_size = Some(9.0);
    let edit = LayoutEdit::StyleChanged {
        name: "Body".into(),
        before: Some(snapshot_paragraph(doc.styles.paragraph("Body").unwrap())),
        after: Some(snapshot_paragraph(&body)),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn a_character_style_removal_round_trips() {
    let mut doc = blank_a4();
    let before = doc.styles.character("Bold").unwrap().clone();
    let edit = LayoutEdit::StyleChanged {
        name: "Bold".into(),
        before: Some(snapshot_character_style(&before)),
        after: None,
    };
    round_trip(&mut doc, edit);
}

#[test]
fn an_ink_change_round_trips() {
    let mut doc = blank_a4();
    let spot = Ink::spot("PANTONE 032 C", [50.0, 60.0, 55.0]);
    doc.inks.push(spot.clone());
    let edit = LayoutEdit::InkChanged {
        name: spot.name.clone(),
        before: Some(snapshot_ink(&spot)),
        after: None,
    };
    round_trip(&mut doc, edit);
}

#[test]
fn a_settings_change_round_trips() {
    let mut doc = blank_a4();
    let mut after = doc.clone();
    after.facing_pages = true;
    after.page_binding = schist_layout::PageBinding::RightToLeft;
    after.grids.document = GridSettings {
        columns: 3,
        ..Default::default()
    };
    let edit = LayoutEdit::DocumentChanged {
        before: Box::new(snapshot_settings(&doc)),
        after: Box::new(snapshot_settings(&after)),
    };
    round_trip(&mut doc, edit);
}

#[test]
fn a_long_sequence_of_undo_returns_to_the_start() {
    let mut doc = blank_a4();
    let mut history = History::default();
    for i in 0..20 {
        let mut object = text_object(0, "Body");
        object.name = format!("Frame {i}");
        let index = doc.objects.len();
        assert!(history.apply(
            &mut doc,
            LayoutEdit::AddedObject {
                index,
                object: snapshot_object(&object),
            }
        ));
        assert_eq!(doc.objects.len(), i + 1);
    }
    while history.undo(&mut doc) {
        assert_eq!(doc.objects.len(), history.undo_depth());
    }
    assert!(doc.objects.is_empty());
    assert!(!history.can_undo());
}

#[test]
fn undo_across_page_edits_keeps_objects_on_their_page() {
    // A page reference that does not move with its page is how an object
    // ends up on the wrong sheet after a reordering.
    let mut doc = doc_with_three_pages();
    // Only the last two pages, so the effect of removing the first is
    // unambiguous.
    for page in 1..3 {
        doc.add_object(text_object(page, "Frame"));
    }
    let mut history = History::default();
    let removed = LayoutEdit::RemovedPage {
        index: 0,
        page: snapshot_page(&doc.pages[0]),
        spreads: doc.spreads.iter().map(snapshot_spread).collect(),
    };
    assert!(history.apply(&mut doc, removed));
    // Page 0 is gone, so the two objects that were on pages 1 and 2 are
    // now on pages 0 and 1.
    assert_eq!(doc.objects[0].page, 0);
    assert_eq!(doc.objects[1].page, 1);
    assert!(history.undo(&mut doc));
    assert_eq!(doc.objects[0].page, 1);
    assert_eq!(doc.objects[1].page, 2);
}

#[test]
fn a_parent_page_follows_its_pages_through_a_move() {
    let mut doc = doc_with_three_pages();
    doc.parents.push(ParentPage {
        name: "A-Master".into(),
        sheets: Vec::new(),
        placements: Vec::new(),
        applied_to: vec![2],
        based_on: None,
        objects: Vec::new(),
        hidden: false,
    });
    let mut history = History::default();
    assert!(history.apply(&mut doc, LayoutEdit::PageMoved { from: 2, to: 0 }));
    assert_eq!(doc.parents[0].applied_to, vec![0]);
}

#[test]
fn an_edit_that_cannot_apply_leaves_the_document_alone() {
    let mut doc = blank_a4();
    let mut history = History::default();
    // There is no page 99, so this cannot happen.
    let edit = LayoutEdit::RemovedObject {
        index: 99,
        object: schist_layout::ObjectSnapshot {
            appearance: Default::default(),
            id: 1,
            page: 0,
            bounds: [0.0; 4],
            name: "Ghost".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
            rotation: 0.0,
            transform: Default::default(),
            payload: serde_json::Value::Null,
        },
    };
    assert!(!history.apply(&mut doc, edit));
    assert!(doc.objects.is_empty());
    assert!(!history.can_undo(), "a failed edit was recorded anyway");
}

#[test]
fn an_undo_that_cannot_apply_keeps_the_history_honest() {
    // If undo cannot be completed, the operation must go back on the
    // stack: a history that has forgotten an edit still in the document
    // will replay it and apply it twice.
    let mut doc = blank_a4();
    let id = doc.add_object(text_object(0, "Body"));
    let mut history = History::default();
    let edit = LayoutEdit::ObjectChanged {
        id: id.0,
        before: snapshot_object(&doc.objects[0]),
        after: snapshot_object(&doc.objects[0]),
    };
    assert!(history.apply(&mut doc, edit));
    // Remove the object behind the history's back.
    doc.objects.clear();
    assert!(!history.undo(&mut doc));
    assert!(
        history.can_undo(),
        "the operation was lost on a failed undo"
    );
}

fn snapshot_paragraph(style: &schist_layout::ParagraphStyle) -> schist_layout::StyleSnapshot {
    schist_layout::edit::snapshot_paragraph_style(style)
}

#[test]
fn branching_after_any_number_of_undos_never_replays_the_abandoned_future() {
    for count in 1..12 {
        for undone in 1..=count {
            let mut doc = blank_a4();
            let mut history = History::default();
            for i in 0..count {
                let before = snapshot_page(&doc.pages[0]);
                let mut after = before.clone();
                after.name = format!("edit {i}");
                assert!(history.apply(
                    &mut doc,
                    LayoutEdit::PageChanged {
                        index: 0,
                        before,
                        after
                    }
                ));
            }
            for _ in 0..undone {
                assert!(history.undo(&mut doc));
            }
            let branch = doc.clone();
            let before = snapshot_page(&doc.pages[0]);
            let mut after = before.clone();
            after.name = "branch".into();
            assert!(history.apply(
                &mut doc,
                LayoutEdit::PageChanged {
                    index: 0,
                    before,
                    after
                }
            ));
            assert_eq!(history.undo_depth(), count - undone + 1);
            assert!(!history.can_redo());
            assert!(history.undo(&mut doc));
            assert_eq!(doc, branch);
            assert!(history.redo(&mut doc));
            assert_eq!(doc.pages[0].name, "branch");
            for _ in 0..count - undone + 1 {
                assert!(history.undo(&mut doc));
            }
            assert_eq!(doc, blank_a4());
            assert!(!history.can_undo());
        }
    }
}

#[test]
fn failed_redo_preserves_both_stacks() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let before = snapshot_page(&doc.pages[0]);
    let mut after = before.clone();
    after.hidden = true;
    assert!(history.apply(
        &mut doc,
        LayoutEdit::PageChanged {
            index: 0,
            before,
            after
        }
    ));
    assert!(history.undo(&mut doc));
    doc.pages.clear();
    let before = history.clone();
    for _ in 0..3 {
        assert!(!history.redo(&mut doc));
        assert_eq!(history, before);
    }
}

#[test]
fn malformed_multi_object_edits_never_partially_apply() {
    let mut doc = blank_a4();
    for _ in 0..3 {
        doc.add_object(text_object(0, "frame"));
    }
    let before = doc.clone();
    let mut history = History::default();
    let snapshot = snapshot_object(&doc.objects[1]);
    for items in [
        vec![(1, snapshot.clone()), (1, snapshot.clone())],
        vec![(1, snapshot.clone()), (99, snapshot)],
    ] {
        assert!(!history.apply(&mut doc, LayoutEdit::RemovedObjects { items }));
        assert_eq!(doc, before);
    }
}
