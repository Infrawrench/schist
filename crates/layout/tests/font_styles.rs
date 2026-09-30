use schist_layout::{blank_a4, compose::spec_for, CharacterStyle, ParagraphStyle, Story};

#[test]
fn nearest_face_choice_wins_while_family_and_size_inherit_independently() {
    for parent in ["Light", "Regular", "Bold Condensed", "書体 W3"] {
        for child in [None, Some("Medium"), Some("Regular")] {
            for reset in [None, Some(false), Some(true)] {
                let mut doc = blank_a4();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Parent".into(),
                    family: Some("IBM Plex Sans".into()),
                    font_style: Some(parent.into()),
                    point_size: Some(18.0),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Child".into(),
                    based_on: Some("Parent".into()),
                    font_style: child.map(str::to_owned),
                    bold: reset,
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Local".into(),
                    point_size: Some(24.0),
                    ..Default::default()
                });
                let expected = child.or_else(|| reset.is_none().then_some(parent));
                assert_eq!(
                    doc.styles.resolve_paragraph("Child").font_style.as_deref(),
                    expected
                );
                let mut story = Story::from_text("aé中z", "Child");
                story.apply_style(1, 6, "Local");
                for local in [None, Some("Light"), Some("Regular")] {
                    doc.styles
                        .characters
                        .iter_mut()
                        .find(|s| s.name == "Local")
                        .unwrap()
                        .font_style = local.map(str::to_owned);
                    let spec = spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 400.0);
                    for byte in [0, 1, 3, 6] {
                        let ranged = (1..6).contains(&byte);
                        let style = spec.style_at(byte);
                        assert_eq!(style.family, "IBM Plex Sans");
                        assert_eq!(style.size, if ranged { 24.0 } else { 18.0 });
                        assert_eq!(
                            style.font_style.as_deref(),
                            if ranged { local.or(expected) } else { expected }
                        );
                    }
                }
                doc.styles.add_character(CharacterStyle {
                    name: "Base".into(),
                    font_style: Some(parent.into()),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Leaf".into(),
                    based_on: Some("Base".into()),
                    font_style: child.map(str::to_owned),
                    italic: reset,
                    ..Default::default()
                });
                assert_eq!(
                    doc.styles.resolve_character("Leaf").font_style.as_deref(),
                    expected
                );
            }
        }
    }
}
