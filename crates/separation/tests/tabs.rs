use schist_layout::{
    authoring, compose,
    lists::{ListStyle, ListTab},
    History, LayoutDocument, Page, ParagraphStyle, Rect, Story,
};
use schist_separation::{separate_page_without_graphics, OutputSettings, Severity};

#[test]
fn tab_preflight_follows_used_settings_and_the_full_paragraph_context() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (alignment, leader, expected) in [
        ("LeftAlign", "", false),
        ("LeftAlign", ".", false),
        ("LeftAlign", ". ", false),
        ("LeftAlign", "123456789", true),
        ("LeftAlign", "\t", true),
        ("RightAlign", "", false),
        ("CenterAlign", "", false),
        ("CharacterAlign", "", false),
        ("unknown", "", true),
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("proof", 240.0, 160.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tabs".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(12.0),
            drop_caps_lines: Some(2),
            drop_caps_characters: Some(1),
            list: ListStyle {
                tabs: Some(vec![ListTab {
                    position: 80.0,
                    alignment: alignment.into(),
                    alignment_character: ".".into(),
                    leader: leader.into(),
                }]),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 120.0),
        )
        .unwrap();
        // The continued line starts with a tab; the enlarged initial is A.
        // Diagnostics must consult the paragraph, not mistake this line's tab
        // for the paragraph's drop-cap character.
        doc.stories[frame.story.0 as usize] = Story::from_text("A word\u{2028}\tH words", "Tabs");
        let flow = compose::compose_story(&doc, frame.story);
        assert!(!flow.has_overflow());
        assert!(flow.lines().any(|line| compose::line_spec(
            line,
            &doc.stories[frame.story.0 as usize],
            &doc
        )
        .text
        .starts_with('\t')));
        let result = separate_page_without_graphics(&doc, 0, OutputSettings::at(36.0)).unwrap();
        let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = "TabList");
        assert_eq!(
            result
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message == message),
            expected
        );
        assert!(!result
            .report
            .findings
            .iter()
            .any(|f| f.message.contains("DropCapCharacters + TabList")));
        doc.stories[frame.story.0 as usize] = Story::from_text("A word without tabs", "Tabs");
        let result = separate_page_without_graphics(&doc, 0, OutputSettings::at(36.0)).unwrap();
        assert!(!result.report.findings.iter().any(|f| f.message == message));
    }
}

#[test]
fn rtl_list_marker_diagnostics_use_the_whole_paragraph_in_both_separation_paths() {
    use schist_layout::{lists::ListKind, styles::ParagraphDirection};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (direction, text, unsupported) in [
        (ParagraphDirection::RightToLeft, "A list item", true),
        (ParagraphDirection::Auto, "אב list item", true),
        (
            ParagraphDirection::Auto,
            "A list item\u{2028}אב continued",
            false,
        ),
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("proof", 240.0, 160.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(12.0),
            direction: Some(direction),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, 120.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text(text, "List");
        let message = schist_i18n::tf!(
            "design.idml_list_unsupported",
            value = "ParagraphDirection + BulletsAndNumberingListType"
        );
        for built in [false, true] {
            let result = if built {
                schist_separation::separate_page_built(
                    &doc,
                    0,
                    OutputSettings::at(72.0),
                    &schist_separation::NoGraphics,
                    &schist_separation::NamedBuilds::new(),
                )
            } else {
                separate_page_without_graphics(&doc, 0, OutputSettings::at(72.0))
            }
            .unwrap();
            assert_eq!(
                result
                    .report
                    .findings
                    .iter()
                    .any(|f| f.severity == Severity::Error && f.message == message),
                unsupported,
                "{direction:?}/{text}"
            );
        }
    }
}

#[path = "../examples/support/tabs.rs"]
mod proof;

#[test]
fn unrenderable_text_is_reported_by_both_separation_paths() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0)]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Tiny leaders".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(1e-6),
        list: ListStyle {
            tabs: Some(vec![ListTab {
                position: 80.0,
                alignment: "LeftAlign".into(),
                alignment_character: ".".into(),
                leader: ".".into(),
            }]),
            ..Default::default()
        },
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 120.0, 120.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("A\tH", "Tiny leaders");
    let flow = compose::compose_story(&doc, frame.story);
    assert!(!flow.has_overflow());
    assert_eq!(flow.lines().count(), 1);
    let settings = OutputSettings::at(72.0);
    for output in [
        separate_page_without_graphics(&doc, 0, settings).unwrap(),
        schist_separation::separate_page_built(
            &doc,
            0,
            settings,
            &schist_separation::NoGraphics,
            &schist_separation::NaiveBuild,
        )
        .unwrap(),
    ] {
        assert!(output
            .report
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error
                && f.message == schist_i18n::t("design.preflight_unavailable_text")));
    }
}

