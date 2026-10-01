use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, compose,
    lists::{ListStyle, ListTab},
    History, ParagraphStyle, Rect, Story,
};

fn tab(position: f32) -> ListTab {
    ListTab {
        position,
        alignment: "LeftAlign".into(),
        alignment_character: ".".into(),
        leader: String::new(),
    }
}

#[test]
fn paragraph_tabs_inherit_replace_and_clear_without_becoming_lists_across_native_saves() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Tabs".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(12.0),
        list: ListStyle {
            tabs: Some(vec![tab(48.0), tab(96.0)]),
            ..Default::default()
        },
        ..Default::default()
    });
    for (name, tabs) in [
        ("Inherited", None),
        ("Replaced", Some(vec![tab(72.0)])),
        ("Cleared", Some(Vec::new())),
    ] {
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            based_on: Some("Tabs".into()),
            list: ListStyle {
                tabs,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 30.0, 250.0, 200.0),
    )
    .unwrap();
    let mut story = Story::new();
    for name in ["Tabs", "Inherited", "Replaced", "Cleared"] {
        story.push_paragraph("é\tH", name);
    }
    doc.stories[frame.story.0 as usize] = story;
    let before = doc.stories.clone();
    let styles = doc.styles.clone();
    let pixels = |doc: &schist_layout::LayoutDocument| {
        compose::compose_story(doc, frame.story)
            .lines()
            .map(|line| {
                assert!(line.generated.is_none());
                let spec = compose::line_spec(line, &doc.stories[frame.story.0 as usize], doc);
                let expected = match line.paragraph_style.as_str() {
                    "Replaced" => 72.0,
                    "Cleared" => 36.0,
                    _ => 48.0,
                };
                assert!(
                    (schist_text_engine::caret_at(&spec, "é\t".len()).unwrap().x - expected).abs()
                        < 0.001
                );
                schist_text_engine::rasterize(&spec).unwrap().coverage
            })
            .collect::<Vec<_>>()
    };
    let expected = pixels(&doc);
    for _ in 0..4 {
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        assert!(package
            .text("Resources/Styles.xml")
            .unwrap()
            .contains("<TabList"));
        let imported = import::read(&written.bytes).unwrap();
        for warnings in [&written.warnings, &imported.report.skipped] {
            assert!(warnings
                .iter()
                .any(|w| w == &schist_i18n::t("design.idml_tabs_implicit").to_string()));
            assert!(
                !warnings
                    .iter()
                    .any(|w| w.contains("Unsupported paragraph tab")),
                "{warnings:?}"
            );
        }
        doc = imported.document;
        assert_eq!(doc.stories, before);
        for name in ["Tabs", "Inherited", "Replaced", "Cleared"] {
            assert_eq!(
                doc.styles.paragraph(name).unwrap().list,
                styles.paragraph(name).unwrap().list
            );
        }
        assert_eq!(pixels(&doc), expected);
    }
}

#[test]
fn tab_alignment_and_leaders_are_retained_and_only_unsupported_used_settings_are_diagnosed() {
    for alignment in ["RightAlign", "CenterAlign", "CharacterAlign", "unknown"] {
        for leader in ["", ". "] {
            let mut doc = schist_layout::blank_a4();
            let mut stop = tab(48.125);
            stop.alignment = alignment.into();
            stop.alignment_character = ",".into();
            stop.leader = leader.into();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Tabs".into(),
                list: ListStyle {
                    tabs: Some(vec![stop]),
                    ..Default::default()
                },
                ..Default::default()
            });
            let story = doc.add_story(Story::from_text("é\tH", "Tabs"));
            let expected = doc.styles.paragraph("Tabs").unwrap().list.clone();
            let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = "TabList");
            let unsupported = alignment == "unknown" || !leader.is_empty();
            for _ in 0..3 {
                let written = export::write(&doc);
                assert_eq!(written.warnings.contains(&message), unsupported);
                let imported = import::read(&written.bytes).unwrap();
                assert_eq!(imported.report.skipped.contains(&message), unsupported);
                doc = imported.document;
                assert_eq!(doc.styles.paragraph("Tabs").unwrap().list, expected);
            }
            doc.stories[story.0 as usize] = Story::from_text("no tabs", "Tabs");
            let written = export::write(&doc);
            assert!(!written.warnings.contains(&message));
            assert!(!import::read(&written.bytes)
                .unwrap()
                .report
                .skipped
                .contains(&message));
        }
    }
}

#[test]
fn inherited_aligned_tabs_keep_their_geometry_and_paints_across_repeated_native_saves() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for alignment in ["RightAlign", "CenterAlign", "CharacterAlign"] {
        let mut doc = schist_layout::blank_a4();
        let mut stop = tab(140.12345);
        stop.alignment = alignment.into();
        stop.alignment_character = ",".into();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tabs".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(18.0),
            list: ListStyle {
                tabs: Some(vec![stop]),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Tabs".into()),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 30.0, 280.0, 200.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] =
            Story::from_text("é\t12,34\nA\t1234\n\t,12,34", "Child");
        let original = doc.stories.clone();
        let capture = |doc: &schist_layout::LayoutDocument| {
            let flow = compose::compose_story(doc, frame.story);
            assert!(!flow.has_overflow());
            assert_eq!(flow.lines().count(), 3);
            flow.lines()
                .map(|line| {
                    let spec = compose::line_spec(line, doc.story(frame.story).unwrap(), doc);
                    let pixels = schist_text_engine::rasterize(&spec).unwrap();
                    let points = schist_text_engine::insertion_points(&spec);
                    (pixels.bounds, pixels.coverage, points)
                })
                .collect::<Vec<_>>()
        };
        let expected = capture(&doc);
        for _ in 0..4 {
            let written = export::write(&doc);
            let imported = import::read(&written.bytes).unwrap();
            let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = "TabList");
            assert!(!written.warnings.contains(&message));
            assert!(!imported.report.skipped.contains(&message));
            assert_eq!(
                imported
                    .document
                    .styles
                    .paragraph("Child")
                    .unwrap()
                    .list
                    .tabs,
                None
            );
            assert_eq!(
                imported.document.styles.paragraph("Tabs").unwrap().list,
                doc.styles.paragraph("Tabs").unwrap().list
            );
            doc = imported.document;
            assert_eq!(doc.stories, original);
            assert_eq!(capture(&doc), expected);
        }
    }
}

#[test]
fn character_tabs_accept_one_literal_character_and_retain_unsupported_values() {
    for character in ["", ".", ",", " ", "€", "::", "\t", "\u{2028}"] {
        let mut doc = schist_layout::blank_a4();
        let mut stop = tab(72.0);
        stop.alignment = "CharacterAlign".into();
        stop.alignment_character = character.into();
        let unsupported = stop.text_alignment().is_none();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tabs".into(),
            list: ListStyle {
                tabs: Some(vec![stop]),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.add_story(Story::from_text("A\t12.34", "Tabs"));
        let written = export::write(&doc);
        let imported = import::read(&written.bytes).unwrap();
        let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = "TabList");
        assert_eq!(
            written.warnings.contains(&message),
            unsupported,
            "{character:?}"
        );
        assert_eq!(
            imported.report.skipped.contains(&message),
            unsupported,
            "{character:?}"
        );
        assert_eq!(
            imported.document.styles.paragraph("Tabs").unwrap().list,
            doc.styles.paragraph("Tabs").unwrap().list
        );
    }
}
