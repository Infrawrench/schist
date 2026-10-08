use schist_layout::{
    authoring, blank_a4, compose, text_path, FrameOverflow, History, LayoutObject, Point, Rect,
    ShapePath, Story,
};

fn path(doc: &mut schist_layout::LayoutDocument, width: f32) -> authoring::TextFrame {
    let shape = ShapePath {
        subpaths: vec![schist_layout::SubPath {
            points: vec![Point::ZERO, Point::new(width, 0.0)],
            handles: Vec::new(),
            closed: false,
        }],
        even_odd: false,
    };
    let mut history = History::default();
    let id = authoring::path_shape(doc, &mut history, 0, shape, authoring::Paint::none()).unwrap();
    text_path::attach(doc, &mut history, id).unwrap()
}

#[test]
fn path_tab_rulers_keep_their_bracket_origin_across_indents_and_directions() {
    use schist_layout::{
        lists::{ListStyle, ListTab},
        styles::{Align, ParagraphDirection},
    };
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for direction in [
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ] {
        for alignment in ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"] {
            for indent in [0.0, 9.0, 15.0] {
                for first in [-6.0, 0.0, 4.0] {
                    let mut doc = blank_a4();
                    let style = &mut doc.styles.paragraphs[0];
                    style.family = Some("IBM Plex Sans".into());
                    style.point_size = Some(12.0);
                    style.direction = Some(direction);
                    style.align = Some(if direction == ParagraphDirection::RightToLeft {
                        Align::Right
                    } else {
                        Align::Left
                    });
                    style.left_indent = Some(indent);
                    style.right_indent = Some(7.0);
                    style.first_line_indent = Some(first);
                    style.list = ListStyle {
                        tabs: Some(
                            [70.0, 110.0]
                                .map(|position| ListTab {
                                    position,
                                    alignment: alignment.into(),
                                    alignment_character: ".".into(),
                                    leader: String::new(),
                                })
                                .into(),
                        ),
                        ..Default::default()
                    };
                    let frame = path(&mut doc, 240.0);
                    doc.stories[frame.story.0 as usize] =
                        Story::from_text("A\t12.34\t56.7", "Default");
                    for bracket in [
                        text_path::Bracket::Start(20.0),
                        text_path::Bracket::End(Some(180.0)),
                    ] {
                        assert!(text_path::set_bracket(
                            &mut doc,
                            &mut History::default(),
                            &[frame.object],
                            bracket
                        ));
                    }
                    let flow = compose::compose_story(&doc, frame.story);
                    assert!(
                        !flow.has_overflow(),
                        "{direction:?}/{alignment}/{indent}/{first}"
                    );
                    assert_eq!(flow.lines().count(), 1);
                    let line = flow.lines().next().unwrap();
                    let spec = compose::line_spec(line, doc.story(frame.story).unwrap(), &doc);
                    assert!(schist_layout::tabs::unsupported_in_mode(
                        &line.paragraph,
                        &spec.text,
                        true,
                        spec.writing_mode
                    )
                    .is_empty());
                    let mut straight = spec.clone();
                    straight.path = None;
                    assert!(
                        (schist_text_engine::measure(&straight).unwrap().width
                            - line.natural_width)
                            .abs()
                            < 0.001
                    );
                    for (start, end, stop) in [(2, 7, 70.0), (8, 12, 110.0)] {
                        let mut field = straight.clone();
                        field.text = spec.text[start..end].into();
                        field.tabs = None;
                        let width = schist_text_engine::measure(&field).unwrap().width;
                        let left = schist_text_engine::caret_at(&spec, start).unwrap().x
                            - schist_text_engine::caret_at(&field, 0).unwrap().x;
                        let actual = match alignment {
                            "LeftAlign" => left,
                            "RightAlign" => left + width,
                            "CenterAlign" => left + width / 2.0,
                            _ => {
                                schist_text_engine::caret_at(
                                    &spec,
                                    start + field.text.find('.').unwrap(),
                                )
                                .unwrap()
                                .x
                            }
                        };
                        let expected = if direction == ParagraphDirection::RightToLeft {
                            180.0 - stop
                        } else {
                            20.0 + stop
                        };
                        assert!(
                            (actual - expected).abs() < 0.002,
                            "{direction:?}/{alignment}/{indent}/{first}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn conversion_brackets_and_curve_edits_are_exactly_reversible_without_losing_frame_paint() {
    for closed in [false, true] {
        let mut doc = blank_a4();
        let mut shape = ShapePath::ellipse(160.0, 80.0);
        shape.subpaths[0].closed = closed;
        shape.map_points(|p| p + Point::new(30.0, 40.0));
        let id = authoring::path_shape(
            &mut doc,
            &mut History::default(),
            0,
            shape,
            authoring::Paint {
                fill: Some("Black".into()),
                stroke: Some("Black".into()),
                stroke_width: 3.0,
            },
        )
        .unwrap();
        let before = doc.clone();
        let mut history = History::default();
        let made = text_path::attach(&mut doc, &mut history, id).unwrap();
        assert_eq!(history.undo_depth(), 1);
        assert_eq!(doc.objects.len(), before.objects.len());
        assert_eq!(doc.objects[0].bounds, before.objects[0].bounds);
        assert_eq!(
            doc.objects[0].object.editable_path(),
            before.objects[0].object.editable_path()
        );
        assert_eq!(
            doc.styles.object_paint(&doc.objects[0]),
            before.styles.object_paint(&before.objects[0])
        );
        assert!(doc.story(made.story).is_some());
        let attached = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, attached);
        let copy = authoring::duplicate(&mut doc, &mut history, id).unwrap();
        assert_eq!(
            doc.object(copy).unwrap().appearance,
            attached.object(id).unwrap().appearance
        );
        assert_eq!(
            doc.object(copy).unwrap().object.editable_path(),
            attached.object(id).unwrap().object.editable_path()
        );
        let LayoutObject::TextFrame {
            story: copied_story,
            ..
        } = doc.object(copy).unwrap().object
        else {
            unreachable!()
        };
        assert_ne!(copied_story, made.story);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, attached);
        for bracket in [
            text_path::Bracket::Start(5.0),
            text_path::Bracket::End(Some(70.0)),
            text_path::Bracket::End(None),
        ] {
            let previous = doc.clone();
            let depth = history.undo_depth();
            assert!(text_path::set_bracket(
                &mut doc,
                &mut history,
                &[id, id],
                bracket
            ));
            assert_eq!(history.undo_depth(), depth + 1);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, previous);
            assert!(history.redo(&mut doc));
        }
        let previous = doc.clone();
        for v in [f32::NAN, f32::INFINITY, -1.0, 1e10] {
            assert!(!text_path::set_bracket(
                &mut doc,
                &mut history,
                &[id],
                text_path::Bracket::Start(v)
            ));
            assert_eq!(doc, previous);
        }
        assert!(authoring::set_point(
            &mut doc,
            id,
            authoring::PointRef {
                subpath: 0,
                index: 0,
                part: authoring::PointPart::Outgoing,
            },
            Point::new(40.0, -20.0)
        ));
        assert_ne!(
            doc.objects[0].object.editable_path(),
            previous.objects[0].object.editable_path()
        );
        assert_eq!(doc.story(made.story), previous.story(made.story));
    }
}

#[test]
fn bounded_paths_thread_to_paths_and_boxes_without_restarting_or_splitting_utf8() {
    for width in [1.0, 25.0, 60.0, 130.0] {
        let mut doc = blank_a4();
        let first = path(&mut doc, width);
        doc.stories[first.story.0 as usize] = Story::from_text(
            "éclair office a\u{301}bc words across containers and paragraphs\nSecond paragraph",
            "Default",
        );
        let second = path(&mut doc, width);
        let third = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 80.0, 400.0, 400.0),
        )
        .unwrap();
        for id in [first.object, second.object, third.object] {
            let frame = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
            let LayoutObject::TextFrame {
                story, overflow, ..
            } = &mut frame.object
            else {
                unreachable!()
            };
            *story = first.story;
            *overflow = FrameOverflow::Thread;
        }
        doc.thread_order = vec![(first.story, vec![first.object, second.object, third.object])];
        let flow = compose::compose_story(&doc, first.story);
        assert!(!flow.has_overflow(), "{width}");
        assert_eq!(flow.frames.len(), 3);
        let text = doc.stories[first.story.0 as usize].text();
        let mut consumed = 0;
        for (i, frame) in flow.frames.iter().enumerate() {
            assert!(frame.consumed_to >= consumed);
            for line in &frame.lines {
                assert!(line.start >= consumed);
                assert!(text.is_char_boundary(line.start) && text.is_char_boundary(line.end));
                if i < 2 {
                    assert!(line.natural_width <= width + 0.0001);
                    assert!(line.text_path.is_some());
                    let spec = compose::line_spec(line, &doc.stories[first.story.0 as usize], &doc);
                    assert_eq!(spec.text, &text[line.start..line.end]);
                    assert_eq!(spec.align, schist_text_engine::Align::Left);
                } else {
                    assert!(line.text_path.is_none());
                }
            }
            if i < 2 {
                assert!(frame.lines.len() <= 1);
            }
            consumed = frame.consumed_to;
        }
        assert_eq!(consumed, text.len());
        if width == 1.0 {
            assert!(flow.frames[0].lines.is_empty());
        }
    }
}

