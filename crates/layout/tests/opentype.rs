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
