use schist_codec_idml::{container, export, import, xml};
use schist_layout::styles::Leading;

fn with_styles(styles: &str) -> import::Imported {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    package.insert(
        "Resources/Styles.xml",
        format!("<idPkg:Styles>{styles}</idPkg:Styles>").into_bytes(),
    );
    import::read(&container::write(&package.into_parts())).unwrap()
}

#[test]
fn native_auto_leading_resets_fixed_inheritance_and_stays_distinct_on_repeated_saves() {
    for (value, kind, expected) in [
        ("Auto", "enumeration", Leading::Auto),
        ("0", "unit", Leading::Points(0.0)),
        ("23.5", "unit", Leading::Points(23.5)),
    ] {
        for percent in [0.0, 120.0, 145.5, 500.0] {
            let mut doc = with_styles(&format!(r#"
                <RootParagraphStyleGroup>
                  <ParagraphStyle Self="p" Name="Parent" PointSize="20" AutoLeading="{percent}"><Properties><Leading type="unit">40</Leading></Properties></ParagraphStyle>
                  <ParagraphStyle Self="c" Name="Child"><Properties><BasedOn type="object">p</BasedOn><Leading type="{kind}">{value}</Leading></Properties></ParagraphStyle>
                  <ParagraphStyle Self="g" Name="Grandchild"><Properties><BasedOn type="object">c</BasedOn></Properties></ParagraphStyle>
                </RootParagraphStyleGroup>
                <RootCharacterStyleGroup>
                  <CharacterStyle Self="a" Name="Character parent"><Properties><Leading type="unit">40</Leading></Properties></CharacterStyle>
                  <CharacterStyle Self="b" Name="Character child"><Properties><BasedOn type="object">a</BasedOn><Leading type="{kind}">{value}</Leading></Properties></CharacterStyle>
                </RootCharacterStyleGroup>"#)).document;
            for _ in 0..4 {
                assert_eq!(
                    doc.styles.resolve_paragraph("Grandchild").leading,
                    Some(expected)
                );
                assert_eq!(
                    doc.styles.resolve_paragraph("Grandchild").auto_leading,
                    Some(percent)
                );
                assert_eq!(doc.styles.paragraph("Grandchild").unwrap().leading, None);
                assert_eq!(
                    doc.styles.resolve_character("Character child").leading,
                    Some(expected)
                );
                let written = export::write(&doc);
                assert!(!written.warnings.iter().any(|w| w.contains("leading")));
                let package = container::read(&written.bytes).unwrap();
                let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                let child = root
                    .find_all("ParagraphStyle")
                    .into_iter()
                    .find(|s| s.attr("Name") == Some("Child"))
                    .unwrap();
                let leading = child.find_all("Leading")[0];
                assert_eq!(leading.attr("type"), Some(kind));
                doc = import::read(&written.bytes).unwrap().document;
            }
        }
    }
}

#[test]
fn malformed_leading_is_reported_without_serializing_nonfinite_geometry() {
    for value in ["-1", "NaN", "Infinity", "Bogus"] {
        let imported = with_styles(&format!(
            r#"<RootParagraphStyleGroup><ParagraphStyle Self="p" Name="Bad" AutoLeading="{value}"><Properties><Leading type="unit">{value}</Leading></Properties></ParagraphStyle></RootParagraphStyleGroup>"#
        ));
        assert_eq!(
            imported.document.styles.paragraph("Bad").unwrap().leading,
            None
        );
        assert_eq!(
            imported
                .document
                .styles
                .paragraph("Bad")
                .unwrap()
                .auto_leading,
            None
        );
        assert!(
            imported
                .report
                .skipped
                .iter()
                .filter(|w| w.contains("leading"))
                .count()
                >= 2
        );
    }
    for value in [-1.0, f32::NAN, f32::INFINITY, 501.0] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.paragraphs[0].leading = Some(Leading::Points(value));
        doc.styles.paragraphs[0].auto_leading = Some(value);
        let written = export::write(&doc);
        assert!(written.warnings.iter().any(|w| w.contains("leading")));
        let reread = import::read(&written.bytes).unwrap().document;
        assert_eq!(reread.styles.paragraphs[0].auto_leading, None);
        if !value.is_finite() || value < 0.0 {
            assert_eq!(reread.styles.paragraphs[0].leading, None);
        }
    }
}

#[test]
fn published_automatic_and_fixed_leading_survive_without_anonymous_style_growth() {
    for bytes in [
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
    ] {
        let mut doc = import::read(bytes).unwrap().document;
        assert!(doc
            .styles
            .paragraphs
            .iter()
            .any(|s| s.leading == Some(Leading::Auto)));
        assert!(doc
            .styles
            .paragraphs
            .iter()
            .any(|s| s.auto_leading == Some(120.0)));
        let original = doc.styles.clone();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.paragraphs.len(), original.paragraphs.len());
            assert_eq!(doc.styles.characters.len(), original.characters.len());
            for style in &original.paragraphs {
                let actual = doc.styles.paragraph(&style.name).unwrap();
                assert_eq!(actual.leading, style.leading);
                assert_eq!(actual.auto_leading, style.auto_leading);
            }
            for style in &original.characters {
                assert_eq!(
                    doc.styles.character(&style.name).unwrap().leading,
                    style.leading
                );
            }
        }
    }
}
