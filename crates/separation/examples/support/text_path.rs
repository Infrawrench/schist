//! Integration controls use independently constructed engine baseline specs.
//! They share the glyph renderer, not Design composition or path conversion.
use schist_layout::{
    authoring, text_path, BezierHandles, CharacterStyle, History, Ink, LayoutDocument, Page,
    ParagraphStyle, Point, ShapePath, Story, SubPath,
};
use schist_separation::{
    coverage::{Coverage, InkMode, Separation},
    raster::{tinted_coats_for, warp_coverage},
    separate_page_without_graphics, OutputSettings, PagePixel, SeparatedPage,
};

const TEXT: &str = "Path é";

fn pattern(case: usize) -> schist_text_engine::TextDecorationPattern {
    use schist_text_engine::{DecorationCap, DecorationDashes, TextDecorationPattern as Pattern};
    match case % 6 {
        0 => Pattern::Solid,
        1 => Pattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]),
        2 => Pattern::Dashes(vec![4.5, 2.25].into()),
        3 => Pattern::Dashes(DecorationDashes {
            lengths: vec![4.5, 2.25],
            cap: DecorationCap::Round,
        }),
        4 => Pattern::Dashes(DecorationDashes {
            lengths: vec![4.5, 2.25],
            cap: DecorationCap::Projecting,
        }),
        _ => Pattern::Dots(vec![5.5, 7.25]),
    }
}

fn fitting(case: usize) -> schist_text_engine::DecorationFit {
    if case % 6 < 2 {
        schist_text_engine::DecorationFit::None
    } else {
        schist_text_engine::DecorationFit::DashesAndGaps
    }
}

fn inks() -> [Ink; 2] {
    [
        Ink::spot("Path spot", [45.0, 60.0, 30.0]),
        Ink::cmyk("Path cyan", [1.0, 0.0, 0.0, 0.0]),
    ]
}

fn geometry(case: usize) -> ShapePath {
    let (points, handles) = match case % 4 {
        0 => (
            vec![Point::new(20.0, 60.0), Point::new(180.0, 60.0)],
            vec![],
        ),
        1 => (
            vec![Point::new(70.0, 20.0), Point::new(70.0, 180.0)],
            vec![],
        ),
        2 => (
            vec![Point::new(150.0, 180.0), Point::new(150.0, 20.0)],
            vec![],
        ),
        _ => (
            vec![Point::new(20.0, 100.0), Point::new(180.0, 100.0)],
            vec![
                BezierHandles {
                    outgoing: Some(Point::new(50.0, -20.0)),
                    ..Default::default()
                },
                BezierHandles {
                    incoming: Some(Point::new(150.0, 220.0)),
                    ..Default::default()
                },
            ],
        ),
    };
    ShapePath {
        subpaths: vec![SubPath {
            points,
            handles,
            closed: false,
        }],
        even_odd: false,
    }
}

