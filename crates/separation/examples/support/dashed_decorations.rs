//! Reference masks enumerate independent solid rectangles, never dash masks.
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
                name: "Dash".into(),
                pattern: TextDecorationPattern::Dashes(
                    (if case % 2 == 0 {
                        vec![6.0, 3.0]
                    } else {
                        vec![3.5, 1.5, 1.0, 2.0]
                    })
                    .into(),
                ),
            }),
            paint: Some(Paint::Ink(Ink::spot("Dash spot", [45.0, 60.0, 30.0]))),
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
            let bounds = schist_core::IntRect::new(
                geometry.x.floor() as i32,
                geometry.y.floor() as i32,
                geometry.right().ceil() as i32,
                geometry.bottom().ceil() as i32,
            );
            let TextDecorationPattern::Dashes(dashes) = &style.stroke.as_ref().unwrap().pattern
            else {
                unreachable!()
            };
            let mut dash_rectangles = Vec::new();
            let mut gap_rectangles = Vec::new();
            let mut cursor = 0.0;
            while cursor < length {
                for (index, span) in dashes.lengths.iter().enumerate() {
                    let end = (cursor + span * settings.scale()).min(length);
                    if end > cursor {
                        let rect = if vertical {
                            Rect::new(geometry.x, geometry.y + cursor, weight, end - cursor)
                        } else {
                            Rect::new(geometry.x + cursor, geometry.y, end - cursor, weight)
                        };
                        if index % 2 == 0 {
                            dash_rectangles.push(rect)
                        } else {
                            gap_rectangles.push(rect)
                        }
                    }
                    cursor += span * settings.scale();
                }
            }
            for (rectangles, paint) in [
                (gap_rectangles, style.gap_paint(&character)),
                (dash_rectangles, style.paint(&character)),
            ] {
                let (ink, tint, overprint) = paint.unwrap();
                let mut mask = Coverage::new(bounds);
                for y in bounds.top..bounds.bottom {
                    for x in bounds.left..bounds.right {
                        let area = rectangles
                            .iter()
                            .map(|r| {
                                let dx = ((x as f32 + 1.0).min(r.right()) - (x as f32).max(r.x))
                                    .max(0.0);
                                let dy = ((y as f32 + 1.0).min(r.bottom()) - (y as f32).max(r.y))
                                    .max(0.0);
                                dx * dy
                            })
                            .sum::<f32>();
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
