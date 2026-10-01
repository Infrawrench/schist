use schist_codec_idml::{container, export, import};
use schist_layout::{
    blank_a4,
    decorations::{DecorationStroke, DecorationStyle},
    ParagraphStyle,
};
use schist_text_engine::TextDecorationPattern;

#[test]
fn native_dot_intervals_keep_names_unused_resources_and_inherited_properties_across_every_save() {
    for intervals in [
        vec![6.0],
        vec![0.0, 6.0],
        vec![5.554054, 6.445946],
        vec![1.0, 2.0, 3.0, 4.0, 5.0],
    ] {
        let mut doc = blank_a4();
        let stroke = DecorationStroke {
            fitting: Default::default(),
            name: "Dots / custom & native".into(),
            pattern: TextDecorationPattern::Dots(intervals),
        };
        let unused = DecorationStroke {
            fitting: Default::default(),
            name: stroke.name.clone(),
            pattern: TextDecorationPattern::Dots(vec![4.0]),
        };
        doc.styles.strokes.push(unused.clone());
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            underline_style: DecorationStyle {
                stroke: Some(stroke.clone()),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            ..Default::default()
        });
        for _ in 0..4 {
            let written = export::write(&doc);
            assert!(
                !written.warnings.iter().any(|w| w.contains("decoration")),
                "{:?}",
                written.warnings
            );
            let package = container::read(&written.bytes).unwrap();
            let graphics = package.text("Resources/Graphic.xml").unwrap();
            let xml = schist_codec_idml::xml::parse(graphics).unwrap();
            assert_eq!(xml.find_all("DottedStrokeStyle").len(), 2);
            assert!(graphics.contains(r#"StrokeCornerAdjustment="None""#));
            let read = import::read(&written.bytes).unwrap();
            assert!(read.report.skipped.is_empty(), "{:?}", read.report.skipped);
            doc = read.document;
            assert_eq!(
                doc.styles.resolve_paragraph("Child").underline_style.stroke,
                Some(stroke.clone())
            );
            assert_eq!(doc.styles.strokes.len(), 2);
            assert!(doc.styles.strokes.contains(&unused));
        }
    }
}

#[test]
fn malformed_dot_arrays_and_unsupported_fitting_are_reported_without_fake_solid_definitions() {
    for (array, adjustment, property) in [
        ("", "None", "DotArray"),
        ("0", "None", "DotArray"),
        ("1 -2", "None", "DotArray"),
        ("NaN", "None", "DotArray"),
        ("1 2 3 4 5 6", "None", "DotArray"),
        ("6", "Unknown", "StrokeCornerAdjustment"),
        ("6", "Dashes", "StrokeCornerAdjustment"),
    ] {
        let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
        package.insert("Resources/Graphic.xml",format!(r#"<idPkg:Graphic><DottedStrokeStyle Self="opaque" Name="Dot" DotArray="{array}" StrokeCornerAdjustment="{adjustment}"/></idPkg:Graphic>"#).into_bytes());
        package.insert("Resources/Styles.xml",br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Using"><Properties><UnderlineType type="object">opaque</UnderlineType></Properties></ParagraphStyle></idPkg:Styles>"#.to_vec());
        let read = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(
            read.report.skipped.iter().any(|w| w.contains(property)),
            "{:?}",
            read.report.skipped
        );
        assert!(read
            .report
            .skipped
            .iter()
            .any(|w| w.contains("UnderlineType")));
        assert!(read.document.styles.strokes.is_empty());
        assert!(read
            .document
            .styles
            .resolve_paragraph("Using")
            .underline_style
            .stroke
            .is_none());
        if adjustment == "None" {
            if let Ok(values) = array
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<f32>, _>>()
            {
                let mut doc = blank_a4();
                let stroke = DecorationStroke {
                    fitting: Default::default(),
                    name: "Bad".into(),
                    pattern: TextDecorationPattern::Dots(values),
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
                assert!(written.warnings.iter().any(|w| w.contains("DotArray")));
                let package = container::read(&written.bytes).unwrap();
                assert!(!package
                    .text("Resources/Graphic.xml")
                    .unwrap()
                    .contains("DottedStrokeStyle"));
                assert!(!package
                    .text("Resources/Styles.xml")
                    .unwrap()
                    .contains("<UnderlineType"));
            }
        }
    }
}
