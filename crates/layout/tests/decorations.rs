use schist_layout::{compose::spec_for, CharacterStyle, ParagraphStyle, Story};

#[test]
fn both_decorations_inherit_independently_through_paragraphs_and_character_overrides() {
    for underline in [false, true] {
        for strike in [false, true] {
            for override_underline in [None, Some(false), Some(true)] {
                for override_strike in [None, Some(false), Some(true)] {
                    let mut doc = schist_layout::blank_a4();
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Base".into(),
                        underline: Some(underline),
                        strikethrough: Some(strike),
                        ..Default::default()
                    });
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Body".into(),
                        based_on: Some("Base".into()),
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Character base".into(),
                        underline: override_underline,
                        strikethrough: override_strike,
                        ..Default::default()
                    });
                    doc.styles.add_character(CharacterStyle {
                        name: "Character".into(),
                        based_on: Some("Character base".into()),
                        ..Default::default()
                    });
                    let mut story = Story::from_text("Aé中Z", "Body");
                    story.apply_style(1, 6, "Character");
                    let spec = spec_for(
                        &story,
                        0,
                        story.text().len(),
                        &doc.styles,
                        "Body",
                        "Default",
                        200.0,
                    );
                    for byte in [0, 1, 3, 6] {
                        let style = spec.style_at(byte);
                        let overridden = (1..6).contains(&byte);
                        assert_eq!(
                            style.underline,
                            if overridden {
                                override_underline.unwrap_or(underline)
                            } else {
                                underline
                            }
                        );
                        assert_eq!(
                            style.strikethrough,
                            if overridden {
                                override_strike.unwrap_or(strike)
                            } else {
                                strike
                            }
                        );
                    }
                }
            }
        }
    }
}
