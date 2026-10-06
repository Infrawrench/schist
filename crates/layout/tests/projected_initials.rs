use schist_layout::{
    blank_a4,
    compose::{compose_thread, line_spec, InsetsLike},
    footnotes::{FootnoteAffixes, FootnoteBody, FootnoteFirstBaseline, FootnoteMarker},
    styles::Leading,
    FrameOverflow, ObjectId, ParagraphStyle, Rect, Story, StoryStructure,
};

#[test]
fn generated_references_never_replace_source_graphemes_in_an_opening_initial() {
    for prefix in ["E\u{301}x", "Éy", "Wx"] {
        for characters in [1, 2, 3] {
            for number in [1, 99, 105] {
                for whole in [false, true] {
                    let mut doc = blank_a4();
                    doc.footnotes.no_splitting = Some(whole);
                    doc.footnotes.first_baseline = Some(FootnoteFirstBaseline::Ascent);
                    doc.footnotes.end_of_story = Some(true);
                    doc.footnotes.rule.on = Some(false);
                    doc.footnotes.continuing_rule.on = Some(false);
                    doc.footnotes.start_at = Some(number);
                    doc.footnotes.affixes = Some(FootnoteAffixes::Both);
                    doc.footnotes.prefix = Some("[".into());
                    doc.footnotes.suffix = Some("]".into());
                    for name in ["Initial", "Note"] {
                        doc.styles.add_paragraph(ParagraphStyle {
                            name: name.into(),
                            point_size: Some(11.0),
                            leading: Some(Leading::Points(14.0)),
                            drop_caps_lines: Some(3),
                            drop_caps_characters: Some(characters),
                            keep_lines: Some(1),
                            ..Default::default()
                        });
                    }
                    let text = format!("{prefix}alpha beta gamma\u{2028}second row\u{2028}third row\u{2028}last row");
                    let end = schist_text_engine::grapheme_boundaries(&text)
                        .nth(characters)
                        .unwrap();
                    let first = schist_text_engine::grapheme_boundaries(&text)
                        .nth(1)
                        .unwrap();
                    for anchor in [0, first, end, text.len()] {
                        let mut story = Story::from_text(&text, "Initial");
                        story.structures.push(StoryStructure {
                            control: None,
                            at: Some(anchor),
                            kind: "Footnote".into(),
                            payload: "retained native source".into(),
                            footnote: Some(FootnoteBody {
                                story: Story::from_text(&text, "Note"),
                                markers: vec![FootnoteMarker {
                                    at: 0,
                                    character_style: "Default".into(),
                                }],
                                reference_paragraph_style: "Initial".into(),
                                reference_character_style: "Default".into(),
                            }),
                            table: None,
                            anchored: None,
                        });
                        let id = doc.add_story(story);
                        let before = doc.clone();
                        let thread = compose_thread(
                            &doc,
                            id,
                            &[(
                                ObjectId::next(),
                                Rect::new(20.0, 30.0, 400.0, 800.0),
                                FrameOverflow::Thread,
                                1,
                                0.0,
                                InsetsLike::default(),
                            )],
                        );
                        assert!(
                            !thread.has_overflow(),
                            "{prefix}/{characters}/{number}/{whole}/{anchor}"
                        );
                        let initial = thread.lines().find(|line| line.initial.is_some()).unwrap();
                        assert_eq!(
                            (initial.start, initial.end),
                            (0, end),
                            "a generated reference must not consume a source initial character"
                        );
                        let spec = line_spec(initial, doc.story(id).unwrap(), &doc);
                        let marker = format!("[{number}]");
                        let expected = if anchor < end {
                            format!("{}{}{}", &text[..anchor], marker, &text[anchor..end])
                        } else {
                            text[..end].to_owned()
                        };
                        assert_eq!(spec.text, expected);
                        for byte in 0..=spec.text.len() {
                            assert!(text.is_char_boundary(initial.source_byte(byte)));
                        }
                        let note = thread
                            .frames
                            .iter()
                            .flat_map(|frame| &frame.footnotes)
                            .flat_map(|area| &area.lines)
                            .find(|line| line.initial.is_some())
                            .unwrap();
                        assert_eq!(
                            line_spec(note, doc.story(id).unwrap(), &doc).text,
                            marker + &text[..end]
                        );
                        assert_eq!(doc, before);
                    }
                }
            }
        }
    }
}
