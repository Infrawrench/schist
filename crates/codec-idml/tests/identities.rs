use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, blank_a4, language::LanguageResource, History, Rect, Story};
use std::collections::BTreeMap;

fn identities(bytes: &[u8]) -> BTreeMap<String, Vec<(String, String)>> {
    let package = container::read(bytes).unwrap();
    let mut out = BTreeMap::new();
    fn walk(node: &xml::Element, part: &str, out: &mut BTreeMap<String, Vec<(String, String)>>) {
        if let Some(id) = node.attr("Self") {
            out.entry(id.into())
                .or_default()
                .push((part.into(), node.name.clone()));
        }
        for child in &node.children {
            walk(child, part, out);
        }
    }
    for part in package.names().into_iter().filter(|p| p.ends_with(".xml")) {
        walk(
            &xml::parse(package.text(part).unwrap()).unwrap(),
            part,
            &mut out,
        );
    }
    out
}

fn source() -> schist_layout::LayoutDocument {
    let mut source = blank_a4();
    let frame = authoring::text_frame(
        &mut source,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    source.stories[frame.story.0 as usize] = Story::from_text("Identity proof", "Body");
    source.styles.add_character(schist_layout::CharacterStyle {
        name: "É <&>".into(),
        ..Default::default()
    });
    source
}

fn unique(bytes: &[u8]) {
    let ids = identities(bytes);
    let duplicates: Vec<_> = ids.iter().filter(|(_, nodes)| nodes.len() != 1).collect();
    assert!(duplicates.is_empty(), "{duplicates:?}");
}

#[test]
fn imported_resource_identities_cannot_collide_with_any_generated_package_object() {
    let source = source();
    let baseline = identities(&export::write(&source).bytes);
    assert!(baseline.values().all(|v| v.len() == 1));
    for id in baseline.keys() {
        for list in [false, true] {
            let mut doc = source.clone();
            if list {
                doc.styles
                    .numbering_lists
                    .push(schist_layout::lists::NumberingList {
                        id: id.clone(),
                        name: "Counter".into(),
                        labels: vec![("Keep".into(), "<&>".into())],
                        ..Default::default()
                    });
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Body")
                    .unwrap();
                style.list.list = Some(id.clone());
                style.list.kind = Some(schist_layout::lists::ListKind::Numbered);
            } else {
                doc.styles.languages.push(LanguageResource {
                    id: id.clone(),
                    name: "$ID/English: USA".into(),
                    labels: vec![("Keep".into(), "<&>".into())],
                    ..Default::default()
                });
                doc.styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Body")
                    .unwrap()
                    .language = Some(id.clone().into());
            }
            let original = doc.clone();
            for _ in 0..3 {
                let before = doc.clone();
                let saved = export::write(&doc);
                unique(&saved.bytes);
                assert_eq!(doc, before);
                let ids = identities(&saved.bytes);
                let package = container::read(&saved.bytes).unwrap();
                let styles = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                let body = styles
                    .find_all("ParagraphStyle")
                    .into_iter()
                    .find(|s| s.attr("Name") == Some("Body"))
                    .unwrap();
                let reference = if list {
                    body.find("AppliedNumberingList").unwrap().text.as_str()
                } else {
                    body.attr("AppliedLanguage").unwrap()
                };
                assert_eq!(
                    ids[reference][0].1,
                    if list { "NumberingList" } else { "Language" }
                );
                doc = import::read(&saved.bytes).unwrap().document;
                assert_eq!(doc.styles.languages, original.styles.languages, "{id}");
                assert_eq!(
                    doc.styles.numbering_lists, original.styles.numbering_lists,
                    "{id}"
                );
                assert_eq!(
                    doc.styles.paragraph("Body"),
                    original.styles.paragraph("Body"),
                    "{id}"
                );
            }
        }
    }
}

#[test]
fn page_item_ids_near_the_numeric_limit_are_unique_and_keep_story_references() {
    for id in [0, 0x7fff_ffff, u32::MAX - 1, u32::MAX] {
        let mut doc = source();
        doc.objects[0].id = schist_layout::ObjectId(id);
        let saved = export::write(&doc);
        unique(&saved.bytes);
        let restored = import::read(&saved.bytes).unwrap().document;
        assert_eq!(restored.objects.len(), 1);
        assert_eq!(restored.stories[0].text(), "Identity proof");
        assert!(matches!(
            restored.objects[0].object,
            schist_layout::LayoutObject::TextFrame {
                story: schist_layout::StoryId(0),
                ..
            }
        ));
    }
}

#[test]
fn fresh_language_ids_do_not_claim_another_resources_id_or_an_unresolved_reference() {
    use schist_layout::{language::TextLanguage, CharacterStyle};
    let mut doc = source();
    doc.styles
        .numbering_lists
        .push(schist_layout::lists::NumberingList {
            id: "SchistLanguage0".into(),
            name: "Counter".into(),
            ..Default::default()
        });
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap()
        .language = Some(TextLanguage::Tag {
        tag: "en-US".into(),
    });
    doc.styles.add_character(CharacterStyle {
        name: "Unresolved".into(),
        language: Some("SchistLanguage1".into()),
        ..Default::default()
    });
    for _ in 0..3 {
        let saved = export::write(&doc);
        unique(&saved.bytes);
        doc = import::read(&saved.bytes).unwrap().document;
        assert_eq!(
            doc.styles.character("Unresolved").unwrap().language,
            Some("SchistLanguage1".into())
        );
        assert_eq!(doc.styles.language_tag("SchistLanguage1"), None);
        assert!(!doc
            .styles
            .languages
            .iter()
            .any(|l| l.id == "SchistLanguage0" || l.id == "SchistLanguage1"));
    }
}

fn colliding(list: bool) -> schist_layout::LayoutDocument {
    let mut doc = source();
    let style = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap();
    if list {
        doc.styles
            .numbering_lists
            .push(schist_layout::lists::NumberingList {
                id: "d".into(),
                name: "Counter".into(),
                ..Default::default()
            });
        style.list.list = Some("d".into());
    } else {
        doc.styles.languages.push(LanguageResource {
            id: "d".into(),
            name: "$ID/English: USA".into(),
            ..Default::default()
        });
        style.language = Some("d".into());
    }
    doc
}

fn edit(bytes: &[u8], mut change: impl FnMut(&str, &str) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = if name.ends_with(".xml") {
                change(name, package.text(name).unwrap()).into_bytes()
            } else {
                package.get(name).unwrap().to_vec()
            };
            (name.to_string(), bytes)
        })
        .collect();
    container::write(&parts)
}

