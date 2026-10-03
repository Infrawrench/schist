use schist_layout::{hyphenation::HyphenationOptions, ParagraphStyle};

#[test]
fn each_hyphenation_option_inherits_independently_even_when_disabled() {
    let base = HyphenationOptions {
        capitalized_words: Some(true),
        last_word: Some(true),
        across_columns: Some(true),
        after_first: Some(3),
        before_last: Some(4),
        words_longer_than: Some(9),
        ladder_limit: Some(5),
        zone: Some(12.5),
        weight: Some(75),
    };
    let base_json = serde_json::to_value(&base).unwrap();
    for (key, reset) in [
        ("capitalized_words", serde_json::json!(false)),
        ("last_word", serde_json::json!(false)),
        ("across_columns", serde_json::json!(false)),
        ("after_first", serde_json::json!(1)),
        ("before_last", serde_json::json!(1)),
        ("words_longer_than", serde_json::json!(3)),
        ("ladder_limit", serde_json::json!(0)),
        ("zone", serde_json::json!(0.0)),
        ("weight", serde_json::json!(0)),
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            hyphenate: Some(true),
            hyphenation: base.clone(),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            hyphenate: Some(false),
            hyphenation: serde_json::from_value(serde_json::json!({key:reset.clone()})).unwrap(),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Leaf".into(),
            based_on: Some("Child".into()),
            ..Default::default()
        });
        let resolved = doc.styles.resolve_paragraph("Leaf");
        assert_eq!(resolved.hyphenate, Some(false));
        let mut expected = base_json.clone();
        expected[key] = reset;
        assert_eq!(
            serde_json::to_value(resolved.hyphenation).unwrap(),
            expected
        );
        assert_eq!(doc.styles.resolve_paragraph("Base").hyphenation, base);
    }
}

#[test]
fn older_snapshots_and_missing_options_inherit_without_creating_policy() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Inherited".into(),
        hyphenate: Some(false),
        ..Default::default()
    });
    let mut saved = serde_json::to_value(&doc).unwrap();
    for style in saved["styles"]["paragraphs"].as_array_mut().unwrap() {
        style.as_object_mut().unwrap().remove("hyphenation");
    }
    let restored: schist_layout::LayoutDocument = serde_json::from_value(saved).unwrap();
    assert_eq!(restored, doc);
    assert_eq!(
        restored.styles.resolve_paragraph("Inherited").hyphenation,
        HyphenationOptions::default()
    );
    assert_eq!(
        restored.styles.resolve_paragraph("Inherited").hyphenate,
        Some(false)
    );
}
