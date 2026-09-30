use schist_layout::{
    authoring, compose, styles::BaselineShift, CharacterStyle, Display, History, LayoutDocument,
    Page, ParagraphStyle, PasteboardView, Rect, Story,
};

#[test]
fn baseline_shift_inherits_per_property_and_explicit_zero_resets_it() {
    for default in [-5.0, 0.0, 9.0] {
        for paragraph in [None, Some(-8.0), Some(0.0), Some(12.0)] {
            for character in [None, Some(-3.0), Some(0.0), Some(7.0)] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.characters[0].baseline_shift = Some(BaselineShift::Offset(default));
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Base".into(),
                    baseline_shift: paragraph.map(BaselineShift::Offset),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Body".into(),
                    based_on: Some("Base".into()),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Base character".into(),
                    baseline_shift: character.map(BaselineShift::Offset),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Child".into(),
                    based_on: Some("Base character".into()),
                    point_size: Some(24.0),
                    ..Default::default()
                });
                let mut story = Story::from_text("Aé中Z", "Body");
                story.apply_style(1, 6, "Child");
                let spec = compose::spec_for(&story, 0, 7, &doc.styles, "Body", "Default", 200.0);
                for byte in [0, 1, 3, 6] {
                    let expected = if (1..6).contains(&byte) {
                        character.or(paragraph).unwrap_or(default)
                    } else {
                        paragraph.unwrap_or(default)
                    };
                    assert_eq!(spec.style_at(byte).baseline_shift, expected);
                }
            }
        }
    }
}

#[test]
fn shifts_preserve_composed_flow_and_scale_with_the_pasteboard() {
    for mode in [
        schist_layout::WritingMode::Horizontal,
        schist_layout::WritingMode::VerticalRightToLeft,
        schist_layout::WritingMode::VerticalLeftToRight,
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 300.0, 300.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Body".into(),
            point_size: Some(20.0),
            leading: Some(28.0),
            writing_mode: Some(mode),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(40.0, 40.0, 100.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] =
            Story::from_text("HH HH HH HH HH HH HH HH HH HH HH HH HH HH HH HH", "Body");
        let original = compose::compose_story(&doc, frame.story);
        for offset in [-12.0, 0.0, 7.0] {
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Body")
                .unwrap()
                .baseline_shift = Some(BaselineShift::Offset(offset));
            let after = compose::compose_story(&doc, frame.story);
            assert_eq!(
                after
                    .frames
                    .iter()
                    .map(|f| (f.object, f.consumed_to, f.passed_on, f.lost))
                    .collect::<Vec<_>>(),
                original
                    .frames
                    .iter()
                    .map(|f| (f.object, f.consumed_to, f.passed_on, f.lost))
                    .collect::<Vec<_>>()
            );
            let geometry = |thread: &compose::ComposedThread| {
                thread
                    .lines()
                    .map(|l| {
                        (
                            l.start,
                            l.end,
                            l.bounds,
                            l.baseline,
                            l.advance,
                            l.natural_width,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(geometry(&after), geometry(&original));
            for scale in [0.5, 1.0, 2.0, 3.0] {
                let board = schist_layout::pasteboard(
                    &doc,
                    &PasteboardView {
                        scale,
                        ..Default::default()
                    },
                )
                .unwrap();
                let mut count = 0;
                for display in board.objects() {
                    if let Display::Text { spec, .. } = display {
                        for byte in spec.text.char_indices().map(|(b, _)| b) {
                            assert_eq!(spec.style_at(byte).baseline_shift, offset * scale);
                        }
                        count += 1;
                    }
                }
                assert!(count > 1);
            }
        }
    }
}

#[test]
fn initial_reservations_ignore_shifts_while_their_ink_keeps_the_authored_offset() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Caps".into(),
        point_size: Some(14.0),
        leading: Some(18.0),
        drop_caps_lines: Some(3),
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 40.0, 150.0, 160.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text(
        "Hello world repeated for enough lines of a paragraph to have a three-line initial.",
        "Caps",
    );
    let original = compose::compose_story(&doc, frame.story);
    let geometry = |t: &compose::ComposedThread| {
        t.lines()
            .map(|l| (l.start, l.end, l.bounds, l.baseline, l.advance))
            .collect::<Vec<_>>()
    };
    let initial = original
        .lines()
        .find(|l| l.initial.is_some())
        .unwrap()
        .initial
        .unwrap();
    for shift in [-8.0, 6.0] {
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Caps")
            .unwrap()
            .baseline_shift = Some(BaselineShift::Offset(shift));
        let after = compose::compose_story(&doc, frame.story);
        assert_eq!(geometry(&after), geometry(&original));
        let line = after.lines().find(|l| l.initial.is_some()).unwrap();
        let changed = line.initial.unwrap();
        assert_eq!(changed.scale, initial.scale);
        assert!((changed.ink.y - initial.ink.y + shift).abs() < 0.001);
        assert!((changed.ink.height - initial.ink.height).abs() < 0.001);
        let spec = compose::line_spec(line, &doc.stories[frame.story.0 as usize], &doc);
        assert_eq!(spec.style_at(0).baseline_shift, shift);
    }
}