fn emitted_resource(bytes: &[u8], list: bool) -> String {
    let package = container::read(bytes).unwrap();
    let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
    root.find(if list { "NumberingList" } else { "Language" })
        .unwrap()
        .attr("Self")
        .unwrap()
        .into()
}

#[test]
fn native_resource_edits_supersede_saved_identities_and_survive_later_saves() {
    for list in [false, true] {
        let saved = export::write(&colliding(list));
        let native = emitted_resource(&saved.bytes, list);
        let bytes = edit(&saved.bytes, |name, text| {
            if name != "designmap.xml" {
                return text.into();
            }
            let (from, to) = if list {
                (
                    "ContinueNumbersAcrossStories=\"false\"",
                    "ContinueNumbersAcrossStories=\"true\"",
                )
            } else {
                ("Name=\"$ID/English: USA\"", "Name=\"$ID/Japanese\"")
            };
            assert_eq!(text.matches(from).count(), 1);
            text.replace(from, to)
        });
        let mut doc = import::read(&bytes).unwrap().document;
        for _ in 0..3 {
            let body = doc.styles.paragraph("Body").unwrap();
            if list {
                assert_eq!(doc.styles.numbering_lists[0].id, native);
                assert!(doc.styles.numbering_lists[0].across_stories);
                assert_eq!(body.list.list.as_deref(), Some(native.as_str()));
            } else {
                assert_eq!(doc.styles.languages[0].id, native);
                assert_eq!(body.language.as_ref().unwrap().as_str(), native);
                assert_eq!(doc.styles.language_tag(&native).as_deref(), Some("ja"));
            }
            let saved = export::write(&doc);
            unique(&saved.bytes);
            doc = import::read(&saved.bytes).unwrap().document;
        }
    }
}

