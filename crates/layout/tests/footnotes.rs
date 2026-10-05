use schist_layout::{
    blank_a4, footnotes::*, snapshot_settings, History, LayoutDocument, SettingsSnapshot,
};

#[test]
fn typed_note_bodies_survive_parent_edits_style_renames_and_one_step_undo() {
    for count in [1, 3, 9] {
        for paragraph in [true, false] {
            let mut doc = blank_a4();
            doc.styles.paragraphs.push(schist_layout::ParagraphStyle {
                name: "Notes".into(),
                ..Default::default()
            });
            doc.styles.characters.push(schist_layout::CharacterStyle {
                name: "Notes".into(),
                ..Default::default()
            });
            let mut story = schist_layout::Story::from_text("AéB", "Body");
            let mut body = schist_layout::Story::from_text("é note", "Notes");
            body.ranges
                .push(schist_layout::StyleRange::new(0, 2, "Notes"));
            story.structures = (0..count)
                .map(|_| schist_layout::StoryStructure {
                    control: None,
                    at: Some(3),
                    kind: "Footnote".into(),
                    payload: "exact native XML".into(),
                    footnote: Some(FootnoteBody {
                        story: body.clone(),
                        markers: vec![FootnoteMarker {
                            at: 0,
                            character_style: "Notes".into(),
                        }],
                        reference_paragraph_style: "Notes".into(),
                        reference_character_style: "Notes".into(),
                    }),
                    anchored: None,
                })
                .collect();
            let original = story.clone();
            for insert in ["", "空", "new\nline"] {
                let changed = story.replace_text(0..0, insert, "Body").unwrap();
                for (before, after) in story.structures.iter().zip(&changed.structures) {
                    assert_eq!(after.at, Some(3 + insert.len()));
                    assert_eq!(before.footnote, after.footnote);
                    assert_eq!(before.payload, after.payload);
                }
            }
            doc.stories.push(story);
            let before = doc.clone();
            let mut history = History::default();
            assert!(schist_layout::properties::rename_style(
                &mut doc,
                &mut history,
                paragraph,
                "Notes",
                "Renamed"
            ));
            assert_eq!(history.undo_depth(), 1);
            for structure in &doc.stories[0].structures {
                let note = structure.footnote.as_ref().unwrap();
                assert_eq!(
                    note.reference_paragraph_style,
                    if paragraph { "Renamed" } else { "Notes" }
                );
                assert_eq!(
                    note.reference_character_style,
                    if paragraph { "Notes" } else { "Renamed" }
                );
                let schist_layout::StoryPoint::Paragraph { style, .. } = &note.story.points[0]
                else {
                    panic!()
                };
                assert_eq!(style, &note.reference_paragraph_style);
                assert_eq!(
                    note.markers[0].character_style,
                    note.reference_character_style
                );
                assert_eq!(note.story.ranges[0].style, note.reference_character_style);
                assert_eq!(structure.payload, "exact native XML");
            }
            let after = doc.clone();
            for _ in 0..4 {
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, after);
            }
            assert_eq!(before.stories[0], original);
        }
    }
}

