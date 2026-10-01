use schist_layout::{compose, CharacterStyle, ParagraphStyle, Story};
use schist_text_engine::Capitalization;

#[test]
fn caps_flags_inherit_independently_through_paragraph_character_and_document_defaults() {
    for all in [None, Some(false), Some(true)] {
        for small in [None, Some(false), Some(true)] {
            for local_all in [None, Some(false), Some(true)] {
                for local_small in [None, Some(false), Some(true)] {
                    let mut doc = schist_layout::blank_a4();
                    doc.styles.characters[0].all_caps = Some(true);
                    doc.styles.characters[0].small_caps = Some(true);
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Base".into(),
                        all_caps: all,
                        small_caps: small,
                        ..Default::default()
                    });
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Child".into(),
                        based_on: Some("Base".into()),
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Local".into(),
                        all_caps: local_all,
                        small_caps: local_small,
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Inherited".into(),
                        based_on: Some("Local".into()),
                        ..Default::default()
                    });
                    let mut story = Story::from_text("aß café", "Child");
                    story.apply_style(1, 3, "Inherited");
                    let spec = compose::spec_for(
                        &story,
                        0,
                        story.text().len(),
                        &doc.styles,
                        "Child",
                        &doc.default_character_style,
                        400.0,
                    );
                    let base =
                        Capitalization::from_flags(all.unwrap_or(true), small.unwrap_or(true));
                    let local = Capitalization::from_flags(
                        local_all.or(all).unwrap_or(true),
                        local_small.or(small).unwrap_or(true),
                    );
                    for (at, _) in story.text().char_indices() {
                        assert_eq!(
                            spec.style_at(at).capitalization,
                            if (1..3).contains(&at) { local } else { base }
                        );
                    }
                    assert_eq!(spec.text, story.text());
                }
            }
        }
    }
}

#[test]
fn small_cap_preferences_reach_every_run_and_old_documents_keep_the_native_default() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.text_preferences.small_cap_size = 42.0;
    doc.styles.add_character(CharacterStyle {
        name: "Small".into(),
        small_caps: Some(true),
        ..Default::default()
    });
    let mut story = Story::from_text("aß a", "Default");
    story.apply_style(0, 3, "Small");
    let spec = compose::spec_for(
        &story,
        0,
        story.text().len(),
        &doc.styles,
        "Default",
        &doc.default_character_style,
        200.0,
    );
    for (at, _) in spec.text.char_indices() {
        assert_eq!(spec.style_at(at).small_cap_scale, 0.42);
    }
    let mut json = serde_json::to_value(&doc).unwrap();
    json["styles"]["text_preferences"]
        .as_object_mut()
        .unwrap()
        .remove("small_cap_size");
    let old: schist_layout::LayoutDocument = serde_json::from_value(json).unwrap();
    assert_eq!(old.styles.text_preferences.small_cap_size, 70.0);
}
