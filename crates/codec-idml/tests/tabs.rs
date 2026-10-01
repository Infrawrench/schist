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
fn unsupported_tab_alignment_and_leaders_are_retained_and_diagnosed_only_when_used() {
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
            for _ in 0..3 {
                let written = export::write(&doc);
                assert!(written.warnings.contains(&message));
                let imported = import::read(&written.bytes).unwrap();
                assert!(imported.report.skipped.contains(&message));
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
