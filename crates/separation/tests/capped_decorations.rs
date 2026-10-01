#[path = "../examples/support/capped_decorations.rs"]
mod capped_decorations;
use schist_separation::{separate_page_without_graphics, OutputSettings};
#[test]
fn capped_inks_match_independent_capsules_through_resolution_and_affine_changes() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = capped_decorations::document();
    for transformed in [false, true] {
        for object in &mut doc.objects {
            object.transform = if transformed {
                schist_core::Affine {
                    a: 0.9,
                    b: 0.12,
                    c: 0.15,
                    d: 0.9,
                    tx: 0.0,
                    ty: 0.0,
                }
            } else {
                schist_core::Affine::IDENTITY
            };
        }
        for dpi in [72.0, 144.0, 216.0] {
            let settings = OutputSettings::at(dpi);
            for page in 0..doc.pages.len() {
                let actual = separate_page_without_graphics(&doc, page, settings).unwrap();
                let reference = capped_decorations::reference(&doc, page, settings);
                for (index, (a, b)) in actual
                    .separation
                    .plates()
                    .iter()
                    .zip(reference.separation.plates())
                    .enumerate()
                {
                    let first = a.data.iter().zip(&b.data).position(|(a, b)| a != b);
                    assert!(
                    first.is_none(),
                    "page={page},dpi={dpi},transformed={transformed},plate={index},first={:?},count={}",
                    first.map(|i| (i, a.data[i], b.data[i])),
                    a.data.iter().zip(&b.data).filter(|(a,b)| a != b).count()
                );
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

#[test]
fn automatic_and_explicit_caps_and_dots_contribute_ink_across_either_side_of_a_gutter() {
    use schist_layout::decorations::{
        DecorationMeasure, DecorationPaint, DecorationStroke, DecorationStyle,
    };
    use schist_layout::{
        authoring, History, Ink, LayoutDocument, Page, ParagraphStyle, Point, Rect, Spread, Story,
    };
    use schist_text_engine::{DecorationCap, DecorationDashes, TextDecorationPattern};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for source in [0, 1] {
        for pattern in [
            TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![100.0, 10.0],
                cap: DecorationCap::Round,
            }),
            TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![100.0, 10.0],
                cap: DecorationCap::Projecting,
            }),
            TextDecorationPattern::Dots(vec![0.125]),
        ] {
            for weight in [None, Some(DecorationMeasure::Points(2.5))] {
                let mut doc = LayoutDocument::new(vec![Page::new("proof", 100.0, 100.0); 2]);
                doc.spreads = vec![Spread {
                    pages: vec![0, 1],
                    binding_location: Some(1),
                    gutter: 0.0,
                    origin: Point::ZERO,
                }];
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Crossing".into(),
                    family: Some("IBM Plex Sans".into()),
                    point_size: Some(32.0),
                    align: Some(if source == 0 {
                        schist_layout::styles::Align::Right
                    } else {
                        schist_layout::styles::Align::Left
                    }),
                    fill_disabled: true,
                    underline: Some(true),
                    underline_style: DecorationStyle {
                        weight,
                        paint: Some(DecorationPaint::Ink(Ink::cmyk(
                            "Cyan",
                            [1.0, 0.0, 0.0, 0.0],
                        ))),
                        stroke: Some(DecorationStroke {
                            fitting: Default::default(),
                            name: "Caps".into(),
                            pattern: pattern.clone(),
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                let frame = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    source,
                    Rect::new(if source == 0 { 75.0 } else { 0.0 }, 15.0, 25.0, 70.0),
                )
                .unwrap();
                doc.stories[frame.story.0 as usize] = Story::from_text("H", "Crossing");
                let target = 1 - source;
                let contributor = doc
                    .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
                    .into_iter()
                    .find(|o| o.id == frame.object)
                    .expect("cap reaches neighbor")
                    .into_owned();
                assert!(!contributor
                    .paint_bounds()
                    .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
                let mut reference = doc.clone();
                reference.objects[0] = contributor;
                reference.objects[0].page = target;
                for dpi in [72.0, 144.0, 216.0] {
                    let a = separate_page_without_graphics(&doc, target, OutputSettings::at(dpi))
                        .unwrap();
                    let b =
                        separate_page_without_graphics(&reference, target, OutputSettings::at(dpi))
                            .unwrap();
                    let actual = &a.separation.plate(a.plan.process[0]).unwrap().data;
                    let expected = &b.separation.plate(b.plan.process[0]).unwrap().data;
                    assert_eq!(actual, expected);
                    assert!(
                        actual.iter().any(|v| *v > 0.0),
                        "source={source},pattern={pattern:?},weight={weight:?},dpi={dpi}"
                    );
                }
            }
        }
    }
}
