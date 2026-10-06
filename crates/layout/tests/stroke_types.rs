//! Dashed, dotted and striped item strokes are laid along the item's own
//! path, which stays as it is. The stroke styles are the public IDML
//! specification's (tables 130 to 132: dash and gap lengths in points, dot
//! spacing centre to centre with dots as wide as the stroke, stripes as
//! percentages of the weight), and its page item table gives the built-in
//! Dashed style the item's own StrokeDashAndGap and StrokeCornerAdjustment.
//! No public InDesign PDF draws a patterned item stroke: InDesign's PDF of
//! the public paged-media `strokes-fills` sample draws the four types its
//! package does not declare solid, so the geometry here is the
//! specification's reading.
use schist_core::IntRect;
use schist_layout::{
    authoring, decorations::DecorationStroke, pasteboard::StrokeOptions, stroke_patterns, Display,
    History, Ink, LayoutDocument, ObjectPaint, ObjectStyle, Page, Paint, PasteboardView, Point,
    Rect, ShapePath, StrokeAlignment, StrokeCap, StrokeType, SubPath,
};
use schist_text_engine::{DecorationCap, DecorationDashes, DecorationFit, TextDecorationPattern};

/// A 200 × 100 rectangle from the origin, running clockwise on the page.
fn rectangle() -> ShapePath {
    ShapePath {
        subpaths: vec![SubPath {
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(200.0, 0.0),
                Point::new(200.0, 100.0),
                Point::new(0.0, 100.0),
            ],
            closed: true,
            handles: Vec::new(),
        }],
        even_odd: false,
    }
}

fn line(length: f32) -> ShapePath {
    ShapePath {
        subpaths: vec![SubPath {
            points: vec![Point::new(0.0, 0.0), Point::new(length, 0.0)],
            closed: false,
            handles: Vec::new(),
        }],
        even_odd: false,
    }
}

fn dashes(lengths: &[f32], cap: DecorationCap) -> TextDecorationPattern {
    TextDecorationPattern::Dashes(DecorationDashes {
        lengths: lengths.to_vec(),
        cap,
    })
}

fn options(pattern: TextDecorationPattern, fitting: DecorationFit) -> StrokeOptions {
    StrokeOptions {
        pattern: Some(DecorationStroke {
            name: "Test".into(),
            fitting,
            pattern,
        }),
        ..Default::default()
    }
}

/// How much of the unit pixel at (x, y) a nonzero outline covers.
fn at(outline: &schist_vector::Path, x: i32, y: i32) -> f32 {
    let rect = IntRect::from_xywh(x, y, 1, 1);
    f32::from(schist_vector::rasterize(outline, rect, schist_vector::FillRule::NonZero)[0]) / 255.0
}

#[test]
fn dashes_start_at_the_first_point_and_run_on_round_corners() {
    let drawn = stroke_patterns::outlines(
        &rectangle(),
        6.0,
        &options(
            dashes(&[12.0, 6.0], DecorationCap::Butt),
            DecorationFit::None,
        ),
        0.1,
    )
    .unwrap();
    let ink = &drawn.ink;
    assert!(at(ink, 5, 0) > 0.99, "the first dash, 0 to 12");
    assert!(at(ink, 14, 0) < 0.01, "its gap, 12 to 18");
    assert!(at(ink, 20, 0) > 0.99);
    // 200 pt in, the twelfth dash (198 to 210) turns the corner and runs 10
    // pt down the right side, mitred round its outside.
    assert!(at(ink, 200, 5) > 0.99);
    assert!(at(ink, 201, -2) > 0.99, "a mitred corner");
    assert!(at(ink, 200, 13) < 0.01);
    // The band a gap colour fills is the solid stroke.
    assert!(at(&drawn.band, 200, 13) > 0.99 && at(&drawn.band, 14, 0) > 0.99);
    assert!(at(&drawn.band, 14, 4) < 0.01);
}

#[test]
fn corner_adjustment_fits_each_side_from_corner_to_corner() {
    let pattern = || dashes(&[12.0, 6.0], DecorationCap::Butt);
    let free = stroke_patterns::outlines(
        &rectangle(),
        6.0,
        &options(pattern(), DecorationFit::None),
        0.1,
    )
    .unwrap();
    let fitted = stroke_patterns::outlines(
        &rectangle(),
        6.0,
        &options(pattern(), DecorationFit::DashesAndGaps),
        0.1,
    )
    .unwrap();
    // Unfitted, 185 pt along the top falls 5 pt into a dash.
    assert!(at(&free.ink, 185, 0) > 0.99);
    // Fitted, the 200 pt top holds ten cycles and a dash, each length
    // stretched by 200/192: dashes of 12.5, gaps of 6.25, so 185 falls in
    // the gap from 181.25 to 187.5 and the last dash reaches the corner.
    assert!(at(&fitted.ink, 185, 0) < 0.01);
    assert!(at(&fitted.ink, 193, 0) > 0.99);
    assert!(at(&fitted.ink, 0, 0) > 0.99 && at(&fitted.ink, 199, 0) > 0.99);
    // That dash joins the first of the 100 pt side round the corner: five
    // cycles and a dash at 100/102, a dash 11.76 long, then a gap to 17.65.
    assert!(at(&fitted.ink, 201, -2) > 0.99, "joined, so mitred");
    assert!(at(&fitted.ink, 200, 9) > 0.99);
    assert!(at(&fitted.ink, 200, 14) < 0.01);
    // And every corner has its dash.
    for (x, y) in [(199, 99), (0, 99), (0, 1)] {
        assert!(at(&fitted.ink, x, y) > 0.99, "({x}, {y})");
    }
}

