use schist_layout::{
    blank_a4,
    compose::{compose_thread, spec_for, InsetsLike},
    FrameOverflow, ObjectId, ParagraphStyle, Rect, Story, StoryOrientation, WritingMode,
};

#[test]
fn mixed_writing_modes_reserve_separate_regions_and_never_lose_source_text() {
    for repeats in [1, 4, 12] {
        for count in [1, 2] {
            let mut doc = blank_a4();
            for (name, mode) in [
                ("H", WritingMode::Horizontal),
                ("RL", WritingMode::VerticalRightToLeft),
                ("LR", WritingMode::VerticalLeftToRight),
            ] {
                doc.styles.add_paragraph(ParagraphStyle {
                    name: name.into(),
                    writing_mode: Some(mode),
                    point_size: Some(11.0),
                    leading: Some(schist_layout::styles::Leading::Points(15.0)),
                    keep_lines: Some(1),
                    ..Default::default()
                });
            }
            let mut story = Story::new();
            for style in ["H", "RL", "LR", "H", "RL", "H"] {
                story.push_paragraph("abc def ghi ".repeat(repeats), style);
                story.push_paragraph("", style);
            }
            let id = doc.add_story(story);
            let size = Rect::new(20.0, 30.0, 170.0, 210.0);
            let frames: Vec<_> = (0..30)
                .map(|_| {
                    (
                        ObjectId::next(),
                        size,
                        FrameOverflow::Thread,
                        count,
                        8.0,
                        InsetsLike::default(),
                    )
                })
                .collect();
            let composed = compose_thread(&doc, id, &frames);
            assert!(!composed.has_overflow());
            let story = doc.story(id).unwrap();
            let mut covered = vec![false; story.text_len()];
            for frame in &composed.frames {
                for line in &frame.lines {
                    assert!((line.advance - 15.0).abs() < 0.001);
                    let cross = if line.paragraph_style == "H" {
                        line.bounds.height
                    } else {
                        line.bounds.width
                    };
                    assert!(cross > 0.0 && cross < 15.0, "wrong axes: {line:?}");
                    assert!(
                        line.bounds.x >= size.x - 0.001
                            && line.bounds.right() <= size.right() + 0.001
                    );
                    assert!(
                        line.bounds.y >= size.y - 0.001
                            && line.bounds.bottom() <= size.bottom() + 0.001
                    );
                    for byte in &mut covered[line.start..line.end] {
                        assert!(!*byte);
                        *byte = true;
                    }
                }
                for (index, a) in frame.lines.iter().enumerate() {
                    for b in &frame.lines[index + 1..] {
                        let width =
                            a.bounds.right().min(b.bounds.right()) - a.bounds.x.max(b.bounds.x);
                        let height =
                            a.bounds.bottom().min(b.bounds.bottom()) - a.bounds.y.max(b.bounds.y);
                        assert!(
                            width < 0.001 || height < 0.001,
                            "overlapping lines: {a:?}, {b:?}"
                        );
                    }
                }
            }
            for (byte, covered) in story.text().bytes().zip(covered) {
                assert!(covered || byte.is_ascii_whitespace());
            }
        }
    }
}

#[test]
fn keep_with_next_crosses_writing_mode_changes_and_exhausted_regions_thread() {
    let modes = [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ];
    for source in modes {
        for destination in modes.into_iter().filter(|m| *m != source) {
            let mut doc = blank_a4();
            for (name, mode, leading, keep) in [
                ("Fill", source, 15.0, false),
                ("Heading", source, 15.0, true),
                ("Body", destination, 150.0, false),
            ] {
                doc.styles.add_paragraph(ParagraphStyle {
                    name: name.into(),
                    writing_mode: Some(mode),
                    // A large cell forces the keep chain into the next frame.
                    // Leading alone never reserves space before its first line.
                    point_size: Some(if name == "Body" { 130.0 } else { 11.0 }),
                    leading: Some(schist_layout::styles::Leading::Points(leading)),
                    keep_with_next: Some(keep),
                    keep_lines: Some(1),
                    ..Default::default()
                });
            }
            let mut story = Story::from_text("f", "Fill");
            story.push_paragraph("h", "Heading");
            story.push_paragraph("b", "Body");
            let id = doc.add_story(story);
            let frames: Vec<_> = [100.0, 300.0]
                .into_iter()
                .map(|size| {
                    (
                        ObjectId::next(),
                        Rect::new(20.0, 30.0, size, size),
                        FrameOverflow::Thread,
                        1,
                        0.0,
                        InsetsLike::default(),
                    )
                })
                .collect();
            let thread = compose_thread(&doc, id, &frames);
            assert!(!thread.has_overflow());
            assert_eq!(thread.frames[0].lines.len(), 1);
            assert_eq!(thread.frames[0].lines[0].paragraph_style, "Fill");
            assert_eq!(
                thread.frames[1]
                    .lines
                    .iter()
                    .map(|l| l.paragraph_style.as_str())
                    .collect::<Vec<_>>(),
                ["Heading", "Body"]
            );
        }
    }
    let mut doc = blank_a4();
    for (name, mode) in [
        ("H", WritingMode::Horizontal),
        ("V", WritingMode::VerticalRightToLeft),
    ] {
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            writing_mode: Some(mode),
            point_size: Some(11.0),
            leading: Some(schist_layout::styles::Leading::Points(15.0)),
            keep_lines: Some(1),
            ..Default::default()
        });
    }
    let mut story = Story::from_text("a\nb", "H");
    story.push_paragraph("c", "V");
    let id = doc.add_story(story);
    let story = doc.story(id).unwrap();
    let spec = spec_for(
        story,
        0,
        3,
        &doc.styles,
        "H",
        &doc.default_character_style,
        170.0,
    );
    let occupied = schist_text_engine::measure(&spec).unwrap().height;
    let frames: Vec<_> = [(30.0 + occupied) - 30.0, 200.0]
        .into_iter()
        .map(|height| {
            (
                ObjectId::next(),
                Rect::new(20.0, 30.0, 170.0, height),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )
        })
        .collect();
    let thread = compose_thread(&doc, id, &frames);
    assert!(!thread.has_overflow());
    assert_eq!(thread.frames[0].lines.len(), 2);
    assert_eq!(thread.frames[1].lines[0].paragraph_style, "V");
}

