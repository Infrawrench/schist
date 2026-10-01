use schist_layout::{
    authoring, styles::Leading, CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle,
    Point, Rect, Spread, Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn tight_mixed_size_ink_crosses_the_gutter_even_when_its_frame_does_not() {
    for source in [0, 1] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0); 2]);
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: Some(1),
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tight".into(),
            point_size: Some(8.0),
            leading: Some(Leading::Points(0.0)),
            writing_mode: Some(if source == 0 {
                WritingMode::VerticalRightToLeft
            } else {
                WritingMode::VerticalLeftToRight
            }),
            fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Large".into(),
            point_size: Some(60.0),
            ..Default::default()
        });
        let bounds = Rect::new(if source == 0 { 45.0 } else { 3.0 }, 10.0, 52.0, 80.0);
        let frame =
            authoring::text_frame(&mut doc, &mut History::default(), source, bounds).unwrap();
        let mut story = Story::from_text("i\nH", "Tight");
        story.apply_style(2, 3, "Large");
        doc.stories[frame.story.0 as usize] = story;
        assert!(!schist_layout::compose::compose_story(&doc, frame.story).has_overflow());
        let target = 1 - source;
        let contributor = doc
            .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
            .into_iter()
            .find(|o| o.id == frame.object)
            .expect("tight leading contributes across the gutter")
            .into_owned();
        assert!(!contributor
            .paint_bounds()
            .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
        let mut reference = doc.clone();
        reference.objects[0] = contributor;
        reference.objects[0].page = target;
        for dpi in [72.0, 144.0, 216.0] {
            let settings = OutputSettings::at(dpi);
            let actual = separate_page_without_graphics(&doc, target, settings).unwrap();
            let expected = separate_page_without_graphics(&reference, target, settings).unwrap();
            let a = &actual
                .separation
                .plate(actual.plan.process[0])
                .unwrap()
                .data;
            let b = &expected
                .separation
                .plate(expected.plan.process[0])
                .unwrap()
                .data;
            assert_eq!(a, b);
            assert!(a.iter().any(|v| *v > 0.0));
        }
    }
}
