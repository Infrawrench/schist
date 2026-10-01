use schist_layout::{authoring, object_styles, *};
use schist_separation::{
    separate_page, GraphicPlacement, GraphicSource, OutputSettings, PlacedGraphic,
};

struct Image;
impl GraphicSource for Image {
    fn sample(&self, _: &Link, placement: &GraphicPlacement) -> Option<PlacedGraphic> {
        Some(PlacedGraphic::solid(placement.dest, [0.0, 0.7, 0.0, 0.0]))
    }
}

#[test]
fn frame_fill_content_and_stroke_match_separately_placed_artwork_under_every_transform() {
    for graphic in [false, true] {
        for curved in [false, true] {
            for matrix in [
                schist_core::Affine::IDENTITY,
                schist_core::Affine {
                    a: 0.9,
                    b: 0.2,
                    c: 0.25,
                    d: 0.85,
                    tx: 0.0,
                    ty: 0.0,
                },
            ] {
                for overprint in [false, true] {
                    let mut doc = LayoutDocument::new(vec![Page::new("1", 140.0, 140.0)]);
                    let mut history = History::default();
                    let rect = Rect::new(20.0, 20.0, 80.0, 65.0);
                    let id = if graphic {
                        authoring::graphic_frame(
                            &mut doc,
                            &mut history,
                            0,
                            rect,
                            "image.png",
                            false,
                        )
                        .unwrap()
                    } else {
                        let frame = authoring::text_frame(&mut doc, &mut history, 0, rect).unwrap();
                        authoring::set_text(&mut doc, &mut history, frame.story, "Frame\npaint");
                        frame.object
                    };
                    let fill = Ink::cmyk("Cyan", [0.8, 0.0, 0.0, 0.0]);
                    let stroke = Ink::spot("Border spot", [40.0, 55.0, 20.0]);
                    doc.styles.objects.push(ObjectStyle {
                        name: "Frame".into(),
                        enable_fill: Some(true),
                        enable_stroke: Some(true),
                        paint: ObjectPaint {
                            fill: Some(Paint::Ink(fill.clone())),
                            stroke: Some(Paint::Ink(stroke.clone())),
                            stroke_width: Some(8.0),
                            fill_tint: Some(0.6),
                            stroke_tint: Some(0.75),
                            overprint_stroke: Some(overprint),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    object_styles::apply_style(&mut doc, &mut history, &[id], Some("Frame"));
                    let object = &mut doc.objects[0];
                    object.transform = matrix;
                    object.transparency = 0.7;
                    if curved {
                        if let LayoutObject::GraphicFrame { clip_path, .. } = &mut object.object {
                            *clip_path = Some(ShapePath::ellipse(1.0, 1.0));
                        } else {
                            object.appearance.outline = Some(ShapePath::ellipse(1.0, 1.0));
                        }
                    }
                    // Independent control: ordinary fill, unchanged content,
                    // then an ordinary stroked path, with explicit paint values.
                    let mut reference = doc.clone();
                    let mut content = reference.objects.remove(0);
                    content.appearance = Default::default();
                    let path = if curved {
                        {
                            // Use the same authored normalized contour. Building a new
                            // ellipse at another size can round one handle differently.
                            let mut path = ShapePath::ellipse(1.0, 1.0);
                            path.map_points(|p| Point::new(p.x * rect.width, p.y * rect.height));
                            path
                        }
                    } else {
                        authoring::path_for(
                            authoring::ShapeKind::Rectangle,
                            rect.width,
                            rect.height,
                        )
                    };
                    let mut background = content.clone();
                    background.id = ObjectId::next();
                    background.object = LayoutObject::Shape {
                        path: path.clone(),
                        fill: Some(fill),
                        stroke: None,
                        stroke_width: 0.0,
                        fill_overprint: false,
                        stroke_overprint: false,
                        tints: PaintTints {
                            fill: 0.6,
                            stroke: 1.0,
                        },
                    };
                    let mut border = content.clone();
                    border.id = ObjectId::next();
                    border.object = LayoutObject::Shape {
                        path,
                        fill: None,
                        stroke: Some(stroke),
                        stroke_width: 8.0,
                        fill_overprint: false,
                        stroke_overprint: overprint,
                        tints: PaintTints {
                            fill: 1.0,
                            stroke: 0.75,
                        },
                    };
                    reference.objects = vec![background, content, border];
                    for dpi in [72.0, 144.0] {
                        let settings = OutputSettings::at(dpi);
                        let actual = separate_page(&doc, 0, settings, &Image).unwrap();
                        let expected = separate_page(&reference, 0, settings, &Image).unwrap();
                        let differences: Vec<_> = actual
                            .separation
                            .plates()
                            .iter()
                            .zip(expected.separation.plates())
                            .enumerate()
                            .flat_map(|(plate, (a, b))| {
                                a.data
                                    .iter()
                                    .zip(&b.data)
                                    .enumerate()
                                    .filter(|(_, (a, b))| a != b)
                                    .map(move |(pixel, (a, b))| (plate, pixel, *a, *b))
                            })
                            .collect();
                        assert!(differences.is_empty(), "graphic={graphic} curved={curved} overprint={overprint} dpi={dpi}; {} differing samples, first {:?}, max {}", differences.len(),differences.first(),differences.iter().map(|(_,_,a,b)|(a-b).abs()).fold(0.0,f32::max));
                        assert!(actual
                            .separation
                            .plates()
                            .iter()
                            .any(|p| p.data.iter().any(|v| *v > 0.1)));
                    }
                }
            }
        }
    }
}

#[test]
fn unavailable_graphics_keep_frame_paint_and_still_fail_preflight() {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 100.0, 100.0)]);
    let mut history = History::default();
    let id = authoring::graphic_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(20.0, 20.0, 50.0, 50.0),
        "missing.png",
        false,
    )
    .unwrap();
    object_styles::edit_paint(
        &mut doc,
        &mut history,
        &[id],
        &ObjectPaint {
            fill: Some(Paint::Ink(Ink::black())),
            ..Default::default()
        },
    );
    for known_missing in [false, true] {
        if let LayoutObject::GraphicFrame { link, .. } = &mut doc.objects[0].object {
            link.present = !known_missing;
        }
        let page = separate_page(
            &doc,
            0,
            OutputSettings::at(72.0),
            &schist_separation::NoGraphics,
        )
        .unwrap();
        assert!(
            page.separation
                .plate(page.plan.process[3])
                .unwrap()
                .at(40, 40)
                > 0.99
        );
        assert!(page
            .report
            .findings
            .iter()
            .any(|f| f.severity == schist_separation::Severity::Error));
    }
}
