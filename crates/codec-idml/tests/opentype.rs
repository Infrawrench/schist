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
        r#"<CharacterStyle Self="c" Name="Test" OTFHVKana="invalid" OTFProportionalMetrics="invalid"/>"#,
    );
    assert_eq!(
        read.report
            .skipped
            .iter()
            .filter(|s| s.contains("invalid"))
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

#[test]
fn native_directional_features_preserve_inheritance_and_explicit_resets_through_saves() {
    use schist_layout::{directional_features::DirectionalFeatures, ParagraphStyle};
    for kana in [None, Some(false), Some(true)] {
        for proportional_metrics in [None, Some(false), Some(true)] {
            let mut doc = schist_layout::blank_a4();
            let value = DirectionalFeatures {
                kana,
                proportional_metrics,
            };
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                directional_features: value,
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                directional_features: DirectionalFeatures {
                    kana: Some(false),
                    ..Default::default()
                },
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Base".into(),
                directional_features: value,
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                directional_features: DirectionalFeatures {
                    proportional_metrics: Some(false),
                    ..Default::default()
                },
                ..Default::default()
            });
            let expected = doc.styles.clone();
            for _ in 0..4 {
                let written = export::write(&doc);
                assert!(!written
                    .warnings
                    .iter()
                    .any(|w| w.contains("Custom features")));
                let read = import::read(&written.bytes).unwrap();
                assert!(!read.report.skipped.iter().any(|w| w.contains("OpenType")));
                doc = read.document;
                for name in ["Base", "Child"] {
                    assert_eq!(doc.styles.paragraph(name), expected.paragraph(name));
                    assert_eq!(doc.styles.character(name), expected.character(name));
                }
                assert_eq!(
                    doc.styles.resolve_paragraph("Child").directional_features,
                    DirectionalFeatures {
                        kana: Some(false),
                        proportional_metrics
                    }
                );
                assert_eq!(
                    doc.styles.resolve_character("Child").directional_features,
                    DirectionalFeatures {
                        kana,
                        proportional_metrics: Some(false)
                    }
                );
            }
        }
    }
}

#[test]
fn independent_cjk_tag_exceptions_are_reported_and_native_edits_win_per_group() {
    use schist_layout::directional_features::DirectionalFeatures;
    for kana in [None, Some(false), Some(true)] {
        let mut doc = schist_layout::blank_a4();
        let features = inherited_features(
            &[
                ("hkna".into(), false),
                ("vkna".into(), true),
                ("palt".into(), true),
                ("vpal".into(), false),
                ("liga".into(), true),
            ],
            &[],
        );
        doc.styles.add_character(CharacterStyle {
            name: "Exceptions".into(),
            directional_features: DirectionalFeatures {
                kana,
                proportional_metrics: Some(false),
            },
            features: features.clone(),
            ..Default::default()
        });
        for _ in 0..4 {
            let written = export::write(&doc);
            assert!(written
                .warnings
                .iter()
                .any(|w| w.contains("Custom features") && w.contains("palt")));
            doc = import::read(&written.bytes).unwrap().document;
            let style = doc.styles.character("Exceptions").unwrap();
            assert_eq!(style.features, features);
            assert_eq!(style.directional_features.kana, kana);
        }
        for new_value in [Some("true"), Some("false"), None] {
            if kana == new_value.map(|value| value == "true") {
                continue;
            }
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            let original = package.text("Resources/Styles.xml").unwrap();
            let replacement = new_value
                .map(|value| format!(" OTFHVKana=\"{value}\""))
                .unwrap_or_default();
            let edited = if let Some(kana) = kana {
                original.replace(&format!(" OTFHVKana=\"{kana}\""), &replacement)
            } else {
                original.replace(
                    "Name=\"Exceptions\"",
                    &format!("Name=\"Exceptions\"{replacement}"),
                )
            };
            package.insert("Resources/Styles.xml", edited.into_bytes());
            let read = import::read(&container::write(&package.into_parts())).unwrap();
            let style = read.document.styles.character("Exceptions").unwrap();
            assert_eq!(
                style.directional_features.kana,
                new_value.map(|value| value == "true")
            );
            assert_eq!(feature(&style.features, "hkna"), None);
            assert_eq!(feature(&style.features, "vkna"), None);
            assert_eq!(feature(&style.features, "palt"), Some(true));
            assert_eq!(feature(&style.features, "vpal"), Some(false));
            assert_eq!(feature(&style.features, "liga"), Some(true));
        }
    }
}

#[test]
fn local_native_cjk_flags_lower_to_stable_styles_without_losing_false() {
    use schist_layout::{authoring, History, Rect};
    let mut doc = schist_layout::blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 200.0, 100.0),
    )
    .unwrap();
    authoring::set_text(&mut doc, &mut History::default(), frame.story, "かなカナ");
    let mut parts = container::read(&export::write(&doc).bytes)
        .unwrap()
        .into_parts();
    for (name, content) in &mut parts {
        if name.starts_with("Stories/") {
            *content = String::from_utf8(content.clone())
                .unwrap()
                .replace(
                    "<CharacterStyleRange ",
                    r#"<CharacterStyleRange OTFHVKana="true" OTFProportionalMetrics="false" "#,
                )
                .into_bytes();
        }
    }
    doc = import::read(&container::write(&parts)).unwrap().document;
    let count = doc.styles.characters.len();
    for _ in 0..4 {
        let style = doc
            .styles
            .resolve_character(&doc.stories[0].ranges[0].style);
        assert_eq!(style.directional_features.kana, Some(true));
        assert_eq!(style.directional_features.proportional_metrics, Some(false));
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.styles.characters.len(), count);
    }
}
