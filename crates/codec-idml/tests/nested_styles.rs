use schist_codec_idml::{container, export, import, xml};
use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};

fn record(
    reference: &str,
    kind: &str,
    delimiter: &str,
    repetitions: &str,
    inclusive: &str,
) -> String {
    format!(
        r#"<ListItem type="record"><AppliedCharacterStyle type="object">{reference}</AppliedCharacterStyle><Delimiter type="{kind}">{delimiter}</Delimiter><Repetition type="long">{repetitions}</Repetition><Inclusive type="boolean">{inclusive}</Inclusive></ListItem>"#
    )
}
fn package(rules: &str) -> container::Package {
    let mut package = container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
    package.insert("Resources/Styles.xml",format!(r#"<idPkg:Styles><RootCharacterStyleGroup><CharacterStyle Self="opaque1" Name="Same"/><CharacterStyle Self="opaque2" Name="Same"/></RootCharacterStyleGroup><ParagraphStyle Self="p" Name="Policy"><Properties><AllNestedStyles type="list">{rules}</AllNestedStyles></Properties></ParagraphStyle><ParagraphStyle Self="child" Name="Child"><Properties><BasedOn type="object">p</BasedOn></Properties></ParagraphStyle><ParagraphStyle Self="reset" Name="Reset"><Properties><BasedOn type="object">p</BasedOn><AllNestedStyles type="list"/></Properties></ParagraphStyle></idPkg:Styles>"#).into_bytes());
    package
}

#[test]
fn nested_records_preserve_order_types_references_and_full_integer_values_through_native_saves() {
    let tokens = [
        "Sentence",
        "AnyWord",
        "AnyCharacter",
        "Letters",
        "Digits",
        "Tabs",
        "InlineGraphic",
        "Dropcap",
        "ForcedLineBreak",
        "EndNestedStyle",
        "IndentHereTab",
        "EmSpace",
        "EnSpace",
        "NonbreakingSpace",
        "AutoPageNumber",
        "SectionMarker",
        "Repeat",
        "FutureDelimiter",
    ];
    let mut xml = String::new();
    let mut expected = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let count = [i32::MIN, -1, 0, 1, i32::MAX][index % 5];
        let truth = [" true ", " false ", " 1 ", " 0 "][index % 4];
        let (reference, character_style) = match index % 4 {
            0 => ("opaque1", CharacterStyle::Named("Same".into())),
            1 => ("opaque2", CharacterStyle::Named("Same [opaque2]".into())),
            2 => (
                "CharacterStyle/$ID/[No character style]",
                CharacterStyle::None,
            ),
            _ => (
                "missing-opaque",
                CharacterStyle::Unresolved("missing-opaque".into()),
            ),
        };
        xml.push_str(&record(
            reference,
            "enumeration",
            token,
            &format!(" {count:+} "),
            truth,
        ));
        expected.push(NestedStyle {
            character_style,
            delimiter: Delimiter::Enumeration((*token).into()),
            repetition: count,
            inclusive: index % 2 == 0,
        });
    }
    xml.push_str(&record("opaque1", "string", " &amp;&lt;é\t ", "1", "true"));
    expected.push(NestedStyle {
        character_style: CharacterStyle::Named("Same".into()),
        delimiter: Delimiter::Text(" &<é\t ".into()),
        repetition: 1,
        inclusive: true,
    });
    let result = import::read(&container::write(&package(&xml).into_parts())).unwrap();
    assert!(result
        .report
        .skipped
        .iter()
        .any(|m| m.contains("AllNestedStyles")));
    let mut doc = result.document;
    for _ in 0..4 {
        assert_eq!(
            doc.styles
                .paragraph("Policy")
                .unwrap()
                .nested_styles
                .as_ref(),
            Some(&expected)
        );
        assert_eq!(
            doc.styles.resolve_paragraph("Child").nested_styles.as_ref(),
            Some(&expected)
        );
        assert_eq!(doc.styles.paragraph("Child").unwrap().nested_styles, None);
        assert_eq!(
            doc.styles.resolve_paragraph("Reset").nested_styles,
            Some(Vec::new())
        );
        let written = export::write(&doc);
        assert!(written
            .warnings
            .iter()
            .any(|m| m.contains("AllNestedStyles")));
        let package = container::read(&written.bytes).unwrap();
        let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
        let policy = root
            .find_all("ParagraphStyle")
            .into_iter()
            .find(|s| s.attr("Name") == Some("Policy"))
            .unwrap();
        let native = policy
            .child("Properties")
            .unwrap()
            .child("AllNestedStyles")
            .unwrap();
        assert_eq!(native.children.len(), expected.len());
        assert_eq!(
            native.children[7].child("Delimiter").unwrap().text,
            "Dropcap"
        );
        assert_eq!(
            native
                .children
                .last()
                .unwrap()
                .child("Delimiter")
                .unwrap()
                .attr("type"),
            Some("string")
        );
        doc = import::read(&written.bytes).unwrap().document;
    }
}

#[test]
fn malformed_nested_lists_are_reported_without_applying_a_partial_sequence() {
    let valid = record("opaque1", "enumeration", "Dropcap", "1", "true");
    for broken in [
        record("opaque1", "enumeration", "Dropcap", "1.5", "true"),
        record("opaque1", "enumeration", "Dropcap", "2147483648", "true"),
        record("opaque1", "enumeration", "Dropcap", "1", "TRUE"),
        record("opaque1", "number", "Dropcap", "1", "true"),
        valid.replace("<Inclusive type=\"boolean\">true</Inclusive>", ""),
    ] {
        let result = import::read(&container::write(
            &package(&(valid.clone() + &broken)).into_parts(),
        ))
        .unwrap();
        assert!(result
            .report
            .skipped
            .iter()
            .any(|m| m.contains("AllNestedStyles")));
        assert_eq!(
            result
                .document
                .styles
                .paragraph("Policy")
                .unwrap()
                .nested_styles,
            Some(Vec::new())
        );
    }
}

#[test]
fn local_nested_lists_override_or_reset_the_whole_inherited_list_without_changing_source() {
    for (attributes, local, count) in [
        ("", Some(String::new()), 0),
        ("", Some(record("opaque2", "string", ":", "1", "true")), 1),
        ("EmptyNestedStyles=\"true\"", None, 0),
        ("EmptyNestedStyles=\"1\"", None, 0),
        ("EmptyNestedStyles=\"false\"", None, 1),
        ("EmptyNestedStyles=\"0\"", None, 1),
    ] {
        let mut doc = schist_layout::blank_a4();
        schist_layout::authoring::text_frame(
            &mut doc,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(0.0, 0.0, 200.0, 200.0),
        )
        .unwrap();
        let mut base = container::read(&export::write(&doc).bytes).unwrap();
        let policy = package(&record("opaque1", "enumeration", "Dropcap", "1", "true"));
        base.insert(
            "Resources/Styles.xml",
            policy
                .text("Resources/Styles.xml")
                .unwrap()
                .as_bytes()
                .to_vec(),
        );
        let path = base
            .names()
            .into_iter()
            .find(|n| n.starts_with("Stories/"))
            .unwrap()
            .to_owned();
        let properties = local.map_or_else(String::new, |local| {
            format!(
                r#"<Properties><AllNestedStyles type="list">{local}</AllNestedStyles></Properties>"#
            )
        });
        base.insert(path,format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p" {attributes}>{properties}<CharacterStyleRange><Content>É café: source</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
        doc = import::read(&container::write(&base.into_parts()))
            .unwrap()
            .document;
        let source = doc.stories[0].clone();
        assert_eq!(source.text(), "É café: source");
        let schist_layout::StoryPoint::Paragraph { style: name, .. } = &source.points[0] else {
            panic!()
        };
        let expected = doc.styles.resolve_paragraph(name);
        assert_eq!(expected.nested_styles.as_ref().unwrap().len(), count);
        let style_count = doc.styles.paragraphs.len();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.stories[0], source);
            assert_eq!(doc.styles.paragraphs.len(), style_count);
            assert_eq!(doc.styles.resolve_paragraph(name), expected);
        }
    }
}