#[test]
fn vertical_lines_wrap_to_frame_height_and_progress_across_frame_width() {
    for mode in [
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for size in [
            Rect::new(17.0, 29.0, 110.0, 220.0),
            Rect::new(17.0, 29.0, 220.0, 110.0),
        ] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Vertical".into(),
                point_size: Some(11.0),
                leading: Some(schist_layout::styles::Leading::Points(15.0)),
                writing_mode: Some(mode),
                keep_lines: Some(1),
                ..Default::default()
            });
            let mut story =
                Story::from_text("日本語の縦書きと Latin 123 の文章。".repeat(12), "Vertical");
            story.prefs.orientation = StoryOrientation::Vertical;
            let expected = schist_text_engine::line_spans(&spec_for(
                &story,
                0,
                story.text_len(),
                &doc.styles,
                "Vertical",
                &doc.default_character_style,
                size.height,
            ));
            let id = doc.add_story(story);
            let frames: Vec<_> = (0..20)
                .map(|_| {
                    (
                        ObjectId::next(),
                        size,
                        FrameOverflow::Thread,
                        1,
                        0.0,
                        InsetsLike::default(),
                    )
                })
                .collect();
            let thread = compose_thread(&doc, id, &frames);
            assert!(!thread.has_overflow());
            assert_eq!(
                thread.lines().map(|l| (l.start, l.end)).collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|l| (l.start, l.end))
                    .collect::<Vec<_>>()
            );
            for frame in &thread.frames {
                for line in &frame.lines {
                    assert!((line.advance - 15.0).abs() < 0.001);
                    assert!(line.bounds.x >= size.x && line.bounds.right() <= size.right() + 0.001);
                    assert!((line.bounds.y - size.y).abs() < 0.001);
                    assert!(line.natural_width <= size.height + 0.001);
                }
                for pair in frame.lines.windows(2) {
                    if mode == WritingMode::VerticalRightToLeft {
                        assert!(pair[0].bounds.x > pair[1].bounds.x);
                    } else {
                        assert!(pair[0].bounds.x < pair[1].bounds.x);
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_bands_balance_and_thread_blank_paragraphs_inside_asymmetric_insets() {
    for count in 1..4 {
        for balanced in [false, true] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Vertical".into(),
                point_size: Some(11.0),
                leading: Some(schist_layout::styles::Leading::Points(15.0)),
                keep_lines: Some(1),
                space_before: Some(2.0),
                space_after: Some(3.0),
                ..Default::default()
            });
            let mut story = Story::new();
            story.prefs.orientation = StoryOrientation::Vertical;
            for _ in 0..if balanced { count } else { count * 4 } {
                story.push_paragraph(
                    "日本語 with words. ".repeat(if balanced { 1 } else { 10 }),
                    "Vertical",
                );
                story.push_paragraph("", "Vertical");
            }
            let id = doc.add_story(story);
            let bounds = Rect::new(21.0, 33.0, 250.0, 300.0);
            let insets = InsetsLike {
                top: 7.0,
                right: 11.0,
                bottom: 13.0,
                left: 17.0,
            };
            let content = bounds.inset(insets.resolve());
            let frames: Vec<_> = (0..30)
                .map(|_| {
                    (
                        ObjectId::next(),
                        bounds,
                        FrameOverflow::Thread,
                        count,
                        9.0,
                        insets,
                    )
                })
                .collect();
            let rl = compose_thread(&doc, id, &frames);
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Vertical")
                .unwrap()
                .writing_mode = Some(WritingMode::VerticalLeftToRight);
            let lr = compose_thread(&doc, id, &frames);
            assert!(!rl.has_overflow() && !lr.has_overflow());
            assert!(rl.lines().any(|l| l.forced_break));
            assert_eq!(rl.lines().count(), lr.lines().count());
            for (a, b) in rl.lines().zip(lr.lines()) {
                assert_eq!(
                    (a.start, a.end, a.bounds.y, a.bounds.height),
                    (b.start, b.end, b.bounds.y, b.bounds.height)
                );
                assert!(
                    (a.bounds.x + b.bounds.right() - content.x - content.right()).abs() < 0.001
                );
                assert!(
                    a.bounds.x >= content.x - 0.001 && a.bounds.right() <= content.right() + 0.001
                );
                assert!(
                    a.bounds.y >= content.y - 0.001
                        && a.bounds.bottom() <= content.bottom() + 0.001
                );
                assert!((a.advance - 15.0).abs() < 0.001);
            }
            let paragraphs = doc.story(id).unwrap().points.iter().filter(|p| matches!(p, schist_layout::story::Point::Paragraph { text, .. } if text.is_empty())).count();
            assert_eq!(rl.lines().filter(|l| l.forced_break).count(), paragraphs);
            if balanced {
                assert!(!rl.frames[0].passed_on);
            }
        }
    }
}
