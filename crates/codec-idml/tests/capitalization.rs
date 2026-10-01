use schist_codec_idml::{container, export, import};
use schist_layout::{CharacterStyle, ParagraphStyle};

#[test]
fn native_caps_and_partial_inheritance_keep_raw_flags_through_repeated_saves() {
    for all in [None, Some(false), Some(true)] {
        for small in [None, Some(false), Some(true)] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Caps".into(),
                all_caps: all,
                small_caps: small,
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Caps".into(),
                all_caps: all,
                small_caps: small,
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Child".into(),
                based_on: Some("Caps".into()),
                ..Default::default()
            });
            doc.styles.text_preferences.small_cap_size = 61.0;
            let expected = doc.styles.clone();
            for _ in 0..4 {
                let written = export::write(&doc);
                assert_eq!(
                    written
                        .warnings
                        .iter()
                        .any(|w| w.contains("Partial capitalization")),
                    all.is_some() != small.is_some()
                );
                let package = container::read(&written.bytes).unwrap();
                let xml = package.text("Resources/Styles.xml").unwrap();
                if all.is_some() && small.is_some() {
                    assert!(xml.contains("Capitalization="));
                }
                doc = import::read(&written.bytes).unwrap().document;
                assert_eq!(doc.styles.paragraph("Caps"), expected.paragraph("Caps"));
                assert_eq!(doc.styles.character("Caps"), expected.character("Caps"));
                assert_eq!(doc.styles.character("Child"), expected.character("Child"));
                assert_eq!(doc.styles.text_preferences.small_cap_size, 61.0);
            }
            if all.is_some() != small.is_some() {
                let mut package = container::read(&export::write(&doc).bytes).unwrap();
                let xml = package
                    .text("Resources/Styles.xml")
                    .unwrap()
                    .replace("Name=\"Caps\"", "Name=\"Caps\" Capitalization=\"Normal\"");
                package.insert("Resources/Styles.xml", xml.into_bytes());
                let doc = import::read(&container::write(&package.into_parts()))
                    .unwrap()
                    .document;
                assert_eq!(doc.styles.resolve_character("Child").all_caps, Some(false));
                assert_eq!(
                    doc.styles.resolve_character("Child").small_caps,
                    Some(false)
                );
            }
        }
    }
}

#[test]
fn invalid_native_caps_and_small_cap_percentages_are_diagnosed() {
    for value in ["NaN", "inf", "0", "201", "bad"] {
        let mut package =
            container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
        let styles = format!("<idPkg:Styles><RootCharacterStyleGroup><CharacterStyle Self=\"c\" Name=\"Invalid\" Capitalization=\"{value}\"/></RootCharacterStyleGroup></idPkg:Styles>");
        package.insert("Resources/Styles.xml", styles.into_bytes());
        let prefs = package
            .text("Resources/Preferences.xml")
            .unwrap()
            .replace("SmallCap=\"70\"", &format!("SmallCap=\"{value}\""));
        package.insert("Resources/Preferences.xml", prefs.into_bytes());
        let read = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(read
            .report
            .skipped
            .iter()
            .any(|m| m.contains("capitalization") && m.contains(value)));
        assert!(read
            .report
            .skipped
            .iter()
            .any(|m| m.contains("SmallCap") && m.contains(value)));
        assert_eq!(read.document.styles.text_preferences.small_cap_size, 70.0);
        assert_eq!(
            read.document.styles.character("Invalid").unwrap().all_caps,
            None
        );
    }
    for value in [f32::NAN, f32::INFINITY, 0.0, 201.0] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.text_preferences.small_cap_size = value;
        let written = export::write(&doc);
        assert!(written.warnings.iter().any(|w| w.contains("SmallCap")));
        assert_eq!(
            import::read(&written.bytes)
                .unwrap()
                .document
                .styles
                .text_preferences
                .small_cap_size,
            70.0
        );
    }
}

#[test]
fn public_all_caps_ranges_keep_source_text_and_do_not_grow_styles_on_resave() {
    for bytes in [
        include_bytes!("../../../fixtures/idml/themes.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/bounded-text.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/shapes.idml").as_slice(),
    ] {
        let mut doc = import::read(bytes).unwrap().document;
        assert!(doc
            .styles
            .characters
            .iter()
            .any(|s| s.all_caps == Some(true)));
        let texts: Vec<_> = doc.stories.iter().map(|s| s.text()).collect();
        let styles = doc.styles.characters.len();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.characters.len(), styles);
            assert_eq!(
                doc.stories.iter().map(|s| s.text()).collect::<Vec<_>>(),
                texts
            );
            assert!(doc
                .styles
                .characters
                .iter()
                .any(|s| s.all_caps == Some(true)));
        }
    }
}
