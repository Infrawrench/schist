use schist_layout::{
    blank_a4,
    compose::spec_for,
    decorations::{DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStyle},
    CharacterStyle, Ink, ParagraphStyle, Story,
};

#[test]
fn decoration_resets_and_independent_properties_survive_every_inheritance_depth() {
    for depth in 1..7 {
        let mut doc = blank_a4();
        let red = Ink::cmyk("Red", [0.0, 1.0, 1.0, 0.0]);
        let cyan = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
        let base = DecorationStyle {
            paint: Some(Paint::Ink(red)),
            weight: Some(Measure::Points(3.5)),
            offset: Some(Measure::Points(8.0)),
            tint: Some(0.6),
            overprint: Some(true),
            ..Default::default()
        };
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            fill: Some(cyan.clone()),
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: base.clone(),
            strike_style: base.clone(),
            ..Default::default()
        });
        let mut last = "Base".to_string();
        for level in 0..depth {
            let name = format!("Level {level}");
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.clone(),
                based_on: Some(last),
                ..Default::default()
            });
            last = name;
        }
        doc.styles.add_character(CharacterStyle {
            name: "Local".into(),
            underline_style: DecorationStyle {
                paint: Some(Paint::Text),
                weight: Some(Measure::Auto),
                offset: Some(Measure::Auto),
                overprint: Some(false),
                ..Default::default()
            },
            strike_style: DecorationStyle {
                paint: Some(Paint::None),
                ..Default::default()
            },
            ..Default::default()
        });
        let resolved = doc.styles.resolve_paragraph(&last);
        assert_eq!(resolved.underline_style, base);
        let character = resolved.character(doc.styles.resolve_character("Default"));
        let local = doc
            .styles
            .resolve_character("Local")
            .with_paint_defaults(&character);
        assert_eq!(local.underline_style.weight, Some(Measure::Auto));
        assert_eq!(local.underline_style.offset, Some(Measure::Auto));
        assert_eq!(
            local.underline_style.paint(&local),
            Some((cyan, 0.6, false))
        );
        assert_eq!(local.strike_style.paint(&local), None);
        let mut story = Story::from_text("aé中z", &last);
        story.apply_style(1, 6, "Local");
        let spec = spec_for(&story, 0, 7, &doc.styles, &last, "Default", 300.0);
        for byte in [0, 1, 3, 6] {
            let local = (1..6).contains(&byte);
            let run = spec.style_at(byte);
            assert!(run.underline && run.strikethrough);
            assert_eq!(
                run.underline_style.weight,
                if local { None } else { Some(3.5) }
            );
            assert_eq!(
                run.underline_style.offset,
                if local { None } else { Some(8.0) }
            );
            assert_eq!(run.strike_style.disabled, local);
        }
    }
}

#[test]
fn explicit_decoration_color_remains_visible_without_glyph_fill_and_legacy_styles_inherit() {
    let mut doc = blank_a4();
    let ink = Ink::black();
    doc.styles.add_character(CharacterStyle {
        name: "Line only".into(),
        fill_disabled: true,
        underline_style: DecorationStyle {
            paint: Some(Paint::Ink(ink.clone())),
            ..Default::default()
        },
        strike_style: DecorationStyle {
            paint: Some(Paint::Text),
            ..Default::default()
        },
        ..Default::default()
    });
    let resolved = doc.styles.resolve_character("Line only");
    assert_eq!(
        resolved.underline_style.paint(&resolved),
        Some((ink, 1.0, false))
    );
    assert!(resolved.strike_style.paint(&resolved).is_none());
    let mut json = serde_json::to_value(doc.styles.character("Line only").unwrap()).unwrap();
    for field in ["underline_style", "strike_style"] {
        json.as_object_mut().unwrap().remove(field);
    }
    let old: CharacterStyle = serde_json::from_value(json).unwrap();
    assert_eq!(old.underline_style, DecorationStyle::default());
    assert_eq!(old.strike_style, DecorationStyle::default());
}

