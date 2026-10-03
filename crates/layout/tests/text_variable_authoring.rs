use schist_layout::{
    blank_a4,
    story::InlineControl,
    text_variables::{self as variables, Cursor},
    History, Story, StoryId, StoryStructure,
};

fn document(text: &str) -> schist_layout::LayoutDocument {
    let mut doc = blank_a4();
    doc.stories.push(Story::from_text(text, "Body"));
    doc
}

#[test]
fn shared_definition_edits_are_one_step_and_leave_every_story_and_recovery_byte_alone() {
    for count in [0, 1, 8, 40] {
        let mut doc = document("aé🙂z");
        let mut history = History::default();
        let id = variables::create(&mut doc, &mut history, "Edition", "first").unwrap();
        for _ in 0..count {
            let cursor = Cursor::capture(&doc, StoryId(0), 1).unwrap();
            let definition = doc.text_variables[0].clone();
            assert!(cursor.insert(&mut doc, &mut history, &definition));
        }
        doc.retained_text_variables
            .push("opaque original XML".into());
        let before = doc.clone();
        let depth = history.undo_depth();
        let expected = doc.text_variables[0].clone();
        assert!(variables::update(
            &mut doc,
            &mut history,
            &expected,
            "New name",
            "  revised café  "
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        assert_eq!(doc.stories, before.stories);
        assert_eq!(doc.retained_text_variables, before.retained_text_variables);
        assert_eq!(doc.text_variables[0].id, id);
        let after = doc.clone();
        for _ in 0..3 {
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
            assert!(history.redo(&mut doc));
            assert_eq!(doc, after);
        }
    }
}

#[test]
fn insertion_requires_grapheme_boundaries_and_keeps_coincident_objects_in_order() {
    let text = "Ae\u{301}👩\u{200d}💻Z";
    let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
    for at in 0..=text.len() + 1 {
        let mut doc = document(text);
        let mut history = History::default();
        variables::create(&mut doc, &mut history, "First", "α").unwrap();
        let definition = doc.text_variables[0].clone();
        let cursor = Cursor::capture(&doc, StoryId(0), at);
        assert_eq!(cursor.is_some(), boundaries.contains(&at), "{at}");
        let Some(cursor) = cursor else {
            continue;
        };
        let before = doc.clone();
        let depth = history.undo_depth();
        assert!(cursor.insert(&mut doc, &mut history, &definition));
        assert_eq!(doc.stories[0].text(), text);
        assert_eq!(history.undo_depth(), depth + 1);
        assert!(
            !cursor.insert(&mut doc, &mut history, &definition),
            "stale snapshot"
        );
        let cursor = Cursor::capture(&doc, StoryId(0), at).unwrap();
        let mut second_definition = definition.clone();
        second_definition.id = "second".into();
        doc.text_variables.push(second_definition.clone());
        assert!(cursor.insert(&mut doc, &mut history, &second_definition));
        let pair = doc.clone();
        let cursor = Cursor::capture(&doc, StoryId(0), at).unwrap();
        assert!(cursor.remove_instance(&mut doc, &mut history, 0));
        assert_eq!(doc.stories[0].structures, pair.stories[0].structures[1..]);
        assert_eq!(history.undo_depth(), depth + 3);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, pair);
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        doc.text_variables.pop(); // independently installed definition was not a history edit
        assert_eq!(doc, before);
    }
}

#[test]
fn identity_allocation_reserves_missing_references_in_main_stories_and_nested_notes() {
    let mut doc = document("text");
    let instance = |id: &str| StoryStructure {
        at: Some(0),
        kind: "TextVariableInstance".into(),
        payload: String::new(),
        footnote: None,
        control: Some(InlineControl::TextVariable {
            variable: id.into(),
            name: String::new(),
            character_style: String::new(),
        }),
    };
    doc.stories[0].structures.push(instance("SchistCustom0"));
    let mut note = schist_layout::footnotes::FootnoteBody {
        story: Story::from_text("note", "Body"),
        markers: Vec::new(),
        reference_paragraph_style: "Body".into(),
        reference_character_style: String::new(),
    };
    note.story.structures.push(instance("SchistCustom1"));
    doc.stories[0].structures.push(StoryStructure {
        at: Some(0),
        kind: "Footnote".into(),
        payload: "opaque".into(),
        control: None,
        footnote: Some(note),
    });
    let original = doc.stories.clone();
    let mut history = History::default();
    let first = variables::create(&mut doc, &mut history, "Same name", "one").unwrap();
    let second = variables::create(&mut doc, &mut history, "Same name", "two").unwrap();
    assert_eq!(first, "SchistCustom2");
    assert_ne!(first, second);
    assert_eq!(doc.stories, original);
    assert_eq!(variables::usage_count(&doc, "SchistCustom1"), 1);
    assert_eq!(variables::usage_count(&doc, &first), 0);
}

#[test]
fn stale_invalid_ambiguous_and_unchanged_authoring_never_consumes_undo_or_redo() {
    let mut doc = document("text");
    let mut history = History::default();
    variables::create(&mut doc, &mut history, "Edition", "literal").unwrap();
    let definition = doc.text_variables[0].clone();
    let cursor = Cursor::capture(&doc, StoryId(0), 0).unwrap();
    assert!(cursor.insert(&mut doc, &mut history, &definition));
    assert!(history.undo(&mut doc));
    let before = doc.clone();
    let depth = (history.undo_depth(), history.redo_depth());
    for (name, value) in [
        ("", "ok"),
        (" \t ", "ok"),
        ("Edition", "bad\nvalue"),
        ("Edition", "a\u{2068}b"),
        ("Edition", "literal"),
    ] {
        assert!(!variables::update(
            &mut doc,
            &mut history,
            &definition,
            name,
            value
        ));
        assert_eq!(doc, before);
        assert_eq!((history.undo_depth(), history.redo_depth()), depth);
    }
    doc.stories[0] = Story::from_text("changed", "Body");
    assert!(!cursor.insert(&mut doc, &mut history, &definition));
    assert!(!cursor.remove_instance(&mut doc, &mut history, 0));
    doc.text_variables.push(definition.clone());
    assert!(!variables::remove(&mut doc, &mut history, &definition));
    assert!(!variables::update(
        &mut doc,
        &mut history,
        &definition,
        "different",
        "value"
    ));
    let cursor = Cursor::capture(&doc, StoryId(0), 0).unwrap();
    assert!(!cursor.insert(&mut doc, &mut history, &definition));
    assert_eq!((history.undo_depth(), history.redo_depth()), depth);
}

#[test]
fn deletion_requires_explicit_instance_removal_and_both_gestures_undo_exactly() {
    for count in [1, 3, 12] {
        let mut doc = document("source");
        let mut history = History::default();
        variables::create(&mut doc, &mut history, "Edition", "").unwrap();
        let definition = doc.text_variables[0].clone();
        for at in 0..count {
            let cursor = Cursor::capture(&doc, StoryId(0), at % 7).unwrap();
            assert!(cursor.insert(&mut doc, &mut history, &definition));
        }
        let before = doc.clone();
        let depth = history.undo_depth();
        for _ in 0..count {
            assert!(!variables::remove(&mut doc, &mut history, &definition));
            let at = doc.stories[0].structures[0].at.unwrap();
            let cursor = Cursor::capture(&doc, StoryId(0), at).unwrap();
            assert!(cursor.remove_instance(&mut doc, &mut history, 0));
        }
        assert!(variables::remove(&mut doc, &mut history, &definition));
        assert_eq!(history.undo_depth(), depth + count + 1);
        for _ in 0..count + 1 {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc, before);
    }
}

#[test]
fn definition_history_rejects_external_replacement_without_overwriting_it() {
    let mut doc = document("");
    let mut history = History::default();
    variables::create(&mut doc, &mut history, "Edition", "before").unwrap();
    doc.text_variables[0].contents = "external".into();
    let before = doc.clone();
    assert!(!history.undo(&mut doc));
    assert_eq!(doc, before);
    assert_eq!(history.undo_depth(), 1);
}
