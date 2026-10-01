use schist_codec_idml::{container, export, import};
use schist_layout::{language::LanguageResource, CharacterStyle, ParagraphStyle};

#[test]
fn authored_tags_and_resets_survive_native_lowering_repeated_saves_and_later_native_edits() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for value in [
        "tr",
        "TR_tr",
        "lt-LT",
        "ro",
        "und",
        "",
        "az-Latn-AZ",
        "en-x-custom",
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.languages.push(LanguageResource {
            id: "tr".into(),
            name: "$ID/Romanian".into(),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Language".into(),
            language: Some(schist_layout::language::TextLanguage::Tag { tag: value.into() }),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(20.0),
            all_caps: Some(true),
            small_caps: Some(false),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Language".into(),
            language: Some(schist_layout::language::TextLanguage::Tag { tag: value.into() }),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Child".into(),
            based_on: Some("Language".into()),
            ..Default::default()
        });
        let frame = schist_layout::authoring::text_frame(
            &mut doc,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(10.0, 10.0, 300.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] =
            schist_layout::Story::from_text("iş ŞŢ i\u{307}\u{301}", "Language");
        let pixels = |doc: &schist_layout::LayoutDocument| {
            let story = &doc.stories[frame.story.0 as usize];
            schist_text_engine::rasterize(&schist_layout::compose::spec_for(
                story,
                0,
                story.text().len(),
                &doc.styles,
                "Language",
                &doc.default_character_style,
                300.0,
            ))
            .unwrap()
        };
        let expected_pixels = pixels(&doc);
        let expected = doc.styles.clone();
        let mut resources = None;
        let mut native_id = String::new();
        for _ in 0..4 {
            let written = export::write(&doc);
            assert_eq!(
                written
                    .warnings
                    .iter()
                    .any(|w| w.contains("Native language unavailable")),
                matches!(value, "az-Latn-AZ" | "en-x-custom")
            );
            let package = container::read(&written.bytes).unwrap();
            let xml = package.text("Resources/Styles.xml").unwrap();
            native_id = xml
                .split_once("AppliedLanguage=\"")
                .unwrap()
                .1
                .split('"')
                .next()
                .unwrap()
                .to_owned();
            assert!(package
                .text("designmap.xml")
                .unwrap()
                .contains(&format!("Self=\"{native_id}\"")));
            assert!(xml.contains("Schist.Language.v1"));
            let read = import::read(&written.bytes).unwrap();
            assert!(read.report.is_complete(), "{:?}", read.report);
            doc = read.document;
            let raster = pixels(&doc);
            assert_eq!(raster.bounds, expected_pixels.bounds);
            assert_eq!(raster.coverage, expected_pixels.coverage);
            assert_eq!(
                doc.styles.paragraph("Language"),
                expected.paragraph("Language")
            );
            assert_eq!(
                doc.styles.character("Language"),
                expected.character("Language")
            );
            assert_eq!(
                doc.styles
                    .resolve_character("Child")
                    .language
                    .as_ref()
                    .map(|v| v.as_str()),
                Some(value)
            );
            if let Some(ref resources) = resources {
                assert_eq!(&doc.styles.languages, resources);
            }
            resources = Some(doc.styles.languages.clone());
        }
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let native = doc
            .styles
            .languages
            .iter()
            .find(|r| r.id == native_id)
            .unwrap();
        let old = native.id.clone();
        let mut xml = package.text("designmap.xml").unwrap().to_owned();
        xml = xml.replace(
            &format!("Name=\"{}\"", native.name),
            "Name=\"$ID/Japanese\"",
        );
        package.insert("designmap.xml", xml.into_bytes());
        let edited = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        assert_eq!(
            edited
                .styles
                .character("Language")
                .unwrap()
                .language
                .as_ref()
                .map(|v| v.as_str()),
            Some(old.as_str())
        );
        assert_eq!(edited.styles.language_tag(&old).as_deref(), Some("ja"));
    }
}

