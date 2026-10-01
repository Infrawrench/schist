//! Independently enumerated dots use analytic circle/rectangle intersections.
use schist_layout::{
    authoring, compose,
    decorations::{
        DecorationMeasure as Measure, DecorationPaint as Paint, DecorationStroke, DecorationStyle,
    },
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, WritingMode,
};
use schist_separation::{
    coverage::{Coverage, InkMode, Separation},
    raster::{scale_spec, tinted_coats_for, warp_coverage},
    separate_page_without_graphics, OutputSettings, PagePixel, SeparatedPage,
};
use schist_text_engine::TextDecorationPattern;

pub fn document() -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 6]);
    doc.styles.add_character(CharacterStyle {
        name: "Default".into(),
        opacity: Some(0.7),
        ..Default::default()
    });
    for case in 0..6 {
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalRightToLeft,
            WritingMode::VerticalLeftToRight,
        ][case / 2];
        let name = format!("Case {case}");
        let line = DecorationStyle {
            stroke: Some(DecorationStroke {
                fitting: Default::default(),
                name: "Dots".into(),
                pattern: TextDecorationPattern::Dots(if case % 2 == 0 {
                    vec![6.0]
                } else {
                    vec![4.5, 7.5]
                }),
            }),
            paint: Some(Paint::Ink(Ink::spot("Dot spot", [45.0, 60.0, 30.0]))),
            gap_paint: Some(Paint::Ink(Ink::cmyk("Gap cyan", [1.0, 0.0, 0.0, 0.0]))),
            weight: Some(Measure::Points(if case % 2 == 0 { 1.5 } else { 4.0 })),
            offset: Some(Measure::Points(8.0)),
            tint: Some(0.8),
            gap_tint: Some(0.65),
            overprint: Some(case % 2 == 1),
            gap_overprint: Some(case % 2 == 0),
        };
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(30.0),
            leading: Some(schist_layout::styles::Leading::Points(48.0)),
            writing_mode: Some(mode),
            fill_disabled: true,
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: line.clone(),
            strike_style: DecorationStyle {
                offset: Some(Measure::Points(if mode == WritingMode::Horizontal {
                    10.0
                } else {
                    -8.0
                })),
                ..line
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            case,
            Rect::new(25.0, 25.0, 140.0, 140.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("HéH AV", &name);
        doc.objects.last_mut().unwrap().transform = schist_core::Affine {
            a: 0.9,
            b: 0.12,
            c: 0.15,
            d: 0.9,
            tx: 0.0,
            ty: 0.0,
        };
    }
    doc
}

