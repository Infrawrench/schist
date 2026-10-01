use schist_codec_idml::{container, export, import};
use schist_layout::{blank_a4, decorations::DecorationStroke};
use schist_text_engine::{DecorationFit, TextDecorationPattern};

#[test]
fn opaque_fitting_resources_keep_distinct_identity_inheritance_and_preview_settings_on_every_save()
{
    for (tag, array, values) in [
        ("DashedStrokeStyle", "DashArray", "6 3 2 4"),
        ("DottedStrokeStyle", "DotArray", "5 7"),
    ] {
        let mut package = container::read(&export::write(&blank_a4()).bytes).unwrap();
        let modes = [
            ("None", DecorationFit::None),
            ("Dashes", DecorationFit::Dashes),
            ("Gaps", DecorationFit::Gaps),
            ("DashesAndGaps", DecorationFit::DashesAndGaps),
        ];
        let modes: Vec<_> = modes
            .into_iter()
            .filter(|(name, _)| tag != "DottedStrokeStyle" || *name != "Dashes")
            .collect();
        let resources: String = modes.iter().map(|(mode,_)| format!(r#"<{tag} Self="opaque-{mode}" Name="Same name" {array}="{values}" StrokeCornerAdjustment="{mode}"/>"#)).collect();
        package.insert(
            "Resources/Graphic.xml",
            format!("<idPkg:Graphic>{resources}</idPkg:Graphic>").into_bytes(),
        );
        let styles: String = modes.iter().map(|(mode,_)| format!(r#"<ParagraphStyle Self="p-{mode}" Name="{mode}"><Properties><UnderlineType type="object">opaque-{mode}</UnderlineType></Properties></ParagraphStyle><ParagraphStyle Self="child-{mode}" Name="Child {mode}"><Properties><BasedOn type="object">p-{mode}</BasedOn></Properties></ParagraphStyle>"#)).collect();
        package.insert(
            "Resources/Styles.xml",
            format!("<idPkg:Styles>{styles}</idPkg:Styles>").into_bytes(),
        );
        let read = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(read.report.skipped.is_empty(), "{:?}", read.report.skipped);
        let mut doc = read.document;
        for _ in 0..4 {
            assert_eq!(doc.styles.strokes.len(), modes.len());
            for (mode, fitting) in &modes {
                let stroke = doc
                    .styles
                    .resolve_paragraph(&format!("Child {mode}"))
                    .underline_style
                    .stroke
                    .unwrap();
                assert_eq!(stroke.name, "Same name");
                assert_eq!(stroke.fitting, *fitting);
                assert!(stroke.valid());
                assert_eq!(
                    schist_layout::decorations::DecorationStyle {
                        stroke: Some(stroke),
                        ..Default::default()
                    }
                    .preview(&Default::default())
                    .fitting,
                    *fitting
                );
            }
            let write = export::write(&doc);
            assert!(
                write
                    .warnings
                    .iter()
                    .all(|w| w == schist_i18n::t("design.idml_style_limits")),
                "{:?}",
                write.warnings
            );
            let read = import::read(&write.bytes).unwrap();
            assert!(read.report.skipped.is_empty(), "{:?}", read.report.skipped);
            doc = read.document;
        }
    }
}

#[test]
fn legacy_strokes_default_to_no_fit_and_unsupported_combinations_are_not_fake_native_styles() {
    let old: DecorationStroke =
        serde_json::from_str(r#"{"name":"Old","pattern":{"Dashes":[6,3]}}"#).unwrap();
    assert_eq!(old.fitting, DecorationFit::None);
    assert!(old.valid());
    for pattern in [
        TextDecorationPattern::Solid,
        TextDecorationPattern::Stripes(vec![0.0, 50.0]),
        TextDecorationPattern::Dots(vec![6.0]),
    ] {
        let stroke = DecorationStroke {
            name: "Invalid fitting".into(),
            pattern,
            fitting: DecorationFit::Dashes,
        };
        assert!(!stroke.valid());
        let mut doc = blank_a4();
        doc.styles.strokes.push(stroke);
        let write = export::write(&doc);
        assert!(!write.warnings.is_empty());
        let package = container::read(&write.bytes).unwrap();
        assert!(!package
            .text("Resources/Graphic.xml")
            .unwrap()
            .contains("Invalid fitting"));
    }
}