#[test]
fn opaque_resources_dictionary_options_and_name_aliases_survive_local_formatting_and_saves() {
    let mut doc = schist_layout::blank_a4();
    let frame = schist_layout::authoring::text_frame(
        &mut doc,
        &mut schist_layout::History::default(),
        0,
        schist_layout::Rect::new(10.0, 10.0, 100.0, 100.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = schist_layout::Story::from_text("ŞŢşţ", "Default");
    let romanian = LanguageResource {
        id: "tr".into(),
        name: "$ID/Romanian".into(),
        language_id: Some(289),
        primary_name: Some("$ID/Romanian".into()),
        sublanguage_name: Some("$ID/".into()),
        single_quotes: Some("‚‘".into()),
        double_quotes: Some("„“".into()),
        spelling_vendor: Some("Example spelling".into()),
        hyphenation_vendor: Some("Example hyphenation".into()),
        labels: vec![("Note".into(), "Preserve <&>".into())],
    };
    doc.styles.languages = vec![
        romanian.clone(),
        LanguageResource {
            id: "unused".into(),
            name: "$ID/Lithuanian".into(),
            ..Default::default()
        },
    ];
    doc.styles.paragraphs[0].language = Some("tr".into());
    doc.styles.characters[0].language = Some("$ID/Romanian".into());
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let path = package
        .names()
        .into_iter()
        .find(|name| name.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    let story = package.text(&path).unwrap().replace(
        "<CharacterStyleRange ",
        "<CharacterStyleRange AppliedLanguage=\"tr\" ",
    );
    package.insert(&path, story.into_bytes());
    let mut doc = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    for _ in 0..4 {
        assert_eq!(doc.styles.languages[0], romanian);
        assert_eq!(doc.styles.languages.len(), 2);
        assert_eq!(doc.styles.language_tag("tr").as_deref(), Some("ro"));
        assert_eq!(
            doc.styles.paragraphs[0]
                .language
                .as_ref()
                .map(|v| v.as_str()),
            Some("tr")
        );
        assert_eq!(
            doc.styles
                .character("Default")
                .unwrap()
                .language
                .as_ref()
                .map(|v| v.as_str()),
            Some("$ID/Romanian")
        );
        let story = &doc.stories[0];
        assert_eq!(story.text(), "ŞŢşţ");
        let local = &story.ranges[0].style;
        assert_eq!(
            doc.styles
                .resolve_character(local)
                .language
                .as_ref()
                .map(|v| v.as_str()),
            Some("tr")
        );
        let spec = schist_layout::compose::spec_for(
            story,
            0,
            story.text().len(),
            &doc.styles,
            "Default",
            "Default",
            100.0,
        );
        assert_eq!(spec.style_at(0).language, "ro");
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn invalid_declarations_and_unresolved_references_are_reported_without_guessing_ids() {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    let root = package.text("designmap.xml").unwrap().replace("</Document>", r#"<Language Self="missing"/><Language Self="" Name="$ID/Turkish"/><Language Self="opaque" Name="$ID/Turkish" Id="NaN"/><Language Self="opaque" Name="$ID/Romanian"/></Document>"#);
    package.insert("designmap.xml", root.into_bytes());
    package.insert("Resources/Styles.xml", br#"<idPkg:Styles><RootCharacterStyleGroup><CharacterStyle Self="c" Name="Unknown" AppliedLanguage="Language/$ID/Turkish"/></RootCharacterStyleGroup></idPkg:Styles>"#.to_vec());
    let read = import::read(&container::write(&package.into_parts())).unwrap();
    assert_eq!(read.document.styles.languages.len(), 1);
    assert_eq!(read.document.styles.languages[0].language_id, None);
    assert_eq!(
        read.report
            .skipped
            .iter()
            .filter(|w| w.contains("Invalid IDML language"))
            .count(),
        4
    );
    assert!(read
        .report
        .skipped
        .iter()
        .any(|w| w.contains("Unresolved language")));
    assert_eq!(
        read.document
            .styles
            .character("Unknown")
            .unwrap()
            .language
            .as_ref()
            .map(|v| v.as_str()),
        Some("Language/$ID/Turkish")
    );
}
