use schist_codec_idml::{container, export, import};
use schist_layout::{authoring, blank_a4, History, LayoutDocument, Rect};

fn names(doc: &LayoutDocument) -> Vec<String> {
    doc.creation_order
        .iter()
        .filter_map(|id| doc.object(*id))
        .map(|object| object.name.clone())
        .collect()
}

#[test]
fn creation_order_survives_native_paint_order_changes_saves_and_new_authoring() {
    for count in [1, 2, 7] {
        let mut doc = blank_a4();
        let mut history = History::default();
        for index in 0..count {
            let frame = authoring::text_frame(
                &mut doc,
                &mut history,
                0,
                Rect::new(20.0 + index as f32, 30.0, 100.0, 100.0),
            )
            .unwrap();
            doc.objects
                .iter_mut()
                .find(|object| object.id == frame.object)
                .unwrap()
                .name = format!("Created {index}");
        }
        let expected = names(&doc);
        doc.objects.reverse();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(names(&doc), expected);
            assert_eq!(
                doc.objects
                    .iter()
                    .map(|object| object.name.clone())
                    .collect::<Vec<_>>(),
                expected.iter().rev().cloned().collect::<Vec<_>>()
            );
        }
        let before = doc.clone();
        let depth = history.undo_depth();
        let frame = authoring::text_frame(
            &mut doc,
            &mut history,
            0,
            Rect::new(80.0, 90.0, 100.0, 100.0),
        )
        .unwrap();
        assert_eq!(history.undo_depth(), depth + 1);
        assert_eq!(doc.creation_rank(frame.object), Some(count));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        let expected = names(&doc);
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(names(&doc), expected);
    }
}

#[test]
fn unlabelled_native_items_do_not_gain_creation_evidence_from_the_xml_walk() {
    let doc = import::read(include_bytes!("../../../fixtures/idml/text.idml"))
        .unwrap()
        .document;
    assert!(!doc.objects.is_empty());
    assert!(doc.creation_order.is_empty());
    assert!(doc
        .objects
        .iter()
        .all(|object| doc.creation_rank(object.id).is_none()));
    let saved = import::read(&export::write(&doc).bytes).unwrap().document;
    assert!(saved.creation_order.is_empty());
}

#[test]
fn renamed_native_identity_invalidates_only_its_retained_creation_entry() {
    let mut doc = blank_a4();
    for index in 0..3 {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0 + index as f32, 30.0, 100.0, 100.0),
        )
        .unwrap();
        doc.objects
            .iter_mut()
            .find(|object| object.id == frame.object)
            .unwrap()
            .name = format!("Created {index}");
    }
    let output = export::write(&doc);
    let package = container::read(&output.bytes).unwrap();
    let mut parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| (name.to_owned(), package.get(name).unwrap().to_vec()))
        .collect();
    let (_, bytes) = parts
        .iter_mut()
        .find(|(name, _)| name.starts_with("Spreads/"))
        .unwrap();
    let xml = String::from_utf8(bytes.clone()).unwrap();
    let tree = schist_codec_idml::xml::parse(&xml).unwrap();
    let id = tree
        .find_all("TextFrame")
        .into_iter()
        .find(|frame| frame.attr("Name") == Some("Created 1"))
        .unwrap()
        .attr("Self")
        .unwrap();
    let source = format!("Self=\"{id}\"");
    assert_eq!(xml.matches(&source).count(), 1);
    *bytes = xml
        .replace(&source, "Self=\"external_recreated_frame\"")
        .into_bytes();
    let imported = import::read(&container::write(&parts)).unwrap();
    assert_eq!(names(&imported.document), vec!["Created 0", "Created 2"]);
    assert_eq!(imported.document.objects.len(), 3);
    assert!(imported
        .report
        .skipped
        .contains(&schist_i18n::t("design.idml_creation_order").to_string()));
}

#[test]
fn known_cross_story_numbering_retains_chronology_source_and_masks_through_native_saves() {
    use schist_layout::{
        list_composition::MarkerPlans,
        lists::{ListKind, ListStyle, NumberingList},
        LayoutObject, Story, StoryPoint,
    };
    for count in [2, 5] {
        let mut doc = blank_a4();
        doc.styles.numbering_lists.push(NumberingList {
            id: "opaque shared".into(),
            name: "Shared".into(),
            across_stories: true,
            ..Default::default()
        });
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Body")
            .unwrap()
            .list = ListStyle {
            kind: Some(ListKind::Numbered),
            list: Some("opaque shared".into()),
            expression: Some("^#.".into()),
            ..Default::default()
        };
        for index in 0..count {
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(72.0, 72.0, 200.0, 200.0),
            )
            .unwrap();
            doc.objects
                .iter_mut()
                .find(|o| o.id == frame.object)
                .unwrap()
                .name = format!("Created {index}");
            doc.stories[frame.story.0 as usize] = Story {
                points: vec![
                    StoryPoint::Paragraph {
                        text: "é第一".into(),
                        style: "Body".into(),
                    },
                    StoryPoint::Paragraph {
                        text: "second".into(),
                        style: "Body".into(),
                    },
                ],
                ..Default::default()
            };
        }
        doc.objects.reverse();
        let styles = doc.styles.clone();
        for _ in 0..4 {
            let output = export::write(&doc);
            let notice = schist_i18n::t("design.idml_cross_story_order").to_string();
            assert_eq!(output.warnings.iter().filter(|m| **m == notice).count(), 1);
            let imported = import::read(&output.bytes).unwrap();
            assert_eq!(
                imported
                    .report
                    .skipped
                    .iter()
                    .filter(|m| **m == notice)
                    .count(),
                1
            );
            assert!(!imported
                .report
                .skipped
                .iter()
                .any(|m| m.contains("Unsupported") && m.contains("ContinueNumbersAcrossStories")));
            doc = imported.document;
            assert_eq!(doc.styles.numbering_lists, styles.numbering_lists);
            assert_eq!(
                doc.styles.paragraph("Body").unwrap().list,
                styles.paragraph("Body").unwrap().list
            );
            for index in 0..count {
                let object = doc
                    .objects
                    .iter()
                    .find(|o| o.name == format!("Created {index}"))
                    .unwrap();
                let LayoutObject::TextFrame { story, .. } = object.object else {
                    panic!("frame")
                };
                let story = doc.story(story).unwrap();
                assert_eq!(story.text(), "é第一\nsecond");
                let counters = MarkerPlans::new(&doc, story);
                let markers: Vec<_> = story
                    .point_offsets()
                    .into_iter()
                    .filter_map(|at| counters.spec(at).map(|spec| spec.text.clone()))
                    .collect();
                assert_eq!(
                    markers,
                    vec![format!("{}.", index * 2 + 1), format!("{}.", index * 2 + 2)]
                );
            }
        }
    }
}
