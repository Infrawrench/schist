use schist_layout::{
    blank_a4,
    compose::{compose_thread, spec_for, InsetsLike},
    CharacterStyle, FrameOverflow, LayoutDocument, ObjectId, ParagraphStyle, Rect, Story,
};

fn styled(size: f32, leading: f32) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Plain".into(),
        point_size: Some(size),
        leading: Some(leading),
        keep_lines: Some(1),
        ..Default::default()
    });
    doc
}

#[test]
fn a_partial_paragraph_never_skips_forward_to_a_smaller_later_paragraph() {
    for size in [20.0, 30.0, 40.0] {
        let mut doc = styled(size, size * 1.3);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Small".into(),
            point_size: Some(5.0),
            leading: Some(6.0),
            ..Default::default()
        });
        let mut story = Story::from_text(
            "Large paragraph must finish before the tail appears. ".repeat(10),
            "Plain",
        );
        story.push_paragraph("TAIL", "Small");
        let text = story.text();
        let id = doc.add_story(story);
        let thread = compose_thread(
            &doc,
            id,
            &[
                (
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, 120.0, size * 1.3 + 8.0),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    InsetsLike::default(),
                ),
                (
                    ObjectId::next(),
                    Rect::new(200.0, 0.0, 120.0, 10000.0),
                    FrameOverflow::Clip,
                    1,
                    0.0,
                    InsetsLike::default(),
                ),
            ],
        );
        assert!(!thread.has_overflow());
        let mut cursor = 0;
        for line in thread.lines() {
            assert!(line.start >= cursor);
            assert!(
                text[cursor..line.start].chars().all(char::is_whitespace),
                "unpainted text between {cursor} and {}",
                line.start
            );
            cursor = line.end;
        }
        assert!(text[cursor..].chars().all(char::is_whitespace));
    }
}

#[test]
fn a_frame_split_uses_the_shapers_complete_line_breaks() {
    for text in [
        "alpha beta gamma delta epsilon zeta eta theta ".repeat(6),
        "école café 世界 texte composé avec des espaces ".repeat(6),
    ] {
        let mut doc = styled(12.0, 16.0);
        let story = Story::from_text(&text, "Plain");
        let spec = spec_for(
            &story,
            0,
            text.len(),
            &doc.styles,
            "Plain",
            &doc.default_character_style,
            100.0,
        );
        let lines = schist_text_engine::line_spans(&spec);
        assert!(lines.len() > 6);
        let id = doc.add_story(story);
        for count in 1..6 {
            let height = lines[..count].iter().map(|line| line.height).sum::<f32>() + 0.01;
            let thread = compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, 100.0, height),
                    FrameOverflow::Clip,
                    1,
                    0.0,
                    InsetsLike::default(),
                )],
            );
            assert_eq!(
                thread.lines().map(|l| (l.start, l.end)).collect::<Vec<_>>(),
                lines[..count]
                    .iter()
                    .map(|l| (l.start, l.end))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn mixed_size_lines_reserve_their_own_height() {
    let mut doc = styled(11.0, 14.0);
    doc.styles.add_character(CharacterStyle {
        name: "Large".into(),
        point_size: Some(32.0),
        leading: Some(40.0),
        ..Default::default()
    });
    let mut story = Story::from_text("small\nBIG\nsmall", "Plain");
    story.apply_style(6, 9, "Large");
    let spec = spec_for(
        &story,
        0,
        story.text_len(),
        &doc.styles,
        "Plain",
        &doc.default_character_style,
        180.0,
    );
    let spans = schist_text_engine::line_spans(&spec);
    assert_eq!(spans.len(), 3);
    assert!(spans[1].height > spans[0].height);
    let id = doc.add_story(story);
    let height = spans.iter().map(|s| s.height).sum::<f32>();
    let thread = compose_thread(
        &doc,
        id,
        &[(
            ObjectId::next(),
            Rect::new(0.0, 0.0, 180.0, height + 0.1),
            FrameOverflow::Clip,
            1,
            0.0,
            InsetsLike::default(),
        )],
    );
    assert!(!thread.has_overflow());
    let mut top = 0.0;
    for (line, span) in thread.lines().zip(spans) {
        assert!((line.bounds.y - top).abs() < 0.001);
        assert!((line.bounds.height - span.height).abs() < 0.001);
        top += span.height;
    }
}

#[test]
fn balanced_columns_keep_paragraph_spacing_and_heading_chains() {
    for columns in [2, 3] {
        for groups in [2, 3, 5] {
            let mut doc = styled(11.0, 14.0);
            for (name, keep) in [("Heading", true), ("Body", false)] {
                doc.styles.add_paragraph(ParagraphStyle {
                    name: name.into(),
                    point_size: Some(11.0),
                    leading: Some(14.0),
                    space_before: Some(4.0),
                    space_after: Some(9.0),
                    keep_with_next: Some(keep),
                    ..Default::default()
                });
            }
            let mut story = Story::new();
            for _ in 0..groups {
                story.push_paragraph("Heading A", "Heading");
                story.push_paragraph("Heading B", "Heading");
                story.push_paragraph("Body", "Body");
            }
            let id = doc.add_story(story);
            let thread = compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, columns as f32 * 140.0, 600.0),
                    FrameOverflow::Clip,
                    columns,
                    10.0,
                    InsetsLike::default(),
                )],
            );
            assert!(!thread.has_overflow());
            let lines: Vec<_> = thread.lines().collect();
            assert_eq!(lines.len(), groups * 3);
            for pair in lines.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if a.paragraph_style == "Heading" {
                    assert_eq!(a.bounds.x, b.bounds.x, "heading stranded");
                }
                if a.bounds.x == b.bounds.x {
                    assert!(b.bounds.y - a.bounds.bottom() >= 12.999, "spacing lost");
                }
            }
        }
    }
}

