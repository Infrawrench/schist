use schist_layout::{compose, styles::inherited_features, CharacterStyle, ParagraphStyle, Story};

#[test]
fn feature_inheritance_is_independent_per_tag_from_document_through_local_style() {
    for paragraph_on in [false, true] {
        for local_on in [None, Some(false), Some(true)] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.characters[0].features =
                vec![("zero".into(), true), ("liga".into(), !paragraph_on)];
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                features: vec![("liga".into(), paragraph_on), ("kern".into(), false)],
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                features: vec![("dlig".into(), true)],
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Local base".into(),
                features: local_on
                    .map(|v| vec![("liga".into(), v)])
                    .unwrap_or_default(),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Local".into(),
                based_on: Some("Local base".into()),
                features: vec![("dlig".into(), false)],
                ..Default::default()
            });
            let mut story = Story::from_text("aé中z", "Child");
            story.apply_style(1, 6, "Local");
            let spec = compose::spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 300.0);
            for byte in [0, 1, 3, 6] {
                let local = (1..6).contains(&byte);
                let style = spec.style_at(byte);
                let get = |tag| {
                    style
                        .features
                        .iter()
                        .find(|f| f.tag == tag)
                        .map(|f| f.value)
                };
                assert_eq!(
                    get("liga"),
                    Some(u32::from(if local {
                        local_on.unwrap_or(paragraph_on)
                    } else {
                        paragraph_on
                    }))
                );
                assert_eq!(get("dlig"), Some(u32::from(!local)));
                assert_eq!(get("zero"), Some(1));
                assert_eq!(get("kern"), Some(0));
            }
            assert_eq!(
                doc.styles.resolve_paragraph("Child").features,
                inherited_features(
                    &doc.styles.paragraph("Child").unwrap().features,
                    &doc.styles.paragraph("Base").unwrap().features
                )
            );
            let mut old = serde_json::to_value(&doc.styles).unwrap();
            for style in old["paragraphs"].as_array_mut().unwrap() {
                style.as_object_mut().unwrap().remove("features");
            }
            let old: schist_layout::StyleSet = serde_json::from_value(old).unwrap();
            assert!(old.paragraphs.iter().all(|style| style.features.is_empty()));
        }
    }
}

#[test]
fn directional_defaults_choose_one_axis_and_nearer_switches_reset_inherited_tags() {
    use schist_layout::{directional_features::DirectionalFeatures, WritingMode};
    for proportional in [false, true] {
        for mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ] {
            let vertical = mode != WritingMode::Horizontal;
            let tags = if proportional {
                ["palt", "vpal"]
            } else {
                ["hkna", "vkna"]
            };
            let setting = |value| {
                if proportional {
                    DirectionalFeatures {
                        proportional_metrics: value,
                        ..Default::default()
                    }
                } else {
                    DirectionalFeatures {
                        kana: value,
                        ..Default::default()
                    }
                }
            };
            for enabled in [false, true] {
                for local in [None, Some(false), Some(true)] {
                    for explicit in [None, Some(false), Some(true)] {
                        let mut doc = schist_layout::blank_a4();
                        doc.styles.characters[0].features =
                            tags.map(|t| (t.to_owned(), true)).to_vec();
                        doc.styles.add_paragraph(ParagraphStyle {
                            name: "Base".into(),
                            directional_features: setting(Some(enabled)),
                            writing_mode: Some(mode),
                            ..Default::default()
                        });
                        doc.styles.add_paragraph(ParagraphStyle {
                            name: "Child".into(),
                            based_on: Some("Base".into()),
                            ..Default::default()
                        });
                        doc.styles.add_character(CharacterStyle {
                            name: "Local base".into(),
                            features: tags.map(|t| (t.to_owned(), true)).to_vec(),
                            ..Default::default()
                        });
                        doc.styles.add_character(CharacterStyle {
                            name: "Local".into(),
                            based_on: Some("Local base".into()),
                            directional_features: setting(local),
                            features: explicit
                                .map(|v| vec![(tags[usize::from(vertical)].to_owned(), v)])
                                .unwrap_or_default(),
                            ..Default::default()
                        });
                        doc.styles.add_character(CharacterStyle {
                            name: "Inherited local".into(),
                            based_on: Some("Local".into()),
                            ..Default::default()
                        });
                        let mut story = Story::from_text("aé中z", "Child");
                        story.apply_style(1, 6, "Inherited local");
                        let spec =
                            compose::spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 300.0);
                        for byte in [0, 1, 3, 6] {
                            let style = spec.style_at(byte);
                            for (index, tag) in tags.iter().enumerate() {
                                let selected = index == usize::from(vertical);
                                let expected = if (1..6).contains(&byte) {
                                    if selected {
                                        explicit.unwrap_or(local.unwrap_or(true))
                                    } else {
                                        local.is_none()
                                    }
                                } else {
                                    enabled && selected
                                };
                                assert_eq!(style.features.iter().find(|f| f.tag == *tag).map(|f| f.value), Some(u32::from(expected)), "{mode:?}, {tag}, local={local:?}, explicit={explicit:?}, byte={byte}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn cjk_metrics_match_explicit_font_features_in_every_writing_mode_and_survive_legacy_json() {
    use schist_layout::{directional_features::DirectionalFeatures, WritingMode};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            family: Some("Noto Sans CJK JP".into()),
            point_size: Some(32.0),
            writing_mode: Some(mode),
            directional_features: DirectionalFeatures {
                proportional_metrics: Some(true),
                ..Default::default()
            },
            ..Default::default()
        });
        let story = Story::from_text("かなカナ。、かな", "Base");
        let spec = compose::spec_for(
            &story,
            0,
            story.text().len(),
            &doc.styles,
            "Base",
            "Default",
            400.0,
        );
        let mut reference = doc.styles.clone();
        let style = reference
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Base")
            .unwrap();
        style.directional_features = DirectionalFeatures::default();
        style.features = vec![
            ("palt".into(), mode == WritingMode::Horizontal),
            ("vpal".into(), mode != WritingMode::Horizontal),
        ];
        let explicit = compose::spec_for(
            &story,
            0,
            story.text().len(),
            &reference,
            "Base",
            "Default",
            400.0,
        );
        assert_eq!(
            schist_text_engine::carets(&spec),
            schist_text_engine::carets(&explicit)
        );
        let a = schist_text_engine::rasterize(&spec).unwrap();
        let b = schist_text_engine::rasterize(&explicit).unwrap();
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(a.coverage, b.coverage);
        reference
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Base")
            .unwrap()
            .features = vec![("palt".into(), false), ("vpal".into(), false)];
        let off = compose::spec_for(
            &story,
            0,
            story.text().len(),
            &reference,
            "Base",
            "Default",
            400.0,
        );
        assert_ne!(
            a.layout_width,
            schist_text_engine::rasterize(&off).unwrap().layout_width,
            "font must exercise proportional metrics in {mode:?}"
        );
        let mut old = serde_json::to_value(&doc.styles).unwrap();
        for key in ["paragraphs", "characters"] {
            for style in old[key].as_array_mut().unwrap() {
                style
                    .as_object_mut()
                    .unwrap()
                    .remove("directional_features");
            }
        }
        let old: schist_layout::StyleSet = serde_json::from_value(old).unwrap();
        assert_eq!(
            old.resolve_paragraph("Base").directional_features,
            DirectionalFeatures::default()
        );
    }
}
