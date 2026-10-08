use schist_codec_idml::{container, export, import, xml};

#[test]
fn native_drop_cap_detail_is_not_erased_by_inactive_counts_or_repeated_saves() {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    let values = [i32::MIN, -1, 0, 1, 2, 3, 0x100, 0x200, 0x400, i32::MAX];
    let mut styles = String::from("<idPkg:Styles><ParagraphStyle Self=\"empty\" Name=\"Empty\"/>");
    for (index, value) in values.iter().enumerate() {
        styles.push_str(&format!("<ParagraphStyle Self=\"p{index}\" Name=\"Policy{index}\" DropCapLines=\"0\" DropCapCharacters=\"0\" DropcapDetail=\" {value:+} \"/>"));
    }
    styles.push_str("</idPkg:Styles>");
    package.insert("Resources/Styles.xml", styles.into_bytes());
    let mut doc = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    for _ in 0..4 {
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
        let styles = root.find_all("ParagraphStyle");
        for (index, value) in values.iter().enumerate() {
            let name = format!("Policy{index}");
            let style = styles
                .iter()
                .find(|s| s.attr("Name") == Some(&name))
                .unwrap();
            assert_eq!(
                style.attr("DropcapDetail"),
                Some(value.to_string().as_str()),
                "{name}"
            );
            assert_eq!(style.attr("DropCapLines"), Some("0"));
            assert_eq!(style.attr("DropCapCharacters"), Some("0"));
        }
        let empty = styles
            .iter()
            .find(|s| s.attr("Name") == Some("Empty"))
            .unwrap();
        assert_eq!(empty.attr("DropcapDetail"), None);
        doc = import::read(&written.bytes).unwrap().document;
    }
}

#[test]
fn drop_cap_integer_ranges_accept_xml_spellings_and_diagnose_invalid_values_without_clamping() {
    for (property, max) in [("DropCapLines", 25), ("DropCapCharacters", 150)] {
        for (raw, expected) in [
            (" -0 ".to_owned(), Some(0)),
            (" +002 ".into(), Some(2)),
            (max.to_string(), Some(max)),
            ((max + 1).to_string(), None),
            ("-1".into(), None),
            ("2.5".into(), None),
            ("NaN".into(), None),
            ("32768".into(), None),
            ("\u{a0}2".into(), None),
        ] {
            let mut package =
                container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
            package.insert("Resources/Styles.xml", format!("<idPkg:Styles><ParagraphStyle Self=\"p\" Name=\"Policy\" {property}=\"{raw}\"/></idPkg:Styles>").into_bytes());
            let result = import::read(&container::write(&package.into_parts())).unwrap();
            let style = result.document.styles.paragraph("Policy").unwrap();
            assert_eq!(
                if property == "DropCapLines" {
                    style.drop_caps_lines
                } else {
                    style.drop_caps_characters
                },
                expected,
                "{property} {raw}"
            );
            assert_eq!(
                result
                    .report
                    .skipped
                    .iter()
                    .any(|message| message.contains(property)),
                expected.is_none(),
                "{property} {raw}"
            );
        }
    }
    for raw in ["2147483648", "-2147483649", "1.5", "NaN", "\u{a0}1"] {
        let mut package =
            container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
        package.insert("Resources/Styles.xml", format!("<idPkg:Styles><ParagraphStyle Self=\"p\" Name=\"Policy\" DropcapDetail=\"{raw}\"/></idPkg:Styles>").into_bytes());
        let result = import::read(&container::write(&package.into_parts())).unwrap();
        assert_eq!(
            result
                .document
                .styles
                .paragraph("Policy")
                .unwrap()
                .drop_caps_detail,
            None
        );
        assert!(result
            .report
            .skipped
            .iter()
            .any(|message| message.contains("DropcapDetail")));
    }
}

#[test]
fn authored_invalid_counts_are_reported_and_never_written_as_native_short_values() {
    for (lines, characters, property) in [
        (Some(26), None, "DropCapLines"),
        (Some(usize::MAX), None, "DropCapLines"),
        (None, Some(151), "DropCapCharacters"),
        (None, Some(usize::MAX), "DropCapCharacters"),
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: "Invalid".into(),
            drop_caps_lines: lines,
            drop_caps_characters: characters,
            ..Default::default()
        });
        let written = export::write(&doc);
        assert!(written
            .warnings
            .iter()
            .any(|message| message.contains(property)));
        let package = container::read(&written.bytes).unwrap();
        let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
        let style = root
            .find_all("ParagraphStyle")
            .into_iter()
            .find(|s| s.attr("Name") == Some("Invalid"))
            .unwrap();
        assert_eq!(style.attr(property), None);
    }
}

