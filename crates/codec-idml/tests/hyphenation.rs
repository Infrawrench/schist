use schist_codec_idml::{container, export, import, xml};

fn policies() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut values = Vec::new();
    for property in [
        "Hyphenation",
        "HyphenateCapitalizedWords",
        "HyphenateLastWord",
        "HyphenateAcrossColumns",
    ] {
        for (raw, canonical) in [
            ("true", "true"),
            ("false", "false"),
            (" 1 ", "true"),
            ("\t0\n", "false"),
        ] {
            values.push((property, raw, canonical));
        }
    }
    for property in ["HyphenateAfterFirst", "HyphenateBeforeLast"] {
        for (raw, canonical) in [("1", "1"), ("15", "15"), (" +03 ", "3")] {
            values.push((property, raw, canonical));
        }
    }
    for (property, raw) in [
        ("HyphenateWordsLongerThan", "3"),
        ("HyphenateWordsLongerThan", "25"),
        ("HyphenateLadderLimit", "0"),
        ("HyphenateLadderLimit", "25"),
        ("HyphenationZone", "0"),
        ("HyphenationZone", "12.75"),
        ("HyphenWeight", "0"),
        ("HyphenWeight", "10"),
        ("HyphenWeight", "50"),
        ("HyphenWeight", "100"),
    ] {
        values.push((property, raw, raw));
    }
    // xsd:short permits signed zero and leading signs/zeros.
    values.push(("HyphenateLadderLimit", " -0 ", "0"));
    values.push(("HyphenWeight", " +050 ", "50"));
    values
}

#[test]
fn native_hyphenation_settings_survive_each_save_without_inventing_defaults() {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    let values = policies();
    let mut styles = String::from("<idPkg:Styles><ParagraphStyle Self=\"empty\" Name=\"Empty\"/>");
    for (index, (property, raw, _)) in values.iter().enumerate() {
        styles.push_str(&format!(
            "<ParagraphStyle Self=\"p{index}\" Name=\"Policy{index}\" {property}=\"{raw}\"/>"
        ));
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
        for (index, (property, _, canonical)) in values.iter().enumerate() {
            let name = format!("Policy{index}");
            let style = styles
                .iter()
                .find(|s| s.attr("Name") == Some(&name))
                .unwrap();
            assert_eq!(style.attr(property), Some(*canonical), "{name}/{property}");
            for (other, _, _) in &values {
                if other != property {
                    assert_eq!(style.attr(other), None, "{name}/{other}");
                }
            }
        }
        let empty = styles
            .iter()
            .find(|s| s.attr("Name") == Some("Empty"))
            .unwrap();
        for (property, _, _) in &values {
            assert_eq!(empty.attr(property), None);
        }
        doc = import::read(&written.bytes).unwrap().document;
    }
}

#[test]
fn local_policy_overrides_lower_once_and_keep_source_and_inherited_fields() {
    for (property, raw, canonical) in policies() {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::text_frame(
            &mut doc,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(0.0, 0.0, 200.0, 200.0),
        )
        .unwrap();
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        package.insert("Resources/Styles.xml", br#"<idPkg:Styles><ParagraphStyle Self="p" Name="Base" Hyphenation="true" HyphenateAfterFirst="7" HyphenateBeforeLast="5" HyphenateLadderLimit="3"/></idPkg:Styles>"#.to_vec());
        let path = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Stories/"))
            .unwrap()
            .to_owned();
        package.insert(path, format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p" {property}="{raw}"><CharacterStyleRange><Content>é café hyphenation</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
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
            resolved.hyphenation.after_first,
            Some(if property == "HyphenateAfterFirst" {
                canonical.parse().unwrap()
            } else {
                7
            })
        );
        assert_eq!(
            resolved.hyphenation.before_last,
            Some(if property == "HyphenateBeforeLast" {
                canonical.parse().unwrap()
            } else {
                5
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
fn invalid_policy_is_diagnosed_without_replacing_it_with_a_default() {
    for (property, values) in [
        ("Hyphenation", ["2", "TRUE", "", "yes"]),
        ("HyphenateCapitalizedWords", ["2", "TRUE", "", "yes"]),
        ("HyphenateLastWord", ["2", "TRUE", "", "yes"]),
        ("HyphenateAcrossColumns", ["2", "TRUE", "", "yes"]),
        ("HyphenateAfterFirst", ["0", "16", "-1", "1.5"]),
        ("HyphenateBeforeLast", ["0", "16", "-1", "1.5"]),
        ("HyphenateWordsLongerThan", ["2", "26", "-1", "3.0"]),
        ("HyphenateLadderLimit", ["26", "256", "-1", "1.5"]),
        ("HyphenWeight", ["101", "256", "-1", "10.5"]),
        ("HyphenationZone", ["-1", "NaN", "inf", "1e999"]),
    ] {
        for value in values.into_iter().chain(["\u{a0}1", "\u{2003}1"]) {
            let mut package =
                container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
            package.insert("Resources/Styles.xml", format!("<idPkg:Styles><ParagraphStyle Self=\"p\" Name=\"Invalid\" {property}=\"{value}\"/></idPkg:Styles>").into_bytes());
            let read = import::read(&container::write(&package.into_parts())).unwrap();
            let expected = schist_i18n::tf!(
                "design.idml_text_preference_invalid",
                property = property,
                value = value
            );
            assert!(
                read.report.skipped.contains(&expected),
                "{property}={value}"
            );
            let resolved = read.document.styles.resolve_paragraph("Invalid");
            assert_eq!(resolved.hyphenate, None);
            assert_eq!(resolved.hyphenation, Default::default());
        }
    }
}

#[test]
fn invalid_authored_values_are_reported_and_never_written_as_valid_native_policy() {
    use schist_layout::{hyphenation::HyphenationOptions, ParagraphStyle};
    for zone in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Invalid".into(),
            hyphenation: HyphenationOptions {
                after_first: Some(0),
                before_last: Some(16),
                words_longer_than: Some(26),
                ladder_limit: Some(26),
                weight: Some(101),
                zone: Some(zone),
                ..Default::default()
            },
            ..Default::default()
        });
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
        let style = root
            .find_all("ParagraphStyle")
            .into_iter()
            .find(|s| s.attr("Name") == Some("Invalid"))
            .unwrap();
        for property in [
            "HyphenateAfterFirst",
            "HyphenateBeforeLast",
            "HyphenateWordsLongerThan",
            "HyphenateLadderLimit",
            "HyphenWeight",
            "HyphenationZone",
        ] {
            assert!(
                written.warnings.iter().any(|w| w.contains(property)),
                "{property}"
            );
            assert_eq!(style.attr(property), None);
        }
    }
}
