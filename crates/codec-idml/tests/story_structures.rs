use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, blank_a4, History, LayoutDocument, Rect, Story, StoryPoint, StoryStructure,
};

fn document(legacy: bool) -> LayoutDocument {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 200.0, 100.0),
    )
    .unwrap();
    let mut story = Story::from_text("AéB", "Body");
    let payload = "<Footnote Self=\"fn\"><Content>é&amp;</Content><?ACE 4?><Table/></Footnote>";
    if legacy {
        story.points.push(StoryPoint::Other {
            kind: "Footnote".into(),
            payload: payload.into(),
        });
        story.push_paragraph("tail", "Body");
    } else {
        story.structures.push(StoryStructure {
            at: Some(1),
            kind: "Footnote".into(),
            payload: payload.into(),
            footnote: None,
        });
    }
    doc.stories[frame.story.0 as usize] = story;
    doc
}

fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, String) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            (
                name.to_owned(),
                if name.ends_with(".xml") {
                    edit(name, String::from_utf8(bytes.to_vec()).unwrap()).into_bytes()
                } else {
                    bytes.to_vec()
                },
            )
        })
        .collect();
    container::write(&parts)
}

#[test]
fn raw_outer_structures_preserve_mixed_content_pis_entities_and_exact_source_anchors() {
    for kind in [
        "Table",
        "Footnote",
        "TextFrame",
        "Rectangle",
        "Polygon",
        "Oval",
        "GraphicLine",
        "Group",
    ] {
        for payload in [
            format!("<{kind} Self=\"inline\"/>"),
            format!("<{kind}>a&amp;<?ACE 4?><Table><Content>nested</Content></Table>尾</{kind}>"),
        ] {
            let bytes = export::write(&document(false)).bytes;
            let bytes = rewrite(&bytes, |name, xml| {
                if name.starts_with("Stories/") {
                    // Remove the Schist retention record; this test imports native XML.
                    let start = xml.find("<Properties>").unwrap();
                    let end = xml.find("</Properties>").unwrap() + "</Properties>".len();
                    let mut xml = xml;
                    xml.replace_range(start..end, "");
                    xml.replace(
                        "<Content>AéB</Content>",
                        &format!("<Content>Aé</Content>{payload}<Content>B</Content>"),
                    )
                } else {
                    xml
                }
            });
            let imported = import::read(&bytes).unwrap();
            let story = &imported.document.stories[0];
            assert_eq!(story.text(), "AéB");
            assert_eq!(story.structures.len(), 1);
            let structure = &story.structures[0];
            assert_eq!(structure.at, Some(3));
            assert_eq!(structure.kind, kind);
            assert_eq!(structure.payload, payload);
            let tree = xml::parse(&payload).unwrap();
            assert_eq!(tree.raw.as_deref(), Some(payload.as_str()));
            assert!(tree.find_all("Table").iter().all(|t| t.raw.is_none()));
        }
    }
}

