use schist_codec_idml::{container, export, import, xml};
use schist_layout::{styles::inherited_features, CharacterStyle};

const SWITCHES: &[(&str, &str)] = &[
    ("Ligatures", "liga"),
    ("OTFDiscretionaryLigature", "dlig"),
    ("OTFContextualAlternate", "calt"),
    ("OTFFraction", "frac"),
    ("OTFOrdinal", "ordn"),
    ("OTFTitling", "titl"),
    ("OTFSwash", "swsh"),
    ("OTFSlashedZero", "zero"),
    ("OTFHistorical", "hist"),
    ("OTFMark", "mark"),
    ("OTFLocale", "locl"),
    ("OTFStylisticAlternate", "salt"),
    ("OTFJustificationAlternate", "jalt"),
    ("OTFStretchedAlternate", "stch"),
    ("OTFOverlapSwash", "cswh"),
    ("OTFRomanItalics", "ital"),
];
fn document_with_styles(styles: &str) -> import::Imported {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    package.insert(
        "Resources/Styles.xml",
        format!("<idPkg:Styles>{styles}</idPkg:Styles>").into_bytes(),
    );
    import::read(&container::write(&package.into_parts())).unwrap()
}
fn feature(features: &[(String, bool)], tag: &str) -> Option<bool> {
    features
        .iter()
        .find(|(name, _)| name == tag)
        .map(|(_, enabled)| *enabled)
}

