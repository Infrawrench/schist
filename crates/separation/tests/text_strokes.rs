use schist_layout::{
    authoring, CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    WritingMode,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

fn frame(doc: &mut LayoutDocument, style: &str) {
    let frame = authoring::text_frame(
        doc,
        &mut History::default(),
        0,
        Rect::new(18.0, 18.0, 120.0, 120.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("HéH AV", style);
    doc.objects.last_mut().unwrap().transform = schist_core::Affine {
        a: 0.9,
        b: 0.12,
        c: 0.15,
        d: 0.9,
        tx: 0.0,
        ty: 0.0,
    };
}

#[test]
fn fill_and_stroke_match_independent_text_objects_with_spot_tint_opacity_and_overprint() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for outside in [false, true] {
            for overprint in [false, true] {
                let mut doc = LayoutDocument::new(vec![Page::new("1", 160.0, 160.0)]);
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Base".into(),
                    point_size: Some(32.0),
                    writing_mode: Some(mode),
                    fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                    stroke: Some(Ink::spot("Outline", [40.0, 55.0, 20.0])),
                    stroke_weight: Some(2.0),
                    stroke_outside: Some(outside),
                    stroke_join: Some(if outside {
                        schist_text_engine::TextStrokeJoin::Round
                    } else if overprint {
                        schist_text_engine::TextStrokeJoin::Miter
                    } else {
                        schist_text_engine::TextStrokeJoin::Bevel
                    }),
                    stroke_miter_limit: Some(if overprint { 8.0 } else { 0.0 }),
                    fill_tint: Some(0.7),
                    stroke_tint: Some(0.6),
                    overprint_stroke: Some(overprint),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Default".into(),
                    opacity: Some(0.65),
                    ..Default::default()
                });
                frame(&mut doc, "Base");
                let mut reference = doc.clone();
                reference.objects.clear();
                reference.styles.add_paragraph(ParagraphStyle {
                    name: "Fill".into(),
                    based_on: Some("Base".into()),
                    stroke_disabled: true,
                    ..Default::default()
                });
                reference.styles.add_paragraph(ParagraphStyle {
                    name: "Stroke".into(),
                    based_on: Some("Base".into()),
                    fill_disabled: true,
                    ..Default::default()
                });
                frame(&mut reference, "Fill");
                frame(&mut reference, "Stroke");
                for dpi in [72.0, 144.0] {
                    let actual =
                        separate_page_without_graphics(&doc, 0, OutputSettings::at(dpi)).unwrap();
                    let expected =
                        separate_page_without_graphics(&reference, 0, OutputSettings::at(dpi))
                            .unwrap();
                    for (plate, reference) in actual
                        .separation
                        .plates()
                        .iter()
                        .zip(expected.separation.plates())
                    {
                        let difference = plate
                            .data
                            .iter()
                            .zip(&reference.data)
                            .position(|(a, b)| a != b);
                        assert!(difference.is_none(), "{mode:?}, outside={outside}, overprint={overprint}, dpi={dpi}, first differing sample: {difference:?}");
                    }
                    assert!(
                        actual
                            .separation
                            .plates()
                            .iter()
                            .filter(|p| p.data.iter().any(|v| *v > 0.1))
                            .count()
                            >= 2
                    );
                }
            }
        }
    }
}

#[test]
fn equal_text_fill_and_stroke_apply_opacity_once_with_every_alignment() {
    for outside in [false, true] {
        for opacity in [0.25, 0.5, 1.0] {
            let mut doc = LayoutDocument::new(vec![Page::new("1", 160.0, 160.0)]);
            let ink = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                point_size: Some(40.0),
                fill: Some(ink.clone()),
                stroke: Some(ink),
                stroke_weight: Some(5.0),
                stroke_outside: Some(outside),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Default".into(),
                opacity: Some(opacity),
                ..Default::default()
            });
            frame(&mut doc, "Base");
            let output =
                separate_page_without_graphics(&doc, 0, OutputSettings::at(144.0)).unwrap();
            let cyan = &output
                .separation
                .plate(output.plan.process[0])
                .unwrap()
                .data;
            assert!(cyan.iter().all(|v| *v <= opacity + 1e-6));
            assert!(cyan.iter().any(|v| (*v - opacity).abs() < 1e-6));
        }
    }
}

#[test]
fn outline_ink_crosses_a_gutter_without_moving_its_text_frame() {
    use schist_layout::{Point, Spread};
    for source in [0, 1] {
        for outside in [false, true] {
            let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0); 2]);
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Crossing".into(),
                point_size: Some(16.0),
                writing_mode: Some(WritingMode::VerticalRightToLeft),
                fill_disabled: true,
                stroke: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                stroke_weight: Some(32.0),
                stroke_outside: Some(outside),
                ..Default::default()
            });
            let bounds = Rect::new(if source == 0 { 75.0 } else { 1.0 }, 20.0, 24.0, 70.0);
            let frame =
                authoring::text_frame(&mut doc, &mut History::default(), source, bounds).unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("HH", "Crossing");
            let target = 1 - source;
            let contributor = doc
                .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
                .into_iter()
                .find(|o| o.id == frame.object)
                .expect("outline contributes across the gutter")
                .into_owned();
            assert!(!contributor
                .paint_bounds()
                .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
            let mut reference = doc.clone();
            reference.objects[0] = contributor;
            reference.objects[0].page = target;
            let settings = OutputSettings::at(144.0);
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
            assert!(
                a.iter().any(|v| *v > 0.0),
                "source={source}, outside={outside}"
            );
        }
    }
}