#[test]
fn active_native_details_are_retained_and_reported_as_uncomposed_without_warning_on_dormant_settings(
) {
    for value in [0, 1, 2, 3, 256, -1] {
        for active in [false, true] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.add_paragraph(schist_layout::ParagraphStyle {
                name: "Policy".into(),
                drop_caps_lines: Some(if active { 3 } else { 0 }),
                drop_caps_characters: Some(1),
                drop_caps_detail: Some(value),
                ..Default::default()
            });
            for _ in 0..3 {
                let written = export::write(&doc);
                assert_eq!(
                    written
                        .warnings
                        .iter()
                        .any(|message| message.contains("DropcapDetail")),
                    active
                );
                let result = import::read(&written.bytes).unwrap();
                assert_eq!(
                    result
                        .report
                        .skipped
                        .iter()
                        .any(|message| message.contains("DropcapDetail")),
                    active
                );
                assert_eq!(
                    result
                        .document
                        .styles
                        .paragraph("Policy")
                        .unwrap()
                        .drop_caps_detail,
                    Some(value)
                );
                doc = result.document;
            }
        }
    }
}

#[test]
fn local_drop_cap_overrides_lower_once_with_independent_inheritance_and_unchanged_source() {
    for (property, raw, canonical) in [
        ("DropCapLines", " +002 ", "2"),
        ("DropCapLines", "0", "0"),
        ("DropCapCharacters", " +002 ", "2"),
        ("DropCapCharacters", "0", "0"),
        ("DropcapDetail", "0", "0"),
        ("DropcapDetail", " -2147483648 ", "-2147483648"),
    ] {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::text_frame(
            &mut doc,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(0.0, 0.0, 200.0, 200.0),
        )
        .unwrap();
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        package.insert("Resources/Styles.xml", br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Base" DropCapLines="3" DropCapCharacters="1" DropcapDetail="3"/></idPkg:Styles>"#.to_vec());
        let path = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Stories/"))
            .unwrap()
            .to_owned();
        package.insert(path, format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p" {property}="{raw}"><CharacterStyleRange><Content>É café text</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
        doc = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        let source = doc.stories[0].clone();
        let count = doc.styles.paragraphs.len();
        let schist_layout::StoryPoint::Paragraph { style: name, .. } = &source.points[0] else {
            panic!()
        };
        let resolved = doc.styles.resolve_paragraph(name);
        assert_eq!(
            resolved.drop_caps_lines,
            Some(if property == "DropCapLines" {
                canonical.parse().unwrap()
            } else {
                3
            })
        );
        assert_eq!(
            resolved.drop_caps_characters,
            Some(if property == "DropCapCharacters" {
                canonical.parse().unwrap()
            } else {
                1
            })
        );
        assert_eq!(
            resolved.drop_caps_detail,
            Some(if property == "DropcapDetail" {
                canonical.parse().unwrap()
            } else {
                3
            })
        );
        for _ in 0..4 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
            let local = root
                .find_all("ParagraphStyle")
                .into_iter()
                .find(|s| s.attr("Name") == Some(name))
                .unwrap();
            assert_eq!(local.attr(property), Some(canonical), "{property}");
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.styles.paragraphs.len(), count);
            assert_eq!(doc.stories[0], source);
            assert_eq!(doc.styles.resolve_paragraph(name), resolved);
        }
    }
}

#[test]
fn public_native_style_flags_survive_without_activating_dormant_initials() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idml");
    let mut observed = 0;
    for path in std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
    {
        if path.extension().and_then(|ext| ext.to_str()) != Some("idml") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let package = container::read(&bytes).unwrap();
        let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
        let native: Vec<_> = root
            .find_all("ParagraphStyle")
            .into_iter()
            .filter_map(|style| {
                Some((
                    style.attr("Name")?.trim_start_matches("$ID/").to_owned(),
                    style.attr("DropcapDetail")?.parse::<i32>().unwrap(),
                ))
            })
            .collect();
        observed += native.len();
        let mut doc = import::read(&bytes).unwrap().document;
        for _ in 0..3 {
            for (name, flags) in &native {
                let style = doc.styles.paragraph(name).unwrap();
                assert_eq!(
                    style.drop_caps_detail,
                    Some(*flags),
                    "{}: {name}",
                    path.display()
                );
                assert_eq!(style.drop_caps_lines, Some(0));
                assert_eq!(style.drop_caps_characters, Some(0));
            }
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
    assert!(observed > 0, "the public corpus must exercise native flags");
}