#[test]
fn public_native_empty_flags_survive_repeated_saves_without_inventing_lists() {
    for bytes in [
        include_bytes!("../../../fixtures/idml/shapes.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/bounded-text.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/placeholders.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/themes.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/text.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/multipage.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/images.idml").as_slice(),
    ] {
        let native = container::read(bytes).unwrap();
        let root = xml::parse(native.text("Resources/Styles.xml").unwrap()).unwrap();
        let names: Vec<_> = root
            .find_all("ParagraphStyle")
            .into_iter()
            .filter(|style| style.attr("EmptyNestedStyles") == Some("true"))
            .map(|style| {
                style
                    .attr("Name")
                    .unwrap()
                    .trim_start_matches("$ID/")
                    .to_owned()
            })
            .collect();
        assert!(!names.is_empty());
        let mut doc = import::read(bytes).unwrap().document;
        let stories = doc.stories.clone();
        for _ in 0..4 {
            for name in &names {
                assert_eq!(
                    doc.styles.paragraph(name).unwrap().nested_styles,
                    Some(Vec::new())
                );
            }
            let written = export::write(&doc);
            let native = container::read(&written.bytes).unwrap();
            let root = xml::parse(native.text("Resources/Styles.xml").unwrap()).unwrap();
            for style in root.find_all("ParagraphStyle") {
                if names
                    .iter()
                    .any(|name| Some(name.as_str()) == style.attr("Name"))
                {
                    assert_eq!(style.attr("EmptyNestedStyles"), Some("true"));
                    assert!(style
                        .child("Properties")
                        .and_then(|p| p.child("AllNestedStyles"))
                        .is_none());
                }
            }
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.stories, stories);
        }
    }
}