#[test]
fn every_split_obeys_both_sides_of_the_minimum_line_count() {
    for keep in [1, 2, 3] {
        let mut doc = styled(12.0, 16.0);
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Plain")
            .unwrap()
            .keep_lines = Some(keep);
        let text = "alpha beta gamma delta epsilon zeta eta theta ".repeat(6);
        let story = Story::from_text(&text, "Plain");
        let spans = schist_text_engine::line_spans(&spec_for(
            &story,
            0,
            text.len(),
            &doc.styles,
            "Plain",
            &doc.default_character_style,
            100.0,
        ));
        let id = doc.add_story(story);
        for room in 1..spans.len() {
            let height = spans[..room].iter().map(|l| l.height).sum::<f32>() + 0.01;
            let thread = compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, 100.0, height),
                    FrameOverflow::Clip,
                    1,
                    0.0,
                    InsetsLike::default(),
                )],
            );
            let count = thread.lines().count();
            assert!(
                count == 0 || (count >= keep && spans.len() - count >= keep),
                "{count} / {} with minimum {keep}",
                spans.len()
            );
        }
    }
}

#[test]
fn balancing_uses_the_same_grid_baselines_as_sequential_flow() {
    for columns in [1, 2, 3] {
        let mut doc = styled(11.0, 14.0);
        doc.grids.document.mode = schist_layout::grid::GridMode::SnapToGrid;
        doc.grids.document.baseline_count = 3.0;
        let mut story = Story::new();
        for _ in 0..6 {
            story.push_paragraph("Line", "Plain");
        }
        let id = doc.add_story(story);
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, 400.0, 300.0),
                FrameOverflow::Clip,
                columns,
                10.0,
                InsetsLike::default(),
            )],
        );
        assert!(!thread.has_overflow());
        assert_eq!(thread.lines().count(), 6);
        for line in thread.lines() {
            let phase = (line.baseline - doc.pages[0].margins.top) / 24.0;
            assert!((phase - phase.round()).abs() < 0.001);
            assert!((line.bounds.height - 14.0).abs() < 0.01);
        }
    }
}
