use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, blank_a4, compose, History, LayoutDocument, Rect, Story, StoryId};

fn native(content: &str, definitions: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("seed", "Body");
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                let root = xml::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
                let id = root.find("Story").unwrap().attr("Self").unwrap();
                format!(r#"<idPkg:Story><Story Self="{id}"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PointSize="23" PageNumberType="TextVariable">{content}</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes()
            } else if name == "designmap.xml" {
                std::str::from_utf8(bytes).unwrap().replace("</Document>", &format!("{definitions}</Document>")).into_bytes()
            } else {
                bytes.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    container::write(&parts)
}

fn instance(id: &str, definition: &str, result: &str) -> String {
    format!(
        r#"<TextVariableInstance Self="{id}" Name="Date &amp; time" AssociatedTextVariable="{definition}" ResultText="{result}"/>"#
    )
}

fn retained(doc: &LayoutDocument) -> Vec<schist_layout::StoryStructure> {
    doc.stories
        .iter()
        .flat_map(|s| s.structures.iter())
        .filter(|s| s.kind == "TextVariableInstance")
        .cloned()
        .collect()
}

#[test]
fn every_native_variable_instance_survives_at_its_source_anchor_with_formatting() {
    let text = "Aé中🙂B";
    for at in text.char_indices().map(|(at, _)| at).chain([text.len()]) {
        let raw = instance("variable", "definition", "2015 &amp; now");
        let content = format!(
            "<Content>{}</Content>{raw}<Content>{}</Content>",
            &text[..at],
            &text[at..]
        );
        let mut doc = import::read(&native(&content, "")).unwrap().document;
        assert_eq!(doc.stories[0].text(), text);
        let expected = retained(&doc);
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].at, Some(at));
        assert_eq!(
            compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
            1
        );
        assert!(expected[0].payload.contains(&raw));
        let context = xml::parse(&expected[0].payload).unwrap();
        let range = context.find("CharacterStyleRange").unwrap();
        assert_eq!(range.attr("PageNumberType"), Some("TextVariable"));
        let style = range
            .attr("AppliedCharacterStyle")
            .unwrap()
            .strip_prefix("CharacterStyle/$ID/")
            .unwrap();
        assert_eq!(doc.styles.resolve_character(style).point_size, Some(23.0));
        for _ in 0..3 {
            let exported = export::write(&doc);
            assert!(exported
                .warnings
                .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
            doc = import::read(&exported.bytes).unwrap().document;
            assert_eq!(retained(&doc), expected);
        }
    }
}

#[test]
fn public_output_date_variables_are_retained_instead_of_disappearing() {
    let bytes = include_bytes!("../../../fixtures/indd/proof/proof.idml");
    let mut doc = import::read(bytes).unwrap().document;
    let expected = retained(&doc);
    assert_eq!(expected.len(), 3);
    let definitions = doc.retained_text_variables.clone();
    assert_eq!(definitions.len(), 11);
    let package = container::read(bytes).unwrap();
    for definition in &definitions {
        assert!(package.text("designmap.xml").unwrap().contains(definition));
    }
    assert!(definitions.iter().any(|definition| {
        let parsed = xml::parse(definition).unwrap();
        parsed.attr("Self") == Some("dTextVariablenOutput Date and Time")
            && parsed
                .child("DateVariablePreference")
                .is_some_and(|preference| preference.attr("Format") == Some("YYYY-MM-dd @ hh:mma"))
    }));
    for value in &expected {
        let context = xml::parse(&value.payload).unwrap();
        let variable = context.find("TextVariableInstance").unwrap();
        assert_eq!(variable.attr("ResultText"), Some("2015-02-19 @ 11:14PM"));
        assert_eq!(
            variable.attr("AssociatedTextVariable"),
            Some("dTextVariablenOutput Date and Time")
        );
    }
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(retained(&doc), expected);
        assert_eq!(doc.retained_text_variables, definitions);
    }
}

