use schist_layout::{
    authoring, compose, CharacterStyle, FrameOverflow, History, Insets, LayoutObject,
    ParagraphStyle, Rect, Story, StoryId, StyleRange, WritingMode,
};

fn document(
    width: f32,
    wide_tail: bool,
    character: bool,
    axis: WritingMode,
) -> schist_layout::LayoutDocument {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Together".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(14.0),
        no_break: Some(!character),
        writing_mode: Some(axis),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Together".into(),
        no_break: Some(true),
        ..Default::default()
    });
    let mut ids = Vec::new();
    for side in [width, if wide_tail { 240.0 } else { width }] {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, side, side),
        )
        .unwrap();
        let object = doc
            .objects
            .iter_mut()
            .find(|o| o.id == frame.object)
            .unwrap();
        if let LayoutObject::TextFrame {
            story,
            overflow,
            insets,
            ..
        } = &mut object.object
        {
            *story = StoryId(0);
            *overflow = FrameOverflow::Thread;
            *insets = Insets::ZERO;
        }
        ids.push(frame.object);
    }
    doc.thread_order = vec![(StoryId(0), ids)];
    doc.stories[0] = Story::from_text("a café name", "Together");
    if character {
        let end = doc.stories[0].text_len();
        doc.stories[0]
            .ranges
            .push(StyleRange::new(0, end, "Together"));
    }
    doc
}

#[test]
fn unbreakable_text_seeks_a_fitting_frame_or_remains_overset_without_changing_source() {
    for width in [20.0, 35.0] {
        for wide in [false, true] {
            for character in [false, true] {
                for axis in [
                    WritingMode::Horizontal,
                    WritingMode::VerticalLeftToRight,
                    WritingMode::VerticalRightToLeft,
                ] {
                    let doc = document(width, wide, character, axis);
                    let original = doc.clone();
                    let flow = compose::compose_story(&doc, StoryId(0));
                    assert!(flow.frames[0].lines.is_empty());
                    assert!(flow.frames[0].passed_on);
                    assert_eq!(flow.frames[0].consumed_to, 0);
                    assert_eq!(flow.frames[1].lost, !wide);
                    assert_eq!(flow.frames[1].lines.len(), usize::from(wide));
                    if wide {
                        assert_eq!(flow.frames[1].consumed_to, doc.stories[0].text_len());
                    }
                    assert_eq!(doc, original);
                }
            }
        }
    }
}

#[test]
fn no_break_inherits_independently_and_false_restores_ordinary_wrapping() {
    for character in [false, true] {
        let mut doc = document(35.0, true, character, WritingMode::Horizontal);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Together".into()),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Child".into(),
            based_on: Some("Together".into()),
            ..Default::default()
        });
        for value in [None, Some(false), Some(true)] {
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Child")
                .unwrap()
                .no_break = value;
            doc.styles
                .characters
                .iter_mut()
                .find(|s| s.name == "Child")
                .unwrap()
                .no_break = value;
            let p = doc.styles.resolve_paragraph("Child");
            let c = doc.styles.resolve_character("Child");
            assert_eq!(p.no_break, value.or(Some(!character)));
            assert_eq!(c.no_break, value.or(Some(true)));
            assert_eq!(c.clone().into_style("Copy").no_break, c.no_break);
            assert_eq!(c.clone().over(&Default::default()).no_break, c.no_break);
            if let schist_layout::StoryPoint::Paragraph { style, .. } =
                &mut doc.stories[0].points[0]
            {
                *style = "Child".into();
            }
            if character {
                doc.stories[0].ranges[0].style = "Child".into();
            }
            let flow = compose::compose_story(&doc, StoryId(0));
            assert_eq!(flow.frames[0].lines.is_empty(), value != Some(false));
        }
    }
}
