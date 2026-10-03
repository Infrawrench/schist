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
            // Literal custom text now has native definitions/references. The
            // two unsupported preference kinds still remain recovery-only.
            let native: Vec<_> = root.children_named("TextVariable").collect();
            assert_eq!(native.len(), 1);
            assert_eq!(native[0].attr("VariableType"), Some("CustomTextType"));
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

fn custom(id: &str, name: &str, contents: &str) -> String {
    format!(
        r#"<TextVariable Self="{id}" Name="{name}" VariableType="CustomTextType"><CustomTextVariablePreference><Properties><Contents type="string">{contents}</Contents></Properties></CustomTextVariablePreference></TextVariable>"#
    )
}

fn variable_ids(doc: &LayoutDocument) -> Vec<String> {
    retained(doc)
        .iter()
        .filter_map(|s| match &s.control {
            Some(schist_layout::story::InlineControl::TextVariable { variable, .. }) => {
                Some(variable.clone())
            }
            _ => None,
        })
        .collect()
}

fn without_label(root: String, key: &str) -> String {
    let parsed = xml::parse(&root).unwrap();
    let mut out = root;
    for entry in parsed.find_all("KeyValuePair") {
        if entry.attr("Key") == Some(key) {
            let value = quick_xml::escape::escape(entry.attr("Value").unwrap());
            out = out.replace(
                &format!(r#"<KeyValuePair Key="{key}" Value="{value}"/>"#),
                "",
            );
        }
    }
    out
}

#[test]
fn custom_definitions_are_shared_and_native_references_do_not_use_cached_text() {
    for count in [1, 3, 20] {
        let body = (0..count)
            .map(|i| {
                instance(
                    &format!("i{i}"),
                    if i % 2 == 0 { "a" } else { "b" },
                    if i == 0 { "" } else { "stale" },
                )
            })
            .collect::<String>();
        let mut doc = import::read(&native(
            &body,
            &(custom("a", "Same", "  First &amp; café  ") + &custom("b", "Same", "Second")),
        ))
        .unwrap()
        .document;
        let expected = doc.text_variables.clone();
        let structures = retained(&doc);
        let ids = variable_ids(&doc);
        assert_eq!(expected.len(), 2);
        assert_eq!(expected[0].contents, "  First & café  ");
        assert_eq!(ids.len(), count);
        assert!(doc.stories[0].text().is_empty());
        for _ in 0..3 {
            let before = doc.clone();
            let written = export::write(&doc);
            assert_eq!(doc, before);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
            let native: std::collections::BTreeSet<_> = root
                .children_named("TextVariable")
                .map(|e| e.attr("Self").unwrap().to_owned())
                .collect();
            assert_eq!(native.len(), 2);
            let mut references = 0;
            let mut all_ids = std::collections::BTreeSet::new();
            for name in package.names().into_iter().filter(|n| n.ends_with(".xml")) {
                let root = xml::parse(package.text(name).unwrap()).unwrap();
                fn collect(element: &xml::Element, ids: &mut std::collections::BTreeSet<String>) {
                    if let Some(id) = element.attr("Self") {
                        assert!(ids.insert(id.to_owned()), "duplicate {id}");
                    }
                    for child in &element.children {
                        collect(child, ids);
                    }
                }
                collect(&root, &mut all_ids);
                for variable in root.find_all("TextVariableInstance") {
                    assert!(native.contains(variable.attr("AssociatedTextVariable").unwrap()));
                    assert_eq!(variable.attr("ResultText"), Some(""));
                    references += 1;
                }
            }
            assert_eq!(references, count);
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.text_variables, expected);
            assert_eq!(retained(&doc), structures);
            assert_eq!(variable_ids(&doc), ids);
            assert!(doc.stories[0].text().is_empty());
        }
    }
}

