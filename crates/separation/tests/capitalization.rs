use schist_layout::{
    authoring, CharacterStyle, History, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn mixed_case_ranges_and_expanding_capitals_match_independent_plate_content() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSans-Regular.ttf").to_vec(),
    );
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 200.0, 200.0)]);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Caps".into(),
            family: Some("Noto Sans".into()),
            point_size: Some(22.0),
            writing_mode: Some(mode),
            all_caps: Some(true),
            small_caps: Some(false),
            fill: Some(schist_layout::Ink::spot("Spot text", [0.8, 0.1, 0.1])),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Small".into(),
            all_caps: Some(false),
            small_caps: Some(true),
            fill_tint: Some(0.65),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 160.0, 160.0),
        )
        .unwrap();
        let mut story = Story::from_text("Abc Straße", "Caps");
        story.apply_style(0, 3, "Small");
        doc.stories[frame.story.0 as usize] = story;
        doc.objects.last_mut().unwrap().transform = schist_core::Affine {
            a: 0.9,
            b: 0.1,
            c: 0.08,
            d: 0.95,
            tx: 0.0,
            ty: 0.0,
        };
        let mut reference = doc.clone();
        let style = reference
            .styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Caps")
            .unwrap();
        style.all_caps = Some(false);
        let style = reference
            .styles
            .characters
            .iter_mut()
            .find(|s| s.name == "Small")
            .unwrap();
        style.small_caps = Some(false);
        style.features = vec![("smcp".into(), true), ("c2sc".into(), false)];
        let mut story = Story::from_text("Abc STRASSE", "Caps");
        story.apply_style(0, 3, "Small");
        reference.stories[frame.story.0 as usize] = story;
        for dpi in [72.0, 144.0] {
            let a = separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi)).unwrap();
            let b = separate_page_without_graphics(&reference, 0, OutputSettings::at(dpi)).unwrap();
            assert_eq!(a.separation.plates().len(), b.separation.plates().len());
            for (a, b) in a.separation.plates().iter().zip(b.separation.plates()) {
                assert_eq!(a.data, b.data, "{mode:?}, {dpi}");
            }
            assert!(a
                .separation
                .plates()
                .iter()
                .any(|p| p.data.iter().any(|v| *v > 0.5)));
        }
    }
}

#[test]
fn oversized_synthetic_caps_contribute_ink_beyond_their_nominal_frame() {
    use schist_layout::{Point, Spread};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0); 2]);
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        binding_location: Some(1),
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    doc.styles.text_preferences.small_cap_size = 200.0;
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Crossing".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(14.0),
        leading: Some(schist_layout::styles::Leading::Points(20.0)),
        writing_mode: Some(WritingMode::VerticalRightToLeft),
        small_caps: Some(true),
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(73.0, 20.0, 25.0, 70.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("hh", "Crossing");
    assert!(!schist_layout::compose::compose_story(&doc, frame.story).has_overflow());
    let contributor = doc
        .page_artwork(1, Rect::new(0.0, 0.0, 100.0, 100.0))
        .into_iter()
        .find(|o| o.id == frame.object)
        .expect("large caps cross the gutter")
        .into_owned();
    assert!(!contributor
        .paint_bounds()
        .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
    let mut reference = doc.clone();
    reference.objects[0] = contributor;
    reference.objects[0].page = 1;
    for dpi in [72.0, 144.0, 216.0] {
        let a = separate_page_without_graphics(&doc, 1, OutputSettings::at(dpi)).unwrap();
        let b = separate_page_without_graphics(&reference, 1, OutputSettings::at(dpi)).unwrap();
        for (a, b) in a.separation.plates().iter().zip(b.separation.plates()) {
            assert_eq!(a.data, b.data);
        }
        assert!(a
            .separation
            .plates()
            .iter()
            .any(|p| p.data.iter().any(|v| *v > 0.0)));
    }
}
