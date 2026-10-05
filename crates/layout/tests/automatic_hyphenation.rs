use schist_layout::{
    authoring,
    compose::{self, InsetsLike},
    hyphenation::HyphenationOptions,
    language::TextLanguage,
    paragraph_keeps::ParagraphKeeps,
    styles::Leading,
    FrameOverflow, History, Insets, LayoutDocument, LayoutObject, ObjectId, ParagraphDirection,
    ParagraphStyle, Rect, Story, StoryId, WritingMode,
};

fn document(text: &str) -> LayoutDocument {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "P".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(12.0),
        leading: Some(Leading::Points(16.0)),
        language: Some(TextLanguage::Tag {
            tag: "en-US".into(),
        }),
        hyphenation: HyphenationOptions {
            zone: Some(0.0),
            weight: Some(0),
            ladder_limit: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    doc.add_story(Story::from_text(text, "P"));
    doc
}

fn input(
    width: f32,
    height: f32,
    columns: u16,
) -> (ObjectId, Rect, FrameOverflow, u16, f32, InsetsLike) {
    (
        ObjectId::next(),
        Rect::new(0.0, 0.0, width, height),
        FrameOverflow::Thread,
        columns,
        4.0,
        InsetsLike::default(),
    )
}

fn signature(lines: &[compose::ComposedLine]) -> Vec<(usize, usize, bool, bool)> {
    lines
        .iter()
        .filter(|line| line.initial.is_none())
        .map(|line| {
            (
                line.start,
                line.end,
                line.generated_hyphen,
                line.discretionary_hyphen,
            )
        })
        .collect()
}

#[test]
fn equal_measure_threads_match_a_continuous_column_and_do_not_save_generated_text() {
    let text = "extensive probability extraordinary extensive probability extensive";
    let mut generated = 0;
    for limit in [0, 1, 2, 3] {
        for width in [52.0, 62.0, 72.0] {
            let mut doc = document(text);
            doc.styles
                .paragraphs
                .last_mut()
                .unwrap()
                .hyphenation
                .ladder_limit = Some(limit);
            let saved = serde_json::to_value(&doc).unwrap();
            let tall = compose::compose_thread(&doc, StoryId(0), &[input(width, 1000.0, 1)]);
            let frames = (0..30).map(|_| input(width, 18.0, 1)).collect::<Vec<_>>();
            let split = compose::compose_thread(&doc, StoryId(0), &frames);
            let actual = split
                .frames
                .iter()
                .flat_map(|frame| frame.lines.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                signature(&actual),
                signature(&tall.frames[0].lines),
                "{limit} {width}"
            );
            generated += actual.iter().filter(|line| line.generated_hyphen).count();
            assert_eq!(
                split.frames.last().unwrap().consumed_to,
                tall.frames[0].consumed_to
            );
            for line in &actual {
                let spec = compose::line_spec(line, &doc.stories[0], &doc);
                assert!(spec.hyphenation_breaks.is_empty());
                assert_eq!(spec.show_final_generated_hyphen, line.generated_hyphen);
                assert!(
                    (schist_text_engine::line_spans(&spec)[0].width - line.natural_width).abs()
                        < 0.001
                );
                assert!(text.is_char_boundary(line.start) && text.is_char_boundary(line.end));
            }
            assert_eq!(serde_json::to_value(&doc).unwrap(), saved);
        }
    }
    assert!(generated > 20);
}

#[test]
fn variable_width_continuations_carry_only_accepted_hyphenated_lines() {
    let text = "extensive probability extraordinary extensive probability extensive";
    for direction in [
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            let mut doc = document(text);
            let paragraph = doc.styles.paragraphs.last_mut().unwrap();
            paragraph.direction = Some(direction);
            paragraph.writing_mode = Some(axis);
            let mut frames = Vec::new();
            for width in [38.0, 52.0, 45.0, 70.0, 55.0, 60.0, 1000.0] {
                let mut frame = input(width, 18.0, 1);
                if axis != WritingMode::Horizontal {
                    frame.1 = Rect::new(0.0, 0.0, 18.0, width);
                }
                frames.push(frame);
            }
            let flow = compose::compose_thread(&doc, StoryId(0), &frames);
            assert!(!flow.has_overflow(), "{direction:?} {axis:?}");
            let mut preceding = false;
            let mut cursor = 0;
            for line in flow.frames.iter().flat_map(|frame| &frame.lines) {
                assert!(
                    !(preceding && line.generated_hyphen),
                    "consecutive limit lost across frames"
                );
                assert!(line.start >= cursor);
                assert!(text[cursor..line.start].trim().is_empty());
                cursor = line.end;
                preceding = line.generated_hyphen;
            }
            assert_eq!(cursor, text.len());
        }
    }
}