#[test]
fn native_custom_values_render_after_repeated_saves_and_inventory_combined_instance_fonts() {
    let body = instance("first", "edition", "stale cached value");
    let mut doc = import::read(&native(&body, &custom("edition", "Edition", "Edition 7")))
        .unwrap()
        .document;
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .family = Some("IBM Plex Sans".into());
    doc.styles.add_character(schist_layout::CharacterStyle {
        name: "Instance face".into(),
        font_style: Some("Light".into()),
        ..Default::default()
    });
    if let Some(schist_layout::story::InlineControl::TextVariable {
        character_style, ..
    }) = &mut doc.stories[0].structures[0].control
    {
        *character_style = "Instance face".into();
    }
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for _ in 0..3 {
        let source = doc.stories[0].clone();
        let composed = compose::compose_story(&doc, StoryId(0));
        assert_eq!(doc.stories[0], source);
        assert_eq!(composed.frames[0].unrendered_structures, 0);
        assert!(composed.frames[0].lines.iter().any(|l| l
            .projected
            .as_ref()
            .is_some_and(|p| p.spec.text.contains("Edition 7"))));
        let saved = export::write(&doc);
        let package = container::read(&saved.bytes).unwrap();
        let fonts = xml::parse(package.text("Resources/Fonts.xml").unwrap()).unwrap();
        assert!(fonts
            .find_all("Font")
            .iter()
            .any(|f| f.attr("FontFamily") == Some("IBM Plex Sans")
                && f.attr("FontStyleName") == Some("Light")));
        doc = import::read(&saved.bytes).unwrap().document;
    }
}

#[test]
fn native_definition_reordering_and_metadata_removal_cannot_retarget_instances() {
    let body = instance("first", "alpha", "") + &instance("second", "beta", "cached");
    let doc = import::read(&native(
        &body,
        &(custom("alpha", "Same", "A") + &custom("beta", "Same", "B")),
    ))
    .unwrap()
    .document;
    let saved = export::write(&doc).bytes;
    for strip in [false, true] {
        let changed = rewrite_designmap(&saved, |root| {
            let parsed = xml::parse(&root).unwrap();
            let values: Vec<_> = parsed
                .children_named("TextVariable")
                .map(|e| e.raw.as_ref().unwrap().to_string())
                .collect();
            let mut out = root.replace(
                &values.concat(),
                &(custom("new", "Same", "new") + &values[1] + &values[0]),
            );
            assert_ne!(out, root);
            if strip {
                out = without_label(out, "Schist.CustomTextVariables.v1");
            }
            out
        });
        let mut reopened = import::read(&changed).unwrap().document;
        for _ in 0..3 {
            let ids = variable_ids(&reopened);
            assert_eq!(ids.len(), 2);
            let values: Vec<_> = ids
                .iter()
                .map(|id| {
                    reopened
                        .text_variables
                        .iter()
                        .find(|d| &d.id == id)
                        .unwrap()
                        .contents
                        .as_str()
                })
                .collect();
            assert_eq!(values, ["A", "B"]);
            reopened = import::read(&export::write(&reopened).bytes)
                .unwrap()
                .document;
        }
    }
}

#[test]
fn native_custom_definition_edits_and_deletions_supersede_saved_identity_data() {
    let doc = import::read(&native(
        &instance("instance", "authored", "cached"),
        &custom("authored", "Original", "before"),
    ))
    .unwrap()
    .document;
    let saved = export::write(&doc).bytes;
    for delete in [false, true] {
        let changed = rewrite_designmap(&saved, |root| {
            let parsed = xml::parse(&root).unwrap();
            let definition = parsed
                .children_named("TextVariable")
                .next()
                .unwrap()
                .raw
                .as_ref()
                .unwrap();
            root.replace(
                definition.as_ref(),
                if delete {
                    String::new()
                } else {
                    definition
                        .replace("Original", "Changed")
                        .replace(">before<", ">after<")
                }
                .as_str(),
            )
        });
        let mut reopened = import::read(&changed).unwrap().document;
        for _ in 0..3 {
            if delete {
                assert!(reopened.text_variables.is_empty());
                assert_eq!(variable_ids(&reopened), ["authored"]);
            } else {
                assert_eq!(reopened.text_variables.len(), 1);
                let definition = &reopened.text_variables[0];
                assert_eq!(definition.name, "Changed");
                assert_eq!(definition.contents, "after");
                assert_eq!(
                    variable_ids(&reopened),
                    std::slice::from_ref(&definition.id)
                );
            }
            assert!(reopened.stories[0].text().is_empty());
            reopened = import::read(&export::write(&reopened).bytes)
                .unwrap()
                .document;
        }
    }
}