#[test]
fn changing_resource_class_does_not_restore_another_classes_saved_identity() {
    for list in [false, true] {
        let saved = export::write(&colliding(list));
        let native = emitted_resource(&saved.bytes, list);
        let bytes = edit(&saved.bytes, |name, text| {
            if name != "designmap.xml" {
                return text.into();
            }
            let (from, to) = if list {
                ("NumberingList", "Language")
            } else {
                ("Language", "NumberingList")
            };
            assert_eq!(text.matches(&format!("<{from} ")).count(), 1);
            text.replace(&format!("<{from} "), &format!("<{to} "))
                .replace(&format!("</{from}>"), &format!("</{to}>"))
        });
        let mut doc = import::read(&bytes).unwrap().document;
        for _ in 0..3 {
            let (id, labels) = if list {
                let resource = &doc.styles.languages[0];
                (&resource.id, &resource.labels)
            } else {
                let resource = &doc.styles.numbering_lists[0];
                (&resource.id, &resource.labels)
            };
            assert_eq!(id, &native);
            assert_eq!(
                labels
                    .iter()
                    .filter(|(key, _)| key == "Schist.ResourceIdentity.v1")
                    .count(),
                1
            );
            let saved = export::write(&doc);
            unique(&saved.bytes);
            doc = import::read(&saved.bytes).unwrap().document;
        }
    }
}

#[test]
fn restoration_never_retargets_a_new_native_reference_to_the_old_spelling() {
    for list in [false, true] {
        let saved = export::write(&colliding(list));
        let native = emitted_resource(&saved.bytes, list);
        let bytes = edit(&saved.bytes, |name, text| {
            if name != "Resources/Styles.xml" {
                return text.into();
            }
            let (from, to) = if list {
                (
                    format!(">{native}</AppliedNumberingList>"),
                    ">d</AppliedNumberingList>",
                )
            } else {
                (
                    format!("AppliedLanguage=\"{native}\""),
                    "AppliedLanguage=\"d\"",
                )
            };
            assert_eq!(text.matches(&from).count(), 1);
            text.replace(&from, to)
        });
        let doc = import::read(&bytes).unwrap().document;
        if list {
            assert_eq!(doc.styles.numbering_lists[0].id, native);
            assert_eq!(
                doc.styles.paragraph("Body").unwrap().list.list.as_deref(),
                Some("d")
            );
        } else {
            assert_eq!(doc.styles.languages[0].id, native);
            assert_eq!(doc.styles.language_tag("d"), None);
        }
    }
}

#[test]
fn missing_malformed_and_duplicate_identity_labels_do_not_break_native_references_or_grow() {
    for list in [false, true] {
        for mode in ["remove", "malformed", "duplicate"] {
            let saved = export::write(&colliding(list));
            let native = emitted_resource(&saved.bytes, list);
            let bytes = edit(&saved.bytes, |name, text| {
                if name != "designmap.xml" {
                    return text.into();
                }
                let start = text
                    .find("<KeyValuePair Key=\"Schist.ResourceIdentity.v1\"")
                    .unwrap();
                let end = start + text[start..].find("/>").unwrap() + 2;
                let replacement = match mode {
                    "remove" => String::new(),
                    "malformed" => {
                        "<KeyValuePair Key=\"Schist.ResourceIdentity.v1\" Value=\"not-json\"/>"
                            .into()
                    }
                    _ => text[start..end].repeat(2),
                };
                let mut out = text.to_string();
                out.replace_range(start..end, &replacement);
                out
            });
            let mut doc = import::read(&bytes).unwrap().document;
            let languages = doc.styles.languages.clone();
            let lists = doc.styles.numbering_lists.clone();
            for _ in 0..3 {
                assert_eq!(doc.styles.languages, languages);
                assert_eq!(doc.styles.numbering_lists, lists);
                if list {
                    assert_eq!(
                        doc.styles.paragraph("Body").unwrap().list.list.as_deref(),
                        Some(native.as_str())
                    );
                } else {
                    assert_eq!(
                        doc.styles
                            .paragraph("Body")
                            .unwrap()
                            .language
                            .as_ref()
                            .unwrap()
                            .as_str(),
                        native
                    );
                    assert_eq!(doc.styles.language_tag(&native).as_deref(), Some("en-us"));
                }
                let saved = export::write(&doc);
                unique(&saved.bytes);
                doc = import::read(&saved.bytes).unwrap().document;
            }
        }
    }
}