#[test]
fn variable_only_and_nested_content_keep_order_and_single_step_edits() {
    let first = instance("one", "shared", "first");
    let second = instance("two", "shared", "second");
    for prefix in ["", "<Content>é</Content><Br/>"] {
        let mut doc = import::read(&native(&format!("{prefix}{first}{second}"), ""))
            .unwrap()
            .document;
        assert_eq!(retained(&doc).len(), 2);
        assert!(retained(&doc)
            .iter()
            .all(|s| s.at == Some(doc.stories[0].text().len())));
        let before = doc.clone();
        let mut history = History::default();
        assert!(authoring::replace_text(
            &mut doc,
            &mut history,
            StoryId(0),
            0..0,
            "Ω"
        ));
        assert_eq!(history.undo_depth(), 1);
        for (now, old) in retained(&doc).iter().zip(retained(&before)) {
            assert_eq!(now.at, old.at.map(|at| at + 2));
            assert_eq!(now.payload, old.payload);
        }
        let edited = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, edited);
    }
    for container in ["Footnote", "Table", "TextFrame"] {
        let raw = format!("<{container}><ParagraphStyleRange><CharacterStyleRange>{first}{second}</CharacterStyleRange></ParagraphStyleRange></{container}>");
        let doc = import::read(&native(&raw, "")).unwrap().document;
        assert!(retained(&doc).is_empty());
        assert_eq!(doc.stories[0].structures.len(), 1);
        assert_eq!(doc.stories[0].structures[0].payload, raw);
        assert!(doc.stories[0].structures[0].footnote.is_none());
    }
}

fn rewrite_designmap(bytes: &[u8], edit: impl FnOnce(String) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    let changed = edit(package.text("designmap.xml").unwrap().to_owned());
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                if name == "designmap.xml" {
                    changed.as_bytes().to_vec()
                } else {
                    package.get(name).unwrap().to_vec()
                },
            )
        })
        .collect();
    container::write(&parts)
}

#[test]
fn definitions_are_shared_exact_recovery_data_including_unknown_preferences() {
    let definitions = [
        r#"<TextVariable Self="opaque&amp;é" Name="Same name" VariableType="OutputDateType"><DateVariablePreference Format="YYYY-MM-dd @ hh:mma" TextBefore=" " TextAfter="&#xA;"/></TextVariable>"#.to_owned(),
        r#"<TextVariable Name="Same name" Self="different" VariableType="FutureType"><FuturePreference a="&lt;value&gt;">before<![CDATA[<nested>&]]><!--retain-->after<?future value?><Child/>tail</FuturePreference></TextVariable>"#.to_owned(),
        format!(r#"<TextVariable Self="big" Name="Shared" VariableType="CustomTextType"><CustomTextVariablePreference><Properties><Contents type="string">{}</Contents></Properties></CustomTextVariablePreference></TextVariable>"#, "shared-contents".repeat(4096)),
    ];
    for count in [0, 1, 32] {
        let body = (0..count)
            .map(|i| instance(&format!("instance{i}"), "big", "cached"))
            .collect::<String>();
        let mut doc = import::read(&native(&body, &definitions.concat()))
            .unwrap()
            .document;
        assert_eq!(doc.retained_text_variables, definitions);
        assert_eq!(retained(&doc).len(), count);
        assert!(retained(&doc)
            .iter()
            .all(|s| !s.payload.contains("shared-contents")));
        for _ in 0..3 {
            let exported = export::write(&doc);
            let package = container::read(&exported.bytes).unwrap();
            let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
            assert!(root.children_named("TextVariable").next().is_none(), "Opaque retention must not pretend to restore unresolved native resource references");
            assert_eq!(root.children_named("Properties").count(), 1);
            let entry = root
                .find_all("KeyValuePair")
                .into_iter()
                .find(|e| e.attr("Key") == Some("Schist.TextVariables.v1"))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Vec<String>>(entry.attr("Value").unwrap()).unwrap(),
                definitions
            );
            doc = import::read(&exported.bytes).unwrap().document;
            assert_eq!(doc.retained_text_variables, definitions);
        }
    }
}