#[test]
fn unsupported_and_ambiguous_custom_preferences_stay_recoverable_and_unresolved() {
    let valid = custom("a", "Custom", "hello");
    for raw in [
        valid.replace("type=\"string\"", "type=\"enum\""),
        valid.replace(
            "<CustomTextVariablePreference>",
            "<CustomTextVariablePreference Future=\"true\">",
        ),
        valid.replace("hello", "before<Unknown/>after"),
        valid.replace("hello", "before<?ACE 18?>after"),
        valid.clone() + &custom("a", "Duplicate", "different"),
    ] {
        let mut doc = import::read(&native(&instance("i", "a", "cached"), &raw))
            .unwrap()
            .document;
        let archive = doc.retained_text_variables.clone();
        for _ in 0..3 {
            assert!(doc.text_variables.is_empty());
            assert_eq!(doc.retained_text_variables, archive);
            assert_eq!(variable_ids(&doc), ["a"]);
            assert_eq!(
                compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
                1
            );
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
            assert!(root.children_named("TextVariable").next().is_none());
            doc = import::read(&written.bytes).unwrap().document;
        }
    }
}

fn rewrite_stories(bytes: &[u8], mut edit: impl FnMut(String) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    container::write(
        &package
            .names()
            .into_iter()
            .map(|name| {
                let bytes = if name.starts_with("Stories/") {
                    edit(package.text(name).unwrap().into()).into_bytes()
                } else {
                    package.get(name).unwrap().to_vec()
                };
                (name.to_owned(), bytes)
            })
            .collect::<Vec<_>>(),
    )
}

#[test]
fn restored_identities_cannot_capture_new_unresolved_native_references() {
    let doc = import::read(&native(
        &instance("instance", "authored", ""),
        &custom("authored", "Custom", "defined"),
    ))
    .unwrap()
    .document;
    let changed = rewrite_stories(&export::write(&doc).bytes, |story| {
        let fresh = format!(
            r#"<ParagraphStyleRange><CharacterStyleRange>{}</CharacterStyleRange></ParagraphStyleRange>"#,
            instance("fresh", "authored", "must stay unresolved")
        );
        story.replace("</Story>", &(fresh + "</Story>"))
    });
    let mut reopened = import::read(&changed).unwrap().document;
    for _ in 0..3 {
        assert_eq!(reopened.text_variables.len(), 1);
        assert_ne!(reopened.text_variables[0].id, "authored");
        let ids = variable_ids(&reopened);
        assert!(ids.contains(&"authored".to_owned()));
        assert!(ids.contains(&reopened.text_variables[0].id));
        reopened = import::read(&export::write(&reopened).bytes)
            .unwrap()
            .document;
    }
}

#[test]
fn removed_native_instances_stay_unplaced_and_class_changes_do_not_restore_definitions() {
    let doc = import::read(&native(
        &instance("instance", "authored", ""),
        &custom("authored", "Custom", "defined"),
    ))
    .unwrap()
    .document;
    let saved = export::write(&doc).bytes;
    let deleted = rewrite_stories(&saved, |story| {
        let root = xml::parse(&story).unwrap();
        let instance = root
            .find("TextVariableInstance")
            .unwrap()
            .raw
            .as_ref()
            .unwrap();
        story.replace(instance.as_ref(), "")
    });
    let mut reopened = import::read(&deleted).unwrap().document;
    for _ in 0..3 {
        assert_eq!(retained(&reopened).len(), 1);
        assert!(retained(&reopened).iter().all(|s| s.at.is_none()));
        reopened = import::read(&export::write(&reopened).bytes)
            .unwrap()
            .document;
    }
    let changed = rewrite_designmap(&saved, |root| {
        root.replace(
            "VariableType=\"CustomTextType\"",
            "VariableType=\"OutputDateType\"",
        )
    });
    let mut reopened = import::read(&changed).unwrap().document;
    for _ in 0..3 {
        assert!(reopened.text_variables.is_empty());
        assert_eq!(variable_ids(&reopened), ["authored"]);
        reopened = import::read(&export::write(&reopened).bytes)
            .unwrap()
            .document;
    }
}

