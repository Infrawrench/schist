use schist_layout::{
    blank_a4,
    compose::{compose_thread, line_spec, InsetsLike},
    footnote_composition,
    footnotes::{FootnoteBody, FootnoteFirstBaseline, FootnoteMarker, FootnoteMarkerPosition},
    paragraph_keeps::ParagraphKeeps,
    styles::Leading,
    FrameOverflow, LayoutDocument, ObjectId, ParagraphStyle, Rect, Story, StoryId, StoryStructure,
};

fn document(whole: bool) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.footnotes.no_splitting = Some(false);
    doc.footnotes.first_baseline = Some(FootnoteFirstBaseline::Ascent);
    doc.footnotes.marker_position = Some(FootnoteMarkerPosition::Normal);
    doc.footnotes.spacer = Some(4.0);
    doc.footnotes.rule.on = Some(false);
    doc.footnotes.continuing_rule.on = Some(false);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Note".into(),
        point_size: Some(9.0),
        leading: Some(Leading::Points(12.0)),
        keeps: ParagraphKeeps {
            enabled: Some(whole),
            all: Some(whole),
            ..Default::default()
        },
        ..Default::default()
    });
    let text = (0..24)
        .map(|n| format!("café {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut story = Story::from_text("éA", "Body");
    story.structures.push(StoryStructure {
        control: None,
        at: Some(story.text_len()),
        kind: "Footnote".into(),
        payload: "retained source XML".into(),
        footnote: Some(FootnoteBody {
            story: Story::from_text(format!(" {text}"), "Note"),
            markers: vec![FootnoteMarker {
                at: 0,
                character_style: "Default".into(),
            }],
            reference_paragraph_style: "Body".into(),
            reference_character_style: "Default".into(),
        }),
        anchored: None,
    });
    doc.add_story(story);
    doc
}

fn frames(heights: &[f32]) -> Vec<(ObjectId, Rect, FrameOverflow, u16, f32, InsetsLike)> {
    heights
        .iter()
        .enumerate()
        .map(|(index, height)| {
            (
                ObjectId::next(),
                Rect::new(
                    index as f32 * 170.0,
                    0.0,
                    110.0 + (index % 3) as f32 * 10.0,
                    *height,
                ),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )
        })
        .collect()
}

#[test]
fn note_continuations_finish_after_main_story_eof_without_duplicating_or_losing_text() {
    let doc = document(false);
    let source = doc.clone();
    let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
    let expected = prepared.notes[0].body.story.text().replace('\n', "");
    for height in [55.0, 75.0, 100.0] {
        let input = frames(&[height; 30]);
        let actual = compose_thread(&doc, StoryId(0), &input);
        let areas: Vec<_> = actual.frames.iter().flat_map(|f| &f.footnotes).collect();
        assert!(areas.len() > 1, "long note was not continued");
        assert!(!actual.has_overflow());
        assert!(actual.frames[0].passed_on);
        assert_eq!(actual.frames[0].consumed_to, doc.stories[0].text_len());
        assert!(!actual.frames[1].footnotes.is_empty());
        let text: String = areas
            .iter()
            .flat_map(|a| &a.lines)
            .map(|l| line_spec(l, &doc.stories[0], &doc).text)
            .collect();
        assert_eq!(text, expected);
        for (frame, (_, bounds, ..)) in actual.frames.iter().zip(&input) {
            assert_eq!(frame.unrendered_structures, 0);
            for area in &frame.footnotes {
                assert_eq!(area.anchor, 3);
                assert!(
                    area.bounds.y >= bounds.y && area.bounds.bottom() <= bounds.bottom() + 0.001
                );
                for line in &area.lines {
                    assert_eq!((line.start, line.end), (3, 3));
                }
            }
        }
        let clipped = compose_thread(&doc, StoryId(0), &input[..1]);
        assert_eq!(clipped.frames[0].consumed_to, doc.stories[0].text_len());
        assert!(
            clipped.has_overflow(),
            "pending note text must remain terminal overset"
        );
        assert_eq!(doc, source);
    }
}

#[test]
fn whole_paragraph_keeps_prevent_note_splitting_and_carry_its_reference() {
    for no_splitting in [false, true] {
        let mut doc = document(true);
        doc.footnotes.no_splitting = Some(no_splitting);
        let source = doc.clone();
        let actual = compose_thread(&doc, StoryId(0), &frames(&[60.0, 140.0, 800.0]));
        assert!(!actual.has_overflow());
        for frame in &actual.frames[..2] {
            assert!(frame.lines.is_empty());
            assert!(frame.footnotes.is_empty());
            assert_eq!(frame.consumed_to, 0);
        }
        assert_eq!(actual.frames[2].footnotes.len(), 1);
        assert!(!actual.frames[2].lines.is_empty());
        assert_eq!(doc, source);
    }
}

#[test]
fn multiple_notes_preserve_each_body_across_columns_spanning_balancing_and_reading_direction() {
    use schist_layout::{authoring, History, LayoutObject, StoryDirection};
    for spanning in [false, true] {
        for reverse in [false, true] {
            for balanced in [false, true] {
                for count in [1, 2, 3] {
                    let mut doc = document(false);
                    doc.footnotes.straddle = Some(spanning);
                    doc.footnotes.space_between = Some(3.0);
                    doc.stories[0].prefs.direction = if reverse {
                        StoryDirection::RightToLeft
                    } else {
                        StoryDirection::LeftToRight
                    };
                    let original = doc.stories[0].structures[0].clone();
                    doc.stories[0].structures = [0, 2, 3]
                        .into_iter()
                        .enumerate()
                        .map(|(index, at)| {
                            let mut note = original.clone();
                            note.at = Some(at);
                            note.footnote.as_mut().unwrap().story = Story::from_text(
                                (0..8)
                                    .map(|line| format!("n{index} é{line}"))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                "Note",
                            );
                            note
                        })
                        .collect();
                    let mut input = frames(&[85.0; 24]);
                    for (id, bounds, _, columns, gutter, _) in &mut input {
                        bounds.width = 300.0;
                        *columns = count;
                        *gutter = 10.0;
                        let frame =
                            authoring::text_frame(&mut doc, &mut History::default(), 0, *bounds)
                                .unwrap();
                        *id = frame.object;
                        if let LayoutObject::TextFrame {
                            balance_columns, ..
                        } = &mut doc
                            .objects
                            .iter_mut()
                            .find(|object| object.id == *id)
                            .unwrap()
                            .object
                        {
                            *balance_columns = Some(balanced);
                        }
                    }
                    let source = doc.clone();
                    let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
                    let actual = compose_thread(&doc, StoryId(0), &input);
                    assert!(!actual.has_overflow(), "spanning={spanning}, reverse={reverse}, balanced={balanced}, count={count}");
                    for note in &prepared.notes {
                        let text: String = actual
                            .frames
                            .iter()
                            .flat_map(|frame| &frame.footnotes)
                            .filter(|area| area.structure == note.structure)
                            .flat_map(|area| &area.lines)
                            .map(|line| line_spec(line, &doc.stories[0], &doc).text)
                            .collect();
                        assert_eq!(text, note.body.story.text().replace('\n', ""));
                    }
                    for (frame, (_, bounds, ..)) in actual.frames.iter().zip(&input) {
                        for area in &frame.footnotes {
                            let width = if spanning {
                                bounds.width
                            } else {
                                (bounds.width - (count - 1) as f32 * 10.0) / count as f32
                            };
                            assert!((area.bounds.width - width).abs() < 0.001);
                            assert!(
                                area.bounds.y >= bounds.y
                                    && area.bounds.bottom() <= bounds.bottom() + 0.001
                            );
                            for line in &frame.lines {
                                if spanning
                                    || (line.bounds.x < area.bounds.right()
                                        && area.bounds.x < line.bounds.right())
                                {
                                    assert!(line.bounds.bottom() + 4.0 <= area.bounds.y + 0.001, "spanning={spanning}, reverse={reverse}, balanced={balanced}, count={count}, line={:?}, area={:?}", line.bounds, area.bounds);
                                }
                            }
                        }
                    }
                    assert_eq!(doc, source);
                }
            }
        }
    }
}

#[test]
fn continued_rules_and_end_of_story_placement_follow_note_completion_not_main_eof() {
    for below_text in [false, true] {
        let mut doc = document(false);
        doc.footnotes.end_of_story = Some(below_text);
        doc.footnotes.rule.on = Some(true);
        doc.footnotes.rule.width = Some(13.0);
        doc.footnotes.continuing_rule.on = Some(true);
        doc.footnotes.continuing_rule.width = Some(29.0);
        doc.footnotes.continuing_rule.weight = Some(2.0);
        doc.footnotes.continuing_rule.tint = Some(0.4);
        doc.footnotes.continuing_rule.overprint = Some(true);
        let input = frames(&[60.0, 60.0, 800.0]);
        let result = compose_thread(&doc, StoryId(0), &input);
        assert!(!result.has_overflow());
        for (index, frame) in result.frames.iter().enumerate() {
            let area = &frame.footnotes[0];
            let rule = area.rule.as_ref().unwrap();
            assert_eq!(rule.bounds.width, if index == 0 { 13.0 } else { 29.0 });
            if index > 0 {
                assert_eq!(rule.bounds.height, 2.0);
                assert_eq!(rule.tint, 0.4);
                assert!(rule.overprint);
            }
            if below_text && index == 2 {
                assert_eq!(area.bounds.y, 0.0);
            } else {
                assert!((area.bounds.bottom() - input[index].1.bottom()).abs() < 0.001);
            }
        }
        let mut clipped = input.clone();
        clipped[0].2 = FrameOverflow::Clip;
        let result = compose_thread(&doc, StoryId(0), &clipped);
        assert!(result.frames[0].lost);
        assert!(!result.frames[0].passed_on);
        assert!(result.frames[1..]
            .iter()
            .all(|frame| frame.lines.is_empty() && frame.footnotes.is_empty()));
    }
}

#[test]
fn pending_notes_do_not_consume_or_repeat_main_story_frame_breaks() {
    use schist_layout::StoryPoint;
    for breaks in [1, 2, 3] {
        let mut doc = document(false);
        let mut main = Story::from_text("A", "Body");
        main.points.extend(vec![StoryPoint::FrameBreak; breaks]);
        main.push_paragraph("B", "Body");
        main.structures = doc.stories[0].structures.clone();
        main.structures[0].at = Some(1);
        doc.stories[0] = main;
        let result = compose_thread(&doc, StoryId(0), &frames(&[90.0; 12]));
        assert!(!result.has_overflow());
        let main: Vec<String> = result
            .frames
            .iter()
            .map(|frame| {
                frame
                    .lines
                    .iter()
                    .map(|line| line_spec(line, &doc.stories[0], &doc).text)
                    .collect()
            })
            .collect();
        assert_eq!(main[0], "A1");
        assert!(main[1..breaks].iter().all(String::is_empty));
        let following = main.iter().position(|text| text == "B").unwrap();
        // A pending note can occupy the whole destination, delaying B further.
        // A forced break specifies the earliest destination, not a fit guarantee.
        assert!(following >= breaks);
        assert!(main
            .iter()
            .enumerate()
            .skip(1)
            .all(|(index, text)| index == following || text.is_empty()));
    }
}

#[test]
fn a_wrapped_generated_reference_stays_whole_with_the_start_of_its_note() {
    use schist_layout::footnotes::FootnoteAffixes;
    let mut doc = document(false);
    doc.footnotes.start_at = Some(1000);
    doc.footnotes.affixes = Some(FootnoteAffixes::Reference);
    doc.footnotes.prefix = Some("REF REF REF REF ".into());
    doc.footnotes.suffix = Some(" END".into());
    doc.footnotes.spacer = Some(0.0);
    let note = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|style| style.name == "Note")
        .unwrap();
    note.point_size = Some(3.0);
    note.leading = Some(Leading::Points(4.0));
    let source = doc.clone();
    for width in [20.0, 25.0, 35.0] {
        for height in [30.0, 32.0, 34.0, 45.0, 48.0, 60.0] {
            let mut input = frames(&[height, 800.0]);
            input[0].1.width = width;
            let result = compose_thread(&doc, StoryId(0), &input);
            assert!(!result.has_overflow());
            let first: String = result.frames[0]
                .lines
                .iter()
                .map(|line| line_spec(line, &doc.stories[0], &doc).text)
                .collect();
            let first = first.split_whitespace().collect::<String>();
            let reference = "REFREFREFREF1000END";
            assert!(
                first.is_empty()
                    || first == "é"
                    || first == "éA"
                    || first == format!("éA{reference}"),
                "partial reference at width={width}, height={height}: {first:?}"
            );
            if !result.frames[0].footnotes.is_empty() {
                assert!(first.contains(reference));
            }
            let all: String = result
                .frames
                .iter()
                .flat_map(|frame| &frame.lines)
                .map(|line| line_spec(line, &doc.stories[0], &doc).text)
                .collect();
            assert_eq!(
                all.split_whitespace().collect::<String>(),
                format!("éA{reference}")
            );
            assert_eq!(doc, source);
        }
    }
}

#[test]
fn omitted_no_splitting_uses_the_published_default_without_mutating_authored_preferences() {
    for spanning in [false, true] {
        for columns in [1, 2, 3] {
            let mut explicit = document(false);
            explicit.footnotes.straddle = Some(spanning);
            let mut omitted = explicit.clone();
            omitted.footnotes.no_splitting = None;
            let source = omitted.clone();
            let mut input = frames(&[65.0; 24]);
            for (_, bounds, _, count, gutter, _) in &mut input {
                bounds.width = 270.0;
                *count = columns;
                *gutter = 12.0;
            }
            let expected = compose_thread(&explicit, StoryId(0), &input);
            assert!(!expected.has_overflow());
            assert!(
                compose_thread(&omitted, StoryId(0), &input) == expected,
                "default differs for spanning={spanning}, columns={columns}"
            );
            assert_eq!(omitted, source);
        }
    }
}
