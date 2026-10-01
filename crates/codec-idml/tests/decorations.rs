use schist_codec_idml::{container, export, import};
use schist_layout::{
    blank_a4,
    decorations::{DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStyle},
    CharacterStyle, Ink, ParagraphStyle,
};

#[test]
fn native_decorations_preserve_auto_inherit_and_all_paints_through_repeated_saves() {
    for paint in [
        None,
        Some(Paint::Text),
        Some(Paint::None),
        Some(Paint::Ink(Ink::spot("Proof / line", [55.0, 40.0, -35.0]))),
    ] {
        for measure in [
            None,
            Some(Measure::Auto),
            Some(Measure::Points(0.0)),
            Some(Measure::Points(0.75)),
        ] {
            let mut doc = blank_a4();
            let decoration = DecorationStyle {
                paint: paint.clone(),
                weight: measure,
                offset: measure,
                tint: Some(0.65),
                overprint: Some(true),
                stroke: Some(schist_layout::decorations::DecorationStroke::solid()),
                ..Default::default()
            };
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Decorated".into(),
                underline: Some(true),
                strikethrough: Some(false),
                underline_style: decoration.clone(),
                strike_style: decoration.clone(),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Local".into(),
                underline: Some(false),
                strikethrough: Some(true),
                underline_style: decoration.clone(),
                strike_style: decoration,
                ..Default::default()
            });
            let expected = doc.styles.clone();
            for _ in 0..4 {
                let written = export::write(&doc);
                assert!(
                    !written.warnings.iter().any(|w| w.contains("decoration")),
                    "{:?}",
                    written.warnings
                );
                let parts = container::read(&written.bytes).unwrap();
                let xml = parts.text("Resources/Styles.xml").unwrap();
                assert!(xml.contains(
                    "<UnderlineType type=\"object\">StrokeStyle/$ID/Solid</UnderlineType>"
                ));
                if measure == Some(Measure::Auto) {
                    assert!(xml.contains("UnderlineWeight=\"-9999\""));
                }
                let imported = import::read(&written.bytes).unwrap();
                assert!(
                    !imported
                        .report
                        .skipped
                        .iter()
                        .any(|w| w.contains("decoration")),
                    "{:?}",
                    imported.report.skipped
                );
                doc = imported.document;
                assert_eq!(
                    doc.styles.paragraph("Decorated"),
                    expected.paragraph("Decorated")
                );
                assert_eq!(doc.styles.character("Local"), expected.character("Local"));
            }
        }
    }
}

