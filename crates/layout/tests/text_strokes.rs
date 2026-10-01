use schist_layout::{blank_a4, compose::spec_for, CharacterStyle, Ink, ParagraphStyle, Story};

#[test]
fn text_no_ink_and_weights_inherit_independently_at_every_style_depth() {
    for depth in 1..7 {
        for disabled in [false, true] {
            let mut doc = blank_a4();
            let ink = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                fill: Some(ink.clone()),
                stroke: Some(ink.clone()),
                stroke_weight: Some(2.0),
                stroke_outside: Some(true),
                ..Default::default()
            });
            let mut last = "Base".to_string();
            for level in 0..depth {
                let name = format!("Level {level}");
                doc.styles.add_paragraph(ParagraphStyle {
                    name: name.clone(),
                    based_on: Some(last),
                    fill_disabled: level == 0 && disabled,
                    stroke_disabled: level == 0 && disabled,
                    ..Default::default()
                });
                last = name;
            }
            let resolved = doc.styles.resolve_paragraph(&last);
            assert_eq!(resolved.fill_disabled, disabled);
            assert_eq!(resolved.stroke_disabled, disabled);
            assert_eq!(resolved.stroke_weight, Some(2.0));
            assert_eq!(resolved.stroke_outside, Some(true));
            doc.styles.add_character(CharacterStyle {
                name: "Local".into(),
                fill: Some(ink.clone()),
                stroke: Some(ink),
                stroke_weight: Some(0.5),
                ..Default::default()
            });
            let mut story = Story::from_text("aé中z", &last);
            story.apply_style(1, 6, "Local");
            let spec = spec_for(&story, 0, 7, &doc.styles, &last, "Default", 300.0);
            for byte in [0, 1, 3, 6] {
                let local = (1..6).contains(&byte);
                let run = spec.style_at(byte);
                assert_eq!(run.fill_disabled, disabled && !local);
                assert_eq!(run.stroke.is_none(), disabled && !local);
                if let Some(stroke) = run.stroke {
                    assert_eq!(stroke.width, if local { 0.5 } else { 2.0 });
                    assert!(stroke.outside);
                }
            }
            let before = doc.styles.clone();
            let mut history = schist_layout::History::default();
            assert!(schist_layout::properties::edit_styles(
                &mut doc,
                &mut history,
                |styles| {
                    let local = styles
                        .characters
                        .iter_mut()
                        .find(|s| s.name == "Local")
                        .unwrap();
                    local.fill = None;
                    local.fill_disabled = true;
                    local.stroke = None;
                    local.stroke_disabled = true;
                }
            ));
            assert!(history.undo(&mut doc));
            assert_eq!(doc.styles, before);
        }
    }
}

#[test]
fn glyph_joins_and_miter_limits_inherit_independently_with_local_resets() {
    use schist_text_engine::TextStrokeJoin;
    for join in [
        TextStrokeJoin::Miter,
        TextStrokeJoin::Round,
        TextStrokeJoin::Bevel,
    ] {
        for limit in [0.0, 1.0, 4.0, 25.0] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                stroke: Some(Ink::black()),
                stroke_weight: Some(3.0),
                stroke_join: Some(join),
                stroke_miter_limit: Some(limit),
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Local base".into(),
                stroke_join: Some(TextStrokeJoin::Miter),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Local".into(),
                based_on: Some("Local base".into()),
                ..Default::default()
            });
            let mut story = Story::from_text("aé中z", "Child");
            story.apply_style(1, 6, "Local");
            let spec = spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 300.0);
            for byte in [0, 1, 3, 6] {
                let stroke = spec.style_at(byte).stroke.unwrap();
                assert_eq!(
                    stroke.join,
                    if (1..6).contains(&byte) {
                        TextStrokeJoin::Miter
                    } else {
                        join
                    }
                );
                assert_eq!(stroke.miter_limit, limit);
                assert_eq!(stroke.width, 3.0);
            }
        }
    }
}
