use schist_codec_idml::{container, export, import, xml};

#[test]
fn native_no_break_flags_and_explicit_resets_survive_repeated_saves() {
    for (value, canonical) in [
        ("true", "true"),
        ("false", "false"),
        ("1", "true"),
        ("0", "false"),
        (" true ", "true"),
        (" false ", "false"),
        ("\t1\t", "true"),
        ("\t0\t", "false"),
    ] {
        let mut package =
            container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
        let styles = format!(
            r#"<idPkg:Styles><RootParagraphStyleGroup>
            <ParagraphStyle Self="pbase" Name="Base" NoBreak="true"/>
            <ParagraphStyle Self="pchild" Name="Child" NoBreak="{value}"><Properties><BasedOn>pbase</BasedOn></Properties></ParagraphStyle>
            </RootParagraphStyleGroup><RootCharacterStyleGroup>
            <CharacterStyle Self="cbase" Name="Base" NoBreak="true"/>
            <CharacterStyle Self="cchild" Name="Child" NoBreak="{value}"><Properties><BasedOn>cbase</BasedOn></Properties></CharacterStyle>
            </RootCharacterStyleGroup></idPkg:Styles>"#
        );
        package.insert("Resources/Styles.xml", styles.into_bytes());
        let mut doc = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        for _ in 0..4 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let styles = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
            for group in ["RootParagraphStyleGroup", "RootCharacterStyleGroup"] {
                assert_eq!(
                    styles
                        .find(group)
                        .unwrap()
                        .children
                        .iter()
                        .filter(|s| matches!(s.attr("Name"), Some("Base" | "Child")))
                        .count(),
                    2
                );
                for style in &styles.find(group).unwrap().children {
                    if style.attr("Name") == Some("Child") {
                        assert_eq!(style.attr("NoBreak"), Some(canonical));
                    }
                    if style.attr("Name") == Some("Base") {
                        assert_eq!(style.attr("NoBreak"), Some("true"));
                    }
                }
            }
            doc = import::read(&written.bytes).unwrap().document;
        }
    }
}

#[test]
fn local_no_break_and_false_resets_lower_once_without_changing_text() {
    for base in [false, true] {
        for local in [false, true] {
            let mut doc = schist_layout::blank_a4();
            schist_layout::authoring::text_frame(
                &mut doc,
                &mut schist_layout::History::default(),
                0,
                schist_layout::Rect::new(20.0, 20.0, 200.0, 200.0),
            )
            .unwrap();
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            package.insert("Resources/Styles.xml", format!(r#"<idPkg:Styles><ParagraphStyle Self="p" Name="Parent" NoBreak="{base}"/><CharacterStyle Self="c" Name="Parent" NoBreak="{base}"/></idPkg:Styles>"#).into_bytes());
            let part = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Stories/"))
                .unwrap()
                .to_owned();
            package.insert(&part, format!(r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="p" NoBreak="{local}"><CharacterStyleRange AppliedCharacterStyle="c" NoBreak="{local}"><Content>é café name</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes());
            let mut doc = import::read(&container::write(&package.into_parts()))
                .unwrap()
                .document;
            let counts = (doc.styles.paragraphs.len(), doc.styles.characters.len());
            let source = doc.stories[0].clone();
            for _ in 0..4 {
                let schist_layout::StoryPoint::Paragraph { style, .. } = &doc.stories[0].points[0]
                else {
                    panic!()
                };
                assert_eq!(doc.styles.resolve_paragraph(style).no_break, Some(local));
                assert_eq!(
                    doc.styles
                        .resolve_character(&doc.stories[0].ranges[0].style)
                        .no_break,
                    Some(local)
                );
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
                assert_eq!(doc.stories[0], source);
                assert_eq!(
                    (doc.styles.paragraphs.len(), doc.styles.characters.len()),
                    counts
                );
            }
        }
    }
}

#[test]
fn invalid_native_no_break_is_reported_and_missing_snapshot_fields_inherit() {
    for value in ["yes", "TRUE", "on", "", "tr ue", "\u{a0}true\u{a0}"] {
        let mut package =
            container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
        package.insert("Resources/Styles.xml", format!(r#"<idPkg:Styles><ParagraphStyle Self="p" Name="Invalid" NoBreak="{value}"/><CharacterStyle Self="c" Name="Invalid" NoBreak="{value}"/></idPkg:Styles>"#).into_bytes());
        let read = import::read(&container::write(&package.into_parts())).unwrap();
        let warning = schist_i18n::tf!(
            "design.idml_text_preference_invalid",
            property = "NoBreak",
            value = value
        );
        assert!(read.report.skipped.contains(&warning));
        assert_eq!(
            read.document.styles.resolve_paragraph("Invalid").no_break,
            None
        );
        assert_eq!(
            read.document.styles.resolve_character("Invalid").no_break,
            None
        );
    }
    let mut old = serde_json::to_value(schist_layout::blank_a4()).unwrap();
    for kind in ["paragraphs", "characters"] {
        for style in old["styles"][kind].as_array_mut().unwrap() {
            style.as_object_mut().unwrap().remove("no_break");
        }
    }
    let old: schist_layout::LayoutDocument = serde_json::from_value(old).unwrap();
    assert!(old.styles.paragraphs.iter().all(|s| s.no_break.is_none()));
    assert!(old.styles.characters.iter().all(|s| s.no_break.is_none()));
}
