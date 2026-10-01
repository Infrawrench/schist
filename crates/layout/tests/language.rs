use schist_layout::{compose, CharacterStyle, ParagraphStyle, Story};

#[test]
fn language_inherits_per_property_with_explicit_resets_and_local_ranges_in_every_mode() {
    for paragraph in [None, Some("tr"), Some("und"), Some("")] {
        for local in [None, Some("lt"), Some("und"), Some("")] {
            for mode in [
                schist_layout::WritingMode::Horizontal,
                schist_layout::WritingMode::VerticalRightToLeft,
                schist_layout::WritingMode::VerticalLeftToRight,
            ] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.characters[0].language = Some("ro".into());
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Base".into(),
                    language: paragraph.map(Into::into),
                    writing_mode: Some(mode),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Child".into(),
                    based_on: Some("Base".into()),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Local".into(),
                    language: local.map(Into::into),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Inherited".into(),
                    based_on: Some("Local".into()),
                    ..Default::default()
                });
                let mut story = Story::from_text("işi", "Child");
                story.apply_style(1, 3, "Inherited");
                let spec = compose::spec_for(
                    &story,
                    0,
                    4,
                    &doc.styles,
                    "Child",
                    &doc.default_character_style,
                    400.0,
                );
                let effective = |v| if matches!(v, "und" | "") { "" } else { v };
                for at in [0, 1, 3] {
                    let wanted = if at == 1 {
                        local.or(paragraph).unwrap_or("ro")
                    } else {
                        paragraph.unwrap_or("ro")
                    };
                    assert_eq!(
                        spec.style_at(at).language,
                        effective(wanted),
                        "{paragraph:?} {local:?} {at}"
                    );
                }
                assert_eq!(spec.text, story.text());
            }
        }
    }
    let mut json = serde_json::to_value(schist_layout::StyleSet::with_defaults()).unwrap();
    json.as_object_mut().unwrap().remove("languages");
    assert!(serde_json::from_value::<schist_layout::StyleSet>(json)
        .unwrap()
        .languages
        .is_empty());
}

#[test]
fn explicit_tags_never_alias_opaque_resources_and_old_string_values_keep_their_meaning() {
    use schist_layout::language::{LanguageResource, TextLanguage};
    for tag in ["tr", "ro", "und", "en-US", "TR_tr", "az-Latn-AZ"] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.languages.push(LanguageResource {
            id: tag.into(),
            name: "$ID/Japanese".into(),
            ..Default::default()
        });
        let reference: TextLanguage =
            serde_json::from_str(&serde_json::to_string(tag).unwrap()).unwrap();
        let authored = TextLanguage::Tag { tag: tag.into() };
        assert_eq!(
            doc.styles.resolve_language(&reference).as_deref(),
            Some("ja")
        );
        assert_eq!(
            doc.styles.resolve_language(&authored),
            schist_text_engine::normalize_language(tag)
        );
        for value in [reference, authored] {
            let restored: TextLanguage =
                serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
            assert_eq!(value, restored);
            doc.styles.paragraphs[0].language = Some(value.clone());
            let story = Story::from_text("iş", "Default");
            let spec = compose::spec_for(&story, 0, 3, &doc.styles, "Default", "Default", 100.0);
            let expected = doc
                .styles
                .resolve_language(&value)
                .filter(|v| v != "und")
                .unwrap_or_default();
            assert_eq!(spec.style_at(0).language, expected);
        }
    }
}
