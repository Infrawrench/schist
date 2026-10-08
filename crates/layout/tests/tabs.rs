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
fn an_ordinary_tab_uses_the_native_hanging_indent_before_the_implicit_grid() {
    // Public InDesign 20.0.1.32 list-markers.pdf, case c12 (paged-media/core
    // 2e3c998e): "Tab\tc12 one", LeftIndent=40, FirstLineIndent=-40,
    // no explicit stops. Its word starts 40pt from the frame, not 36pt.
    // This is placement evidence, not a comparison with the fixture's font.
    let mut doc = document(WritingMode::Horizontal, 40.0, -40.0);
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Tabs")
        .unwrap();
    paragraph.point_size = Some(10.0);
    paragraph.list.tabs = Some(Vec::new());
    let text = "Tab\tc12 one";
    let id = doc.add_story(Story::from_text(text, "Tabs"));
    let flow = compose::compose_thread(
        &doc,
        id,
        &[(
            ObjectId::next(),
            Rect::new(72.0, 72.0, 200.0, 100.0),
            FrameOverflow::Thread,
            1,
            0.0,
            compose::InsetsLike::default(),
        )],
    );
    assert!(!flow.has_overflow());
    let line = flow.lines().next().unwrap();
    let spec = compose::line_spec(line, doc.story(id).unwrap(), &doc);
    let caret = schist_text_engine::caret_at(&spec, "Tab\t".len()).unwrap();
    assert!((line.bounds.x + caret.x - 72.0 - 40.0).abs() < 0.001);
    assert_eq!(spec.text, text);
    assert!((schist_text_engine::measure(&spec).unwrap().width - line.natural_width).abs() < 0.001);
}

