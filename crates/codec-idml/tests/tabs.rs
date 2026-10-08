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
fn hanging_indent_tab_geometry_and_leader_ownership_survive_native_saves() {
    use schist_layout::{
        styles::{Align, ParagraphDirection},
        WritingMode,
    };
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for position in [None, Some(30.0), Some(40.0), Some(80.0)] {
                let reverse =
                    mode == WritingMode::Horizontal && direction == ParagraphDirection::RightToLeft;
                let mut doc = schist_layout::blank_a4();
                let mut stop = tab(position.unwrap_or(80.0));
                stop.alignment = if reverse { "RightAlign" } else { "LeftAlign" }.into();
                stop.leader = ". ".into();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Hanging".into(),
                    family: Some("IBM Plex Sans".into()),
                    point_size: Some(10.0),
                    writing_mode: Some(mode),
                    direction: Some(direction),
                    align: Some(if reverse { Align::Right } else { Align::Left }),
                    left_indent: Some(if reverse { 0.0 } else { 40.0 }),
                    right_indent: Some(if reverse { 40.0 } else { 0.0 }),
                    first_line_indent: Some(-40.0),
                    list: ListStyle {
                        tabs: Some(position.map(|_| vec![stop]).unwrap_or_default()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Inherited hanging".into(),
                    based_on: Some("Hanging".into()),
                    ..Default::default()
                });
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(20.0, 30.0, 300.0, 300.0),
                )
                .unwrap();
                let mut story = Story::new();
                for style in ["Hanging", "Inherited hanging"] {
                    story.push_paragraph("Tab\tH words", style);
                }
                doc.stories[frame.story.0 as usize] = story;
                let stories = doc.stories.clone();
                let paragraph = doc.styles.paragraph("Hanging").unwrap().clone();
                let inherited = doc.styles.paragraph("Inherited hanging").unwrap().clone();
                let capture = |doc: &schist_layout::LayoutDocument| {
                    let flow = compose::compose_story(doc, frame.story);
                    assert!(!flow.has_overflow());
                    flow.lines()
                        .map(|line| {
                            let spec =
                                compose::line_spec(line, doc.story(frame.story).unwrap(), doc);
                            assert_eq!(spec.tabs.as_ref().unwrap().hanging_indent, Some(40.0));
                            assert!(
                                (schist_text_engine::measure(&spec).unwrap().width
                                    - line.natural_width)
                                    .abs()
                                    < 0.001
                            );
                            let paint = schist_text_engine::rasterize(&spec).unwrap();
                            if position.is_none_or(|position| position > 40.0) {
                                // The virtual indent has no paint. A later explicit
                                // leader must not appear in its gap after any save.
                                let mut plain = spec.clone();
                                plain.tabs.as_mut().unwrap().leaders.clear();
                                let expected = schist_text_engine::rasterize(&plain).unwrap();
                                assert_eq!(paint.bounds, expected.bounds);
                                assert!(
                                    paint.coverage == expected.coverage,
                                    "{mode:?}/{direction:?}/{position:?}, {}..{}",
                                    line.start,
                                    line.end
                                );
                            }
                            (
                                line.bounds,
                                line.start,
                                line.end,
                                schist_text_engine::insertion_points(&spec),
                                paint.bounds,
                                paint.coverage,
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let expected = capture(&doc);
                for _ in 0..4 {
                    let output = export::write(&doc);
                    let message = schist_i18n::t("design.idml_paragraph_orientation").to_string();
                    assert_eq!(
                        output
                            .warnings
                            .iter()
                            .filter(|warning| **warning == message)
                            .count(),
                        1
                    );
                    let imported = import::read(&output.bytes).unwrap();
                    assert_eq!(
                        imported
                            .report
                            .skipped
                            .iter()
                            .filter(|warning| **warning == message)
                            .count(),
                        1
                    );
                    doc = imported.document;
                    assert_eq!(doc.styles.paragraph("Hanging").unwrap(), &paragraph);
                    assert_eq!(
                        doc.styles.paragraph("Inherited hanging").unwrap(),
                        &inherited
                    );
                    assert_eq!(doc.stories, stories);
                    assert_eq!(
                        capture(&doc),
                        expected,
                        "{mode:?}/{direction:?}/{position:?}"
                    );
                }
            }
        }
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
        for leader in ["", ". ", "        ", "é— ", "123456789", "\t", "\u{2028}"] {
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
            let unsupported =
                alignment == "unknown" || !schist_text_engine::valid_tab_leader(leader);
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
        stop.leader = ". ".into();
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

#[test]
fn rtl_tabs_keep_native_stop_names_and_resolved_story_axes_across_repeated_saves() {
    use schist_layout::{
        styles::{Align, ParagraphDirection},
        StoryOrientation,
    };
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (vertical, on_path) in [(false, false), (true, false), (false, true)] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
            ParagraphDirection::Auto,
        ] {
            for alignment in ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"] {
                let mut doc = schist_layout::blank_a4();
                let mut stop = tab(120.125);
                stop.alignment = alignment.into();
                stop.leader = ". ".into();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "RTL tabs".into(),
                    family: Some("IBM Plex Sans".into()),
                    point_size: Some(18.0),
                    direction: Some(direction),
                    align: Some(
                        if vertical || direction == ParagraphDirection::LeftToRight {
                            Align::Left
                        } else {
                            Align::Right
                        },
                    ),
                    left_indent: Some(7.0),
                    right_indent: Some(18.0),
                    first_line_indent: Some(12.0),
                    list: ListStyle {
                        tabs: Some(vec![stop]),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                let frame = if on_path {
                    use schist_layout::{BezierHandles, Point, ShapePath, SubPath};
                    let shape = ShapePath {
                        subpaths: vec![SubPath {
                            points: vec![Point::new(20.0, 100.0), Point::new(320.0, 100.0)],
                            handles: vec![
                                BezierHandles {
                                    outgoing: Some(Point::new(80.0, 20.0)),
                                    ..Default::default()
                                },
                                BezierHandles {
                                    incoming: Some(Point::new(260.0, 180.0)),
                                    ..Default::default()
                                },
                            ],
                            closed: false,
                        }],
                        even_odd: false,
                    };
                    let id = authoring::path_shape(
                        &mut doc,
                        &mut History::default(),
                        0,
                        shape,
                        authoring::Paint::none(),
                    )
                    .unwrap();
                    let frame =
                        schist_layout::text_path::attach(&mut doc, &mut History::default(), id)
                            .unwrap();
                    for bracket in [
                        schist_layout::text_path::Bracket::Start(20.0),
                        schist_layout::text_path::Bracket::End(Some(260.0)),
                    ] {
                        assert!(schist_layout::text_path::set_bracket(
                            &mut doc,
                            &mut History::default(),
                            &[id],
                            bracket
                        ));
                    }
                    frame
                } else {
                    authoring::text_frame(
                        &mut doc,
                        &mut History::default(),
                        0,
                        Rect::new(20.0, 30.0, 280.0, 280.0),
                    )
                    .unwrap()
                };
                let mut story = Story::from_text(
                    if on_path {
                        "אב\t12.34"
                    } else {
                        "אב\t12.34\nאב\t56.78"
                    },
                    "RTL tabs",
                );
                if vertical {
                    story.prefs.orientation = StoryOrientation::Vertical;
                }
                doc.stories[frame.story.0 as usize] = story;
                let original = doc.stories.clone();
                let original_style = doc.styles.paragraph("RTL tabs").unwrap().clone();
                let capture = |doc: &schist_layout::LayoutDocument| {
                    let flow = compose::compose_story(doc, frame.story);
                    assert!(!flow.has_overflow());
                    flow.lines()
                        .map(|line| {
                            let spec =
                                compose::line_spec(line, doc.story(frame.story).unwrap(), doc);
                            assert_eq!(spec.writing_mode.is_vertical(), vertical);
                            assert!(schist_layout::tabs::unsupported_in_mode(
                                &line.paragraph,
                                &spec.text,
                                on_path,
                                spec.writing_mode
                            )
                            .is_empty());
                            let raster = schist_text_engine::rasterize(&spec).unwrap();
                            (
                                line.bounds,
                                line.inline_origin,
                                raster.bounds,
                                raster.coverage,
                                schist_text_engine::insertion_points(&spec),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let expected = capture(&doc);
                assert_eq!(expected.len(), if on_path { 1 } else { 2 });
                for _ in 0..4 {
                    let written = export::write(&doc);
                    let imported = import::read(&written.bytes).unwrap();
                    for warnings in [&written.warnings, &imported.report.skipped] {
                        assert!(
                            !warnings
                                .iter()
                                .any(|w| w.contains("Unsupported paragraph tab")),
                            "{warnings:?}"
                        );
                    }
                    let xml = container::read(&written.bytes)
                        .unwrap()
                        .text("Resources/Styles.xml")
                        .unwrap()
                        .to_owned();
                    assert!(xml.contains(&format!(
                        "<Alignment type=\"enumeration\">{alignment}</Alignment>"
                    )));
                    doc = imported.document;
                    assert_eq!(doc.stories, original);
                    assert_eq!(doc.styles.paragraph("RTL tabs").unwrap(), &original_style);
                    assert_eq!(capture(&doc), expected);
                }
            }
        }
    }
}
