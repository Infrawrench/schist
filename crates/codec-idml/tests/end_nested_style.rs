use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, blank_a4, compose,
    nested_styles::{CharacterStyle as NestedCharacter, Delimiter, NestedStyle},
    CharacterStyle, History, LayoutDocument, Rect, Story, StoryId,
};

fn native(text: &str, controls: &[usize], rules: Vec<NestedStyle>) -> LayoutDocument {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap();
    paragraph.point_size = Some(14.0);
    paragraph.nested_styles = Some(rules);
    doc.styles.add_character(CharacterStyle {
        name: "Nested".into(),
        point_size: Some(19.0),
        ..Default::default()
    });
    doc.stories[frame.story.0 as usize] = Story::from_text(text, "Body");
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let mut content = String::new();
    let mut cursor = 0;
    for at in controls {
        content.push_str(&quick_xml::escape::escape(&text[cursor..*at]));
        content.push_str("<?ACE 3?>");
        cursor = *at;
    }
    content.push_str(&quick_xml::escape::escape(&text[cursor..]));
    let parts: Vec<_> = package.names().into_iter().map(|name| {
        let original = package.get(name).unwrap();
        let bytes = if name.starts_with("Stories/") {
            let root = xml::parse(std::str::from_utf8(original).unwrap()).unwrap();
            let id = root.find("Story").unwrap().attr("Self").unwrap();
            format!(r#"<idPkg:Story><Story Self="{id}"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>{content}</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes()
        } else { original.to_vec() };
        (name.to_owned(), bytes)
    }).collect();
    import::read(&container::write(&parts)).unwrap().document
}

fn rule(delimiter: &str, repetition: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: NestedCharacter::Named("Nested".into()),
        delimiter: Delimiter::Enumeration(delimiter.into()),
        repetition,
        inclusive,
    }
}

fn assert_prefix(doc: &LayoutDocument, end: usize) {
    assert_spans(doc, std::slice::from_ref(&(0..end)));
}

fn assert_spans(doc: &LayoutDocument, spans: &[std::ops::Range<usize>]) {
    let story = &doc.stories[0];
    for start in schist_text_engine::grapheme_boundaries(&story.text()) {
        let spec = compose::spec_for(
            story,
            start,
            story.text_len(),
            &doc.styles,
            "Body",
            "Default",
            300.0,
        );
        for (byte, _) in spec.text.char_indices() {
            assert_eq!(
                spec.style_at(byte).size,
                if spans.iter().any(|range| range.contains(&(start + byte))) {
                    19.0
                } else {
                    14.0
                },
                "start={start}, byte={byte}, spans={spans:?}"
            );
        }
    }
}

#[test]
fn coincident_controls_advance_ordered_rules_and_repeats_without_source_bytes() {
    for inclusive in [false, true] {
        let mut skip = rule("EndNestedStyle", 1, inclusive);
        skip.character_style = NestedCharacter::None;
        let mut repeat = skip.clone();
        repeat.delimiter = Delimiter::Enumeration("Repeat".into());
        repeat.repetition = 2;
        for (controls, spans) in [
            (vec![0, 0, 3, 6, 6, 7], vec![3..6, 6..7]),
            (vec![0, 3, 3, 6, 7, 7], vec![0..3, 3..6]),
            (vec![0, 0, 0, 0, 0, 0], vec![]),
        ] {
            let doc = native(
                "Aé中 body",
                &controls,
                vec![
                    skip.clone(),
                    rule("EndNestedStyle", 1, inclusive),
                    repeat.clone(),
                ],
            );
            assert_spans(&doc, &spans);
        }
    }
}

#[test]
fn a_control_at_an_ordinary_boundary_belongs_to_the_unconsumed_rule() {
    for inclusive in [false, true] {
        let mut skip = rule("AnyCharacter", if inclusive { 1 } else { 2 }, inclusive);
        skip.character_style = NestedCharacter::None;
        let doc = native(
            "Aé中 body",
            &[1, 6],
            vec![skip, rule("EndNestedStyle", 1, true)],
        );
        // Through the first character finishes before the marker at byte 1.
        // Up-to the second character reaches that marker before its delimiter.
        assert_spans(
            &doc,
            if inclusive {
                &[]
            } else {
                std::slice::from_ref(&(1..6))
            },
        );
    }
}

