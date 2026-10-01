use schist_codec_idml::{container, export, import};
use schist_layout::{blank_a4, CharacterStyle, Ink, ParagraphStyle};

#[test]
fn native_text_strokes_and_explicit_no_ink_survive_inheritance_and_repeated_saves() {
    for width in [0.0, 0.25, 4.5] {
        for outside in [None, Some(false), Some(true)] {
            let mut doc = blank_a4();
            let ink = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
            doc.inks.push(ink.clone());
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                fill: Some(ink.clone()),
                stroke: Some(ink),
                stroke_weight: Some(width),
                stroke_outside: outside,
                stroke_tint: Some(0.45),
                overprint_stroke: Some(true),
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                fill_disabled: true,
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "No outline".into(),
                stroke_disabled: true,
                ..Default::default()
            });
            let expected = doc.styles.clone();
            for _ in 0..4 {
                let written = export::write(&doc);
                assert!(!written.warnings.iter().any(|w| w.contains("text stroke")));
                let parts = container::read(&written.bytes).unwrap();
                let xml = parts.text("Resources/Styles.xml").unwrap();
                assert!(xml.contains("FillColor=\"Swatch/None\""));
                assert!(xml.contains("StrokeColor=\"Swatch/None\""));
                doc = import::read(&written.bytes).unwrap().document;
                assert_eq!(doc.styles.paragraph("Base"), expected.paragraph("Base"));
                assert_eq!(doc.styles.paragraph("Child"), expected.paragraph("Child"));
                assert_eq!(
                    doc.styles.character("No outline"),
                    expected.character("No outline")
                );
                let resolved = doc.styles.resolve_paragraph("Child");
                assert!(resolved.fill_disabled);
                assert_eq!(resolved.stroke_weight, Some(width));
                assert_eq!(resolved.stroke_outside, outside);
            }
        }
    }
}

#[test]
fn invalid_weights_and_unknown_join_settings_are_reported_in_named_styles() {
    for value in ["-1", "NaN", "inf", "bogus"] {
        for tag in ["ParagraphStyle", "CharacterStyle"] {
            let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
            package.insert("Resources/Styles.xml", format!(r#"<idPkg:Styles><{tag} Self="Bad" Name="Bad" StrokeWeight="{value}" EndJoin="UnknownJoin" MiterLimit="NaN"/></idPkg:Styles>"#).into_bytes());
            let imported = import::read(&container::write(&package.into_parts())).unwrap();
            assert_eq!(
                imported
                    .report
                    .skipped
                    .iter()
                    .filter(|w| w.contains("text stroke"))
                    .count(),
                3
            );
            let weight = if tag == "ParagraphStyle" {
                imported
                    .document
                    .styles
                    .paragraph("Bad")
                    .unwrap()
                    .stroke_weight
            } else {
                imported
                    .document
                    .styles
                    .character("Bad")
                    .unwrap()
                    .stroke_weight
            };
            assert_eq!(weight, None);
        }
    }
}

#[test]
fn local_text_outline_properties_lower_to_stable_styles_without_losing_explicit_no_fill() {
    use schist_layout::{authoring, History, Rect};
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
        "Local outline",
    );
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let resources =
        schist_codec_idml::xml::parse(package.text("Resources/Graphic.xml").unwrap()).unwrap();
    let black = resources
        .find_all("Color")
        .into_iter()
        .find(|color| color.attr("Name") == Some("Black"))
        .unwrap()
        .attr("Self")
        .unwrap()
        .to_owned();
    let mut parts = package.into_parts();
    for (name, content) in &mut parts {
        if name.starts_with("Stories/") {
            let xml = String::from_utf8(content.clone()).unwrap().replace("<CharacterStyleRange ", &format!(r#"<CharacterStyleRange FillColor="Swatch/None" StrokeColor="{black}" StrokeWeight="2.75" StrokeAlignment="OutsideAlignment" EndJoin="RoundEndJoin" MiterLimit="7" StrokeTint="65" OverprintStroke="true" "#));
            *content = xml.into_bytes();
        }
    }
    doc = import::read(&container::write(&parts)).unwrap().document;
    let count = doc.styles.characters.len();
    for _ in 0..4 {
        let story = &doc.stories[0];
        assert!(!story.ranges.is_empty());
        let style = doc.styles.resolve_character(&story.ranges[0].style);
        assert!(style.fill_disabled);
        assert!(style.stroke.is_some());
        assert_eq!(style.stroke_weight, Some(2.75));
        assert_eq!(style.stroke_outside, Some(true));
        assert_eq!(
            style.stroke_join,
            Some(schist_text_engine::TextStrokeJoin::Round)
        );
        assert_eq!(style.stroke_miter_limit, Some(7.0));
        assert_eq!(style.stroke_tint, Some(0.65));
        assert_eq!(style.overprint_stroke, Some(true));
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.styles.characters.len(), count);
    }
}

#[test]
fn native_text_join_choices_and_inherited_miter_limits_survive_every_save() {
    use schist_text_engine::TextStrokeJoin;
    for (join, native) in [
        (TextStrokeJoin::Miter, "MiterEndJoin"),
        (TextStrokeJoin::Round, "RoundEndJoin"),
        (TextStrokeJoin::Bevel, "BevelEndJoin"),
    ] {
        for limit in [0.0, 0.5, 4.0, 16.0] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                stroke_join: Some(join),
                stroke_miter_limit: Some(limit),
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                stroke_join: Some(TextStrokeJoin::Miter),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Base".into(),
                stroke_join: Some(join),
                stroke_miter_limit: Some(limit),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                stroke_miter_limit: Some(0.0),
                ..Default::default()
            });
            let expected = doc.styles.clone();
            for _ in 0..4 {
                let written = export::write(&doc);
                assert!(!written.warnings.iter().any(|w| w.contains("text stroke")));
                let parts = container::read(&written.bytes).unwrap();
                assert!(parts
                    .text("Resources/Styles.xml")
                    .unwrap()
                    .contains(&format!("EndJoin=\"{native}\"")));
                let imported = import::read(&written.bytes).unwrap();
                assert!(!imported
                    .report
                    .skipped
                    .iter()
                    .any(|w| w.contains("text stroke")));
                doc = imported.document;
                for name in ["Base", "Child"] {
                    assert_eq!(doc.styles.paragraph(name), expected.paragraph(name));
                    assert_eq!(doc.styles.character(name), expected.character(name));
                }
                assert_eq!(
                    doc.styles.resolve_paragraph("Child").stroke_miter_limit,
                    Some(limit)
                );
                assert_eq!(
                    doc.styles.resolve_character("Child").stroke_join,
                    Some(join)
                );
            }
        }
    }
    for limit in [f32::NAN, f32::INFINITY, -1.0] {
        let mut doc = blank_a4();
        doc.styles.add_character(CharacterStyle {
            name: "Bad".into(),
            stroke_miter_limit: Some(limit),
            ..Default::default()
        });
        let written = export::write(&doc);
        assert!(written.warnings.iter().any(|w| w.contains("MiterLimit")));
        let reread = import::read(&written.bytes).unwrap();
        assert_eq!(
            reread
                .document
                .styles
                .character("Bad")
                .unwrap()
                .stroke_miter_limit,
            None
        );
    }
}
