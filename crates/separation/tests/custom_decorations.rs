use schist_layout::{
    authoring,
    decorations::{DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStyle},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, WritingMode,
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
fn decoration_layers_match_independent_objects_with_spot_tint_opacity_and_overprint() {
    for mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for weight in [0.75, 3.5] {
            for overprint in [false, true] {
                let mut doc = LayoutDocument::new(vec![Page::new("1", 160.0, 160.0)]);
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Base".into(),
                    point_size: Some(32.0),
                    writing_mode: Some(mode),
                    fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                    underline: Some(true),
                    strikethrough: Some(true),
                    underline_style: DecorationStyle {
                        paint: Some(Paint::Ink(Ink::spot("Underline", [40.0, 55.0, 20.0]))),
                        weight: Some(Measure::Points(weight)),
                        offset: Some(Measure::Points(5.0)),
                        tint: Some(0.6),
                        overprint: Some(overprint),
                        ..Default::default()
                    },
                    strike_style: DecorationStyle {
                        paint: Some(Paint::Ink(Ink::cmyk("Strike", [0.0, 0.0, 1.0, 0.0]))),
                        weight: Some(Measure::Points(weight * 2.0)),
                        offset: Some(Measure::Points(8.0)),
                        tint: Some(0.8),
                        overprint: Some(overprint),
                        ..Default::default()
                    },
                    fill_tint: Some(0.7),
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
                for (name, fill_disabled, underline, strike) in [
                    ("Underline", true, true, false),
                    ("Fill", false, false, false),
                    ("Strike", true, false, true),
                ] {
                    reference.styles.add_paragraph(ParagraphStyle {
                        name: name.into(),
                        based_on: Some("Base".into()),
                        fill_disabled,
                        underline: Some(underline),
                        strikethrough: Some(strike),
                        ..Default::default()
                    });
                    frame(&mut reference, name);
                }
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
                        assert!(difference.is_none(), "{mode:?}, weight={weight}, overprint={overprint}, dpi={dpi}, first differing sample: {difference:?}");
                    }
                    assert!(
                        actual
                            .separation
                            .plates()
                            .iter()
                            .filter(|p| p.data.iter().any(|v| *v > 0.1))
                            .count()
                            >= 3
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_decoration_offsets_contribute_across_both_sides_of_a_gutter() {
    use schist_layout::{Point, Spread};
    for source in [0, 1] {
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
            underline: Some(true),
            underline_style: DecorationStyle {
                paint: Some(Paint::Ink(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]))),
                weight: Some(Measure::Points(2.5)),
                offset: Some(Measure::Points(if source == 0 { 30.0 } else { -30.0 })),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            source,
            Rect::new(if source == 0 { 75.0 } else { 1.0 }, 20.0, 24.0, 70.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("HH", "Crossing");
        let target = 1 - source;
        let contributor = doc
            .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
            .into_iter()
            .find(|o| o.id == frame.object)
            .expect("decoration reaches neighbor")
            .into_owned();
        assert!(!contributor
            .paint_bounds()
            .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
        let mut reference = doc.clone();
        reference.objects[0] = contributor;
        reference.objects[0].page = target;
        let actual =
            separate_page_without_graphics(&doc, target, OutputSettings::at(144.0)).unwrap();
        let expected =
            separate_page_without_graphics(&reference, target, OutputSettings::at(144.0)).unwrap();
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
        assert!(a.iter().any(|v| *v > 0.0), "source={source}");
    }
}
