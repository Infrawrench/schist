use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, blank_a4, compose, History, Rect, Story, StoryId};

fn native(content: &str, page_number_type: Option<&str>) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("marker-source", "Body");
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                // Replace the whole native story so the synthetic input carries
                // no Schist retention/automatic-direction labels from the seed.
                let root = xml::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
                let id = root.find("Story").unwrap().attr("Self").unwrap();
                let mode = page_number_type.map(|value| format!(r#" PageNumberType="{value}""#)).unwrap_or_default();
                format!(r#"<idPkg:Story><Story Self="{id}"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PointSize="23"{mode}>{content}</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes()
            } else {
                bytes.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    container::write(&parts)
}

fn instruction(value: &schist_layout::StoryStructure) -> (String, Option<String>) {
    assert_eq!(value.kind, "ProcessingInstruction");
    let root = xml::parse(&value.payload).unwrap();
    let content = root.find("Content").unwrap();
    assert!(content.text.is_empty());
    assert_eq!(content.instructions.len(), 1);
    assert_eq!(content.instructions[0].0, 0);
    let character = root.find("CharacterStyleRange").unwrap();
    assert!(root.attr("AppliedParagraphStyle").is_some());
    assert!(character.attr("AppliedCharacterStyle").is_some());
    (
        content.instructions[0].1.clone(),
        character.attr("PageNumberType").map(str::to_owned),
    )
}

#[test]
fn content_instructions_keep_order_context_and_every_utf8_anchor_without_source_characters() {
    let text = "Aé中🙂B";
    let instructions = [
        "ACE 3",
        "ACE 7",
        "ACE 8",
        "ACE 18",
        "ACE 19",
        "ACE 4",
        "future data='é&<>'",
        "ACE   18 ",
    ];
    for at in text
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
    {
        for kind in [
            None,
            Some("AutoPageNumber"),
            Some("NextPageNumber"),
            Some("PreviousPageNumber"),
            Some("FuturePageNumber"),
        ] {
            let controls = instructions
                .iter()
                .map(|pi| format!("<?{pi}?>"))
                .collect::<String>();
            let content = format!(
                "<Content>{}{}{}</Content>",
                &text[..at],
                controls,
                &text[at..]
            );
            let imported = import::read(&native(&content, kind)).unwrap();
            assert!(imported
                .report
                .skipped
                .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
            let story = &imported.document.stories[0];
            assert_eq!(story.text(), text);
            assert_eq!(story.structures.len(), instructions.len());
            for (saved, pi) in story.structures.iter().zip(instructions) {
                assert_eq!(saved.at, Some(at));
                let tree = xml::parse(&saved.payload).unwrap();
                let name = tree
                    .find("CharacterStyleRange")
                    .unwrap()
                    .attr("AppliedCharacterStyle")
                    .unwrap()
                    .strip_prefix("CharacterStyle/$ID/")
                    .unwrap();
                assert_eq!(
                    imported.document.styles.resolve_character(name).point_size,
                    Some(23.0)
                );
                assert_eq!(instruction(saved), (pi.into(), kind.map(str::to_owned)));
            }
            assert_eq!(
                compose::compose_story(&imported.document, StoryId(0)).frames[0]
                    .unrendered_structures,
                instructions.len() - 1
            );
        }
    }
}

#[test]
fn instruction_only_and_split_content_paragraphs_keep_global_anchors_through_saves_and_undo() {
    for (content,text,anchors) in [
        ("<Content><?ACE 18?></Content>","",vec![0]),
        ("<Content>Aé<?ACE 3?></Content><Content>&amp;中<?ACE 18?></Content><Br/><Content><![CDATA[尾]]><?ACE 19?></Content>","Aé&中\n尾",vec![3,7,11]),
    ] {
        let mut doc=import::read(&native(content,Some("NextPageNumber"))).unwrap().document;
        assert_eq!(doc.stories[0].text(),text);
        assert_eq!(doc.stories[0].structures.iter().map(|s|s.at.unwrap()).collect::<Vec<_>>(),anchors);
        let before=doc.clone();
        let mut history=History::default();
        assert!(authoring::replace_text(&mut doc,&mut history,StoryId(0),0..0,"Ω"));
        assert_eq!(history.undo_depth(),1);
        for (structure,at) in doc.stories[0].structures.iter().zip(&anchors) {assert_eq!(structure.at,Some(at+2));}
        let edited=doc.clone();
        assert!(history.undo(&mut doc));assert_eq!(doc,before);
        assert!(history.redo(&mut doc));assert_eq!(doc,edited);
        for _ in 0..3 {
            let saved=export::write(&doc);
            assert!(saved.warnings.contains(&schist_i18n::t("design.idml_story_structure").to_string()));
            let package=container::read(&saved.bytes).unwrap();
            for name in package.names().into_iter().filter(|n|n.starts_with("Stories/")) {
                let root=xml::parse(package.text(name).unwrap()).unwrap();
                assert!(root.find_all("Content").iter().all(|e|e.instructions.iter().all(|(_, pi)| pi.split_whitespace().eq(["ACE", "3"]))),"Only the supported end-nested-style control has native output");
            }
            doc=import::read(&saved.bytes).unwrap().document;
            assert_eq!(doc.stories[0],edited.stories[0]);
        }
    }
}

#[test]
fn native_page_number_instructions_in_public_templates_survive_as_diagnosed_data() {
    for bytes in [
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
    ] {
        let imported = import::read(bytes).unwrap();
        assert!(imported
            .report
            .skipped
            .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
        let mut doc = imported.document;
        let retained = |doc: &schist_layout::LayoutDocument| {
            doc.stories
                .iter()
                .flat_map(|s| s.structures.iter())
                .filter(|s| s.kind == "ProcessingInstruction")
                .cloned()
                .collect::<Vec<_>>()
        };
        let expected = retained(&doc);
        assert_eq!(expected.len(), 2);
        assert!(expected.iter().all(|s| instruction(s).0 == "ACE 18"));
        for _ in 0..3 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(retained(&doc), expected);
        }
    }
}

#[test]
fn footnote_markers_are_typed_only_inside_supported_notes() {
    for pi in ["ACE 4", "ACE 3"] {
        let content=format!("<Content>A<?ACE 4?>B</Content><Footnote><ParagraphStyleRange AppliedParagraphStyle=\"ParagraphStyle/$ID/Body\"><CharacterStyleRange AppliedCharacterStyle=\"CharacterStyle/$ID/[No character style]\"><Content>head<?{pi}?>tail</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>");
        let doc = import::read(&native(&content, None)).unwrap().document;
        let story = &doc.stories[0];
        assert_eq!(story.text(), "AB");
        assert_eq!(story.structures.len(), 2);
        assert_eq!(instruction(&story.structures[0]).0, "ACE 4");
        assert_eq!(story.structures[0].at, Some(1));
        let note = &story.structures[1];
        assert_eq!(note.kind, "Footnote");
        assert_eq!(note.at, Some(2));
        if pi == "ACE 4" {
            let note = note.footnote.as_ref().unwrap();
            assert_eq!(note.story.text(), "headtail");
            assert_eq!(note.markers.len(), 1);
            assert_eq!(note.markers[0].at, 4);
        } else {
            assert!(note.footnote.is_none());
            assert!(note.payload.contains("<?ACE 3?>"));
        }
    }
}

#[test]
fn a_native_instruction_edit_invalidates_stale_recovery_coordinates_without_losing_either_marker() {
    let doc = import::read(&native("<Content>Aé<?ACE 18?>B</Content>", None))
        .unwrap()
        .document;
    let saved = export::write(&doc);
    let package = container::read(&saved.bytes).unwrap();
    let mut changed = false;
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                let xml = std::str::from_utf8(bytes).unwrap();
                let edited =
                    xml.replace("<Content>AéB</Content>", "<Content>Aé<?ACE 19?>B</Content>");
                changed |= edited != xml;
                edited.into_bytes()
            } else {
                bytes.to_vec()
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
    let expected = doc.stories[0].clone();
    assert_eq!(expected.text(), "AéB");
    assert_eq!(expected.structures.len(), 2);
    assert_eq!(expected.structures[0].at, Some(3));
    assert_eq!(instruction(&expected.structures[0]).0, "ACE 19");
    assert_eq!(expected.structures[1].at, None);
    assert_eq!(instruction(&expected.structures[1]).0, "ACE 18");
    for _ in 0..3 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.stories[0], expected);
    }
}