#[test]
fn named_decoration_tints_detach_on_nearer_percent_and_swatch_changes_undo_together() {
    let mut doc = blank_a4();
    let base = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
    let named = base.named_tint("Quarter", 0.25).unwrap();
    doc.inks.extend([base.clone(), named.clone()]);
    let decoration = DecorationStyle {
        paint: Some(Paint::Ink(named.clone())),
        gap_paint: Some(Paint::Ink(named)),
        ..Default::default()
    };
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Base".into(),
        underline_style: decoration.clone(),
        strike_style: decoration.clone(),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Base".into(),
        underline_style: decoration.clone(),
        strike_style: decoration,
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Child".into(),
        based_on: Some("Base".into()),
        underline_style: DecorationStyle {
            tint: Some(0.7),
            gap_tint: Some(0.7),
            ..Default::default()
        },
        ..Default::default()
    });
    let resolved = doc.styles.resolve_character("Child");
    assert_eq!(
        resolved.underline_style.paint(&resolved),
        Some((base.clone(), 0.7, false))
    );
    assert_eq!(
        resolved.underline_style.gap_paint(&resolved),
        Some((base.clone(), 0.7, false))
    );
    assert_eq!(
        resolved
            .strike_style
            .paint(&resolved)
            .unwrap()
            .0
            .tint_amount(),
        0.25
    );
    let before = doc.clone();
    let mut history = schist_layout::History::default();
    let changed = Ink::cmyk("Cyan", [0.5, 0.0, 0.0, 0.0]);
    assert!(schist_layout::swatches::replace(
        &mut doc,
        &mut history,
        &base,
        changed.clone()
    ));
    assert_eq!(history.undo_depth(), 1);
    let para = doc
        .styles
        .resolve_paragraph("Base")
        .character(doc.styles.resolve_character("Default"));
    let character = doc.styles.resolve_character("Base");
    for style in [&para, &character] {
        for decoration in [&style.underline_style, &style.strike_style] {
            for ink in [
                decoration.paint(style).unwrap().0,
                decoration.gap_paint(style).unwrap().0,
            ] {
                assert_eq!(ink.base_color().as_ref(), &changed);
                assert_eq!(ink.tint_amount(), 0.25);
            }
        }
    }
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
}

#[test]
fn stroke_and_gap_properties_inherit_independently_and_explicit_solid_resets_them() {
    use schist_layout::decorations::DecorationStroke;
    use schist_text_engine::TextDecorationPattern;
    for depth in 1..6 {
        let mut doc = blank_a4();
        let stripe = DecorationStroke {
            fitting: Default::default(),
            name: "Two lines".into(),
            pattern: TextDecorationPattern::Stripes(vec![0.0, 20.0, 60.0, 100.0]),
        };
        let gap = Ink::spot("Gap", [60.0, 20.0, 30.0]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: DecorationStyle {
                stroke: Some(stripe.clone()),
                gap_paint: Some(Paint::Ink(gap.clone())),
                gap_tint: Some(0.3),
                gap_overprint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut parent = "Base".to_string();
        for level in 0..depth {
            let name = format!("Level {level}");
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.clone(),
                based_on: Some(parent),
                underline_style: DecorationStyle {
                    weight: Some(Measure::Points(level as f32 + 1.0)),
                    ..Default::default()
                },
                ..Default::default()
            });
            parent = name;
        }
        let base = doc
            .styles
            .resolve_paragraph(&parent)
            .character(doc.styles.resolve_character("Default"));
        assert_eq!(base.underline_style.stroke, Some(stripe.clone()));
        assert_eq!(
            base.underline_style.gap_paint(&base),
            Some((gap.clone(), 0.3, true))
        );
        for paint in [Paint::None, Paint::Text] {
            let local = DecorationStyle {
                stroke: Some(DecorationStroke::solid()),
                gap_paint: Some(paint.clone()),
                gap_overprint: Some(false),
                ..Default::default()
            }
            .over(&base.underline_style);
            assert_eq!(local.stroke, Some(DecorationStroke::solid()));
            assert_eq!(local.gap_tint, Some(0.3));
            assert_eq!(local.gap_paint(&base).is_none(), paint == Paint::None);
            assert!(!local.gap_paint(&base).is_some_and(|p| p.2));
        }
        assert!(doc.all_inks().contains(&gap));
        doc.styles.strokes.extend([stripe.clone(), stripe.clone()]);
        assert_eq!(doc.all_decoration_strokes(), vec![stripe]);
    }
}