#[test]
fn custom_variable_metadata_corruption_does_not_override_native_definitions() {
    let doc = import::read(&native(
        &instance("instance", "authored", ""),
        &custom("authored", "Custom", "defined"),
    ))
    .unwrap()
    .document;
    let saved = export::write(&doc).bytes;
    for mode in 0..3 {
        let changed = rewrite_designmap(&saved, |root| {
            if mode > 0 {
                let parsed = xml::parse(&root).unwrap();
                let value = parsed
                    .find_all("KeyValuePair")
                    .into_iter()
                    .find(|e| e.attr("Key") == Some("Schist.CustomTextVariables.v1"))
                    .unwrap()
                    .attr("Value")
                    .unwrap();
                let mut records: Vec<serde_json::Value> = serde_json::from_str(value).unwrap();
                records.push(records[0].clone());
                let invalid = serde_json::to_string(&records).unwrap();
                let value = if mode == 1 { value } else { &invalid };
                let entry = format!(
                    r#"<KeyValuePair Key="Schist.CustomTextVariables.v1" Value="{}"/></Label>"#,
                    quick_xml::escape::escape(value)
                );
                let root = if mode == 1 {
                    root.clone()
                } else {
                    without_label(root.clone(), "Schist.CustomTextVariables.v1")
                };
                root.replace("</Label>", &entry)
            } else {
                without_label(root, "Schist.CustomTextVariables.v1").replace("</Label>", r#"<KeyValuePair Key="Schist.CustomTextVariables.v1" Value="invalid"/></Label>"#)
            }
        });
        let mut reopened = import::read(&changed).unwrap().document;
        let archive = reopened.retained_text_variables.clone();
        assert!(archive
            .iter()
            .any(|raw| raw.contains("Schist.CustomTextVariables.v1")));
        for _ in 0..3 {
            assert_eq!(reopened.text_variables.len(), 1);
            assert_eq!(reopened.text_variables[0].id, "SchistTextVariable0");
            assert_eq!(reopened.text_variables[0].contents, "defined");
            assert_eq!(variable_ids(&reopened), ["SchistTextVariable0"]);
            assert_eq!(reopened.retained_text_variables, archive);
            reopened = import::read(&export::write(&reopened).bytes)
                .unwrap()
                .document;
        }
    }
}

#[test]
fn native_variable_output_preserves_order_at_every_source_boundary_without_private_story_data() {
    let source = "Aé中🙂B";
    for at in source
        .char_indices()
        .map(|(at, _)| at)
        .chain([source.len()])
    {
        let controls = instance("first", "a", "")
            + "<Content><?ACE 3?></Content>"
            + &instance("second", "b", "");
        let body = format!(
            "<Content>{}</Content>{controls}<Content>{}</Content>",
            &source[..at],
            &source[at..]
        );
        let original = import::read(&native(
            &body,
            &(custom("a", "Name", "A") + &custom("b", "Name", "B")),
        ))
        .unwrap()
        .document;
        for removed in 0..4 {
            let mut doc = original.clone();
            for _ in 0..3 {
                let mut saved = export::write(&doc).bytes;
                if removed & 1 != 0 {
                    saved = rewrite_stories(&saved, |story| {
                        without_label(story, "Schist.StructuredStory.v1")
                    });
                }
                if removed & 2 != 0 {
                    saved = rewrite_designmap(&saved, |root| {
                        without_label(root, "Schist.CustomTextVariables.v1")
                    });
                }
                doc = import::read(&saved).unwrap().document;
                assert_eq!(doc.stories[0].text(), source);
                assert_eq!(doc.stories[0].structures.len(), 3);
                assert!(doc.stories[0].structures.iter().all(|s| s.at == Some(at)));
                let ids = variable_ids(&doc);
                let values: Vec<_> = ids
                    .iter()
                    .map(|id| {
                        doc.text_variables
                            .iter()
                            .find(|d| &d.id == id)
                            .unwrap()
                            .contents
                            .as_str()
                    })
                    .collect();
                assert_eq!(values, ["A", "B"]);
                assert!(matches!(
                    doc.stories[0].structures[1].control,
                    Some(schist_layout::story::InlineControl::EndNestedStyle { .. })
                ));
            }
        }
    }
}

#[test]
fn variable_style_renames_and_source_replacements_each_undo_once() {
    let mut doc = import::read(&native(
        &format!(
            "<Content>Aé</Content>{}<Content>B</Content>",
            instance("i", "a", "")
        ),
        &custom("a", "Custom", "display"),
    ))
    .unwrap()
    .document;
    let original = doc.clone();
    let old = match &doc.stories[0].structures[0].control {
        Some(schist_layout::story::InlineControl::TextVariable {
            character_style, ..
        }) => character_style.clone(),
        _ => panic!("missing typed instance"),
    };
    let mut history = History::default();
    assert!(schist_layout::properties::rename_style(
        &mut doc,
        &mut history,
        false,
        &old,
        "Renamed"
    ));
    assert_eq!(history.undo_depth(), 1);
    let renamed = doc.clone();
    assert!(
        matches!(&doc.stories[0].structures[0].control, Some(schist_layout::story::InlineControl::TextVariable { character_style, .. }) if character_style == "Renamed")
    );
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, renamed);
    assert!(authoring::replace_text(
        &mut doc,
        &mut history,
        StoryId(0),
        0..1,
        "Ω"
    ));
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(doc.stories[0].structures[0].at, Some(4));
    assert!(history.undo(&mut doc));
    assert_eq!(doc, renamed);
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(retained(&doc), retained(&renamed));
        assert_eq!(doc.text_variables, renamed.text_variables);
    }
}