#[test]
fn a_closed_contour_without_corners_fits_whole_cycles() {
    // A circle 50 pt in radius is 314.16 pt round. Dashes of 9 and gaps of 3
    // leave a 2.16 pt dash just before the start; fitted, 26 whole cycles
    // end with a gap there.
    let circle = ShapePath::ellipse(100.0, 100.0);
    let pattern = || dashes(&[9.0, 3.0], DecorationCap::Butt);
    let free =
        stroke_patterns::outlines(&circle, 6.0, &options(pattern(), DecorationFit::None), 0.01)
            .unwrap();
    let fitted = stroke_patterns::outlines(
        &circle,
        6.0,
        &options(pattern(), DecorationFit::DashesAndGaps),
        0.01,
    )
    .unwrap();
    // The circle starts at (100, 50) and runs down; (99, 48) is 1 to 2 pt
    // before its end.
    assert!(at(&free.ink, 99, 48) > 0.5);
    assert!(at(&fitted.ink, 99, 48) < 0.01);
    assert!(at(&free.ink, 99, 51) > 0.99 && at(&fitted.ink, 99, 51) > 0.99);
}

#[test]
fn dots_are_as_wide_as_the_stroke_and_spaced_centre_to_centre() {
    let drawn = stroke_patterns::outlines(
        &line(100.0),
        4.0,
        &options(TextDecorationPattern::Dots(vec![10.0]), DecorationFit::None),
        0.1,
    )
    .unwrap();
    // Dots of radius 2 at 0, 10, 20 and on.
    assert!(at(&drawn.ink, 9, -1) > 0.95 && at(&drawn.ink, 10, 0) > 0.95);
    assert!(at(&drawn.ink, 0, -1) > 0.95);
    assert!(at(&drawn.ink, 14, -1) < 0.01);
    assert!(at(&drawn.ink, 10, -3) < 0.01, "no wider than the stroke");
    // Fitted, a dot sits at each end.
    let fitted = stroke_patterns::outlines(
        &line(95.0),
        4.0,
        &options(TextDecorationPattern::Dots(vec![10.0]), DecorationFit::Gaps),
        0.1,
    )
    .unwrap();
    assert!(at(&fitted.ink, 94, -1) > 0.95);
}

#[test]
fn round_and_projecting_dashes_extend_past_their_ends() {
    let drawn = |cap| {
        stroke_patterns::outlines(
            &line(100.0),
            4.0,
            &options(dashes(&[10.0, 10.0], cap), DecorationFit::None),
            0.1,
        )
        .unwrap()
        .ink
    };
    // The first dash runs 0 to 10; its caps reach 2 pt further.
    assert!(at(&drawn(DecorationCap::Butt), 10, -1) < 0.01);
    assert!(at(&drawn(DecorationCap::Projecting), 10, -2) > 0.99);
    assert!(at(&drawn(DecorationCap::Round), 10, -1) > 0.9);
    assert!(at(&drawn(DecorationCap::Round), 11, -2) < 0.3);
}

#[test]
fn stripes_run_across_the_stroke_from_its_left_edge() {
    // An 8 pt stroke left to right: 0 % is its top edge.
    let pattern = || TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]);
    let drawn = stroke_patterns::outlines(
        &line(100.0),
        8.0,
        &options(pattern(), DecorationFit::None),
        0.1,
    )
    .unwrap();
    assert!(at(&drawn.ink, 50, -4) > 0.99, "0 to 25 %: 4 to 2 pt above");
    assert!(at(&drawn.ink, 50, -2) < 0.01 && at(&drawn.ink, 50, 1) < 0.01);
    assert!(at(&drawn.ink, 50, 3) > 0.99, "75 to 100 %: 2 to 4 pt below");
    assert!(at(&drawn.band, 50, -1) > 0.99);
    // Clockwise round a rectangle, the left edge is outside: the 0-25 %
    // stripe is the outer one, mitred at the corners.
    let framed = stroke_patterns::outlines(
        &rectangle(),
        8.0,
        &options(pattern(), DecorationFit::None),
        0.1,
    )
    .unwrap();
    assert!(at(&framed.ink, 100, -4) > 0.99 && at(&framed.ink, 100, 3) > 0.99);
    assert!(at(&framed.ink, 100, -1) < 0.01);
    assert!(at(&framed.ink, -4, -4) > 0.99, "the outer stripe's mitre");
    assert!(
        at(&framed.ink, -1, -1) < 0.01,
        "the gap turns the corner too"
    );
    assert!(at(&framed.ink, 3, 3) > 0.99, "the inner stripe's mitre");
    assert!(at(&framed.ink, 5, 1) < 0.01, "and nothing between");
}

