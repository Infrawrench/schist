use schist_layout::{
    blank_a4, compose,
    lists::{ListKind, ListStyle, ListTab},
    styles::Align,
    FrameOverflow, ObjectId, ParagraphStyle, Rect, Story, WritingMode,
};

fn document(mode: WritingMode, indent: f32, initial: f32) -> schist_layout::LayoutDocument {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Tabs".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(12.0),
        writing_mode: Some(mode),
        left_indent: Some(indent),
        first_line_indent: Some(initial),
        list: ListStyle {
            tabs: Some(
                [60.0, 120.0, 180.0]
                    .map(|position| ListTab {
                        position,
                        alignment: "LeftAlign".into(),
                        alignment_character: ".".into(),
                        leader: String::new(),
                    })
                    .into(),
            ),
            ..Default::default()
        },
        ..Default::default()
    });
    doc
}

#[test]
fn tab_rulers_stay_with_columns_across_indents_initials_lists_and_threaded_writing_modes() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for (indent, initial) in [(0.0, 0.0), (12.0, 9.0), (24.0, -12.0)] {
            for treatment in 0..3 {
                if mode != WritingMode::Horizontal && treatment != 0 {
                    continue;
                }
                for columns in [1, 2] {
                    let mut doc = document(mode, indent, initial);
                    let style = doc
                        .styles
                        .paragraphs
                        .iter_mut()
                        .find(|p| p.name == "Tabs")
                        .unwrap();
                    if treatment == 1 {
                        style.drop_caps_lines = Some(2);
                    }
                    if treatment == 2 {
                        style.list.kind = Some(ListKind::Numbered);
                    }
                    let mut story = Story::new();
                    for _ in 0..12 {
                        story.push_paragraph("A\tH words", "Tabs");
                    }
                    let original = story.clone();
                    let id = doc.add_story(story);
                    let bounds = if mode == WritingMode::Horizontal {
                        Rect::new(20.0, 30.0, 420.0, 80.0)
                    } else {
                        Rect::new(20.0, 30.0, 80.0, 420.0)
                    };
                    let frames = (0..20)
                        .map(|_| {
                            (
                                ObjectId::next(),
                                bounds,
                                FrameOverflow::Thread,
                                columns,
                                12.0,
                                compose::InsetsLike::default(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let flow = compose::compose_thread(&doc, id, &frames);
                    assert!(
                        !flow.has_overflow(),
                        "{mode:?}/{indent}/{initial}/{treatment}/{columns}"
                    );
                    let mut covered = vec![false; original.text_len()];
                    let mut tabs = 0;
                    for line in flow.lines().filter(|line| line.generated.is_none()) {
                        for byte in &mut covered[line.start..line.end] {
                            assert!(!*byte);
                            *byte = true;
                        }
                        let spec = compose::line_spec(line, &original, &doc);
                        if let Some(at) = spec.text.find('\t') {
                            tabs += 1;
                            let caret = schist_text_engine::caret_at(&spec, at + 1).unwrap();
                            let absolute = if mode == WritingMode::Horizontal {
                                line.bounds.x + caret.x
                            } else {
                                line.bounds.y + caret.top
                            };
                            // The list's generated marker consumes the first stop;
                            // ordinary and enlarged-initial text use the first stop.
                            let expected = if treatment == 2 { 120.0 } else { 60.0 };
                            assert!(
                                (absolute - line.inline_origin - expected).abs() < 0.001,
                                "{mode:?}/{treatment}: {absolute}, {line:?}"
                            );
                            assert!(
                                (schist_text_engine::measure(&spec).unwrap().width
                                    - line.natural_width)
                                    .abs()
                                    < 0.001
                            );
                        }
                    }
                    assert_eq!(tabs, 12);
                    for (byte, covered) in original.text().bytes().zip(covered) {
                        assert!(covered || byte == b'\n');
                    }
                    assert_eq!(doc.story(id).unwrap(), &original);
                }
            }
        }
    }
}

#[test]
fn tabs_that_cannot_fit_remain_overset_and_resume_unchanged_in_a_wider_frame() {
    for width in [5.0, 30.0, 59.0] {
        let mut doc = document(WritingMode::Horizontal, 0.0, 0.0);
        let id = doc.add_story(Story::from_text("\tH", "Tabs"));
        let narrow = (
            ObjectId::next(),
            Rect::new(10.0, 20.0, width, 100.0),
            FrameOverflow::Thread,
            1,
            0.0,
            compose::InsetsLike::default(),
        );
        let flow = compose::compose_thread(&doc, id, &[narrow]);
        assert!(flow.has_overflow());
        assert!(flow.lines().next().is_none());
        assert_eq!(flow.frames[0].consumed_to, 0);
        let wide = (
            ObjectId::next(),
            Rect::new(40.0, 20.0, 200.0, 100.0),
            FrameOverflow::Thread,
            1,
            0.0,
            compose::InsetsLike::default(),
        );
        let flow = compose::compose_thread(&doc, id, &[narrow, wide]);
        assert!(!flow.has_overflow());
        assert!(flow.frames[0].lines.is_empty());
        assert_eq!(flow.frames[1].lines[0].start, 0);
        assert_eq!(flow.frames[1].consumed_to, 2);
    }
}

#[test]
fn justification_expands_only_spaces_after_the_final_tab_and_keeps_tab_stops_fixed() {
    for text in [
        "A B\tC D E",
        "A\tB C\tD E F",
        "A\tB\u{a0}C D",
        "A\tB\u{2003}C D",
    ] {
        for width in [200.0, 280.0] {
            let mut doc = document(WritingMode::Horizontal, 12.0, 9.0);
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Tabs")
                .unwrap()
                .align = Some(Align::JustifyAll);
            let id = doc.add_story(Story::from_text(text, "Tabs"));
            let flow = compose::compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(20.0, 30.0, width, 100.0),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    compose::InsetsLike::default(),
                )],
            );
            assert!(!flow.has_overflow());
            assert_eq!(flow.lines().count(), 1);
            let line = flow.lines().next().unwrap();
            let spec = compose::line_spec(line, doc.story(id).unwrap(), &doc);
            let mut natural = spec.clone();
            natural.word_spacing = 0.0;
            assert!(spec.word_spacing > 0.0);
            assert!(
                (schist_text_engine::measure(&spec).unwrap().width - line.bounds.width).abs()
                    < 0.001
            );
            for (at, _) in text.match_indices('\t') {
                assert_eq!(
                    schist_text_engine::caret_at(&spec, at + 1),
                    schist_text_engine::caret_at(&natural, at + 1)
                );
            }
        }
    }
}