#[test]
fn legacy_recovery_controls_upgrade_only_after_native_agreement_and_remain_stable() {
    let mut doc = native("Aé中 body", &[3], vec![rule("EndNestedStyle", 1, true)]);
    for control in &mut doc.stories[0].structures {
        control.control = None;
    }
    let mut imported = import::read(&export::write(&doc).bytes).unwrap().document;
    assert!(imported.stories[0].structures[0].control.is_some());
    for _ in 0..3 {
        assert_prefix(&imported, 3);
        let before = imported.stories[0].clone();
        imported = import::read(&export::write(&imported).bytes)
            .unwrap()
            .document;
        assert_eq!(imported.stories[0], before);
    }
}

#[test]
fn controls_restart_per_paragraph_and_follow_one_undoable_edit() {
    let mut doc = native("Aé body", &[3], vec![rule("EndNestedStyle", 1, false)]);
    let second = doc.stories[0].push_paragraph("中 tail", "Body").0;
    let mut marker = doc.stories[0].structures[0].clone();
    marker.at = Some(second + 3);
    doc.stories[0].structures.push(marker);
    let before = doc.clone();
    let mut history = History::default();
    assert!(authoring::replace_text(
        &mut doc,
        &mut history,
        StoryId(0),
        0..0,
        "Ω"
    ));
    assert_eq!(history.undo_depth(), 1);
    let edited = doc.clone();
    for (state, shift) in [(&before, 0), (&edited, 2)] {
        for (point, start) in state.stories[0]
            .points
            .iter()
            .zip(state.stories[0].point_offsets())
        {
            let schist_layout::StoryPoint::Paragraph { text, .. } = point else {
                continue;
            };
            let marker = state.stories[0]
                .structures
                .iter()
                .find_map(|s| s.at.filter(|at| *at >= start))
                .unwrap();
            for cut in schist_text_engine::grapheme_boundaries(text) {
                let spec = compose::spec_for(
                    &state.stories[0],
                    start + cut,
                    start + text.len(),
                    &state.styles,
                    "Body",
                    "Default",
                    300.0,
                );
                for (byte, _) in spec.text.char_indices() {
                    assert_eq!(
                        spec.style_at(byte).size,
                        if start + cut + byte < marker {
                            19.0
                        } else {
                            14.0
                        }
                    );
                }
            }
        }
        assert_eq!(state.stories[0].structures[1].at, Some(second + 3 + shift));
        assert_eq!(
            compose::compose_story(state, StoryId(0)).frames[0].unrendered_structures,
            0
        );
    }
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, edited);
}

#[test]
fn unsupported_control_locations_and_initial_interactions_stay_diagnosed() {
    for at in [None, Some(1), Some(usize::MAX)] {
        let mut doc = native("E\u{301}tail", &[0], vec![rule("EndNestedStyle", 1, true)]);
        doc.stories[0].structures[0].at = at;
        assert_prefix(&doc, doc.stories[0].text_len());
        assert_eq!(
            compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
            1
        );
    }
    for lines in [1, 3] {
        let mut doc = native("Aé body", &[3], vec![rule("EndNestedStyle", 1, true)]);
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Body")
            .unwrap()
            .drop_caps_lines = Some(lines);
        assert_eq!(
            compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
            1
        );
    }
    for count in [0, 2, i32::MAX] {
        let doc = native("Aé body", &[3], vec![rule("EndNestedStyle", count, true)]);
        assert!(
            schist_layout::nested_styles::unsupported(&doc.styles.resolve_paragraph("Body"))
                .is_some()
        );
        assert_prefix(&doc, 0);
    }
}

