use schist_layout::{
    blank_a4,
    compose::{compose_thread, InsetsLike},
    FrameOverflow, ObjectId, ParagraphStyle, Rect, Story,
};

#[test]
fn every_line_wraps_to_its_actual_indented_measure() {
    for text in [
        "a few short words to wrap well ".repeat(30),
        "é au thé et café 世界 ".repeat(30),
    ] {
        for (left, right, first) in [(12.0, 18.0, 0.0), (20.0, 10.0, 30.0), (35.0, 15.0, -20.0)] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Indented".into(),
                point_size: Some(11.0),
                leading: Some(schist_layout::styles::Leading::Points(14.0)),
                left_indent: Some(left),
                right_indent: Some(right),
                first_line_indent: Some(first),
                keep_lines: Some(1),
                ..Default::default()
            });
            let story = doc.add_story(Story::from_text(&text, "Indented"));
            let thread = compose_thread(
                &doc,
                story,
                &[(
                    ObjectId::next(),
                    Rect::new(10.0, 20.0, 120.0, 10000.0),
                    FrameOverflow::Clip,
                    1,
                    0.0,
                    InsetsLike::default(),
                )],
            );
            assert!(!thread.has_overflow());
            for (i, line) in thread.lines().enumerate() {
                let first = if i == 0 { first } else { 0.0 };
                assert!((line.bounds.x - (10.0 + left + first)).abs() < 0.001);
                assert!((line.bounds.width - (120.0 - left - right - first)).abs() < 0.001);
                assert!(
                    line.natural_width <= line.bounds.width + 0.001,
                    "{i}: {} exceeds {}",
                    line.natural_width,
                    line.bounds.width
                );
            }
        }
    }
}

#[test]
fn a_first_line_indent_occurs_once_per_paragraph_across_threaded_frames() {
    for first in [-20.0, 20.0, 40.0] {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Indented".into(),
            point_size: Some(11.0),
            leading: Some(schist_layout::styles::Leading::Points(14.0)),
            left_indent: Some(20.0),
            first_line_indent: Some(first),
            keep_lines: Some(1),
            ..Default::default()
        });
        let text = "a few short words to wrap across many frames ".repeat(8);
        let mut story = Story::from_text(&text, "Indented");
        story.push_paragraph(&text, "Indented");
        let starts = story.point_offsets();
        let id = doc.add_story(story);
        let frames: Vec<_> = (0..100)
            .map(|_| {
                (
                    ObjectId::next(),
                    Rect::new(10.0, 0.0, 120.0, 28.01),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    InsetsLike::default(),
                )
            })
            .collect();
        let thread = compose_thread(&doc, id, &frames);
        assert!(!thread.has_overflow());
        for line in thread.lines() {
            let expected = 30.0
                + if starts.contains(&line.start) {
                    first
                } else {
                    0.0
                };
            assert!(
                (line.bounds.x - expected).abs() < 0.001,
                "{}: {} != {expected}",
                line.start,
                line.bounds.x
            );
        }
        let single = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(10.0, 0.0, 120.0, 10000.0),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        assert_eq!(
            thread.lines().map(|l| (l.start, l.end)).collect::<Vec<_>>(),
            single.lines().map(|l| (l.start, l.end)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn automatic_direction_comes_from_the_paragraph_not_each_composed_line() {
    use schist_text_engine::ParagraphDirection as Direction;
    for (prefix, tail, expected) in [
        (
            "عربي ",
            "a few Latin words with punctuation (123). ",
            Direction::RightToLeft,
        ),
        ("English ", "نص عربي مع أرقام ١٢٣ ", Direction::LeftToRight),
    ] {
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text(
            format!("{prefix}{}", tail.repeat(12)),
            "Body",
        ));
        let frames: Vec<_> = (0..20)
            .map(|_| {
                (
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, 150.0, 45.0),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    InsetsLike::default(),
                )
            })
            .collect();
        let thread = compose_thread(&doc, story, &frames);
        assert!(!thread.has_overflow());
        for line in thread.lines() {
            let spec = schist_layout::compose::spec_for(
                doc.story(story).unwrap(),
                line.start,
                line.end,
                &doc.styles,
                &line.paragraph_style,
                &doc.default_character_style,
                line.bounds.width,
            );
            let mut with_context = spec.clone();
            with_context.direction = expected;
            assert_eq!(
                schist_text_engine::carets(&spec),
                schist_text_engine::carets(&with_context),
                "paragraph context lost at {}",
                line.start,
            );
        }
    }
}
