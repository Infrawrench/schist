use schist_layout::{
    compose,
    nested_styles::{CharacterStyle as NestedCharacter, Delimiter, NestedStyle},
    CharacterStyle, Ink, ParagraphStyle, Story, StyleRange, WritingMode,
};

fn rule() -> NestedStyle {
    NestedStyle {
        character_style: NestedCharacter::Named("Initial".into()),
        delimiter: Delimiter::Enumeration("Dropcap".into()),
        repetition: 1,
        inclusive: true,
    }
}

#[test]
fn named_initials_follow_source_graphemes_and_direct_properties_on_every_slice() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for writing in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for prefix in ["E\u{301}", "É", "W"] {
            for count in [0, 1, 2, 5] {
                for lines in [0, 1, 3] {
                    let mut doc = schist_layout::blank_a4();
                    doc.styles.add_character(CharacterStyle {
                        name: "Initial".into(),
                        point_size: Some(19.0),
                        tracking: Some(70.0),
                        no_break: Some(true),
                        fill: Some(Ink::spot("Initial", [50.0, 60.0, -20.0])),
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Direct".into(),
                        tracking: Some(120.0),
                        fill: Some(Ink::cmyk("Direct", [0.0, 1.0, 0.0, 0.0])),
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Combined".into(),
                        based_on: Some("Initial".into()),
                        tracking: Some(120.0),
                        fill: Some(Ink::cmyk("Direct", [0.0, 1.0, 0.0, 0.0])),
                        ..Default::default()
                    });
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Base".into(),
                        family: Some("IBM Plex Sans".into()),
                        point_size: Some(12.0),
                        writing_mode: Some(writing),
                        drop_caps_lines: Some(lines),
                        drop_caps_characters: Some(count),
                        nested_styles: Some(vec![rule()]),
                        ..Default::default()
                    });
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Child".into(),
                        based_on: Some("Base".into()),
                        ..Default::default()
                    });
                    let text = format!("{prefix}abcdef more words");
                    let mut source = Story::from_text(&text, "Child");
                    source
                        .ranges
                        .push(StyleRange::new(prefix.len(), prefix.len() + 3, "Direct"));
                    let mut control = source.clone();
                    control.ranges.clear();
                    let boundaries: Vec<_> =
                        schist_text_engine::grapheme_boundaries(&text).collect();
                    for (i, pair) in boundaries.windows(2).enumerate() {
                        let initial = lines > 0 && i < count;
                        let direct = pair[0] >= prefix.len() && pair[0] < prefix.len() + 3;
                        let style = match (initial, direct) {
                            (true, true) => Some("Combined"),
                            (true, false) => Some("Initial"),
                            (false, true) => Some("Direct"),
                            _ => None,
                        };
                        if let Some(style) = style {
                            control
                                .ranges
                                .push(StyleRange::new(pair[0], pair[1], style));
                        }
                    }
                    let mut expected_styles = doc.styles.clone();
                    expected_styles
                        .paragraphs
                        .iter_mut()
                        .find(|p| p.name == "Child")
                        .unwrap()
                        .nested_styles = Some(Vec::new());
                    let before = (source.clone(), doc.clone());
                    for &start in &boundaries {
                        let actual = compose::spec_for(
                            &source,
                            start,
                            text.len(),
                            &doc.styles,
                            "Child",
                            &doc.default_character_style,
                            160.0,
                        );
                        let expected = compose::spec_for(
                            &control,
                            start,
                            text.len(),
                            &expected_styles,
                            "Child",
                            &doc.default_character_style,
                            160.0,
                        );
                        for (byte, _) in actual.text.char_indices() {
                            assert_eq!(
                                actual.style_at(byte),
                                expected.style_at(byte),
                                "{writing:?}/{prefix}/{count}/{lines}/{start}/{byte}"
                            );
                        }
                    }
                    assert_eq!((source, doc), before);
                }
            }
        }
    }
}

#[test]
fn unsupported_rules_do_not_hide_behind_a_supported_initial() {
    use schist_layout::nested_styles::unsupported;
    let mut paragraph = schist_layout::ResolvedParagraph::default();
    for delimiter in [
        Delimiter::Text("Dropcap".into()),
        Delimiter::Enumeration("AnyWord".into()),
        Delimiter::Enumeration("Future".into()),
    ] {
        for count in [i32::MIN, 0, 1, i32::MAX] {
            for inclusive in [false, true] {
                let mut unknown = rule();
                unknown.delimiter = delimiter.clone();
                unknown.repetition = count;
                unknown.inclusive = inclusive;
                for rules in [vec![unknown.clone()], vec![rule(), unknown]] {
                    paragraph.nested_styles = Some(rules);
                    assert_eq!(unsupported(&paragraph), Some("AllNestedStyles"));
                }
            }
        }
    }
    for count in [i32::MIN, 0, 2, i32::MAX] {
        let mut unknown = rule();
        unknown.repetition = count;
        paragraph.nested_styles = Some(vec![unknown]);
        assert!(unsupported(&paragraph).is_some());
    }
    let mut unknown = rule();
    unknown.inclusive = false;
    paragraph.nested_styles = Some(vec![unknown]);
    assert!(unsupported(&paragraph).is_some());
    let mut unresolved = rule();
    unresolved.character_style = NestedCharacter::Unresolved("missing".into());
    paragraph.nested_styles = Some(vec![unresolved]);
    assert!(unsupported(&paragraph).is_some());
    for lines in [0, 1, 3] {
        paragraph.drop_caps_lines = Some(lines);
        paragraph.nested_styles = Some(vec![rule()]);
        assert_eq!(unsupported(&paragraph), None);
    }
}

