//! Cropping a spread into pages must not change its inks, stacking or text flow.
use schist_layout::{
    affine::Affine,
    authoring::{self, Paint},
    History, Ink, Insets, LayerId, LayoutDocument, LayoutLayer, Page, Point, Rect, Spread, Story,
};
use schist_separation::{
    separate_page, separate_page_built, GraphicPlacement, GraphicSource, NoGraphics,
    OutputSettings, PlacedGraphic,
};

struct Native;
impl GraphicSource for Native {
    fn sample(&self, _: &schist_layout::Link, p: &GraphicPlacement) -> Option<PlacedGraphic> {
        Some(PlacedGraphic::solid(p.dest, [0.23, 0.57, 0.11, 0.09]))
    }
}

fn document(count: usize, reverse: bool, gutter: f32) -> LayoutDocument {
    let mut doc = LayoutDocument::new(
        (0..count)
            .map(|i| Page::new(i.to_string(), 60.0 + i as f32 * 12.0, 72.0))
            .collect(),
    );
    let mut pages: Vec<_> = (0..count).collect();
    if reverse {
        pages.reverse();
    }
    doc.spreads = vec![Spread {
        pages,
        binding_location: Some(1),
        gutter,
        origin: Point::ZERO,
    }];
    doc.inks.push(Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0]));
    doc.layers = vec![LayerId(1), LayerId(0)];
    doc.layer_properties.push(LayoutLayer {
        id: LayerId(1),
        name: "Top".into(),
        visible: true,
        locked: false,
    });
    let mut history = History::default();
    // Deliberately interleave ownership and layers. Every object reaches a neighbor.
    for page in (0..count).rev() {
        let slot = doc.spreads[0]
            .pages
            .iter()
            .position(|p| *p == page)
            .unwrap();
        let x = if slot == 0 {
            doc.pages[page].width - 24.0
        } else {
            -24.0 - gutter
        };
        let id = authoring::rectangle(
            &mut doc,
            &mut history,
            page,
            Rect::new(x, 4.0, 60.0 + gutter, 22.0),
            Paint::filled("Magenta"),
        )
        .unwrap();
        doc.objects.last_mut().unwrap().transparency = 0.35;
        doc.object_layers
            .iter_mut()
            .find(|(object, _)| *object == id)
            .unwrap()
            .1 = LayerId(0);
        authoring::graphic_frame(
            &mut doc,
            &mut history,
            page,
            Rect::new(x, 24.0, 58.0 + gutter, 18.0),
            "native",
            false,
        )
        .unwrap();
        doc.objects.last_mut().unwrap().transform = Affine::skew(0.25, 0.0);
        let text = authoring::text_frame(
            &mut doc,
            &mut history,
            page,
            Rect::new(x, 44.0, 64.0 + gutter, 24.0),
        )
        .unwrap();
        doc.stories[text.story.0 as usize] = Story::from_text("Crossover é ffi", "Body");
        doc.objects.last_mut().unwrap().transform = Affine::translate(-3.0, 0.0);
    }
    doc
}

#[test]
fn each_page_and_its_inside_bleed_matches_the_same_region_of_the_whole_spread() {
    for count in 2..=4 {
        for reverse in [false, true] {
            for gutter in [0.0, 6.0] {
                let mut doc = document(count, reverse, gutter);
                for p in &mut doc.pages {
                    p.bleed = Insets::uniform(6.0);
                }
                let spread = &doc.spreads[0];
                let offsets: Vec<_> = (0..count)
                    .map(|page| {
                        spread.page_origin(
                            &doc.pages,
                            spread.pages.iter().position(|p| *p == page).unwrap(),
                        )
                    })
                    .collect();
                let mut whole = doc.clone();
                let mut paper = Page::new("spread", spread.bounds(&doc.pages).width, 72.0);
                paper.bleed = Insets::uniform(6.0);
                whole.pages = vec![paper];
                whole.spreads = vec![Spread::single(0)];
                for object in &mut whole.objects {
                    *object = object.translated_artwork(offsets[object.page]);
                    object.page = 0;
                }
                for dpi in [72.0, 144.0] {
                    let settings = OutputSettings::at(dpi);
                    let reference = separate_page(&whole, 0, settings, &Native).unwrap();
                    for (page, offset) in offsets.iter().enumerate() {
                        // Both ICC-supplied and default separation paths must use the spread.
                        let actual = if page.is_multiple_of(2) {
                            separate_page(&doc, page, settings, &Native).unwrap()
                        } else {
                            separate_page_built(
                                &doc,
                                page,
                                settings,
                                &Native,
                                &schist_separation::build::NaiveBuild,
                            )
                            .unwrap()
                        };
                        let mut ink = 0.0;
                        for (p, expected) in reference.separation.plates().iter().enumerate() {
                            let a = actual.separation.plate(p).unwrap();
                            for y in a.rect.top..a.rect.bottom {
                                for x in a.rect.left..a.rect.right {
                                    let b = expected.at(x + settings.to_pixels(offset.x), y);
                                    assert!((a.at(x,y) - b).abs() < 0.0001,
                                        "count={count} reverse={reverse} gutter={gutter} dpi={dpi} page={page} plate={p} at={x},{y}: {} != {b}", a.at(x,y));
                                    ink += a.at(x, y);
                                }
                            }
                        }
                        assert!(ink > 10.0);
                    }
                }
            }
        }
    }
}

#[test]
fn a_neighbors_stroke_and_missing_resource_contribute_only_where_they_reach() {
    let mut doc = LayoutDocument::new(vec![Page::new("page", 60.0, 60.0); 3]);
    doc.spreads = vec![
        Spread {
            pages: vec![0, 1],
            ..Spread::single(0)
        },
        Spread::single(2),
    ];
    authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(59.0, 10.0, 0.0, 40.0),
        authoring::ShapeKind::Line,
        Paint::stroked("Black", 8.0),
    )
    .unwrap();
    let settings = OutputSettings {
        include_bleed: false,
        ..OutputSettings::at(72.0)
    };
    let right = separate_page(&doc, 1, settings, &NoGraphics).unwrap();
    assert!(
        right
            .separation
            .plate(right.plan.process[3])
            .unwrap()
            .at(1, 30)
            > 0.9
    );
    authoring::graphic_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(55.0, 5.0, 20.0, 20.0),
        "missing",
        false,
    )
    .unwrap();
    for page in 0..3 {
        let result = separate_page(&doc, page, settings, &NoGraphics).unwrap();
        assert_eq!(result.report.is_printable(), page == 2);
    }
    // Bleed-only contributions participate in preflight only when bleed is requested.
    doc.objects.last_mut().unwrap().bounds.x = 45.0;
    doc.objects.last_mut().unwrap().bounds.width = 10.0;
    doc.pages[1].bleed.left = 10.0;
    assert!(separate_page(&doc, 1, settings, &NoGraphics)
        .unwrap()
        .report
        .is_printable());
    assert!(
        !separate_page(&doc, 1, OutputSettings::at(72.0), &NoGraphics)
            .unwrap()
            .report
            .is_printable()
    );
}