#[test]
fn variable_native_ids_remain_unique_alongside_colliding_opaque_resources() {
    let source = import::read(&native(
        &instance("i", "d", ""),
        &custom("d", "Variable", "contents"),
    ))
    .unwrap()
    .document;
    for collision in ["SchistTextVariable0", "SchistVariableInstanceu101_0"] {
        let mut doc = source.clone();
        doc.styles
            .languages
            .push(schist_layout::language::LanguageResource {
                id: collision.into(),
                name: "$ID/English: USA".into(),
                ..Default::default()
            });
        doc.styles
            .numbering_lists
            .push(schist_layout::lists::NumberingList {
                id: collision.into(),
                name: "List".into(),
                ..Default::default()
            });
        for _ in 0..3 {
            let before = doc.clone();
            let saved = export::write(&doc);
            assert_eq!(doc, before);
            let package = container::read(&saved.bytes).unwrap();
            let mut ids = std::collections::BTreeSet::new();
            fn walk(element: &xml::Element, ids: &mut std::collections::BTreeSet<String>) {
                if let Some(id) = element.attr("Self") {
                    assert!(ids.insert(id.into()), "duplicate: {id}");
                }
                for child in &element.children {
                    walk(child, ids);
                }
            }
            for part in package.names().into_iter().filter(|p| p.ends_with(".xml")) {
                walk(&xml::parse(package.text(part).unwrap()).unwrap(), &mut ids);
            }
            doc = import::read(&saved.bytes).unwrap().document;
            assert_eq!(doc.text_variables, source.text_variables);
            assert_eq!(variable_ids(&doc), ["d"]);
            assert_eq!(doc.styles.languages[0].id, collision);
            assert_eq!(doc.styles.numbering_lists[0].id, collision);
        }
    }
}

#[test]
fn generated_native_ids_cannot_activate_previously_unresolved_saved_instances() {
    let body = instance("resolved", "known", "")
        + &instance("unresolved", "SchistTextVariable0", "cached");
    let original = import::read(&native(&body, &custom("known", "Custom", "defined")))
        .unwrap()
        .document;
    for legacy in [false, true] {
        let mut doc = original.clone();
        if legacy {
            doc.stories[0].structures[1].control = None;
        }
        for _ in 0..3 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(variable_ids(&doc), ["known", "SchistTextVariable0"]);
            assert_eq!(doc.text_variables.len(), 1);
            assert_eq!(doc.text_variables[0].id, "known");
        }
    }
}

