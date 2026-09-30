use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    styles::{BaselineShift, TextPosition, TextPreferences},
    CharacterStyle, ParagraphStyle,
};

#[test]
fn native_positions_keep_independent_offsets_inheritance_and_preferences_on_resave() {
    for position in [
        TextPosition::Normal,
        TextPosition::Superscript,
        TextPosition::Subscript,
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.text_preferences = TextPreferences {
            superscript_size: 65.0,
            superscript_position: 42.0,
            subscript_size: 70.0,
            subscript_position: 17.0,
        };
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Parent".into(),
            position: Some(position),
            baseline_shift: Some(BaselineShift::Offset(3.5)),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Parent".into()),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Script".into(),
            position: Some(position),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Normal".into(),
            based_on: Some("Script".into()),
            position: Some(TextPosition::Normal),
            baseline_shift: Some(BaselineShift::Offset(-2.0)),
            ..Default::default()
        });
        let original = doc.styles.clone();
        for _ in 0..4 {
            let written = export::write(&doc);
            assert!(!written.warnings.iter().any(|w| w.contains("position")));
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.styles.text_preferences, original.text_preferences);
            assert_eq!(
                doc.styles.resolve_paragraph("Child").position,
                Some(position)
            );
            assert_eq!(
                doc.styles.resolve_paragraph("Child").baseline_shift,
                Some(BaselineShift::Offset(3.5))
            );
            assert_eq!(doc.styles.paragraph("Child").unwrap().position, None);
            assert_eq!(
                doc.styles.resolve_character("Normal").position,
                Some(TextPosition::Normal)
            );
            assert_eq!(
                doc.styles.resolve_character("Normal").baseline_shift,
                Some(BaselineShift::Offset(-2.0))
            );
        }
    }
    for (legacy, native) in [
        (BaselineShift::Superscript, TextPosition::Superscript),
        (BaselineShift::Subscript, TextPosition::Subscript),
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.characters[0].baseline_shift = Some(legacy);
        let written = export::write(&doc);
        assert!(!written.warnings.iter().any(|w| w.contains("position")));
        let read = import::read(&written.bytes).unwrap().document;
        assert_eq!(read.styles.characters[0].position, Some(native));
        assert_eq!(read.styles.characters[0].baseline_shift, None);
    }
}

#[test]
fn invalid_preferences_are_reported_per_field_and_never_become_nonfinite_layout() {
    for raw in ["NaN", "inf", "nonsense", "-501", "501"] {
        let mut package =
            container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
        package.insert("Resources/Preferences.xml", format!(r#"<idPkg:Preferences><TextPreference SuperscriptSize="{raw}" SuperscriptPosition="{raw}" SubscriptSize="72" SubscriptPosition="-23"/></idPkg:Preferences>"#).into_bytes());
        let read = import::read(&container::write(&package.into_parts())).unwrap();
        assert_eq!(
            read.report
                .skipped
                .iter()
                .filter(|w| w.contains(raw))
                .count(),
            2
        );
        assert_eq!(
            read.document.styles.text_preferences,
            TextPreferences {
                subscript_size: 72.0,
                subscript_position: -23.0,
                ..Default::default()
            }
        );
    }
    let mut doc = schist_layout::blank_a4();
    for value in [f32::NAN, f32::INFINITY, 0.0, 201.0] {
        doc.styles.text_preferences.superscript_size = value;
        let written = export::write(&doc);
        assert!(written
            .warnings
            .iter()
            .any(|w| w.contains("SuperscriptSize")));
        let read = import::read(&written.bytes).unwrap().document;
        assert_eq!(read.styles.text_preferences.superscript_size, 58.3);
    }
}

#[test]
fn published_native_superscripts_and_local_overrides_survive_without_repeated_style_growth() {
    for bytes in [
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
    ] {
        let native = container::read(bytes).unwrap();
        let styles = xml::parse(native.text("Resources/Styles.xml").unwrap()).unwrap();
        assert!(styles
            .find_all("CharacterStyle")
            .iter()
            .any(|s| s.attr("Position") == Some("Superscript")));
        let read = import::read(bytes).unwrap();
        assert!(!read
            .report
            .skipped
            .iter()
            .any(|w| w.contains("text position: Superscript")));
        let mut doc = read.document;
        let positions = |doc: &schist_layout::LayoutDocument| {
            doc.styles
                .characters
                .iter()
                .filter_map(|c| c.position.map(|position| (c.name.clone(), position)))
                .collect::<Vec<_>>()
        };
        let before = positions(&doc);
        assert!(before
            .iter()
            .any(|(name, p)| name == "footnote number" && *p == TextPosition::Superscript));
        let count = doc.styles.characters.len();
        assert_eq!(doc.styles.text_preferences, TextPreferences::default());
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.characters.len(), count);
            assert_eq!(positions(&doc), before);
            assert_eq!(doc.styles.text_preferences, TextPreferences::default());
        }
    }
}
