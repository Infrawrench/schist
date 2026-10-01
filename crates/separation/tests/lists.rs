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