pub fn reference(doc: &LayoutDocument, page: usize, settings: OutputSettings) -> SeparatedPage {
    // Reuse only page/plate metadata; discard every generated ink sample.
    let mut result = separate_page_without_graphics(doc, page, settings).unwrap();
    let page_geometry = &doc.pages[page];
    let output = settings.to_pixels_rect(settings.output_box(page_geometry));
    result.separation = Separation::new(result.plan.plates.len(), output);
    let placed = doc.objects.iter().find(|o| o.page == page).unwrap();
    let composed = compose::compose_object(doc, placed).unwrap();
    let schist_layout::LayoutObject::TextFrame { story, .. } = placed.object else {
        unreachable!()
    };
    for line in composed.lines {
        let mut spec = scale_spec(
            compose::line_spec(&line, &doc.stories[story.0 as usize], doc),
            settings.scale(),
        );
        spec.wrap_width = None;
        let metrics = schist_text_engine::measure(&spec).unwrap();
        let character = line
            .paragraph
            .character(doc.styles.resolve_character("Default"));
        let frame = PagePixel::rect(settings, page_geometry, line.bounds);
        let vertical = spec.writing_mode != schist_text_engine::WritingMode::Horizontal;
        for strike in [false, true] {
            let style = if strike {
                &character.strike_style
            } else {
                &character.underline_style
            };
            let weight = style.weight.unwrap().points().unwrap() * settings.scale();
            let offset = style.offset.unwrap().points().unwrap() * settings.scale();
            let center = if vertical {
                metrics.height / 2.0
                    + if spec.writing_mode == schist_text_engine::WritingMode::VerticalRl {
                        offset
                    } else {
                        -offset
                    }
            } else {
                metrics.first_baseline + if strike { -offset } else { offset }
            };
            let length = metrics.width.ceil();
            let geometry = if vertical {
                Rect::new(center - weight / 2.0, 0.0, weight, length)
            } else {
                Rect::new(0.0, center - weight / 2.0, length, weight)
            };
            let TextDecorationPattern::Dots(intervals) = &style.stroke.as_ref().unwrap().pattern
            else {
                unreachable!()
            };
            let radius = weight / 2.0;
            let bounds = if vertical {
                schist_core::IntRect::new(
                    geometry.x.floor() as i32,
                    (-radius).floor() as i32,
                    geometry.right().ceil() as i32,
                    (length + radius).ceil() as i32,
                )
            } else {
                schist_core::IntRect::new(
                    (-radius).floor() as i32,
                    geometry.y.floor() as i32,
                    (length + radius).ceil() as i32,
                    geometry.bottom().ceil() as i32,
                )
            };
            let mut centers = Vec::new();
            let mut cursor = 0.0;
            while cursor <= length {
                for interval in intervals {
                    if cursor <= length {
                        centers.push(cursor);
                    }
                    cursor += interval * settings.scale();
                }
            }
            for (gap, paint) in [
                (true, style.gap_paint(&character)),
                (false, style.paint(&character)),
            ] {
                let (ink, tint, overprint) = paint.unwrap();
                let mut mask = Coverage::new(bounds);
                for y in bounds.top..bounds.bottom {
                    for x in bounds.left..bounds.right {
                        let (along, across) = if vertical { (y, x) } else { (x, y) };
                        let pixel = [
                            f64::from(along),
                            f64::from(across),
                            f64::from(along + 1),
                            f64::from(across + 1),
                        ];
                        let cross = if vertical { geometry.x } else { geometry.y };
                        let area = centers
                            .iter()
                            .map(|position| {
                                circle_area(
                                    f64::from(*position),
                                    f64::from(cross) + f64::from(radius),
                                    f64::from(radius),
                                    pixel,
                                )
                            })
                            .sum::<f64>() as f32;
                        // Fixture spacing exceeds the diameter, so these circles
                        // are disjoint. Gap paint occupies the base rectangle only.
                        let whole = rectangle_area(
                            [
                                0.0,
                                f64::from(if vertical { geometry.x } else { geometry.y }),
                                f64::from(length),
                                f64::from(if vertical {
                                    geometry.right()
                                } else {
                                    geometry.bottom()
                                }),
                            ],
                            pixel,
                        ) as f32;
                        let area = if gap { (whole - area).max(0.0) } else { area };
                        mask.data[((y - bounds.top) * bounds.width() + x - bounds.left) as usize] =
                            (area * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                }
                // Rasterize in line-local coordinates, then place at the integer
                // frame origin. Adding a distant origin before rasterization
                // would round away fractional coverage near half-pixel edges.
                mask.rect = schist_core::IntRect::new(
                    mask.rect.left + frame.left,
                    mask.rect.top + frame.top,
                    mask.rect.right + frame.left,
                    mask.rect.bottom + frame.top,
                );
                let mask = warp_coverage(mask, placed, settings, page_geometry);
                let (coats, build) = tinted_coats_for(&mut result.plan, &ink, tint);
                let mode = if overprint {
                    InkMode::Overprint
                } else {
                    InkMode::Knockout
                };
                result.separation.paint(&mask, &coats, mode, 0.7);
                result
                    .separation
                    .paint_composite(&mask, &coats, &build, mode, 0.7);
            }
        }
    }
    result
}

fn rectangle_area(rect: [f64; 4], pixel: [f64; 4]) -> f64 {
    (rect[2].min(pixel[2]) - rect[0].max(pixel[0])).max(0.0)
        * (rect[3].min(pixel[3]) - rect[1].max(pixel[1])).max(0.0)
}

/// Integral of a disk over an axis-aligned rectangle, via four signed quadrants.
/// This antiderivative is independent of the renderer's adaptive quadrature.
fn circle_area(cx: f64, cy: f64, r: f64, p: [f64; 4]) -> f64 {
    if p[0] >= p[2] || p[1] >= p[3] {
        return 0.0;
    }
    let quadrant = |x: f64, y: f64| {
        let sign = x.signum() * y.signum();
        let x = x.abs().min(r);
        let y = y.abs().min(r);
        let cutoff = (r * r - y * y).max(0.0).sqrt();
        let integral = |t: f64| {
            0.5 * (t * (r * r - t * t).max(0.0).sqrt() + r * r * (t / r).clamp(-1.0, 1.0).asin())
        };
        sign * (y * x.min(cutoff)
            + if x > cutoff {
                integral(x) - integral(cutoff)
            } else {
                0.0
            })
    };
    (quadrant(p[2] - cx, p[3] - cy)
        - quadrant(p[0] - cx, p[3] - cy)
        - quadrant(p[2] - cx, p[1] - cy)
        + quadrant(p[0] - cx, p[1] - cy))
    .max(0.0)
}