#[test]
fn source_initial_language_and_no_break_control_complete_dictionary_words() {
    use schist_layout::{hyphenation, language::TextLanguage};
    let text = "extraordinary";
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_character(CharacterStyle {
        name: "Initial".into(),
        language: Some(TextLanguage::Tag {
            tag: "en-US".into(),
        }),
        no_break: Some(false),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        drop_caps_lines: Some(3),
        drop_caps_characters: Some(text.len()),
        nested_styles: Some(vec![rule()]),
        ..Default::default()
    });
    let mut story = Story::from_text(text, "Source");
    for protected in [false, true] {
        doc.styles
            .characters
            .iter_mut()
            .find(|s| s.name == "Initial")
            .unwrap()
            .no_break = Some(protected);
        let paragraph = doc.styles.resolve_paragraph("Source");
        let character =
            paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
        let full =
            hyphenation::opportunities(&story, 0..text.len(), &doc.styles, &paragraph, &character);
        assert_eq!(full.is_empty(), protected);
        for start in 0..text.len() {
            let expected: Vec<_> = full
                .iter()
                .copied()
                .filter(|at| *at > start)
                .map(|at| at - start)
                .collect();
            assert_eq!(
                hyphenation::opportunities(
                    &story,
                    start..text.len(),
                    &doc.styles,
                    &paragraph,
                    &character
                ),
                expected
            );
        }
    }
    // A local unknown dictionary changes only that source letter, preventing
    // automatic dictionary guesses for the complete mixed-language word.
    doc.styles.add_character(CharacterStyle {
        name: "Direct".into(),
        language: Some(TextLanguage::Tag { tag: "und".into() }),
        ..Default::default()
    });
    doc.styles
        .characters
        .iter_mut()
        .find(|s| s.name == "Initial")
        .unwrap()
        .no_break = Some(false);
    story.ranges.push(StyleRange::new(0, 1, "Direct"));
    let paragraph = doc.styles.resolve_paragraph("Source");
    let character = paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
    assert!(
        hyphenation::opportunities(&story, 4..text.len(), &doc.styles, &paragraph, &character)
            .is_empty()
    );
}

#[path = "../../separation/examples/support/named_initials.rs"]
mod proof;
#[test]
fn source_edits_recompute_initial_ranges_without_persisting_them_and_undo_once() {
    proof::register_font();
    for case in [0, 4, 6] {
        let mut doc = proof::document(false, case);
        let before = doc.clone();
        let mut history = schist_layout::History::default();
        assert!(schist_layout::authoring::replace_text(
            &mut doc,
            &mut history,
            schist_layout::StoryId(0),
            0..0,
            "Q"
        ));
        assert_eq!(history.undo_depth(), 1);
        let edited = doc.clone();
        let story = &doc.stories[0];
        let spec = compose::spec_for(
            story,
            0,
            story.text_len(),
            &doc.styles,
            "Source",
            &doc.default_character_style,
            300.0,
        );
        assert_eq!(spec.style_at(0).font_style.as_deref(), Some("Light"));
        assert_ne!(spec.style_at(4).font_style.as_deref(), Some("Light"));
        let thread = compose::compose_story(&doc, schist_layout::StoryId(0));
        assert!(!thread.has_overflow());
        assert_eq!(doc, edited);
        assert_eq!(doc.styles, before.styles);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, edited);
    }
}

#[test]
fn generated_list_markers_inherit_source_initial_context_without_reapplying_rules() {
    use schist_layout::{list_composition::MarkerPlans, lists::ListKind};
    proof::register_font();
    for explicit in [false, true] {
        let mut doc = proof::document(false, 7);
        let paragraph = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Source")
            .unwrap();
        paragraph.list.kind = Some(ListKind::Bullet);
        paragraph.list.bullet_character_style = explicit.then(|| "Direct".into());
        let marker = MarkerPlans::new(&doc, &doc.stories[0]);
        let spec = marker.spec(0).unwrap();
        for (byte, _) in spec.text.char_indices() {
            let style = spec.style_at(byte);
            assert_eq!(
                style.font_style.as_deref(),
                Some(if explicit { "Regular" } else { "Light" })
            );
            assert_eq!(style.size, 18.0);
        }
    }
}