#[test]
fn malformed_dimensions_and_unknown_patterns_are_reported_while_gap_values_are_retained() {
    for tag in ["ParagraphStyle", "CharacterStyle"] {
        for value in ["-1", "NaN", "inf", "bogus"] {
            let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
            package.insert("Resources/Styles.xml", format!(r#"<idPkg:Styles><{tag} Self="Bad" Name="Bad" UnderlineWeight="{value}" StrikeThroughOffset="NaN" UnderlineGapTint="35" StrikeThroughGapOverprint="true"><Properties><UnderlineType type="object">StrokeStyle/Dots</UnderlineType><StrikeThroughGapColor type="object">Color/Black</StrikeThroughGapColor></Properties></{tag}></idPkg:Styles>"#).into_bytes());
            let imported = import::read(&container::write(&package.into_parts())).unwrap();
            assert_eq!(
                imported
                    .report
                    .skipped
                    .iter()
                    .filter(|w| w.contains("decoration"))
                    .count(),
                3,
                "{:?}",
                imported.report.skipped
            );
            for key in ["UnderlineWeight", "StrikeThroughOffset", "UnderlineType"] {
                assert!(imported.report.skipped.iter().any(|m| m.contains(key)));
            }
            let (underline, strike) = if tag == "ParagraphStyle" {
                let style = imported.document.styles.paragraph("Bad").unwrap();
                (&style.underline_style, &style.strike_style)
            } else {
                let style = imported.document.styles.character("Bad").unwrap();
                (&style.underline_style, &style.strike_style)
            };
            assert_eq!(underline.gap_tint, Some(0.35));
            assert_eq!(strike.gap_overprint, Some(true));
        }
    }
}

#[test]
fn local_native_decoration_properties_lower_once_and_keep_opaque_color_references() {
    use schist_layout::{authoring, History, Rect};
    use schist_text_engine::TextDecorationPattern;
    for (resource, pattern) in [
        (
            r#"<StripedStrokeStyle Self="opaque-pattern" Name="Native pattern" StripeArray="0 25 75 100"/>"#,
            TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
        ),
        (
            r#"<DashedStrokeStyle Self="opaque-pattern" Name="Native pattern" DashArray="6 3" EndCap="ButtEndCap" StrokeCornerAdjustment="None"/>"#,
            TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
        ),
        (
            r#"<DashedStrokeStyle Self="opaque-pattern" Name="Native pattern" DashArray="6 3" EndCap="RoundEndCap" StrokeCornerAdjustment="None"/>"#,
            TextDecorationPattern::Dashes(schist_text_engine::DecorationDashes {
                lengths: vec![6.0, 3.0],
                cap: schist_text_engine::DecorationCap::Round,
            }),
        ),
        (
            r#"<DashedStrokeStyle Self="opaque-pattern" Name="Native pattern" DashArray="6 3" EndCap="ProjectingEndCap" StrokeCornerAdjustment="None"/>"#,
            TextDecorationPattern::Dashes(schist_text_engine::DecorationDashes {
                lengths: vec![6.0, 3.0],
                cap: schist_text_engine::DecorationCap::Projecting,
            }),
        ),
        (
            r#"<DottedStrokeStyle Self="opaque-pattern" Name="Native pattern" DotArray="5 7" StrokeCornerAdjustment="None"/>"#,
            TextDecorationPattern::Dots(vec![5.0, 7.0]),
        ),
    ] {
        let mut doc = blank_a4();
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 100.0),
        )
        .unwrap();
        authoring::set_text(
            &mut doc,
            &mut History::default(),
            frame.story,
            "Local lines",
        );
        let package = container::read(&export::write(&doc).bytes).unwrap();
        let resources =
            schist_codec_idml::xml::parse(package.text("Resources/Graphic.xml").unwrap()).unwrap();
        let black = resources
            .find_all("Color")
            .into_iter()
            .find(|c| c.attr("Name") == Some("Black"))
            .unwrap()
            .attr("Self")
            .unwrap()
            .to_owned();
        let mut parts = package.into_parts();
        for (name, content) in &mut parts {
            if name.starts_with("Stories/") {
                let xml = String::from_utf8(content.clone()).unwrap()
                .replace("<CharacterStyleRange ", r#"<CharacterStyleRange Underline="true" UnderlineWeight="0.75" UnderlineOffset="-9999" UnderlineTint="65" UnderlineOverprint="true" UnderlineGapTint="35" UnderlineGapOverprint="false" StrikeThru="true" StrikeThroughWeight="2" "#)
                .replace("<Content>", &format!(r#"<Properties><UnderlineColor type="object">{black}</UnderlineColor><StrikeThroughColor type="string">Text Color</StrikeThroughColor><UnderlineType type="object">opaque-pattern</UnderlineType><UnderlineGapColor type="object">{black}</UnderlineGapColor></Properties><Content>"#));
                *content = xml.into_bytes();
            }
        }
        let (_, graphics) = parts
            .iter_mut()
            .find(|(name, _)| name == "Resources/Graphic.xml")
            .unwrap();
        *graphics = String::from_utf8(graphics.clone())
            .unwrap()
            .replace("</idPkg:Graphic>", &format!("{resource}</idPkg:Graphic>"))
            .into_bytes();
        doc = import::read(&container::write(&parts)).unwrap().document;
        let count = doc.styles.characters.len();
        for _ in 0..4 {
            let story = &doc.stories[0];
            let style = doc.styles.resolve_character(&story.ranges[0].style);
            assert_eq!(style.underline, Some(true));
            assert_eq!(style.strikethrough, Some(true));
            assert_eq!(style.underline_style.weight, Some(Measure::Points(0.75)));
            assert_eq!(style.underline_style.offset, Some(Measure::Auto));
            assert_eq!(style.underline_style.tint, Some(0.65));
            assert_eq!(style.underline_style.overprint, Some(true));
            assert_eq!(style.underline_style.gap_tint, Some(0.35));
            assert_eq!(style.underline_style.gap_overprint, Some(false));
            assert!(matches!(
                style.underline_style.gap_paint,
                Some(Paint::Ink(_))
            ));
            assert_eq!(
                style.underline_style.stroke.as_ref().unwrap().name,
                "Native pattern"
            );
            assert_eq!(
                style.underline_style.stroke.as_ref().unwrap().pattern,
                pattern
            );
            assert!(matches!(style.underline_style.paint, Some(Paint::Ink(_))));
            assert_eq!(style.strike_style.paint, Some(Paint::Text));
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.characters.len(), count);
        }
    }
}

#[test]
fn striped_resources_and_independent_gap_paints_survive_opaque_ids_and_repeated_saves() {
    use schist_layout::decorations::DecorationStroke;
    use schist_text_engine::TextDecorationPattern;
    for paint in [
        None,
        Some(Paint::None),
        Some(Paint::Text),
        Some(Paint::Ink(
            Ink::spot("Gap & spot", [60.0, 20.0, 30.0])
                .named_tint("Half gap", 0.5)
                .unwrap(),
        )),
    ] {
        let mut doc = blank_a4();
        let stroke = DecorationStroke {
            fitting: Default::default(),
            name: "Same / name & <".into(),
            pattern: TextDecorationPattern::Stripes(vec![0.0, 20.0, 75.0, 100.0]),
        };
        let unused = DecorationStroke {
            pattern: TextDecorationPattern::Stripes(vec![10.0, 90.0]),
            ..stroke.clone()
        };
        doc.styles.strokes = vec![unused.clone()];
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            underline_style: DecorationStyle {
                stroke: Some(stroke.clone()),
                gap_paint: paint.clone(),
                gap_tint: if paint
                    .as_ref()
                    .and_then(Paint::ink)
                    .is_some_and(|i| i.tint.is_some())
                {
                    None
                } else {
                    Some(0.5)
                },
                gap_overprint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            underline_style: DecorationStyle {
                weight: Some(Measure::Points(4.0)),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Reset".into(),
            underline_style: DecorationStyle {
                stroke: Some(DecorationStroke::solid()),
                gap_paint: Some(Paint::None),
                gap_overprint: Some(false),
                ..Default::default()
            },
            ..Default::default()
        });
        let paragraphs = doc.styles.paragraphs.clone();
        let characters = doc.styles.characters.clone();
        for pass in 0..4 {
            let written = export::write(&doc);
            assert_eq!(
                written.warnings,
                vec![schist_i18n::t("design.idml_style_limits").to_string()]
            );
            let mut package = container::read(&written.bytes).unwrap();
            if pass == 0 {
                // Resource identity never follows a naming convention.
                let graphics = package.text("Resources/Graphic.xml").unwrap();
                let root = schist_codec_idml::xml::parse(graphics).unwrap();
                let resources = root.find_all("StripedStrokeStyle");
                assert_eq!(resources.len(), 2);
                let replacements: Vec<_> = resources
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (e.attr("Self").unwrap().to_string(), format!("opaque{i}")))
                    .collect();
                let mut parts = package.into_parts();
                for (_, bytes) in &mut parts {
                    if let Ok(mut xml) = String::from_utf8(bytes.clone()) {
                        for (old, new) in &replacements {
                            xml = xml.replace(old, new);
                        }
                        *bytes = xml.into_bytes();
                    }
                }
                package = container::read(&container::write(&parts)).unwrap();
            }
            let imported = import::read(&container::write(&package.into_parts())).unwrap();
            assert!(
                imported.report.skipped.is_empty(),
                "{:?}",
                imported.report.skipped
            );
            doc = imported.document;
            assert_eq!(doc.styles.paragraphs, paragraphs);
            assert_eq!(doc.styles.characters, characters);
            assert_eq!(doc.styles.strokes.len(), 2);
            assert!(doc.styles.strokes.contains(&unused));
            let child = doc.styles.resolve_paragraph("Child");
            assert_eq!(child.underline_style.stroke, Some(stroke.clone()));
            assert_eq!(child.underline_style.gap_paint, paint);
        }
    }
}

#[test]
fn malformed_stripe_resources_are_reported_on_read_and_write_without_fake_solid_references() {
    use schist_layout::decorations::DecorationStroke;
    use schist_text_engine::TextDecorationPattern;
    for raw in [
        "",
        "0",
        "0 25 75",
        "0 20 10 100",
        "0 101",
        "-1 100",
        "0 NaN",
        "0 inf",
        "0 bad",
        "20 20",
    ] {
        let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
        package.insert("Resources/Graphic.xml", format!(r#"<idPkg:Graphic><StripedStrokeStyle Self="bad" Name="Invalid" StripeArray="{raw}"/></idPkg:Graphic>"#).into_bytes());
        package.insert("Resources/Styles.xml", br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Bad"><Properties><UnderlineType type="object">bad</UnderlineType></Properties></ParagraphStyle></idPkg:Styles>"#.to_vec());
        let imported = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(imported
            .report
            .skipped
            .iter()
            .any(|w| w.contains("StripeArray")));
        assert!(imported
            .report
            .skipped
            .iter()
            .any(|w| w.contains("UnderlineType")));
        assert!(imported
            .document
            .styles
            .paragraph("Bad")
            .unwrap()
            .underline_style
            .stroke
            .is_none());
        if let Ok(edges) = raw
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<Vec<f32>, _>>()
        {
            let mut doc = blank_a4();
            let stroke = DecorationStroke {
                fitting: Default::default(),
                name: "Invalid".into(),
                pattern: TextDecorationPattern::Stripes(edges),
            };
            doc.styles.strokes.push(stroke.clone());
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Bad".into(),
                underline_style: DecorationStyle {
                    stroke: Some(stroke),
                    ..Default::default()
                },
                ..Default::default()
            });
            let written = export::write(&doc);
            assert!(written.warnings.iter().any(|w| w.contains("StripeArray")));
            assert!(written.warnings.iter().any(|w| w.contains("UnderlineType")));
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("<UnderlineType"));
            assert!(!package
                .text("Resources/Graphic.xml")
                .unwrap()
                .contains("StripedStrokeStyle"));
        }
    }
}