#[test]
fn published_tables_footnotes_and_inline_math_survive_repeated_saves_as_inert_data() {
    let bytes = include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml");
    let package = container::read(bytes).unwrap();
    let mut doc = import::read(bytes).unwrap().document;
    let expected: Vec<_> = doc
        .stories
        .iter()
        .filter(|s| s.retained_structures() > 0)
        .cloned()
        .collect();
    let structures: Vec<_> = expected.iter().flat_map(|s| &s.structures).collect();
    for kind in ["Table", "Footnote", "Rectangle"] {
        assert!(structures.iter().any(|s| s.kind == kind), "{kind}");
    }
    assert!(structures.iter().any(|s| s.payload.contains("MathObject")));
    for s in &structures {
        assert!(package
            .names()
            .iter()
            .filter(|n| n.starts_with("Stories/"))
            .any(|name| std::str::from_utf8(package.get(name).unwrap())
                .unwrap()
                .contains(&s.payload)));
    }
    for _ in 0..4 {
        let saved = export::write(&doc);
        assert!(saved
            .warnings
            .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
        let imported = import::read(&saved.bytes).unwrap();
        assert!(imported
            .report
            .skipped
            .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
        assert!(!imported
            .report
            .skipped
            .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
        doc = imported.document;
        assert_eq!(
            doc.stories
                .iter()
                .filter(|s| s.retained_structures() > 0)
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn legacy_points_and_anchored_data_round_trip_without_making_them_native_content() {
    for legacy in [false, true] {
        let mut doc = document(legacy);
        let expected = doc.stories.clone();
        for _ in 0..4 {
            let saved = export::write(&doc);
            let package = container::read(&saved.bytes).unwrap();
            let story_xml = package
                .names()
                .iter()
                .find(|n| n.starts_with("Stories/"))
                .map(|name| std::str::from_utf8(package.get(name).unwrap()).unwrap())
                .unwrap();
            assert!(!story_xml.contains("<Footnote"));
            assert!(story_xml.contains("Schist.StructuredStory.v1"));
            assert_eq!(story_xml.matches("<Properties>").count(), 1);
            doc = import::read(&saved.bytes).unwrap().document;
            assert_eq!(doc.stories, expected);
        }
    }
}

#[test]
fn native_edits_win_while_stale_opaque_payloads_keep_unknown_locations() {
    for legacy in [false, true] {
        for change in ["text", "format", "identity", "style_name"] {
            let doc = document(legacy);
            let original = if legacy {
                let StoryPoint::Other { payload, .. } = &doc.stories[0].points[1] else {
                    panic!()
                };
                payload.clone()
            } else {
                doc.stories[0].structures[0].payload.clone()
            };
            let mut changed = false;
            let bytes = rewrite(&export::write(&doc).bytes, |name, xml| {
                let replacement = if name.starts_with("Stories/") {
                    match change {
                        "text" => xml
                            .replace("<Content>AéB</Content>", "<Content>External text</Content>"),
                        "format" => xml.replace(
                            "<CharacterStyleRange ",
                            "<CharacterStyleRange FontStyle=\"Bold\" ",
                        ),
                        "identity" => xml.replace("<Story Self=\"", "<Story Self=\"external"),
                        _ => xml.clone(),
                    }
                } else if name == "Resources/Styles.xml" && change == "style_name" {
                    xml.replace("Name=\"Body\"", "Name=\"Renamed\"")
                } else {
                    xml.clone()
                };
                changed |= replacement != xml;
                replacement
            });
            assert!(changed, "{change}");
            let imported = import::read(&bytes).unwrap();
            assert!(imported
                .report
                .skipped
                .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
            let mut doc = imported.document;
            let story = &doc.stories[0];
            assert_eq!(
                story.structures,
                vec![StoryStructure {
                    at: None,
                    kind: "Footnote".into(),
                    payload: original,
                    footnote: None,
                }]
            );
            if change == "text" {
                assert!(story.text().starts_with("External text"));
            }
            if change == "format" {
                assert!(!story.ranges.is_empty());
            }
            if change == "style_name" {
                let StoryPoint::Paragraph { style, .. } = &story.points[0] else {
                    panic!()
                };
                // A native local direction may lower to a derived style; its base
                // must follow the renamed resource, never the stale model name.
                assert!(
                    style == "Renamed"
                        || doc
                            .styles
                            .paragraph(style)
                            .is_some_and(|p| p.based_on.as_deref() == Some("Renamed")),
                    "{legacy}: {style:?}"
                );
            }
            let expected = story.clone();
            for _ in 0..3 {
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
                assert_eq!(doc.stories[0], expected);
            }
        }
    }
}

#[test]
fn malformed_and_duplicate_metadata_remains_recoverable_without_replacing_native_text() {
    for count in [1, 2, 5] {
        let bytes = rewrite(&export::write(&document(false)).bytes, |name, xml| {
            if name.starts_with("Stories/") {
                let start = xml.find("<Properties>").unwrap();
                let end = xml.find("</Properties>").unwrap() + "</Properties>".len();
                let mut xml = xml;
                let entries = r#"<KeyValuePair Key="Schist.StructuredStory.v1" Value="opaque &lt;data&gt; &amp; é"/>"#.repeat(count);
                xml.replace_range(
                    start..end,
                    &format!("<Properties><Label>{entries}</Label></Properties>"),
                );
                xml
            } else {
                xml
            }
        });
        let mut doc = import::read(&bytes).unwrap().document;
        assert_eq!(doc.stories[0].text(), "AéB");
        assert_eq!(doc.stories[0].structures.len(), count);
        assert!(doc.stories[0]
            .structures
            .iter()
            .all(|s| s.at.is_none() && s.payload == "opaque <data> & é"));
        let expected = doc.stories[0].clone();
        for _ in 0..4 {
            let saved = import::read(&export::write(&doc).bytes).unwrap();
            assert!(saved
                .report
                .skipped
                .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
            doc = saved.document;
            assert_eq!(doc.stories[0], expected);
        }
    }
}

#[test]
fn xml_formatting_changes_do_not_invalidate_unchanged_structure_coordinates() {
    for legacy in [false, true] {
        let doc = document(legacy);
        let bytes = rewrite(&export::write(&doc).bytes, |name, xml| {
            if name.starts_with("Stories/") {
                xml.replace("><", ">\n  <").replace(
                    "UserText=\"true\" IsEndnoteStory=\"false\"",
                    "IsEndnoteStory=\"false\" UserText=\"true\"",
                )
            } else {
                xml
            }
        });
        let imported = import::read(&bytes).unwrap();
        assert_eq!(imported.document.stories, doc.stories);
        assert!(!imported
            .report
            .skipped
            .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
    }
}