#[test]
fn path_alignment_is_applied_once_and_legacy_boxes_remain_boxes() {
    for align in [
        schist_layout::styles::Align::Left,
        schist_layout::styles::Align::Center,
        schist_layout::styles::Align::Right,
    ] {
        let mut doc = blank_a4();
        let frame = path(&mut doc, 240.0);
        doc.styles.paragraphs[0].align = Some(align);
        doc.stories[frame.story.0 as usize] = Story::from_text("office é", "Default");
        let mut history = History::default();
        assert!(text_path::set_bracket(
            &mut doc,
            &mut history,
            &[frame.object],
            text_path::Bracket::Start(20.0)
        ));
        assert!(text_path::set_bracket(
            &mut doc,
            &mut history,
            &[frame.object],
            text_path::Bracket::End(Some(200.0))
        ));
        let flow = compose::compose_story(&doc, frame.story);
        let line = &flow.frames[0].lines[0];
        let spec = compose::line_spec(line, &doc.stories[frame.story.0 as usize], &doc);
        let mut plain = spec.clone();
        plain.path = None;
        let width = schist_text_engine::measure(&plain).unwrap().width;
        let expected = 20.0
            + match align {
                schist_layout::styles::Align::Left => 0.0,
                schist_layout::styles::Align::Center => (180.0 - width) / 2.0,
                _ => 180.0 - width,
            };
        let actual = schist_text_engine::carets(&spec);
        let reference = schist_text_engine::carets(&plain);
        for ((a, p), (b, q)) in actual.iter().zip(&reference) {
            assert_eq!(a, b);
            assert!((p.x - q.x - expected).abs() < 0.001);
            assert!(
                (p.top - q.top + schist_text_engine::measure(&plain).unwrap().first_baseline).abs()
                    < 0.001
            );
        }
    }
    let mut json = serde_json::to_value(LayoutObject::TextFrame {
        balance_columns: Some(false),
        footnotes: Default::default(),
        story: schist_layout::StoryId(0),
        text_path: None,
        columns: 1,
        gutter: 0.0,
        insets: schist_layout::Insets::ZERO,
        overflow: FrameOverflow::Clip,
    })
    .unwrap();
    json["TextFrame"]
        .as_object_mut()
        .unwrap()
        .remove("text_path");
    assert!(matches!(
        serde_json::from_value::<LayoutObject>(json).unwrap(),
        LayoutObject::TextFrame {
            text_path: None,
            ..
        }
    ));
}