#[test]
fn marker_formatting_renames_with_its_style_in_one_step_and_saves_without_private_metadata() {
    use schist_layout::story::InlineControl;
    let mut doc = native("Aé body", &[3], vec![rule("EndNestedStyle", 1, true)]);
    doc.stories[0].structures[0].control = Some(InlineControl::EndNestedStyle {
        character_style: "Nested".into(),
    });
    let before = doc.clone();
    let mut history = History::default();
    assert!(schist_layout::properties::rename_style(
        &mut doc,
        &mut history,
        false,
        "Nested",
        "Renamed"
    ));
    assert_eq!(history.undo_depth(), 1);
    let edited = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, edited);
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let mut removed = 0;
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = if name.starts_with("Stories/") {
                let source = package.text(name).unwrap();
                let mut edited = source.to_string();
                if let Some(start) = edited.find("<KeyValuePair Key=\"Schist.StructuredStory.v1\"")
                {
                    let end = start + edited[start..].find("/>").unwrap() + 2;
                    edited.replace_range(start..end, "");
                    removed += 1;
                }
                edited.into_bytes()
            } else {
                package.get(name).unwrap().to_vec()
            };
            (name.to_string(), bytes)
        })
        .collect();
    assert_eq!(removed, 1);
    let doc = import::read(&container::write(&parts)).unwrap().document;
    assert_eq!(
        doc.stories[0].structures[0].control,
        Some(InlineControl::EndNestedStyle {
            character_style: "Renamed".into()
        })
    );
    assert_prefix(&doc, 3);
}

#[test]
fn deleting_a_native_control_does_not_resurrect_its_archived_anchor() {
    let doc = native("Aé body", &[3], vec![rule("EndNestedStyle", 1, true)]);
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let mut removed = 0;
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = if name.starts_with("Stories/") {
                let source = package.text(name).unwrap();
                removed += source.matches("<Content><?ACE 3?></Content>").count();
                source
                    .replace("<Content><?ACE 3?></Content>", "")
                    .into_bytes()
            } else {
                package.get(name).unwrap().to_vec()
            };
            (name.to_string(), bytes)
        })
        .collect();
    assert_eq!(removed, 1);
    let mut doc = import::read(&container::write(&parts)).unwrap().document;
    for _ in 0..3 {
        assert_eq!(doc.stories[0].structures.len(), 1);
        assert_eq!(doc.stories[0].structures[0].at, None);
        assert_prefix(&doc, doc.stories[0].text_len());
        assert_eq!(
            compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
            1
        );
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn native_end_controls_terminate_ordinary_rules_without_adding_source_characters() {
    let text = "Aé中 e\u{301} body 1";
    for delimiter in ["AnyCharacter", "AnyWord", "Letters", "Digits"] {
        for inclusive in [false, true] {
            for at in schist_text_engine::grapheme_boundaries(text) {
                let doc = native(text, &[at], vec![rule(delimiter, 100, inclusive)]);
                assert_eq!(doc.stories[0].text(), text);
                assert_prefix(&doc, at);
                assert_eq!(
                    compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
                    0
                );
            }
        }
    }
}

#[test]
fn explicit_end_delimiters_preserve_source_styles_and_native_controls_through_saves() {
    let text = "Aé中 body";
    for inclusive in [false, true] {
        let mut doc = native(text, &[6], vec![rule("EndNestedStyle", 1, inclusive)]);
        for _ in 0..3 {
            assert_prefix(&doc, 6);
            assert_eq!(doc.stories[0].text(), text);
            let saved = export::write(&doc);
            let package = container::read(&saved.bytes).unwrap();
            let instructions = package
                .names()
                .into_iter()
                .filter(|name| name.starts_with("Stories/"))
                .map(|name| {
                    let tree = xml::parse(package.text(name).unwrap()).unwrap();
                    tree.find_all("Content")
                        .iter()
                        .flat_map(|content| &content.instructions)
                        .filter(|(_, instruction)| instruction.split_whitespace().eq(["ACE", "3"]))
                        .count()
                })
                .sum::<usize>();
            assert_eq!(instructions, 1);
            doc = import::read(&saved.bytes).unwrap().document;
        }
    }
}
