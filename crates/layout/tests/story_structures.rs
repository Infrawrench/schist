use schist_layout::{
    authoring, blank_a4, threading, History, Rect, Story, StoryPoint, StoryStructure,
};

fn structure(at: Option<usize>) -> StoryStructure {
    StoryStructure {
        control: None,
        at,
        kind: "Footnote".into(),
        payload: "<Footnote>é<?ACE 4?></Footnote>".into(),
        footnote: None,
        table: None,
        anchored: None,
    }
}

#[test]
fn every_utf8_edit_either_preserves_the_anchor_or_refuses_to_cross_it() {
    let original = "Aé中\ntail";
    let boundaries: Vec<_> = original
        .char_indices()
        .map(|(i, _)| i)
        .chain([original.len()])
        .collect();
    for at in &boundaries {
        let mut story = Story::from_text("Aé中", "Body");
        story.push_paragraph("tail", "Body");
        story.structures = vec![structure(Some(*at)), structure(None)];
        for start in &boundaries {
            for end in boundaries.iter().filter(|end| *end >= start) {
                for insert in ["", "é", "新\nline"] {
                    let edited = story.replace_text(*start..*end, insert, "Body");
                    if start < at && at < end {
                        assert!(edited.is_none(), "{start} < {at} < {end}");
                    } else {
                        let edited = edited.unwrap();
                        let mut text = original.to_string();
                        text.replace_range(*start..*end, insert);
                        assert_eq!(edited.text(), text);
                        let anchor = if at >= end {
                            at - end + start + insert.len()
                        } else {
                            *at
                        };
                        assert_eq!(
                            edited.structures,
                            vec![structure(Some(anchor)), structure(None)]
                        );
                        assert!(edited.text().is_char_boundary(anchor));
                    }
                }
            }
        }
    }
}

#[test]
fn corrupt_source_coordinates_cannot_turn_an_edit_into_payload_loss() {
    for at in [2, 4, usize::MAX] {
        let mut story = Story::from_text("Aé", "Body");
        story.structures.push(structure(Some(at)));
        assert!(story.replace_text(0..0, "insert", "Body").is_none());
    }
    let old: Story = serde_json::from_str(r#"{"points":[],"ranges":[]}"#).unwrap();
    assert!(old.structures.is_empty());
    assert!(old.replace_text(0..0, "new", "Body").is_some());
}

#[test]
fn authoring_snapshots_preserve_structures_and_exact_one_step_undo() {
    for count in [1, 3, 9] {
        let mut doc = blank_a4();
        let mut history = History::default();
        let frame =
            authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0))
                .unwrap();
        let mut story = Story::from_text("AéB", "Body");
        story.structures = (0..count).map(|_| structure(Some(1))).collect();
        doc.stories[frame.story.0 as usize] = story;
        let before = doc.clone();
        let depth = history.undo_depth();
        assert!(authoring::set_text(
            &mut doc,
            &mut history,
            frame.story,
            "newAéB"
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        assert!(doc
            .story(frame.story)
            .unwrap()
            .structures
            .iter()
            .all(|s| s.at == Some(4)));
        let after = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, after);
        let depth = history.undo_depth();
        assert!(!authoring::set_text(
            &mut doc,
            &mut history,
            frame.story,
            ""
        ));
        assert_eq!(history.undo_depth(), depth);
        assert_eq!(doc, after);
    }
}

#[test]
fn opaque_only_stories_are_not_empty_thread_targets() {
    for at in [Some(0), None] {
        for legacy in [false, true] {
            let mut doc = blank_a4();
            let mut history = History::default();
            let a =
                authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0))
                    .unwrap();
            let b =
                authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 50.0, 100.0, 40.0))
                    .unwrap();
            if legacy {
                doc.stories[b.story.0 as usize]
                    .points
                    .push(StoryPoint::Other {
                        kind: "Table".into(),
                        payload: "raw".into(),
                    });
            } else {
                doc.stories[b.story.0 as usize]
                    .structures
                    .push(structure(at));
            }
            assert_eq!(doc.story(b.story).unwrap().text_len(), 0);
            let before = doc.clone();
            let old_history = history.clone();
            assert!(!threading::link(&mut doc, &mut history, a.object, b.object));
            assert_eq!(doc, before);
            assert_eq!(history, old_history);
        }
    }
}

#[test]
fn paragraph_and_character_style_edits_keep_opaque_data_in_their_undo_snapshots() {
    for paragraph in [false, true] {
        let mut doc = blank_a4();
        let mut history = History::default();
        let frame =
            authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0))
                .unwrap();
        let mut story = Story::from_text("AéB", "Body");
        story.structures = vec![structure(Some(1)), structure(None)];
        doc.stories[frame.story.0 as usize] = story;
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: "Changed".into(),
            ..Default::default()
        });
        doc.styles.add_character(schist_layout::CharacterStyle {
            name: "Changed".into(),
            ..Default::default()
        });
        let before = doc.clone();
        let depth = history.undo_depth();
        if paragraph {
            assert!(authoring::set_paragraph_style(
                &mut doc,
                &mut history,
                &[frame.object],
                "Changed"
            ));
        } else {
            assert!(authoring::set_character_style(
                &mut doc,
                &mut history,
                frame.story,
                0..3,
                "Changed"
            ));
        }
        assert_eq!(doc.stories[0].structures, before.stories[0].structures);
        assert_eq!(history.undo_depth(), depth + 1);
        let after = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, after);
    }
}