#[test]
fn note_marker_coordinates_validate_every_byte_boundary_and_old_payloads_remain_opaque() {
    let text = "Aé中";
    for at in (0..=text.len() + 1).chain([usize::MAX]) {
        let note = FootnoteBody {
            story: schist_layout::Story::from_text(text, "Body"),
            markers: vec![FootnoteMarker {
                at,
                character_style: String::new(),
            }],
            reference_paragraph_style: String::new(),
            reference_character_style: String::new(),
        };
        assert_eq!(note.valid(), text.is_char_boundary(at));
    }
    let old: schist_layout::StoryStructure =
        serde_json::from_str(r#"{"at":3,"kind":"Footnote","payload":"<Footnote/>"}"#).unwrap();
    assert!(old.footnote.is_none());
}

#[test]
fn footnote_settings_commit_as_one_step_and_invalid_drafts_leave_history_untouched() {
    for count in [0, 1, 20] {
        let mut doc = blank_a4();
        doc.stories = (0..count)
            .map(|_| schist_layout::Story::from_text("é main", "Body"))
            .collect();
        let before = doc.clone();
        let mut history = History::default();
        let options = FootnoteOptions {
            start_at: Some(4),
            numbering: Some(FootnoteNumbering::RomanLower),
            separator: Some("\t\n\r空".into()),
            prefix: Some("é".repeat(100)),
            no_splitting: Some(false),
            space_between: Some(2.0),
            spacer: Some(7.2),
            rule: FootnoteRule {
                on: Some(true),
                width: Some(72.0),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(set_options(&mut doc, &mut history, options.clone()));
        assert_eq!(history.undo_depth(), 1);
        let after = doc.clone();
        assert!(!set_options(&mut doc, &mut history, options.clone()));
        for field in ["start", "text", "spacer", "weight", "tint", "continuing"] {
            let mut invalid = options.clone();
            match field {
                "start" => invalid.start_at = Some(0),
                "text" => invalid.prefix = Some("é".repeat(101)),
                "spacer" => invalid.spacer = Some(f32::NAN),
                "weight" => invalid.rule.weight = Some(-1.0),
                "tint" => invalid.rule.tint = Some(1.1),
                "continuing" => invalid.continuing_rule.width = Some(f32::INFINITY),
                _ => unreachable!(),
            }
            assert!(!set_options(&mut doc, &mut history, invalid));
            assert_eq!(doc, after);
            assert_eq!(history.undo_depth(), 1);
        }
        for _ in 0..4 {
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
            assert!(history.redo(&mut doc));
            assert_eq!(doc, after);
        }
    }
}

#[test]
fn old_documents_and_settings_snapshots_do_not_invent_footnote_defaults() {
    let doc = blank_a4();
    let mut serialized = serde_json::to_value(&doc).unwrap();
    assert!(serialized.get("footnotes").is_none());
    let back: LayoutDocument = serde_json::from_value(serialized.take()).unwrap();
    assert!(back.footnotes.is_empty());
    let mut snapshot = serde_json::to_value(snapshot_settings(&doc)).unwrap();
    snapshot.as_object_mut().unwrap().remove("footnotes");
    let back: SettingsSnapshot = serde_json::from_value(snapshot).unwrap();
    assert!(back.footnotes.is_empty());
    let prefs: FootnoteOptions = serde_json::from_str("{}").unwrap();
    assert!(prefs.is_empty());
}

#[test]
fn style_renames_update_only_resolved_references_of_the_right_kind_and_undo_once() {
    for paragraph in [true, false] {
        for reference in [
            FootnoteReference::None,
            FootnoteReference::Unresolved("Notes".into()),
            FootnoteReference::Resolved("Notes".into()),
        ] {
            let mut doc = blank_a4();
            doc.styles.paragraphs.push(schist_layout::ParagraphStyle {
                name: "Notes".into(),
                ..Default::default()
            });
            doc.styles.characters.push(schist_layout::CharacterStyle {
                name: "Notes".into(),
                ..Default::default()
            });
            doc.footnotes.text_style = Some(reference.clone());
            doc.footnotes.marker_style = Some(reference.clone());
            let original = doc.clone();
            let mut history = History::default();
            assert!(schist_layout::properties::rename_style(
                &mut doc,
                &mut history,
                paragraph,
                "Notes",
                "Renamed 空"
            ));
            assert_eq!(history.undo_depth(), 1);
            let (changed, unchanged) = if paragraph {
                (&doc.footnotes.text_style, &doc.footnotes.marker_style)
            } else {
                (&doc.footnotes.marker_style, &doc.footnotes.text_style)
            };
            assert_eq!(unchanged.as_ref(), Some(&reference));
            assert_eq!(
                *changed,
                Some(if matches!(reference, FootnoteReference::Resolved(_)) {
                    FootnoteReference::Resolved("Renamed 空".into())
                } else {
                    reference
                })
            );
            let after = doc.clone();
            for _ in 0..4 {
                assert!(history.undo(&mut doc));
                assert_eq!(doc, original);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, after);
            }
        }
    }
}