#[test]
fn an_inside_pattern_runs_along_the_path_moved_half_its_weight_in() {
    let drawn = stroke_patterns::outlines(
        &rectangle(),
        6.0,
        &StrokeOptions {
            alignment: StrokeAlignment::Inside,
            ..options(
                dashes(&[12.0, 6.0], DecorationCap::Butt),
                DecorationFit::None,
            )
        },
        0.1,
    )
    .unwrap();
    assert!(at(&drawn.ink, 6, 1) > 0.99 && at(&drawn.ink, 6, 5) > 0.99);
    assert!(at(&drawn.ink, 6, -1) < 0.01, "nothing outside the path");
    assert!(at(&drawn.band, 100, -1) < 0.01 && at(&drawn.band, 100, 5) > 0.99);
}

#[test]
fn the_built_in_dashed_type_dashes_with_the_items_own_lengths_and_cap() {
    let paint = ObjectPaint {
        stroke_type: Some(StrokeType::Dashed),
        dash_and_gap: Some(vec![12.0, 4.0]),
        corner_adjustment: Some(DecorationFit::Gaps),
        stroke_cap: Some(StrokeCap::Round),
        ..Default::default()
    };
    let pattern = paint.stroke_pattern().unwrap();
    assert_eq!(pattern.fitting, DecorationFit::Gaps);
    assert_eq!(pattern.pattern, dashes(&[12.0, 4.0], DecorationCap::Round));
    // Without dashes, or a built-in style the specification gives no look
    // for, it strokes solid.
    let solid = ObjectPaint {
        dash_and_gap: None,
        ..paint.clone()
    };
    assert_eq!(solid.stroke_pattern(), None);
    let dots = ObjectPaint {
        stroke_type: Some(StrokeType::Builtin("$ID/Japanese Dots".into())),
        ..paint
    };
    assert_eq!(dots.stroke_pattern(), None);
}

fn styled_document(enable: Option<bool>) -> (LayoutDocument, schist_layout::ObjectId) {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    doc.styles.objects.push(ObjectStyle {
        name: "Dashed".into(),
        enable_stroke: Some(true),
        enable_stroke_options: enable,
        paint: ObjectPaint {
            stroke: Some(Paint::Ink(Ink::black())),
            stroke_width: Some(2.0),
            stroke_type: Some(StrokeType::Style(DecorationStroke {
                name: "Long".into(),
                fitting: DecorationFit::None,
                pattern: dashes(&[8.0, 4.0], DecorationCap::Butt),
            })),
            gap: Some(Paint::Ink(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]))),
            gap_tint: Some(0.5),
            ..Default::default()
        },
        ..Default::default()
    });
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.style = Some("Dashed".into());
    object.object = ObjectPaint::default().shape(match &object.object {
        schist_layout::LayoutObject::Shape { path, .. } => path.clone(),
        _ => unreachable!(),
    });
    (doc, id)
}

#[test]
fn an_object_style_strokes_with_its_type_and_gap_when_its_options_are_enabled() {
    let (doc, id) = styled_document(Some(true));
    let object = doc.object(id).unwrap();
    let paint = doc.styles.object_paint(object);
    assert!(paint.stroke_pattern().is_some());
    assert_eq!(paint.gap_tint, Some(0.5));
    assert_eq!(paint.gap_ink().map(|i| i.name.as_str()), Some("Cyan"));
    // The category off, the style's type and gap do not apply.
    let (doc, id) = styled_document(None);
    let paint = doc.styles.object_paint(doc.object(id).unwrap());
    assert_eq!(paint.stroke_pattern(), None);
    assert_eq!(paint.gap, None);
    // A local type overrides the style's; a local tint detaches a named
    // tint's base as other paints do.
    let (mut doc, id) = styled_document(Some(true));
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.set_local_paint(&ObjectPaint {
        stroke_type: Some(StrokeType::Style(DecorationStroke::solid())),
        ..Default::default()
    });
    let paint = doc.styles.object_paint(doc.object(id).unwrap());
    assert_eq!(paint.stroke_pattern(), None);
    assert!(paint.gap_ink().is_some(), "the gap is kept");
}

#[test]
fn the_canvas_gets_the_pattern_at_its_zoom_and_the_gap_colour() {
    let (doc, id) = styled_document(Some(true));
    let plan = schist_layout::pasteboard(
        &doc,
        &PasteboardView {
            scale: 2.0,
            ..Default::default()
        },
    )
    .unwrap();
    let shape = plan
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .find_map(|d| match d {
            Display::Shape {
                object,
                stroke_options,
                gap,
                ..
            } if *object == id => Some((stroke_options.clone(), *gap)),
            _ => None,
        })
        .unwrap();
    let pattern = shape.0.pattern.unwrap();
    assert_eq!(pattern.pattern, dashes(&[16.0, 8.0], DecorationCap::Butt));
    let gap = shape.1.expect("a gap colour");
    // Cyan at half tint.
    assert!(gap[0] > 0.4 && gap[0] < 0.6 && gap[1] > 0.9 && gap[2] > 0.9);
}
