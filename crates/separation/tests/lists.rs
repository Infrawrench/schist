#[path = "../examples/support/lists.rs"]
mod proof;
#[test]
fn automatic_marker_ink_matches_independent_text_frames_for_sizes_alignments_and_dpi() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let actual = proof::document(false);
    let expected = proof::document(true);
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

#[test]
fn generated_markers_contribute_real_ink_across_both_sides_of_a_gutter() {
    use schist_layout::{
        authoring,
        lists::{ListKind, ListStyle, MarkerAlignment},
        CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Point, Rect, Spread,
        Story,
    };
    use schist_separation::{separate_page_without_graphics, OutputSettings};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for source in [0, 1] {
        let mut doc = LayoutDocument::new(vec![Page::new("proof", 100.0, 100.0); 2]);
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: Some(1),
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        doc.styles.add_character(CharacterStyle {
            name: "Marker".into(),
            fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(24.0),
            left_indent: Some(24.0),
            first_line_indent: Some(-24.0),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                start: Some(999),
                numbering_alignment: Some(MarkerAlignment::Right),
                numbering_character_style: Some("Marker".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            source,
            Rect::new(if source == 0 { 36.0 } else { 4.0 }, 15.0, 60.0, 70.0),
        )
        .unwrap();
        if source == 0 {
            doc.objects[0].rotation = 180.0;
        }
        doc.stories[frame.story.0 as usize] = Story::from_text("H", "List");
        let target = 1 - source;
        let contributor = doc
            .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
            .into_iter()
            .find(|o| o.id == frame.object)
            .expect("marker reaches neighboring page")
            .into_owned();
        assert!(!contributor
            .paint_bounds()
            .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
        let mut reference = doc.clone();
        reference.objects[0] = contributor;
        reference.objects[0].page = target;
        for dpi in [72.0, 144.0, 216.0] {
            let a = separate_page_without_graphics(&doc, target, OutputSettings::at(dpi)).unwrap();
            let b = separate_page_without_graphics(&reference, target, OutputSettings::at(dpi))
                .unwrap();
            let actual = &a.separation.plate(a.plan.process[0]).unwrap().data;
            let expected = &b.separation.plate(b.plan.process[0]).unwrap().data;
            assert_eq!(actual, expected, "source={source},dpi={dpi}");
            assert!(actual.iter().any(|v| *v > 0.1), "source={source},dpi={dpi}");
        }
    }
}

#[test]
fn cross_story_numbering_matches_per_story_reference_ink_and_reports_unknown_order() {
    use schist_layout::{
        authoring,
        lists::{ListKind, ListStyle, NumberingList},
        History, ParagraphStyle, Rect, Story,
    };
    use schist_separation::{
        separate_page, separate_page_built, separate_page_without_graphics, NaiveBuild, NoGraphics,
        OutputSettings, Severity,
    };
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for count in [2, 5] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.numbering_lists.push(NumberingList {
            id: "shared".into(),
            name: "Shared".into(),
            across_stories: true,
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Shared".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(12.0),
            left_indent: Some(24.0),
            first_line_indent: Some(-24.0),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                list: Some("shared".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        for index in 0..count {
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(72.0, 72.0 + index as f32 * 50.0, 200.0, 45.0),
            )
            .unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("Alpha", "Shared");
            doc.stories[frame.story.0 as usize].push_paragraph("Beta", "Shared");
        }
        doc.objects.reverse();
        let mut reference = doc.clone();
        reference.styles.numbering_lists[0].across_stories = false;
        for index in 0..count {
            let name = format!("Local {index}");
            reference.styles.add_paragraph(ParagraphStyle {
                name: name.clone(),
                based_on: Some("Shared".into()),
                list: ListStyle {
                    start: Some(index as u32 * 2 + 1),
                    ..Default::default()
                },
                ..Default::default()
            });
            for point in &mut reference.stories[index].points {
                if let schist_layout::StoryPoint::Paragraph { style, .. } = point {
                    *style = name.clone();
                }
            }
        }
        for dpi in [72.0, 144.0, 216.0] {
            let a = separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi)).unwrap();
            let b = separate_page_without_graphics(&reference, 0, OutputSettings::at(dpi)).unwrap();
            assert_eq!(a.separation.plates().len(), b.separation.plates().len());
            for (plate, expected) in a.separation.plates().iter().zip(b.separation.plates()) {
                assert_eq!(plate.data, expected.data, "count={count},dpi={dpi}");
            }
            let notice = schist_i18n::t("design.idml_cross_story_order");
            assert_eq!(
                a.report
                    .findings
                    .iter()
                    .filter(|f| f.message == notice && f.severity == Severity::Warning)
                    .count(),
                1
            );
            assert!(!a
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error
                    && f.message.contains("ContinueNumbersAcrossStories")));
        }
        doc.creation_order.clear();
        for output in [
            separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap(),
            separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild)
                .unwrap(),
        ] {
            assert!(output
                .report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Error
                    && f.message
                        .contains("ContinueNumbersAcrossStories.UnknownCreationOrder")));
        }
    }
}
