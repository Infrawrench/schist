use schist_layout::{
    compose,
    styles::{BaselineShift, TextPosition},
    CharacterStyle, ParagraphStyle, Story,
};
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn script_position_size_and_baseline_offset_inherit_independently() {
    for parent in [
        TextPosition::Normal,
        TextPosition::Superscript,
        TextPosition::Subscript,
    ] {
        for child in [
            None,
            Some(TextPosition::Normal),
            Some(TextPosition::Superscript),
            Some(TextPosition::Subscript),
        ] {
            for shift in [-4.0, 0.0, 8.0] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.text_preferences.superscript_size = 60.0;
                doc.styles.text_preferences.superscript_position = 40.0;
                doc.styles.text_preferences.subscript_size = 50.0;
                doc.styles.text_preferences.subscript_position = 25.0;
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Base".into(),
                    position: Some(parent),
                    point_size: Some(20.0),
                    leading: Some(30.0),
                    baseline_shift: Some(BaselineShift::Offset(shift)),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Child".into(),
                    based_on: Some("Base".into()),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Character base".into(),
                    position: child,
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Character child".into(),
                    based_on: Some("Character base".into()),
                    point_size: Some(40.0),
                    leading: Some(50.0),
                    ..Default::default()
                });
                let mut story = Story::from_text("aé中z", "Child");
                story.apply_style(1, 6, "Character child");
                let spec = compose::spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 400.0);
                for byte in [0, 1, 3, 6] {
                    let local = (1..6).contains(&byte);
                    let position = if local {
                        child.unwrap_or(parent)
                    } else {
                        parent
                    };
                    let nominal = if local { 40.0 } else { 20.0 };
                    let leading = if local { 50.0 } else { 30.0 };
                    let (factor, displacement) = match position {
                        TextPosition::Normal => (1.0, 0.0),
                        TextPosition::Superscript => (0.6, leading * 0.4),
                        TextPosition::Subscript => (0.5, -leading * 0.25),
                    };
                    let style = spec.style_at(byte);
                    close(style.size, nominal * factor);
                    close(style.baseline_shift, shift + displacement);
                    assert_eq!(
                        style.metric_size,
                        (position != TextPosition::Normal).then_some(nominal)
                    );
                }
            }
        }
    }
}

#[test]
fn positions_preserve_logical_spacing_and_scale_every_metric_with_canvas_zoom() {
    use schist_layout::{authoring, Display, History, PasteboardView, Rect, WritingMode};
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Body".into(),
            point_size: Some(24.0),
            leading: Some(32.0),
            writing_mode: Some(mode),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(30.0, 30.0, 400.0, 400.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("HH HH\nHH HH", "Body");
        let before = compose::compose_story(&doc, frame.story);
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Body")
            .unwrap()
            .position = Some(TextPosition::Superscript);
        let after = compose::compose_story(&doc, frame.story);
        assert_eq!(before.lines().count(), after.lines().count());
        for (a, b) in before.lines().zip(after.lines()) {
            close(a.baseline, b.baseline);
            close(a.advance, b.advance);
        }
        for zoom in [0.5, 1.0, 3.0] {
            let board = schist_layout::pasteboard(
                &doc,
                &PasteboardView {
                    scale: zoom,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut count = 0;
            for item in board.objects() {
                if let Display::Text { spec, .. } = item {
                    if spec.text.is_empty() {
                        continue;
                    }
                    count += 1;
                    let style = spec.style_at(0);
                    close(style.size, 24.0 * 0.583 * zoom);
                    close(style.metric_size.unwrap(), 24.0 * zoom);
                    close(style.baseline_shift, 32.0 * 0.333 * zoom);
                }
            }
            assert!(count >= 2);
        }
    }
}

#[test]
fn preferences_and_legacy_script_variants_have_stable_serialization_defaults() {
    let mut doc = schist_layout::blank_a4();
    for (shift, position) in [
        (BaselineShift::Superscript, TextPosition::Superscript),
        (BaselineShift::Subscript, TextPosition::Subscript),
    ] {
        doc.styles.paragraphs[0].baseline_shift = Some(shift);
        let story = Story::from_text("x", "Default");
        let spec = compose::spec_for(&story, 0, 1, &doc.styles, "Default", "Default", 100.0);
        assert_eq!(spec.style_at(0).metric_size, Some(11.0));
        close(spec.style_at(0).size, 11.0 * 0.583);
        assert_eq!(
            spec.style_at(0).baseline_shift > 0.0,
            position == TextPosition::Superscript
        );
    }
    let mut json = serde_json::to_value(&doc).unwrap();
    assert_eq!(
        serde_json::from_value::<schist_layout::LayoutDocument>(json.clone()).unwrap(),
        doc
    );
    json["styles"]
        .as_object_mut()
        .unwrap()
        .remove("text_preferences");
    for key in ["paragraphs", "characters"] {
        for style in json["styles"][key].as_array_mut().unwrap() {
            style.as_object_mut().unwrap().remove("position");
        }
    }
    let old: schist_layout::LayoutDocument = serde_json::from_value(json).unwrap();
    assert_eq!(old.styles.text_preferences, Default::default());
    assert!(old.styles.paragraphs.iter().all(|s| s.position.is_none()));
}