#[test]
fn diagnosed_tabs_inside_an_initial_keep_ordinary_source_flow_and_carets() {
    use schist_layout::styles::ParagraphDirection;
    for direction in [
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ] {
        for prefix in ["A\t", "Å\u{301}\t", "אב\t", "\tA"] {
            for width in [90.0, 180.0, 360.0] {
                let mut doc = document(WritingMode::Horizontal, 7.0, 9.0);
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Tabs")
                    .unwrap();
                style.direction = Some(direction);
                style.align = Some(if direction == ParagraphDirection::RightToLeft {
                    Align::Right
                } else {
                    Align::Left
                });
                style.drop_caps_lines = Some(3);
                style.drop_caps_characters =
                    Some(schist_text_engine::grapheme_boundaries(prefix).count() - 1);
                let text = format!("{prefix}body text with words to wrap and source tabs\t12.34");
                let original = Story::from_text(&text, "Tabs");
                let id = doc.add_story(original.clone());
                let frames = [width, 360.0].map(|width| {
                    (
                        ObjectId::next(),
                        Rect::new(20.0, 30.0, width, 360.0),
                        FrameOverflow::Thread,
                        1,
                        0.0,
                        compose::InsetsLike::default(),
                    )
                });
                assert!(schist_layout::tabs::unsupported(
                    &doc.styles.resolve_paragraph("Tabs"),
                    &text,
                    false
                )
                .contains(&"DropCapCharacters + TabList"));
                let actual = compose::compose_thread(&doc, id, &frames);
                let mut ordinary = doc.clone();
                ordinary
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Tabs")
                    .unwrap()
                    .drop_caps_lines = None;
                let expected = compose::compose_thread(&ordinary, id, &frames);
                assert_eq!(actual.has_overflow(), expected.has_overflow());
                let capture = |flow: &compose::ComposedThread,
                               doc: &schist_layout::LayoutDocument| {
                    flow.lines()
                        .map(|line| {
                            assert!(line.initial.is_none());
                            let spec = compose::line_spec(line, &original, doc);
                            (
                                line.start,
                                line.end,
                                line.bounds,
                                line.natural_width,
                                schist_text_engine::insertion_points(&spec),
                                schist_text_engine::rasterize(&spec).unwrap().coverage,
                            )
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    capture(&actual, &doc),
                    capture(&expected, &ordinary),
                    "{direction:?}/{prefix:?}/{width}"
                );
                assert_eq!(doc.story(id).unwrap(), &original);
            }
        }
    }
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
                            // A generated marker may consume a virtual hanging
                            // indent before an explicit stop. The source tab
                            // still chooses its next ruler stop from the body's
                            // independently measured prefix and placed origin.
                            let mut prefix = spec.clone();
                            prefix.text.truncate(at);
                            prefix.tabs = None;
                            prefix.runs.retain(|r| r.start < at);
                            for run in &mut prefix.runs {
                                run.end = run.end.min(at);
                            }
                            let origin = if mode == WritingMode::Horizontal {
                                line.bounds.x
                            } else {
                                line.bounds.y
                            };
                            let pen = origin - line.inline_origin
                                + schist_text_engine::measure(&prefix).unwrap().width;
                            let mut expected = [60.0, 120.0, 180.0]
                                .into_iter()
                                .find(|stop| *stop > pen)
                                .unwrap();
                            if indent > pen {
                                expected = expected.min(indent);
                            }
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
fn a_tab_beyond_the_frame_edge_breaks_after_text_without_losing_source_or_carets() {
    use schist_layout::styles::ParagraphDirection;
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for width in [60.0, 120.0, 200.0] {
                for indent in [0.0, 9.0] {
                    let mut doc = document(mode, 0.0, indent);
                    let style = doc
                        .styles
                        .paragraphs
                        .iter_mut()
                        .find(|p| p.name == "Tabs")
                        .unwrap();
                    style.direction = Some(direction);
                    let reverse = mode == WritingMode::Horizontal
                        && direction == ParagraphDirection::RightToLeft;
                    style.align = Some(if reverse { Align::Right } else { Align::Left });
                    style.list.tabs = Some(vec![ListTab {
                        position: width + 50.0,
                        alignment: if reverse { "RightAlign" } else { "LeftAlign" }.into(),
                        alignment_character: ".".into(),
                        leader: String::new(),
                    }]);
                    let text = "Alpha\tBeta gamma";
                    let original = Story::from_text(text, "Tabs");
                    let id = doc.add_story(original.clone());
                    let bounds = if mode == WritingMode::Horizontal {
                        Rect::new(20.0, 30.0, width, 200.0)
                    } else {
                        Rect::new(20.0, 30.0, 200.0, width)
                    };
                    let flow = compose::compose_thread(
                        &doc,
                        id,
                        &[(
                            ObjectId::next(),
                            bounds,
                            FrameOverflow::Thread,
                            1,
                            0.0,
                            compose::InsetsLike::default(),
                        )],
                    );
                    assert!(
                        !flow.has_overflow(),
                        "{mode:?}/{direction:?}/{width}/{indent}"
                    );
                    let lines = flow.lines().collect::<Vec<_>>();
                    assert_eq!(&text[lines[0].start..lines[0].end], "Alpha\t");
                    assert!(text[lines[1].start..lines[1].end].starts_with("Beta"));
                    let mut next = 0;
                    for line in lines {
                        assert_eq!(line.start, next);
                        next = line.end;
                        let spec = compose::line_spec(line, &original, &doc);
                        assert_eq!(spec.text, text[line.start..line.end]);
                        assert!(
                            (schist_text_engine::measure(&spec).unwrap().width
                                - line.natural_width)
                                .abs()
                                < 0.001,
                            "{mode:?}/{direction:?}/{width}/{indent}: {line:?}"
                        );
                        let inline_width = if mode == WritingMode::Horizontal {
                            line.bounds.width
                        } else {
                            line.bounds.height
                        };
                        assert!(line.natural_width <= inline_width + 0.001);
                        assert!(schist_text_engine::caret_at(&spec, spec.text.len()).is_some());
                        assert!(schist_text_engine::rasterize(&spec).is_some());
                    }
                    assert_eq!(next, text.len());
                    assert_eq!(doc.story(id).unwrap(), &original);
                }
            }
        }
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
                        let pen = inline(schist_text_engine::caret_at(&spec, at).unwrap());
                        assert!(left + 0.001 >= pen);
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
                        let pen = origin + pen - line.inline_origin;
                        let offset = anchor - left;
                        let stop = [60.0, 120.0, 180.0]
                            .into_iter()
                            .find(|position| *position > pen);
                        let mut expected = if indent > pen && stop.is_none_or(|stop| indent < stop)
                        {
                            // An outdented first line reaches the virtual
                            // leading indent before a later aligned stop.
                            indent
                        } else {
                            stop.map_or_else(
                                || (pen.max(180.0) / 36.0).floor() * 36.0 + 36.0,
                                |position| (position - offset).max(pen),
                            )
                        };
                        if start == end {
                            let width = if mode == WritingMode::Horizontal {
                                line.bounds.width
                            } else {
                                line.bounds.height
                            };
                            expected = expected.min(origin - line.inline_origin + width);
                        }
                        // A colliding field clamps to the pen. Only stops
                        // already passed resume the implicit leading grid.
                        let actual = origin + left - line.inline_origin;
                        assert!(
                            (actual - expected).abs() < 0.001,
                            "{mode:?}/{alignment}: {actual} != {expected}, {line:?}"
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

#[test]
fn rtl_rulers_and_first_line_indents_follow_the_leading_column_edge_through_threads() {
    use schist_layout::styles::ParagraphDirection;
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for direction in [ParagraphDirection::RightToLeft, ParagraphDirection::Auto] {
            for alignment in ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"] {
                for (indent, first) in [(0.0, 0.0), (12.0, 9.0), (24.0, -12.0)] {
                    for columns in [1, 2] {
                        let mut doc = document(mode, 7.0, first);
                        let style = doc
                            .styles
                            .paragraphs
                            .iter_mut()
                            .find(|p| p.name == "Tabs")
                            .unwrap();
                        style.direction = Some(direction);
                        style.right_indent = Some(indent);
                        style.align = Some(if mode == WritingMode::Horizontal {
                            Align::Right
                        } else {
                            Align::Left
                        });
                        for stop in style.list.tabs.as_mut().unwrap() {
                            stop.alignment = alignment.into();
                            stop.leader = ". ".into();
                        }
                        let mut story = Story::new();
                        for _ in 0..12 {
                            story.push_paragraph("אב\t12.34 words more words", "Tabs");
                        }
                        let original = story.clone();
                        let id = doc.add_story(story);
                        let bounds = if mode == WritingMode::Horizontal {
                            Rect::new(20.0, 30.0, 440.0, 80.0)
                        } else {
                            Rect::new(20.0, 30.0, 80.0, 440.0)
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
                            "{mode:?}/{direction:?}/{alignment}/{indent}/{first}/{columns}"
                        );
                        let mut covered = vec![false; original.text_len()];
                        let mut tabs = 0;
                        for line in flow.lines() {
                            for byte in &mut covered[line.start..line.end] {
                                assert!(!*byte);
                                *byte = true;
                            }
                            let spec = compose::line_spec(line, &original, &doc);
                            assert_eq!(
                                spec.direction,
                                schist_text_engine::ParagraphDirection::RightToLeft
                            );
                            assert!(
                                (schist_text_engine::measure(&spec).unwrap().width
                                    - line.natural_width)
                                    .abs()
                                    < 0.002
                            );
                            let Some(at) = spec.text.find('\t') else {
                                continue;
                            };
                            tabs += 1;
                            let field = &spec.text[at + 1..];
                            let ordinary = schist_text_engine::TextSpec {
                                text: field.into(),
                                tabs: None,
                                runs: Vec::new(),
                                ..spec.clone()
                            };
                            let field_width = schist_text_engine::measure(&ordinary).unwrap().width;
                            let start = schist_text_engine::caret_at(&spec, at + 1).unwrap();
                            let ordinary_start =
                                schist_text_engine::caret_at(&ordinary, 0).unwrap();
                            // The first logical character of a bidi field need
                            // not sit at its physical left/top edge.
                            let local = if mode == WritingMode::Horizontal {
                                start.x - ordinary_start.x
                            } else {
                                start.top - ordinary_start.top
                            };
                            let anchor = match alignment {
                                "LeftAlign" => local,
                                "RightAlign" => local + field_width,
                                "CenterAlign" => local + field_width / 2.0,
                                _ => {
                                    let c = schist_text_engine::caret_at(
                                        &spec,
                                        at + 1 + field.find('.').unwrap(),
                                    )
                                    .unwrap();
                                    if mode == WritingMode::Horizontal {
                                        c.x
                                    } else {
                                        c.top
                                    }
                                }
                            };
                            let origin = compose::aligned_origin(
                                line.bounds,
                                line.natural_width,
                                spec.align,
                                spec.writing_mode,
                            );
                            let absolute = if mode == WritingMode::Horizontal {
                                origin.x
                            } else {
                                origin.y
                            } + anchor;
                            let position = if mode == WritingMode::Horizontal {
                                line.inline_origin - absolute
                            } else {
                                absolute - line.inline_origin
                            };
                            let leading_inset = if mode == WritingMode::Horizontal {
                                line.inline_origin - line.bounds.right()
                            } else {
                                line.bounds.y - line.inline_origin
                            };
                            let expected = if mode == WritingMode::Horizontal {
                                indent + first
                            } else {
                                7.0 + first
                            };
                            assert!((leading_inset - expected).abs() < 0.002);
                            let prefix = schist_text_engine::TextSpec {
                                text: spec.text[..at].into(),
                                tabs: None,
                                runs: Vec::new(),
                                ..spec.clone()
                            };
                            let pen =
                                leading_inset + schist_text_engine::measure(&prefix).unwrap().width;
                            let anchor_offset = if mode == WritingMode::Horizontal {
                                local + field_width - anchor
                            } else {
                                anchor - local
                            };
                            let stop = [60.0, 120.0, 180.0].into_iter().find(|stop| *stop > pen);
                            let leading_indent = if mode == WritingMode::Horizontal {
                                indent
                            } else {
                                7.0
                            };
                            let expected_start = if leading_indent > pen
                                && stop.is_none_or(|stop| leading_indent < stop)
                            {
                                leading_indent
                            } else {
                                stop.map_or_else(
                                    || (pen.max(180.0) / 36.0).floor() * 36.0 + 36.0,
                                    |stop| (stop - anchor_offset).max(pen),
                                )
                            };
                            assert!(
                                (position - expected_start - anchor_offset).abs() < 0.002,
                                "{mode:?}/{alignment}/{indent}/{first}: {position}, {line:?}"
                            );
                            assert!(schist_text_engine::rasterize(&spec).is_some());
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
}

#[test]
fn tab_diagnostics_use_the_resolved_axis_and_keep_unimplemented_alignment_cases_visible() {
    use schist_layout::styles::ParagraphDirection;
    for direction in [
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
        ParagraphDirection::Auto,
    ] {
        for align in [Align::Left, Align::Center, Align::Right, Align::JustifyAll] {
            let mut doc = document(WritingMode::Horizontal, 0.0, 0.0);
            let style = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Tabs")
                .unwrap();
            style.direction = Some(direction);
            style.align = Some(align);
            for tab in style.list.tabs.as_mut().unwrap() {
                tab.alignment = "CenterAlign".into();
            }
            let paragraph = doc.styles.resolve_paragraph("Tabs");
            for mode in [
                schist_text_engine::WritingMode::Horizontal,
                schist_text_engine::WritingMode::VerticalRl,
            ] {
                for text in ["אב\t12.34", "A\t12.34"] {
                    let rtl = direction == ParagraphDirection::RightToLeft
                        || direction == ParagraphDirection::Auto && text.starts_with('א');
                    let reverse = rtl && !mode.is_vertical();
                    let issues =
                        schist_layout::tabs::unsupported_in_mode(&paragraph, text, false, mode);
                    assert_eq!(
                        issues.contains(&"ParagraphDirection + TabList"),
                        reverse && align != Align::Right
                    );
                    assert_eq!(
                        issues.contains(&"Justification + TabList"),
                        align == Align::Center
                            || align == Align::Right && !reverse
                            || align == Align::JustifyAll
                    );
                    assert!(schist_layout::tabs::unsupported_in_mode(
                        &paragraph,
                        "unused options",
                        true,
                        mode
                    )
                    .is_empty());
                }
            }
        }
    }
}