#[test]
fn prohibiting_column_end_hyphens_survives_balancing_keeps_and_axis_changes() {
    let text =
        "extensive probability extensive probability extensive probability\nextensive probability";
    let mut internal = 0;
    for height in [18.0, 31.0, 47.0, 63.0, 95.0] {
        for keep in [false, true] {
            for change_axis in [false, true] {
                let mut doc = document(text);
                let style = doc.styles.paragraphs.last_mut().unwrap();
                style.hyphenation.across_columns = Some(false);
                style.hyphenation.ladder_limit = Some(0);
                style.keeps = ParagraphKeeps {
                    next: Some(usize::from(keep)),
                    ..Default::default()
                };
                let mut vertical = style.clone();
                vertical.name = "V".into();
                vertical.writing_mode = Some(WritingMode::VerticalRightToLeft);
                doc.styles.add_paragraph(vertical);
                if change_axis {
                    if let schist_layout::StoryPoint::Paragraph { style, .. } =
                        doc.stories[0].points.last_mut().unwrap()
                    {
                        *style = "V".into();
                    }
                }
                let mut frames = (0..4).map(|_| input(128.0, height, 2)).collect::<Vec<_>>();
                frames.push(input(500.0, 500.0, 1));
                let before = doc.clone();
                let flow = compose::compose_thread(&doc, StoryId(0), &frames);
                assert!(!flow.has_overflow(), "{height} {keep} {change_axis}");
                for frame in &flow.frames {
                    assert!(frame.lines.last().is_none_or(|line| !line.generated_hyphen));
                    for pair in frame.lines.windows(2) {
                        if pair[0].paragraph.writing_mode == pair[1].paragraph.writing_mode
                            && match pair[0].paragraph.writing_mode {
                                Some(
                                    WritingMode::VerticalLeftToRight
                                    | WritingMode::VerticalRightToLeft,
                                ) => pair[0].bounds.y != pair[1].bounds.y,
                                _ => pair[0].bounds.x != pair[1].bounds.x,
                            }
                        {
                            assert!(!pair[0].generated_hyphen);
                        }
                    }
                    internal += frame
                        .lines
                        .iter()
                        .filter(|line| line.generated_hyphen)
                        .count();
                }
                assert_eq!(doc, before);
            }
        }
    }
    assert!(internal > 0, "rule must allow internal line breaks");
}

#[test]
fn path_frames_apply_column_policy_but_manual_discretionary_breaks_remain_authoritative() {
    use schist_layout::{Point, ShapePath, SubPath};
    for automatic in [false, true] {
        for across in [false, true] {
            let mut doc = document(if automatic {
                "extensive"
            } else {
                "ex\u{ad}tensive"
            });
            doc.styles
                .paragraphs
                .last_mut()
                .unwrap()
                .hyphenation
                .across_columns = Some(across);
            let shape = ShapePath {
                subpaths: vec![SubPath {
                    points: vec![Point::ZERO, Point::new(20.0, 0.0)],
                    closed: false,
                    handles: Vec::new(),
                }],
                even_odd: false,
            };
            let id = authoring::path_shape(
                &mut doc,
                &mut History::default(),
                0,
                shape,
                authoring::Paint::none(),
            )
            .unwrap();
            schist_layout::text_path::attach(&mut doc, &mut History::default(), id).unwrap();
            let tail = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(100.0, 100.0, 200.0, 200.0),
            )
            .unwrap();
            for placed in &mut doc.objects {
                if let LayoutObject::TextFrame {
                    story,
                    overflow,
                    insets,
                    ..
                } = &mut placed.object
                {
                    *story = StoryId(0);
                    *overflow = FrameOverflow::Thread;
                    *insets = Insets::ZERO;
                }
            }
            doc.thread_order = vec![(StoryId(0), vec![id, tail.object])];
            let before = doc.clone();
            let flow = compose::compose_story(&doc, StoryId(0));
            assert!(!flow.has_overflow());
            assert_eq!(flow.frames[0].lines.is_empty(), automatic && !across);
            if let Some(line) = flow.frames[0].lines.first() {
                assert_eq!(line.generated_hyphen, automatic);
                assert_eq!(line.discretionary_hyphen, !automatic);
            }
            assert_eq!(doc, before);
        }
    }
}

