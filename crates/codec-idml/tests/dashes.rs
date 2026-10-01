use schist_codec_idml::{container, export, import};
use schist_layout::{
    blank_a4,
    decorations::{DecorationStroke, DecorationStyle},
    ParagraphStyle,
};
use schist_text_engine::TextDecorationPattern;

#[test]
fn native_dash_resources_and_inherited_lengths_survive_every_save() {
    for (cap, native) in [
        (schist_text_engine::DecorationCap::Butt, "ButtEndCap"),
        (schist_text_engine::DecorationCap::Round, "RoundEndCap"),
        (
            schist_text_engine::DecorationCap::Projecting,
            "ProjectingEndCap",
        ),
    ] {
        for values in [
            vec![6.0, 3.0],
            vec![0.0, 2.0],
            vec![2.0, 0.0],
            vec![1.5, 0.75, 8.0, 2.0, 3.0, 5.0, 1.0, 2.0, 1.0, 1.0],
        ] {
            let mut doc = blank_a4();
            let stroke = DecorationStroke {
                fitting: Default::default(),
                name: "Dash / custom & native".into(),
                pattern: TextDecorationPattern::Dashes(schist_text_engine::DecorationDashes {
                    lengths: values,
                    cap,
                }),
            };
            let mut unused = stroke.clone();
            if let TextDecorationPattern::Dashes(dashes) = &mut unused.pattern {
                dashes.cap = if cap == schist_text_engine::DecorationCap::Butt {
                    schist_text_engine::DecorationCap::Round
                } else {
                    schist_text_engine::DecorationCap::Butt
                };
            }
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
                assert_eq!(
                    schist_codec_idml::xml::parse(graphics)
                        .unwrap()
                        .find_all("DashedStrokeStyle")
                        .len(),
                    2,
                    "same-named cap variants retain separate identities"
                );
                assert!(graphics.contains(&format!(
                    r#"EndCap="{native}" StrokeCornerAdjustment="None""#
                )));
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
                    doc.styles.resolve_paragraph("Child").underline_style.stroke,
                    Some(stroke.clone())
                );
                assert_eq!(doc.styles.strokes.len(), 2);
                assert!(doc.styles.strokes.contains(&stroke));
                assert!(doc.styles.strokes.contains(&unused));
            }
        }
    }
}

#[test]
fn unknown_caps_corner_fitting_and_invalid_cycles_are_never_silently_treated_as_supported() {
    for (attribute, value) in [
        ("DashArray", "0 0"),
        ("DashArray", "1 2 3"),
        ("DashArray", "1 NaN"),
        ("DashArray", "1 -2"),
        ("DashArray", "1 1 1 1 1 1 1 1 1 1 1 1"),
        ("EndCap", "UnknownCap"),
        ("StrokeCornerAdjustment", "Unknown"),
    ] {
        let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
        let attributes = [
            ("DashArray", "6 3"),
            ("EndCap", "ButtEndCap"),
            ("StrokeCornerAdjustment", "None"),
        ]
        .into_iter()
        .map(|(key, default)| {
            format!(
                r#" {key}="{}""#,
                if key == attribute { value } else { default }
            )
        })
        .collect::<String>();
        package.insert("Resources/Graphic.xml", format!(r#"<idPkg:Graphic><DashedStrokeStyle Self="opaque" Name="Dash"{attributes}/></idPkg:Graphic>"#).into_bytes());
        package.insert("Resources/Styles.xml", br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Using"><Properties><UnderlineType type="object">opaque</UnderlineType></Properties></ParagraphStyle></idPkg:Styles>"#.to_vec());
        let imported = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(
            imported
                .report
                .skipped
                .iter()
                .any(|w| w.contains(attribute)),
            "{:?}",
            imported.report.skipped
        );
        assert!(imported
            .report
            .skipped
            .iter()
            .any(|w| w.contains("UnderlineType")));
        assert!(imported
            .document
            .styles
            .paragraph("Using")
            .unwrap()
            .underline_style
            .stroke
            .is_none());
    }
}