#[test]
fn consecutive_zero_width_breaks_advance_path_destinations_without_eating_source_text() {
    use schist_layout::story::Point as StoryPoint;
    for kind in [
        StoryPoint::FrameBreak,
        StoryPoint::ColumnBreak,
        StoryPoint::PageBreak,
    ] {
        for count in 1..=3 {
            for leading in [true, false] {
                let mut doc = blank_a4();
                doc.pages = vec![doc.pages[0].clone(); 4];
                let frames: Vec<_> = (0..8).map(|_| path(&mut doc, 240.0)).collect();
                let story = frames[0].story;
                for (index, frame) in frames.iter().enumerate() {
                    let object = doc
                        .objects
                        .iter_mut()
                        .find(|o| o.id == frame.object)
                        .unwrap();
                    object.page = index / 2;
                    let LayoutObject::TextFrame {
                        story: id,
                        overflow,
                        ..
                    } = &mut object.object
                    else {
                        unreachable!()
                    };
                    *id = story;
                    *overflow = FrameOverflow::Thread;
                }
                doc.thread_order = vec![(story, frames.iter().map(|f| f.object).collect())];
                let paragraph = |text: &str| StoryPoint::Paragraph {
                    text: text.into(),
                    style: "Default".into(),
                };
                let mut points = Vec::new();
                if !leading {
                    points.push(paragraph("é"));
                }
                points.extend(std::iter::repeat_n(kind.clone(), count));
                points.push(paragraph("a\u{301}Z"));
                points.push(paragraph(""));
                doc.stories[story.0 as usize] = Story {
                    points,
                    ..Default::default()
                };
                let flow = compose::compose_story(&doc, story);
                assert!(
                    !flow.has_overflow(),
                    "{kind:?}, count={count}, leading={leading}"
                );
                let destination = count * if kind == StoryPoint::PageBreak { 2 } else { 1 };
                for (index, frame) in flow.frames.iter().enumerate() {
                    let text: String = frame
                        .lines
                        .iter()
                        .map(|line| doc.stories[story.0 as usize].slice(line.start, line.end))
                        .collect();
                    let expected = if index == 0 && !leading {
                        "é"
                    } else if index == destination {
                        "a\u{301}Z"
                    } else {
                        ""
                    };
                    assert_eq!(
                        text, expected,
                        "{kind:?}, count={count}, leading={leading}, frame={index}"
                    );
                }
                // The final blank paragraph still owns exactly one baseline.
                assert_eq!(flow.frames[destination + 1].lines.len(), 1);
                assert_eq!(
                    flow.frames[destination + 1].consumed_to,
                    doc.stories[story.0 as usize].text_len()
                );
            }
        }
    }
}