#[test]
fn split_note_trials_preserve_each_notes_hyphen_history_and_column_policy() {
    use schist_layout::{
        footnotes::{FootnoteBody, FootnoteFirstBaseline, FootnoteMarker, FootnoteMarkerPosition},
        StoryStructure,
    };
    let mut generated = 0;
    for across in [false, true] {
        for span in [false, true] {
            for height in [65.0, 85.0, 110.0] {
                let mut doc = document("extensive probability extensive probability");
                doc.footnotes.no_splitting = Some(false);
                doc.footnotes.straddle = Some(span);
                doc.footnotes.first_baseline = Some(FootnoteFirstBaseline::Ascent);
                doc.footnotes.marker_position = Some(FootnoteMarkerPosition::Normal);
                doc.footnotes.rule.on = Some(false);
                doc.footnotes.continuing_rule.on = Some(false);
                doc.styles
                    .paragraphs
                    .last_mut()
                    .unwrap()
                    .hyphenation
                    .across_columns = Some(across);
                for anchor in [9, 21] {
                    doc.stories[0].structures.push(StoryStructure {
control: None,
                        at: Some(anchor), kind: "Footnote".into(), payload: String::new(),
                        footnote: Some(FootnoteBody {
                            story: Story::from_text(" extensive probability extensive probability extensive probability", "P"),
                            markers: vec![FootnoteMarker { at: 0, character_style: String::new() }],
                            reference_paragraph_style: "P".into(), reference_character_style: String::new(),
                        }),
                        anchored: None,
                    });
                }
                let saved = doc.clone();
                let prepared =
                    schist_layout::footnote_composition::prepare(&doc, StoryId(0)).unwrap();
                let mut frames = (0..12).map(|_| input(132.0, height, 2)).collect::<Vec<_>>();
                frames.push(input(300.0, 500.0, 1));
                let flow = compose::compose_thread(&doc, StoryId(0), &frames);
                assert!(!flow.has_overflow(), "{across} {span} {height}");
                for (index, note) in prepared.notes.iter().enumerate() {
                    let mut text = String::new();
                    let mut previous = false;
                    for area in flow
                        .frames
                        .iter()
                        .flat_map(|frame| &frame.footnotes)
                        .filter(|area| area.structure == index)
                    {
                        if !across {
                            assert!(area.lines.last().is_none_or(|line| !line.generated_hyphen));
                        }
                        for line in &area.lines {
                            assert!(
                                !(previous && line.generated_hyphen),
                                "note history lost across a trial or container"
                            );
                            previous = line.generated_hyphen;
                            generated += usize::from(previous);
                            let rendered = line.projected.as_ref().unwrap();
                            assert_eq!(
                                rendered.spec.show_final_generated_hyphen,
                                line.generated_hyphen
                            );
                            assert!(rendered.spec.hyphenation_breaks.is_empty());
                            text.push_str(&rendered.spec.text);
                        }
                    }
                    assert_eq!(
                        text.replace(' ', ""),
                        note.body.story.text().replace(' ', ""),
                        "{across} {span} {height}"
                    );
                }
                assert_eq!(doc, saved);
            }
        }
    }
    assert!(generated > 0);
}

#[test]
fn an_inline_reference_does_not_disable_its_original_words_dictionary_breaks() {
    use schist_layout::{
        footnotes::{FootnoteBody, FootnoteFirstBaseline, FootnoteMarkerPosition},
        StoryStructure,
    };
    for whole in [false, true] {
        let mut doc = document("extensive probability");
        doc.footnotes.no_splitting = Some(whole);
        doc.footnotes.first_baseline = Some(FootnoteFirstBaseline::Ascent);
        doc.footnotes.marker_position = Some(FootnoteMarkerPosition::Normal);
        doc.footnotes.rule.on = Some(false);
        doc.footnotes.continuing_rule.on = Some(false);
        doc.stories[0].structures.push(StoryStructure {
            control: None,
            at: Some(9),
            kind: "Footnote".into(),
            payload: String::new(),
            footnote: Some(FootnoteBody {
                story: Story::from_text("note", "P"),
                markers: Vec::new(),
                reference_paragraph_style: "P".into(),
                reference_character_style: String::new(),
            }),
            anchored: None,
        });
        let flow = compose::compose_thread(&doc, StoryId(0), &[input(40.0, 300.0, 1)]);
        let first = &flow.frames[0].lines[0];
        assert!(first.generated_hyphen);
        assert_eq!(first.end, 5);
        assert_eq!(first.projected.as_ref().unwrap().spec.text, "exten");
        assert!(!flow.has_overflow());
    }
}

#[test]
fn an_unbreakable_remaining_suffix_stays_overset_in_a_narrower_continuation() {
    let doc = document("extensive");
    for width in [1.0, 5.0, 10.0, 15.0] {
        let frames = [
            input(40.0, 18.0, 1),
            input(width, 18.0, 1),
            input(100.0, 18.0, 1),
        ];
        let flow = compose::compose_thread(&doc, StoryId(0), &frames);
        assert_eq!(flow.frames[0].lines[0].end, 5);
        assert!(flow.frames[0].lines[0].generated_hyphen);
        assert!(
            flow.frames[1].lines.is_empty(),
            "suffix crossed a {width} pt frame"
        );
        assert_eq!(flow.frames[1].consumed_to, 5);
        assert!(flow.frames[1].passed_on);
        assert_eq!(flow.frames[2].lines[0].start, 5);
        assert!(!flow.has_overflow());
    }
}