#[test]
fn external_definition_edits_cannot_retarget_archived_unresolved_native_spellings() {
    let body = instance("resolved", "known", "")
        + &instance("unresolved", "SchistTextVariable0", "cached");
    let original = import::read(&native(&body, &custom("known", "Custom", "before")))
        .unwrap()
        .document;
    for legacy in [false, true] {
        let mut source = original.clone();
        if legacy {
            source.stories[0].structures[1].control = None;
        }
        let changed = rewrite_designmap(&export::write(&source).bytes, |root| {
            let parsed = xml::parse(&root).unwrap();
            let raw = parsed
                .children_named("TextVariable")
                .next()
                .unwrap()
                .raw
                .as_ref()
                .unwrap();
            root.replace(raw.as_ref(), &raw.replace(">before<", ">after<"))
        });
        let mut doc = import::read(&changed).unwrap().document;
        for _ in 0..3 {
            let ids = variable_ids(&doc);
            assert_eq!(ids.len(), 2);
            assert_ne!(ids[0], ids[1]);
            assert_eq!(ids[1], "SchistTextVariable0");
            assert_eq!(doc.text_variables.len(), 1);
            assert_eq!(doc.text_variables[0].id, ids[0]);
            assert_eq!(doc.text_variables[0].contents, "after");
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
        }
    }
}

#[test]
fn deleted_bindings_keep_distinct_missing_identities_when_new_resources_reuse_their_names() {
    let body = instance("resolved", "known", "")
        + &instance("unresolved", "SchistTextVariable0", "cached");
    let original = import::read(&native(&body, &custom("known", "Custom", "before")))
        .unwrap()
        .document;
    for legacy in [false, true] {
        for fresh_instance in [false, true] {
            let mut source = original.clone();
            if legacy {
                source.stories[0].structures[1].control = None;
            }
            let mut changed = rewrite_designmap(&export::write(&source).bytes, |root| {
                let parsed = xml::parse(&root).unwrap();
                let raw = parsed
                    .children_named("TextVariable")
                    .next()
                    .unwrap()
                    .raw
                    .as_ref()
                    .unwrap();
                root.replace(
                    raw.as_ref(),
                    &custom("known", "New resource", "replacement"),
                )
            });
            if fresh_instance {
                changed = rewrite_stories(&changed, |story| {
                    story.replace("</Story>", &format!(
                        "<ParagraphStyleRange><CharacterStyleRange>{}</CharacterStyleRange></ParagraphStyleRange></Story>",
                        instance("fresh", "known", "")
                    ))
                });
            }
            let mut doc = import::read(&changed).unwrap().document;
            let mut stable_ids = None;
            for _ in 0..3 {
                assert_eq!(doc.text_variables.len(), 1);
                let definition = &doc.text_variables[0];
                assert_eq!(definition.contents, "replacement");
                assert_ne!(definition.id, "known");
                assert_ne!(definition.id, "SchistTextVariable0");
                let ids: Vec<_> = retained(&doc)
                    .iter()
                    .map(|structure| {
                        if let Some(schist_layout::story::InlineControl::TextVariable {
                            variable,
                            ..
                        }) = &structure.control
                        {
                            variable.clone()
                        } else {
                            // A stale story guard preserves legacy instances as raw,
                            // unplaced data until a subsequent guarded save upgrades them.
                            xml::parse(&structure.payload)
                                .unwrap()
                                .find("TextVariableInstance")
                                .unwrap()
                                .attr("AssociatedTextVariable")
                                .unwrap()
                                .to_owned()
                        }
                    })
                    .collect();
                if fresh_instance {
                    // Editing the native body leaves the old instances unplaced;
                    // its original native reference is still present but missing.
                    // Only the newly authored native instance resolves.
                    assert_eq!(
                        ids,
                        [
                            "SchistTextVariable0",
                            &definition.id,
                            "known",
                            "SchistTextVariable0"
                        ]
                    );
                    assert!(retained(&doc)[2..].iter().all(|s| s.at.is_none()));
                } else {
                    assert_eq!(ids, ["known", "SchistTextVariable0"]);
                }
                if let Some(expected) = &stable_ids {
                    assert_eq!(&ids, expected);
                }
                stable_ids = Some(ids);
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
            }
        }
    }
}
