use schist_layout::{
    authoring, text_path, History, Ink, LayoutDocument, Page, ParagraphStyle, Point, Rect,
    ShapePath, Spread, Story, SubPath,
};
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn rotated_path_glyphs_contribute_on_both_sides_of_a_gutter_when_the_baseline_does_not() {
    for source in [0, 1] {
        let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 2]);
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: Some(1),
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Crossing".into(),
            point_size: Some(24.0),
            fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
            ..Default::default()
        });
        let x = if source == 0 { 96.0 } else { 4.0 };
        let points = if source == 0 {
            vec![Point::new(x, 15.0), Point::new(x, 85.0)]
        } else {
            vec![Point::new(x, 85.0), Point::new(x, 15.0)]
        };
        let id = authoring::path_shape(
            &mut doc,
            &mut History::default(),
            source,
            ShapePath {
                subpaths: vec![SubPath {
                    points,
                    ..Default::default()
                }],
                even_odd: false,
            },
            authoring::Paint::none(),
        )
        .unwrap();
        let frame = text_path::attach(&mut doc, &mut History::default(), id).unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("HHH", "Crossing");
        let target = 1 - source;
        let contributor = doc
            .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
            .into_iter()
            .find(|o| o.id == id)
            .expect("glyphs reach the neighboring page")
            .into_owned();
        assert!(!contributor
            .paint_bounds()
            .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
        let mut expected = doc.clone();
        expected.objects[0] = contributor;
        expected.objects[0].page = target;
        for dpi in [72.0, 144.0, 216.0] {
            let a = separate_page_without_graphics(&doc, target, OutputSettings::at(dpi)).unwrap();
            let b =
                separate_page_without_graphics(&expected, target, OutputSettings::at(dpi)).unwrap();
            let a = &a.separation.plate(a.plan.process[0]).unwrap().data;
            let b = &b.separation.plate(b.plan.process[0]).unwrap().data;
            assert_eq!(a, b, "source={source},dpi={dpi}");
            assert!(a.iter().any(|v| *v > 0.1), "source={source},dpi={dpi}");
        }
    }
}

#[path = "../examples/support/text_path.rs"]
mod proof;

#[test]
fn path_decorations_on_spaces_contribute_across_gutters_without_visible_glyphs() {
    use schist_layout::decorations::{
        DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStyle,
    };
    for source in [0, 1] {
        for offset in [None, Some(30.0)] {
            let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 2]);
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Crossing".into(),
                point_size: Some(48.0),
                fill_disabled: true,
                underline: Some(true),
                underline_style: DecorationStyle {
                    paint: Some(Paint::Ink(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]))),
                    offset: offset.map(Measure::Points),
                    weight: Some(Measure::Points(2.5)),
                    ..Default::default()
                },
                ..Default::default()
            });
            let x = if source == 0 { 99.0 } else { 1.0 };
            let points = if source == 0 {
                vec![Point::new(x, 85.0), Point::new(x, 15.0)]
            } else {
                vec![Point::new(x, 15.0), Point::new(x, 85.0)]
            };
            let id = authoring::path_shape(
                &mut doc,
                &mut History::default(),
                source,
                ShapePath {
                    subpaths: vec![SubPath {
                        points,
                        ..Default::default()
                    }],
                    even_odd: false,
                },
                authoring::Paint::none(),
            )
            .unwrap();
            let frame = text_path::attach(&mut doc, &mut History::default(), id).unwrap();
            doc.stories[frame.story.0 as usize] = Story::from_text("  ", "Crossing");
            let target = 1 - source;
            let contributor = doc
                .page_artwork(target, Rect::new(0.0, 0.0, 100.0, 100.0))
                .into_iter()
                .find(|o| o.id == id)
                .expect("path line reaches neighbor")
                .into_owned();
            assert!(!contributor
                .paint_bounds()
                .intersects(Rect::new(0.0, 0.0, 100.0, 100.0)));
            let mut expected = doc.clone();
            expected.objects[0] = contributor;
            expected.objects[0].page = target;
            for dpi in [72.0, 144.0, 216.0] {
                let a =
                    separate_page_without_graphics(&doc, target, OutputSettings::at(dpi)).unwrap();
                let b = separate_page_without_graphics(&expected, target, OutputSettings::at(dpi))
                    .unwrap();
                let a = &a.separation.plate(a.plan.process[0]).unwrap().data;
                let b = &b.separation.plate(b.plan.process[0]).unwrap().data;
                assert_eq!(a, b, "source={source},offset={offset:?},dpi={dpi}");
                assert!(
                    a.iter().any(|v| *v > 0.1),
                    "source={source},offset={offset:?},dpi={dpi}"
                );
            }
        }
    }
}

#[test]
fn design_path_text_matches_direct_engine_baselines_at_every_resolution_and_affine() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = proof::document();
    for transformed in [true, false] {
        if !transformed {
            for object in &mut doc.objects {
                object.transform = schist_core::Affine::IDENTITY;
            }
        }
        for dpi in [72.0, 144.0, 216.0] {
            for page in 0..doc.pages.len() {
                let settings = OutputSettings::at(dpi);
                let actual = separate_page_without_graphics(&doc, page, settings).unwrap();
                let expected = proof::reference(&doc, page, settings);
                for (a, b) in actual
                    .separation
                    .plates()
                    .iter()
                    .zip(expected.separation.plates())
                {
                    let first = a.data.iter().zip(&b.data).position(|(a, b)| a != b);
                    assert!(
                        first.is_none(),
                        "page={page},dpi={dpi},affine={transformed},first={first:?}"
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
