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

#[test]
fn aligned_tab_anchors_survive_columns_indents_wrapping_and_threading() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for alignment in ["RightAlign", "CenterAlign", "CharacterAlign"] {
            for (indent, initial) in [(0.0, 0.0), (12.0, 9.0), (24.0, -12.0)] {
                let mut doc = document(mode, indent, initial);
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Tabs")
                    .unwrap();
                for tab in style.list.tabs.as_mut().unwrap() {
                    tab.alignment = alignment.into();
                }
                let mut story = Story::new();
                for _ in 0..12 {
                    story.push_paragraph("A\t12.34 words more words\tH", "Tabs");
                }
                let original = story.clone();
                let id = doc.add_story(story);
                let frames = (0..20)
                    .map(|_| {
                        (
                            ObjectId::next(),
                            if mode == WritingMode::Horizontal {
                                Rect::new(20.0, 30.0, 420.0, 80.0)
                            } else {
                                Rect::new(20.0, 30.0, 80.0, 420.0)
                            },
                            FrameOverflow::Thread,
                            2,
                            12.0,
                            compose::InsetsLike::default(),
                        )
                    })
                    .collect::<Vec<_>>();
                let flow = compose::compose_thread(&doc, id, &frames);
                assert!(!flow.has_overflow(), "{mode:?}/{alignment}/{indent}");
                let mut covered = vec![false; original.text_len()];
                let mut tabs = 0;
                for line in flow.lines() {
                    for byte in &mut covered[line.start..line.end] {
                        assert!(!*byte);
                        *byte = true;
                    }
                    let spec = compose::line_spec(line, &original, &doc);
                    assert!(
                        (schist_text_engine::measure(&spec).unwrap().width - line.natural_width)
                            .abs()
                            < 0.001
                    );
                    let inline = |c: schist_text_engine::Caret| {
                        if mode == WritingMode::Horizontal {
                            c.x
                        } else {
                            c.top
                        }
                    };
                    for (at, _) in spec.text.match_indices('\t') {
                        tabs += 1;
                        let start = at + 1;
                        let end = spec.text[start..]
                            .find('\t')
                            .map_or(spec.text.len(), |i| start + i);
                        let left = inline(schist_text_engine::caret_at(&spec, start).unwrap());
                        let right = inline(schist_text_engine::caret_at(&spec, end).unwrap());
                        assert!(left > inline(schist_text_engine::caret_at(&spec, at).unwrap()));
                        let anchor = match alignment {
                            "RightAlign" => right,
                            "CenterAlign" => (left + right) / 2.0,
                            _ => spec.text[start..end].find('.').map_or(right, |i| {
                                inline(schist_text_engine::caret_at(&spec, start + i).unwrap())
                            }),
                        };
                        let origin = if mode == WritingMode::Horizontal {
                            line.bounds.x
                        } else {
                            line.bounds.y
                        };
                        let absolute = origin + anchor - line.inline_origin;
                        // A fitting explicit anchor is on the 60pt ruler. If
                        // every explicit stop collides, the field starts on
                        // Schist's implicit leading grid after the final stop.
                        let implicit = origin + left - line.inline_origin;
                        assert!(
                            [60.0, 120.0, 180.0]
                                .iter()
                                .any(|v| (absolute - v).abs() < 0.001)
                                || implicit > 180.0
                                    && (implicit / 36.0 - (implicit / 36.0).round()).abs() < 0.001,
                            "{mode:?}/{alignment}: {absolute}, {line:?}"
                        );
                    }
                }
                assert_eq!(tabs, 24);
                for (byte, covered) in original.text().bytes().zip(covered) {
                    assert!(covered || byte == b'\n');
                }
                assert_eq!(doc.story(id).unwrap(), &original);
            }
        }
    }
}

#[test]
fn aligned_tabs_keep_natural_spacing_and_diagnose_unimplemented_justification() {
    for alignment in ["RightAlign", "CenterAlign", "CharacterAlign"] {
        for align in [Align::Justify, Align::JustifyAll] {
            let mut doc = document(WritingMode::Horizontal, 0.0, 0.0);
            let style = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Tabs")
                .unwrap();
            style.align = Some(align);
            for tab in style.list.tabs.as_mut().unwrap() {
                tab.alignment = alignment.into();
            }
            let text = "A\t12.34 words more words";
            let paragraph = doc.styles.resolve_paragraph("Tabs");
            assert!(schist_layout::tabs::unsupported(&paragraph, text, false)
                .contains(&"Justification + TabList"));
            let id = doc.add_story(Story::from_text(text, "Tabs"));
            let flow = compose::compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(20.0, 30.0, 240.0, 100.0),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    compose::InsetsLike::default(),
                )],
            );
            assert!(!flow.has_overflow());
            for line in flow.lines() {
                let spec = compose::line_spec(line, doc.story(id).unwrap(), &doc);
                assert_eq!(spec.word_spacing, 0.0);
                assert!(
                    (schist_text_engine::measure(&spec).unwrap().width - line.natural_width).abs()
                        < 0.001
                );
            }
        }
    }
}

#[test]
fn preview_zoom_keeps_every_tabbed_caret_on_its_document_position() {
    use schist_layout::{
        authoring,
        pasteboard::{pasteboard, Display, PasteboardView},
        History, Point,
    };
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for alignment in ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"] {
            for (indent, initial) in [(0.0, 0.0), (12.0, 9.0), (24.0, -12.0)] {
                let mut doc = document(mode, indent, initial);
                for tab in doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Tabs")
                    .unwrap()
                    .list
                    .tabs
                    .as_mut()
                    .unwrap()
                {
                    tab.alignment = alignment.into();
                }
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(30.0, 40.0, 250.0, 250.0),
                )
                .unwrap();
                doc.stories[frame.story.0 as usize] = Story::from_text("A\t12.34\nB\t56.7", "Tabs");
                let flow = compose::compose_story(&doc, frame.story);
                assert!(!flow.has_overflow());
                for scale in [0.5, 0.75, 1.0, 1.5, 3.0] {
                    let view = PasteboardView {
                        origin: Point::new(17.0, -23.0),
                        page: Some(0),
                        scale,
                        ..Default::default()
                    };
                    let board = pasteboard(&doc, &view).unwrap();
                    let mut count = 0;
                    for display in &board.pages[0].objects {
                        let Display::Text {
                            start, rect, spec, ..
                        } = display
                        else {
                            continue;
                        };
                        let line = flow.lines().find(|line| line.start == *start).unwrap();
                        let reference =
                            compose::line_spec(line, doc.story(frame.story).unwrap(), &doc);
                        for byte in schist_text_engine::grapheme_boundaries(&spec.text) {
                            let actual = schist_text_engine::caret_at(spec, byte).unwrap();
                            let expected = schist_text_engine::caret_at(&reference, byte).unwrap();
                            let origin = view.to_pasteboard(line.bounds.origin());
                            assert!(
                                (rect.x + actual.x - origin.x - expected.x * scale).abs() < 0.002,
                                "{mode:?}/{alignment}/{scale}/{byte}"
                            );
                            assert!(
                                (rect.y + actual.top - origin.y - expected.top * scale).abs()
                                    < 0.002,
                                "{mode:?}/{alignment}/{scale}/{byte}"
                            );
                        }
                        count += 1;
                    }
                    assert_eq!(count, 2);
                }
            }
        }
    }
}