pub fn document() -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 12]);
    doc.styles.add_character(CharacterStyle {
        name: "Default".into(),
        opacity: Some(0.7),
        ..Default::default()
    });
    for case in 0..12 {
        let name = format!("Path {case}");
        let [fill, stroke] = inks();
        use schist_layout::decorations::{
            DecorationMeasure, DecorationPaint, DecorationStroke, DecorationStyle,
        };
        let line = DecorationStyle {
            paint: Some(DecorationPaint::Ink(stroke.clone())),
            gap_paint: Some(DecorationPaint::Ink(fill.clone())),
            weight: Some(DecorationMeasure::Points(2.5)),
            offset: Some(DecorationMeasure::Points(6.25)),
            stroke: Some(DecorationStroke {
                name: "Proof".into(),
                pattern: pattern(case),
                fitting: fitting(case),
            }),
            tint: Some(0.65),
            gap_tint: Some(0.8),
            overprint: Some(case % 2 == 1),
            gap_overprint: Some(case % 2 == 0),
        };
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(18.0),
            align: Some(
                [
                    schist_layout::styles::Align::Left,
                    schist_layout::styles::Align::Center,
                    schist_layout::styles::Align::Right,
                ][case / 4],
            ),
            fill: Some(fill),
            fill_tint: Some(0.8),
            stroke: Some(stroke),
            stroke_tint: Some(0.65),
            stroke_weight: Some(0.6),
            overprint_fill: Some(case % 2 == 0),
            overprint_stroke: Some(case % 2 == 1),
            underline: Some(true),
            strikethrough: Some(true),
            underline_style: line.clone(),
            strike_style: line,
            ..Default::default()
        });
        let id = authoring::path_shape(
            &mut doc,
            &mut History::default(),
            case,
            geometry(case),
            authoring::Paint::none(),
        )
        .unwrap();
        let frame = text_path::attach(&mut doc, &mut History::default(), id).unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text(TEXT, name);
        text_path::set_bracket(
            &mut doc,
            &mut History::default(),
            &[id],
            text_path::Bracket::Start(9.5),
        );
        text_path::set_bracket(
            &mut doc,
            &mut History::default(),
            &[id],
            text_path::Bracket::End(Some(142.5)),
        );
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
    let mut out = separate_page_without_graphics(doc, page, settings).unwrap();
    out.separation = Separation::new(
        out.plan.plates.len(),
        settings.to_pixels_rect(settings.output_box(&doc.pages[page])),
    );
    let placed = doc.objects.iter().find(|o| o.page == page).unwrap();
    let geometry = geometry(page);
    let contour = &geometry.subpaths[0];
    let mut spec = schist_text_engine::TextSpec {
        text: TEXT.into(),
        direction: schist_text_engine::ParagraphDirection::LeftToRight,
        family: "IBM Plex Sans".into(),
        size: 18.0,
        runs: vec![schist_text_engine::StyleRun {
            start: 0,
            end: TEXT.len(),
            color: Some([1, 0, 0, 255]),
            stroke: Some(schist_text_engine::TextStroke {
                color: Some([2, 0, 0, 255]),
                width: 0.6,
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let line = schist_text_engine::TextDecoration {
        pattern: pattern(page),
        fitting: fitting(page),
        color: Some([2, 0, 0, 255]),
        gap_color: Some([1, 0, 0, 255]),
        weight: Some(2.5),
        offset: Some(6.25),
        ..Default::default()
    };
    spec.runs[0].underline = Some(true);
    spec.runs[0].strikethrough = Some(true);
    spec.runs[0].underline_style = Some(line.clone());
    spec.runs[0].strike_style = Some(line);
    let width = schist_text_engine::measure(&spec).unwrap().width;
    let offset = 9.5
        + match page / 4 {
            0 => 0.0,
            1 => (133.0 - width) / 2.0,
            _ => 133.0 - width,
        };
    let scale = settings.scale();
    spec.size *= scale;
    spec.runs[0].stroke.as_mut().unwrap().width *= scale;
    spec.runs[0].underline_style.as_mut().unwrap().scaled(scale);
    spec.runs[0].strike_style.as_mut().unwrap().scaled(scale);
    spec.path = Some(schist_text_engine::TextPath {
        curve: schist_core::path::SubPath {
            anchors: contour
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let h = contour.handles_at(i);
                    let relative = |p: Point| {
                        (
                            (p.x - placed.bounds.x) * scale,
                            (p.y - placed.bounds.y) * scale,
                        )
                    };
                    let handle = |h: Option<Point>| {
                        let h = h.unwrap_or(*p) - *p;
                        (h.x * scale, h.y * scale)
                    };
                    schist_core::path::Anchor {
                        point: relative(*p),
                        handle_in: handle(h.incoming),
                        handle_out: handle(h.outgoing),
                    }
                })
                .collect(),
            closed: false,
        },
        offset: offset * scale,
        span: Some(133.0 * scale),
    });
    let raster = schist_text_engine::rasterize_with_paints(&spec).unwrap();
    let origin = PagePixel::rect(settings, &doc.pages[page], placed.bounds);
    let bounds = schist_core::IntRect::new(
        raster.bounds.left + origin.left,
        raster.bounds.top + origin.top,
        raster.bounds.right + origin.left,
        raster.bounds.bottom + origin.top,
    );
    for paint in raster.paints {
        let index = usize::from(paint.color.unwrap()[0] - 1);
        let mask = warp_coverage(
            Coverage {
                rect: bounds,
                data: paint.coverage,
            },
            placed,
            settings,
            &doc.pages[page],
        );
        let (coats, build) = tinted_coats_for(&mut out.plan, &inks()[index], [0.8, 0.65][index]);
        let mode = if (page + index).is_multiple_of(2) {
            InkMode::Overprint
        } else {
            InkMode::Knockout
        };
        out.separation.paint(&mask, &coats, mode, 0.7);
        out.separation
            .paint_composite(&mask, &coats, &build, mode, 0.7);
    }
    out
}
