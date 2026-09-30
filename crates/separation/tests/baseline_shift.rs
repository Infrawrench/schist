use schist_layout::{
    authoring, styles::BaselineShift, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect,
    Story, WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn baseline_shift_matches_translated_ink_at_every_output_resolution() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for dpi in [72.0, 144.0, 216.0] {
            let mut doc = LayoutDocument::new(vec![Page::new("1", 160.0, 160.0)]);
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Body".into(),
                point_size: Some(20.0),
                underline: Some(true),
                strikethrough: Some(true),
                writing_mode: Some(mode),
                fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                ..Default::default()
            });
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(40.0, 40.0, 70.0, 70.0),
            )
            .unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("HH HH", "Body");
            let settings = OutputSettings::at(dpi);
            let before = separate_page_without_graphics(&doc, 0, settings).unwrap();
            for shift in [-12.0, 6.0] {
                doc.styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Body")
                    .unwrap()
                    .baseline_shift = Some(BaselineShift::Offset(shift));
                let after = separate_page_without_graphics(&doc, 0, settings).unwrap();
                let a = &before
                    .separation
                    .plate(before.plan.process[0])
                    .unwrap()
                    .data;
                let b = &after.separation.plate(after.plan.process[0]).unwrap().data;
                let side = (160.0 * dpi / 72.0) as i32;
                let delta = (shift * dpi / 72.0) as i32;
                let (dx, dy) = if mode == WritingMode::Horizontal {
                    (0, -delta)
                } else {
                    (delta, 0)
                };
                let mut ink = 0;
                for y in 0..side {
                    for x in 0..side {
                        let actual = b[(y * side + x) as usize];
                        let (sx, sy) = (x - dx, y - dy);
                        let expected = if (0..side).contains(&sx) && (0..side).contains(&sy) {
                            a[(sy * side + sx) as usize]
                        } else {
                            0.0
                        };
                        assert!(
                            (actual - expected).abs() < 1e-6,
                            "{mode:?} {dpi} {shift} at {x},{y}"
                        );
                        ink += usize::from(actual > 0.5);
                    }
                }
                assert!(ink > 100);
            }
        }
    }
}

#[test]
fn shifted_ink_crosses_the_gutter_even_when_its_frame_does_not() {
    use schist_layout::{Point, Spread};
    for source in [0, 1] {
        for scripted in [false, true] {
            let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0); 2]);
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            let shift = if source == 0 { 30.0 } else { -30.0 };
            doc.styles.text_preferences.superscript_position = 150.0;
            doc.styles.text_preferences.subscript_position = 150.0;
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Crossing".into(),
                point_size: Some(14.0),
                writing_mode: Some(WritingMode::VerticalRightToLeft),
                leading: Some(20.0),
                position: scripted.then_some(if source == 0 {
                    schist_layout::styles::TextPosition::Superscript
                } else {
                    schist_layout::styles::TextPosition::Subscript
                }),
                baseline_shift: Some(BaselineShift::Offset(if scripted { 0.0 } else { shift })),
                fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                ..Default::default()
            });
            let bounds = Rect::new(if source == 0 { 70.0 } else { 5.0 }, 20.0, 25.0, 70.0);
            let frame =
                authoring::text_frame(&mut doc, &mut History::default(), source, bounds).unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("HH", "Crossing");
            assert!(!schist_layout::compose::compose_story(&doc, frame.story).has_overflow());
            let target = 1 - source;
            let contributor = doc
                .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
                .into_iter()
                .find(|o| o.id == frame.object)
                .expect("shifted text contributes across the gutter")
                .into_owned();
            assert!(
                !contributor
                    .paint_bounds()
                    .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)),
                "the unshifted frame must be wholly outside the target"
            );
            let settings = OutputSettings::at(144.0);
            let actual = separate_page_without_graphics(&doc, target, settings).unwrap();
            // The same source composition placed explicitly on the destination page
            // gives an independent reference without page-contributor filtering.
            let mut reference = doc.clone();
            reference.objects[0] = contributor;
            reference.objects[0].page = target;
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
            // Script glyphs are smaller; the rule is nonempty retained ink,
            // not the original full-size specimen's arbitrary pixel count.
            assert!(
                a.iter().any(|v| *v > 0.0),
                "source={source} scripted={scripted}"
            );
        }
    }
}
