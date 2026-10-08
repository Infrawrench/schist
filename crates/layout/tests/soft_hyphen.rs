use schist_layout::{
    authoring, compose, FrameOverflow, History, Insets, LayoutObject, ParagraphDirection,
    ParagraphStyle, Rect, Story, StoryId, WritingMode,
};

#[test]
fn selected_discretionary_breaks_survive_frame_slicing_with_source_carets() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for axis in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for prefix in ["hy", "café"] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Body".into(),
                    family: Some("IBM Plex Sans".into()),
                    point_size: Some(14.0),
                    writing_mode: Some(axis),
                    direction: Some(direction),
                    ..Default::default()
                });
                let reference = Story::from_text(format!("{prefix}-"), "Body");
                let mut literal = compose::spec_for(
                    &reference,
                    0,
                    reference.text_len(),
                    &doc.styles,
                    "Body",
                    &doc.default_character_style,
                    0.0,
                );
                literal.direction = schist_text_engine::ParagraphDirection::LeftToRight;
                let width = schist_text_engine::line_spans(&literal)[0].width + 0.5;
                let first = if axis == WritingMode::Horizontal {
                    Rect::new(20.0, 20.0, width, 20.0)
                } else {
                    Rect::new(20.0, 20.0, 20.0, width)
                };
                let mut ids = Vec::new();
                for bounds in [first, Rect::new(100.0, 100.0, 200.0, 200.0)] {
                    let made = authoring::text_frame(&mut doc, &mut History::default(), 0, bounds)
                        .unwrap();
                    if let LayoutObject::TextFrame {
                        story,
                        overflow,
                        insets,
                        ..
                    } = &mut doc.objects.last_mut().unwrap().object
                    {
                        *story = StoryId(0);
                        *overflow = FrameOverflow::Thread;
                        *insets = Insets::ZERO;
                    }
                    ids.push(made.object);
                }
                doc.thread_order = vec![(StoryId(0), ids)];
                doc.stories[0] = Story::from_text(format!("{prefix}\u{ad}continuation"), "Body");
                let before = doc.clone();
                let flow = compose::compose_story(&doc, StoryId(0));
                assert_eq!(
                    flow.frames[0].lines.len(),
                    1,
                    "{axis:?}/{direction:?}/{prefix}"
                );
                assert!(flow.frames[0].passed_on);
                assert!(!flow.frames[1].lost);
                let line = &flow.frames[0].lines[0];
                assert_eq!(line.end, prefix.len() + 2);
                assert_eq!(flow.frames[1].lines[0].start, line.end);
                assert_eq!(flow.frames[1].consumed_to, doc.stories[0].text_len());
                let rendered = compose::line_spec(line, &doc.stories[0], &doc);
                assert_eq!(rendered.text, format!("{prefix}\u{ad}"));
                assert!(rendered.show_final_soft_hyphen);
                assert!(
                    (line.natural_width - schist_text_engine::line_spans(&rendered)[0].width).abs()
                        < 0.001
                );
                let a = schist_text_engine::rasterize(&rendered).unwrap();
                let b = schist_text_engine::rasterize(&literal).unwrap();
                assert_eq!(a.coverage, b.coverage);
                for (byte, _) in schist_text_engine::carets(&rendered) {
                    assert!(rendered.text.is_char_boundary(byte));
                    assert!(doc.stories[0].text().is_char_boundary(line.start + byte));
                }
                assert_eq!(doc, before);
            }
        }
    }
}

#[test]
fn path_brackets_measure_the_selected_hyphen_and_preserve_no_break() {
    use schist_layout::{Point, ShapePath, SubPath};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for protected in [false, true] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.paragraphs[0].family = Some("IBM Plex Sans".into());
        doc.styles.paragraphs[0].point_size = Some(14.0);
        doc.styles.paragraphs[0].no_break = Some(protected);
        let shape = ShapePath {
            subpaths: vec![SubPath {
                points: vec![Point::ZERO, Point::new(40.0, 0.0)],
                handles: Vec::new(),
                closed: false,
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
        let first =
            schist_layout::text_path::attach(&mut doc, &mut History::default(), id).unwrap();
        let second = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(100.0, 100.0, 250.0, 100.0),
        )
        .unwrap();
        for object in &mut doc.objects {
            if let LayoutObject::TextFrame {
                story, overflow, ..
            } = &mut object.object
            {
                *story = first.story;
                *overflow = FrameOverflow::Thread;
            }
        }
        doc.thread_order = vec![(first.story, vec![first.object, second.object])];
        doc.stories[first.story.0 as usize] = Story::from_text("café\u{ad}continuation", "Default");
        let source = doc.clone();
        let flow = compose::compose_story(&doc, first.story);
        assert_eq!(flow.frames[0].lines.len(), usize::from(!protected));
        assert!(flow.frames[0].passed_on);
        assert!(!flow.frames[1].lost);
        if !protected {
            let line = &flow.frames[0].lines[0];
            let painted = compose::line_spec(line, &doc.stories[first.story.0 as usize], &doc);
            assert!(painted.show_final_soft_hyphen && painted.path.is_some());
            let mut plain = painted.clone();
            plain.path = None;
            assert!(
                (schist_text_engine::line_spans(&plain)[0].width - line.natural_width).abs()
                    < 0.001
            );
            assert_eq!(line.end, "café\u{ad}".len());
        }
        assert_eq!(doc, source);
    }
}

#[test]
fn explicit_line_ends_never_turn_a_hidden_hyphen_into_printed_ink() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for separator in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.paragraphs[0].family = Some("IBM Plex Sans".into());
            doc.styles.paragraphs[0].writing_mode = Some(axis);
            authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(20.0, 20.0, 200.0, 200.0),
            )
            .unwrap();
            doc.stories[0] = Story::from_text(format!("hy\u{ad}{separator}word"), "Default");
            let before = doc.clone();
            let flow = compose::compose_story(&doc, StoryId(0));
            assert_eq!(flow.frames[0].lines.len(), 2);
            let spec = compose::line_spec(&flow.frames[0].lines[0], &doc.stories[0], &doc);
            assert_eq!(spec.text, "hy\u{ad}");
            assert!(!spec.show_final_soft_hyphen, "{separator:?}/{axis:?}");
            assert!(
                (schist_text_engine::line_spans(&spec)[0].width
                    - flow.frames[0].lines[0].natural_width)
                    .abs()
                    < 0.001
            );
            let mut expected = spec.clone();
            expected.text = "hy".into();
            assert_eq!(
                schist_text_engine::rasterize(&spec).unwrap().coverage,
                schist_text_engine::rasterize(&expected).unwrap().coverage
            );
            assert_eq!(doc, before);
        }
    }
}