#[test]
fn resources_with_the_same_authored_id_keep_separate_typed_references() {
    let mut doc = colliding(false);
    doc.styles.numbering_lists = colliding(true).styles.numbering_lists;
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap()
        .list
        .list = Some("d".into());
    let original = doc.clone();
    for _ in 0..3 {
        let saved = export::write(&doc);
        unique(&saved.bytes);
        assert_ne!(
            emitted_resource(&saved.bytes, false),
            emitted_resource(&saved.bytes, true)
        );
        doc = import::read(&saved.bytes).unwrap().document;
        assert_eq!(doc.styles.languages, original.styles.languages);
        assert_eq!(doc.styles.numbering_lists, original.styles.numbering_lists);
        assert_eq!(
            doc.styles.paragraph("Body"),
            original.styles.paragraph("Body")
        );
    }
}

#[test]
fn remapped_ids_do_not_intercept_existing_aliases_or_unresolved_references() {
    use schist_layout::CharacterStyle;
    let mut doc = colliding(false);
    doc.styles.add_character(CharacterStyle {
        name: "Unresolved".into(),
        language: Some("SchistResourceIdentity0".into()),
        ..Default::default()
    });
    doc.styles.languages.push(LanguageResource {
        id: "declared".into(),
        name: "SchistResourceIdentity1".into(),
        primary_name: Some("$ID/French".into()),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Alias".into(),
        language: Some("SchistResourceIdentity1".into()),
        ..Default::default()
    });
    doc.styles.add_paragraph(schist_layout::ParagraphStyle {
        name: "Unknown list".into(),
        list: schist_layout::lists::ListStyle {
            list: Some("SchistResourceIdentity2".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    for _ in 0..3 {
        let saved = export::write(&doc);
        unique(&saved.bytes);
        assert!(![
            "SchistResourceIdentity0",
            "SchistResourceIdentity1",
            "SchistResourceIdentity2"
        ]
        .contains(&emitted_resource(&saved.bytes, false).as_str()));
        doc = import::read(&saved.bytes).unwrap().document;
        assert_eq!(doc.styles.language_tag("SchistResourceIdentity0"), None);
        assert_eq!(
            doc.styles
                .language_tag("SchistResourceIdentity1")
                .as_deref(),
            Some("fr")
        );
        assert_eq!(
            doc.styles
                .paragraph("Unknown list")
                .unwrap()
                .list
                .list
                .as_deref(),
            Some("SchistResourceIdentity2")
        );
    }
}

#[test]
fn every_public_idml_fixture_saves_with_unique_package_identities() {
    for (name, bytes) in [
        (
            "fixtures/idml/shapes.idml",
            include_bytes!("../../../fixtures/idml/shapes.idml").as_slice(),
        ),
        (
            "fixtures/idml/bounded-text.idml",
            include_bytes!("../../../fixtures/idml/bounded-text.idml").as_slice(),
        ),
        (
            "fixtures/idml/placeholders.idml",
            include_bytes!("../../../fixtures/idml/placeholders.idml").as_slice(),
        ),
        (
            "fixtures/idml/themes.idml",
            include_bytes!("../../../fixtures/idml/themes.idml").as_slice(),
        ),
        (
            "fixtures/idml/text.idml",
            include_bytes!("../../../fixtures/idml/text.idml").as_slice(),
        ),
        (
            "fixtures/idml/multipage.idml",
            include_bytes!("../../../fixtures/idml/multipage.idml").as_slice(),
        ),
        (
            "fixtures/idml/images.idml",
            include_bytes!("../../../fixtures/idml/images.idml").as_slice(),
        ),
        (
            "fixtures/indd/proof/proof.idml",
            include_bytes!("../../../fixtures/indd/proof/proof.idml").as_slice(),
        ),
        (
            "fixtures/indd/psu-academic-2/psu-academic-2.idml",
            include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        ),
        (
            "fixtures/indd/psu-literary/psu-literary.idml",
            include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
        ),
    ] {
        let mut doc = import::read(bytes).unwrap().document;
        for _ in 0..3 {
            let saved = export::write(&doc);
            let ids = identities(&saved.bytes);
            let duplicates: Vec<_> = ids
                .iter()
                .filter(|(_, entries)| entries.len() != 1)
                .collect();
            assert!(duplicates.is_empty(), "{name}: {duplicates:?}");
            doc = import::read(&saved.bytes).unwrap().document;
        }
    }
}