#[test]
fn malformed_empty_flags_and_conflicting_lists_are_diagnosed_without_enabling_parent_rules() {
    let rule = record("opaque1", "string", ":", "1", "true");
    for (flag, local, count) in [
        ("TRUE", None, 0),
        ("yes", None, 0),
        ("", None, 0),
        ("TRUE", Some(rule.as_str()), 1),
        ("true", Some(rule.as_str()), 0),
        ("false", Some(""), 0),
    ] {
        for based_on in ["", r#"<BasedOn type="object">p</BasedOn>"#] {
            let mut native = package(&rule);
            let list = local.map_or_else(String::new, |value| {
                format!(r#"<AllNestedStyles type="list">{value}</AllNestedStyles>"#)
            });
            let properties = if based_on.is_empty() && list.is_empty() {
                String::new()
            } else {
                format!("<Properties>{based_on}{list}</Properties>")
            };
            let extra = format!(
                r#"<ParagraphStyle Self="broken" Name="Broken" EmptyNestedStyles="{flag}">{properties}</ParagraphStyle>"#
            );
            let styles = native
                .text("Resources/Styles.xml")
                .unwrap()
                .replace("</idPkg:Styles>", &(extra + "</idPkg:Styles>"));
            native.insert("Resources/Styles.xml", styles.into_bytes());
            let read = import::read(&container::write(&native.into_parts())).unwrap();
            assert!(read
                .report
                .skipped
                .iter()
                .any(|message| message.contains("EmptyNestedStyles")));
            assert_eq!(
                read.document
                    .styles
                    .resolve_paragraph("Broken")
                    .nested_styles
                    .as_ref()
                    .unwrap()
                    .len(),
                count
            );
            let written = export::write(&read.document);
            let again = import::read(&written.bytes).unwrap();
            assert!(!again
                .report
                .skipped
                .iter()
                .any(|message| message.contains("EmptyNestedStyles")));
            assert_eq!(
                again.document.styles.paragraph("Broken"),
                read.document.styles.paragraph("Broken")
            );
        }
    }
}

#[test]
fn a_native_nested_list_must_survive_a_read_write_without_flattening_or_disappearing() {
    let rules = record("opaque1", "enumeration", "Dropcap", "1", "true")
        + &record("opaque2", "string", ":", "2", "false");
    let document = import::read(&container::write(&package(&rules).into_parts()))
        .unwrap()
        .document;
    let written = export::write(&document);
    let package = container::read(&written.bytes).unwrap();
    let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
    let policy = root
        .find_all("ParagraphStyle")
        .into_iter()
        .find(|s| s.attr("Name") == Some("Policy"))
        .unwrap();
    let nested = policy.child("Properties").unwrap().child("AllNestedStyles");
    assert!(
        nested.is_some(),
        "read/write discarded the entire native nested-style list"
    );
    let rules = &nested.unwrap().children;
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].child("Delimiter").unwrap().text, "Dropcap");
    assert_eq!(rules[1].child("Delimiter").unwrap().text, ":");
}

#[test]
fn native_empty_flags_clear_inherited_rules_without_requiring_a_list_element() {
    for (value, empty) in [
        ("true", true),
        (" 1 ", true),
        ("false", false),
        (" 0 ", false),
    ] {
        let mut native = package(&record("opaque1", "string", ":", "1", "true"));
        let styles = native.text("Resources/Styles.xml").unwrap().replace(
            "<ParagraphStyle Self=\"child\" Name=\"Child\">",
            &format!(
                "<ParagraphStyle Self=\"child\" Name=\"Child\" EmptyNestedStyles=\"{value}\">"
            ),
        );
        native.insert("Resources/Styles.xml", styles.into_bytes());
        let mut doc = import::read(&container::write(&native.into_parts()))
            .unwrap()
            .document;
        let expected = if empty { Some(Vec::new()) } else { None };
        for _ in 0..4 {
            assert_eq!(
                doc.styles.paragraph("Child").unwrap().nested_styles,
                expected,
                "native empty flag {value} must reset inherited rules"
            );
            assert_eq!(
                doc.styles
                    .resolve_paragraph("Child")
                    .nested_styles
                    .unwrap()
                    .is_empty(),
                empty
            );
            let written = export::write(&doc);
            let native = container::read(&written.bytes).unwrap();
            let root = xml::parse(native.text("Resources/Styles.xml").unwrap()).unwrap();
            let styles = root.find_all("ParagraphStyle");
            let child = styles
                .iter()
                .find(|style| style.attr("Name") == Some("Child"))
                .unwrap();
            assert_eq!(child.attr("EmptyNestedStyles"), empty.then_some("true"));
            assert!(child
                .child("Properties")
                .and_then(|p| p.child("AllNestedStyles"))
                .is_none());
            let policy = styles
                .iter()
                .find(|style| style.attr("Name") == Some("Policy"))
                .unwrap();
            assert_eq!(policy.attr("EmptyNestedStyles"), Some("false"));
            doc = import::read(&written.bytes).unwrap().document;
        }
    }
}