#[test]
fn leader_only_ink_crosses_either_gutter_with_its_source_frame_unchanged() {
    use schist_layout::{Ink, Point, Spread, WritingMode};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for source in [0, 1] {
        for outside in [false, true] {
            let mut doc = LayoutDocument::new(vec![Page::new("proof", 100.0, 100.0); 2]);
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Leaders".into(),
                family: Some("IBM Plex Sans".into()),
                point_size: Some(16.0),
                writing_mode: Some(WritingMode::VerticalRightToLeft),
                fill_disabled: true,
                stroke: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                stroke_weight: Some(32.0),
                stroke_outside: Some(outside),
                list: ListStyle {
                    tabs: Some(vec![ListTab {
                        position: 60.0,
                        alignment: "LeftAlign".into(),
                        alignment_character: ".".into(),
                        leader: "H".into(),
                    }]),
                    ..Default::default()
                },
                ..Default::default()
            });
            let bounds = Rect::new(if source == 0 { 75.0 } else { 1.0 }, 20.0, 24.0, 70.0);
            let frame =
                authoring::text_frame(&mut doc, &mut History::default(), source, bounds).unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("\t", "Leaders");
            let target = 1 - source;
            let contributor = doc
                .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
                .into_iter()
                .find(|o| o.id == frame.object)
                .expect("leader stroke contributes")
                .into_owned();
            assert!(!contributor
                .paint_bounds()
                .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
            let mut reference = doc.clone();
            reference.objects[0] = contributor;
            reference.objects[0].page = target;
            for dpi in [72.0, 144.0, 216.0] {
                let settings = OutputSettings::at(dpi);
                let a = separate_page_without_graphics(&doc, target, settings).unwrap();
                let b = separate_page_without_graphics(&reference, target, settings).unwrap();
                let actual = &a.separation.plate(a.plan.process[0]).unwrap().data;
                assert_eq!(actual, &b.separation.plate(b.plan.process[0]).unwrap().data);
                assert!(actual.iter().any(|v| *v > 0.0), "{source}/{outside}/{dpi}");
            }
            assert_eq!(doc.objects[0].bounds, bounds);
            assert_eq!(doc.story(frame.story).unwrap().text(), "\t");
        }
    }
}
#[test]
fn tabbed_fields_match_independent_frames_across_writing_modes_sizes_indents_and_dpi() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let actual = proof::document(false);
    let expected = proof::document(true);
    for doc in [&actual, &expected] {
        for object in &doc.objects {
            if let schist_layout::LayoutObject::TextFrame { story, .. } = object.object {
                let flow = compose::compose_story(doc, story);
                assert!(
                    !flow.has_overflow(),
                    "proof page {}, story {story:?}",
                    object.page
                );
                assert_eq!(
                    flow.lines().count(),
                    1,
                    "proof page {}, story {story:?}",
                    object.page
                );
            }
        }
    }
    for dpi in [72.0, 144.0, 216.0] {
        for page in 0..actual.pages.len() {
            let settings = schist_separation::OutputSettings::at(dpi);
            let a =
                schist_separation::separate_page_without_graphics(&actual, page, settings).unwrap();
            let b = schist_separation::separate_page_without_graphics(&expected, page, settings)
                .unwrap();
            assert_eq!(a.separation.plates().len(), b.separation.plates().len());
            for (index, (plate, other)) in a
                .separation
                .plates()
                .iter()
                .zip(b.separation.plates())
                .enumerate()
            {
                assert_eq!(plate.data.len(), other.data.len());
                let difference = plate
                    .data
                    .iter()
                    .zip(&other.data)
                    .enumerate()
                    .find(|(_, (a, b))| a != b);
                assert!(
                    difference.is_none(),
                    "page={page},dpi={dpi},plate={index},first={difference:?}"
                );
            }
            assert!(a
                .separation
                .plates()
                .iter()
                .any(|p| p.data.iter().any(|v| *v > 0.1)));
        }
    }
}