#[test]
fn fresh_native_definitions_replace_archived_identity_without_name_matching_or_growth() {
    let old = [
        r#"<TextVariable Self="one" Name="Same" VariableType="OutputDateType"/>"#,
        r#"<TextVariable Self="two" Name="Same" VariableType="CreationDateType"/>"#,
        r#"<TextVariable Name="No identity"/>"#,
    ];
    let fresh = r#"<TextVariable Self="one" Name="Edited" VariableType="CustomTextType"><CustomTextVariablePreference><Properties><Contents type="string">changed</Contents></Properties></CustomTextVariablePreference></TextVariable>"#;
    let added = r#"<TextVariable Self="three" Name="Same" VariableType="FutureType"/>"#;
    let original = import::read(&native("", &old.concat())).unwrap().document;
    let bytes = rewrite_designmap(&export::write(&original).bytes, |root| {
        root.replace("</Document>", &format!("{fresh}{added}</Document>"))
    });
    let mut doc = import::read(&bytes).unwrap().document;
    let expected = [fresh, added, old[1], old[2]].map(str::to_owned).to_vec();
    assert_eq!(doc.retained_text_variables, expected);
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.retained_text_variables, expected);
    }
}

#[test]
fn malformed_or_duplicate_variable_metadata_remains_inert_and_legacy_documents_default_empty() {
    let def = r#"<TextVariable Self="one" Name="One"/>"#;
    let doc = import::read(&native("", def)).unwrap().document;
    let bytes = rewrite_designmap(&export::write(&doc).bytes, |root| {
        let root = root.replace("</Label>", r#"<KeyValuePair Key="Schist.TextVariables.v1" Value="bad&lt;json&amp;&quot;"/></Label>"#);
        // Duplicate valid labels remain recovery data, not additional native resources.
        let parsed = xml::parse(&root).unwrap();
        let value = parsed.find_all("KeyValuePair")[0].attr("Value").unwrap();
        let value = quick_xml::escape::escape(value);
        root.replace(
            "</Label>",
            &format!(r#"<KeyValuePair Key="Schist.TextVariables.v1" Value="{value}"/></Label>"#),
        )
    });
    let mut doc = import::read(&bytes).unwrap().document;
    let expected = doc.retained_text_variables.clone();
    assert_eq!(expected.len(), 3);
    assert_eq!(expected.iter().filter(|raw| raw.as_str() == def).count(), 2);
    let wrapper = xml::parse(&expected[1]).unwrap();
    assert_eq!(
        wrapper.find("KeyValuePair").unwrap().attr("Value"),
        Some("bad<json&\"")
    );
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.retained_text_variables, expected);
    }
    let mut legacy = serde_json::to_value(blank_a4()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("retained_text_variables");
    let legacy: LayoutDocument = serde_json::from_value(legacy).unwrap();
    assert!(legacy.retained_text_variables.is_empty());
    assert!(!serde_json::to_string(&legacy)
        .unwrap()
        .contains("retained_text_variables"));
}

#[test]
fn native_instance_edits_keep_fresh_positions_and_preserve_archived_payloads_unplaced() {
    let first = instance("one", "shared", "old");
    let fresh = instance("one", "shared", "new");
    let doc = import::read(&native(
        &format!("<Content>Aé</Content>{first}<Content>B</Content>"),
        "",
    ))
    .unwrap()
    .document;
    let saved = export::write(&doc);
    let package = container::read(&saved.bytes).unwrap();
    let mut changed = false;
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let original = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                let original = std::str::from_utf8(original).unwrap();
                let edited = original.replace(
                    "<Content>AéB</Content>",
                    &format!("<Content>Aé</Content>{fresh}<Content>B</Content>"),
                );
                changed |= original != edited;
                edited.into_bytes()
            } else {
                original.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    assert!(changed);
    let imported = import::read(&container::write(&parts)).unwrap();
    assert!(imported
        .report
        .skipped
        .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
    let mut doc = imported.document;
    let expected = retained(&doc);
    assert_eq!(expected.len(), 2);
    assert_eq!(expected[0].at, Some(3));
    assert!(expected[0].payload.contains(&fresh));
    assert_eq!(expected[1].at, None);
    assert!(expected[1].payload.contains(&first));
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(retained(&doc), expected);
    }
}