#[test]
fn native_switches_figures_and_complete_set_masks_keep_explicit_resets_on_every_save() {
    for enabled in [false, true] {
        for figure in [
            "Default",
            "TabularLining",
            "ProportionalOldstyle",
            "ProportionalLining",
            "TabularOldstyle",
        ] {
            for mask in [0, 1, 1 << 19, (1 << 20) - 1] {
                let attrs = SWITCHES
                    .iter()
                    .map(|(property, _)| format!(" {property}=\"{enabled}\""))
                    .collect::<String>();
                let mut doc = document_with_styles(&format!(r#"
                    <RootParagraphStyleGroup><ParagraphStyle Self="p" Name="Base" {attrs} OTFFigureStyle="{figure}" OTFStylisticSets="{mask}"/>
                    <ParagraphStyle Self="p2" Name="Child"><Properties><BasedOn type="object">p</BasedOn></Properties></ParagraphStyle></RootParagraphStyleGroup>
                    <RootCharacterStyleGroup><CharacterStyle Self="c" Name="Char" {attrs} OTFFigureStyle="{figure}" OTFStylisticSets="{mask}"/>
                    <CharacterStyle Self="c2" Name="Reset" Ligatures="false" OTFStylisticSets="0"><Properties><BasedOn type="object">c</BasedOn></Properties></CharacterStyle></RootCharacterStyleGroup>"#)).document;
                let expected = doc.styles.clone();
                for _ in 0..3 {
                    for (_, tag) in SWITCHES {
                        assert_eq!(
                            feature(&doc.styles.resolve_paragraph("Child").features, tag),
                            Some(enabled)
                        );
                    }
                    for i in 1..=20 {
                        assert_eq!(
                            feature(
                                &doc.styles.resolve_character("Char").features,
                                &format!("ss{i:02}")
                            ),
                            Some(mask & (1 << (i - 1)) != 0)
                        );
                        assert_eq!(
                            feature(
                                &doc.styles.resolve_character("Reset").features,
                                &format!("ss{i:02}")
                            ),
                            Some(false)
                        );
                    }
                    assert_eq!(
                        feature(&doc.styles.resolve_character("Reset").features, "liga"),
                        Some(false)
                    );
                    let written = export::write(&doc);
                    assert!(!written
                        .warnings
                        .iter()
                        .any(|w| w.contains("Custom features")));
                    let package = container::read(&written.bytes).unwrap();
                    let styles =
                        std::str::from_utf8(package.get("Resources/Styles.xml").unwrap()).unwrap();
                    let root = xml::parse(styles).unwrap();
                    let native = root
                        .find_all("CharacterStyle")
                        .into_iter()
                        .find(|s| s.attr("Name") == Some("Char"))
                        .unwrap();
                    assert_eq!(native.attr("OTFFigureStyle"), Some(figure));
                    assert_eq!(
                        native.attr("OTFStylisticSets"),
                        Some(mask.to_string().as_str())
                    );
                    assert!(!styles.contains("Schist.OpenTypeFeatures.v1"));
                    doc = import::read(&written.bytes).unwrap().document;
                    assert_eq!(
                        doc.styles.resolve_paragraph("Child").features,
                        expected.resolve_paragraph("Child").features
                    );
                    assert_eq!(
                        doc.styles.resolve_character("Char").features,
                        expected.resolve_character("Char").features
                    );
                    assert!(doc.styles.paragraph("Child").unwrap().features.is_empty());
                }
            }
        }
    }
}

#[test]
fn partial_sets_and_arbitrary_tags_preserve_inheritance_with_an_honest_native_notice() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_character(CharacterStyle {
        name: "Base".into(),
        features: vec![
            ("ss01".into(), true),
            ("liga".into(), true),
            ("cv05".into(), false),
        ],
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Child".into(),
        based_on: Some("Base".into()),
        features: vec![("ss02".into(), true), ("liga".into(), false)],
        ..Default::default()
    });
    let expected = doc.styles.resolve_character("Child").features;
    for _ in 0..4 {
        let written = export::write(&doc);
        assert!(written
            .warnings
            .iter()
            .any(|w| w.contains("ss01") && w.contains("cv05")));
        assert!(written.warnings.iter().any(|w| w.contains("ss02")));
        let package = container::read(&written.bytes).unwrap();
        let styles = std::str::from_utf8(package.get("Resources/Styles.xml").unwrap()).unwrap();
        assert!(!styles.contains("OTFStylisticSets="));
        assert!(styles.contains("Ligatures=\"false\""));
        doc = import::read(&written.bytes).unwrap().document;
        assert_eq!(doc.styles.resolve_character("Child").features, expected);
    }
    // An external application can add a native mask while retaining unknown
    // Labels. That explicit edit wins over the preserved partial set values.
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let styles = String::from_utf8(package.get("Resources/Styles.xml").unwrap().to_vec()).unwrap();
    package.insert(
        "Resources/Styles.xml",
        styles
            .replace("Name=\"Base\"", "Name=\"Base\" OTFStylisticSets=\"2\"")
            .into_bytes(),
    );
    let doc = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert_eq!(
        feature(&doc.styles.resolve_character("Base").features, "ss01"),
        Some(false)
    );
    assert_eq!(
        feature(&doc.styles.resolve_character("Base").features, "ss02"),
        Some(true)
    );
}

#[test]
fn invalid_native_features_report_each_property_without_poisoning_valid_values() {
    for invalid in ["NaN", "-1", "1048576", "garbage"] {
        let read = document_with_styles(&format!(
            r#"<CharacterStyle Self="c" Name="Test" Ligatures="{invalid}" OTFFigureStyle="{invalid}" OTFStylisticSets="{invalid}" OTFSlashedZero="true"/>"#
        ));
        assert_eq!(
            read.report
                .skipped
                .iter()
                .filter(|s| s.contains(invalid))
                .count(),
            3
        );
        assert_eq!(
            read.document.styles.character("Test").unwrap().features,
            vec![("zero".into(), true)]
        );
    }
    let read = document_with_styles(
        r#"<CharacterStyle Self="c" Name="Test" OTFHVKana="true" OTFProportionalMetrics="true"/>"#,
    );
    assert_eq!(
        read.report
            .skipped
            .iter()
            .filter(|s| s.contains("Unsupported OpenType"))
            .count(),
        2
    );
}

#[test]
fn real_native_feature_defaults_and_local_resets_remain_stable_without_style_growth() {
    for bytes in [
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
    ] {
        let mut doc = import::read(bytes).unwrap().document;
        assert!(doc
            .styles
            .paragraphs
            .iter()
            .any(|s| feature(&s.features, "liga") == Some(true)));
        let original = doc.styles.clone();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.paragraphs.len(), original.paragraphs.len());
            assert_eq!(doc.styles.characters.len(), original.characters.len());
            for style in &original.paragraphs {
                assert_eq!(
                    doc.styles.paragraph(&style.name).unwrap().features,
                    inherited_features(&style.features, &[])
                );
            }
            for style in &original.characters {
                assert_eq!(
                    doc.styles.character(&style.name).unwrap().features,
                    inherited_features(&style.features, &[])
                );
            }
        }
    }
}
