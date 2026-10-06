//! Corner options shape a rectangle from its settings: the item keeps its
//! rectangle and draws, clips and sets its text in the cornered outline.
//! InDesign's PDF of the public paged-media `stroke-inset` sample draws a
//! 200 × 66 pt frame rounded 12 pt as quarter circles whose handles reach
//! 5.373 pt from each corner, and sets its first line of 10/12 text 12 pt in,
//! its middle lines against the edge.
use schist_layout::{
    authoring, compose::compose_story, corners, CornerShape, Corners, History, Ink, LayoutDocument,
    LayoutObject, ObjectPaint, ObjectStyle, Page, Paint, Point, Rect, ShapePath, StoryId,
    StrokeAlignment, SubPath,
};

fn uniform(shape: CornerShape, radius: f32) -> Corners {
    Corners {
        shapes: [shape; 4],
        radii: [radius; 4],
    }
}

fn near(point: Point, x: f32, y: f32) {
    assert!(
        (point.x - x).abs() < 0.01 && (point.y - y).abs() < 0.01,
        "{point:?} vs ({x}, {y})"
    );
}

#[test]
fn a_rounded_corner_is_indesigns_quarter_circle() {
    let path = uniform(CornerShape::Rounded, 12.0).path(Point::ZERO, 200.0, 66.0);
    let sub = &path.subpaths[0];
    assert_eq!(sub.points.len(), 8);
    assert!(sub.closed);
    near(sub.points[0], 0.0, 12.0);
    near(sub.handles_at(0).outgoing.unwrap(), 0.0, 5.373);
    near(sub.handles_at(1).incoming.unwrap(), 5.373, 0.0);
    near(sub.points[1], 12.0, 0.0);
    // The top-right corner comes next, clockwise.
    near(sub.points[2], 188.0, 0.0);
    near(sub.points[3], 200.0, 12.0);
    near(sub.points[7], 0.0, 54.0);
}

#[test]
fn each_corner_shape_cuts_its_own_way() {
    let at = Point::new(10.0, 20.0);
    let bevel = uniform(CornerShape::Bevel, 5.0).path(at, 100.0, 50.0);
    let sub = &bevel.subpaths[0];
    assert_eq!(sub.points.len(), 8);
    assert!(sub.handles.is_empty(), "straight cuts");
    near(sub.points[0], 10.0, 25.0);
    near(sub.points[1], 15.0, 20.0);

    let inset = uniform(CornerShape::Inset, 5.0).path(at, 100.0, 50.0);
    let sub = &inset.subpaths[0];
    assert_eq!(sub.points.len(), 12);
    near(sub.points[0], 10.0, 25.0);
    near(sub.points[1], 15.0, 25.0);
    near(sub.points[2], 15.0, 20.0);

    // Inverse rounded: an arc about the corner, its handles running into
    // the item.
    let inverse = uniform(CornerShape::InverseRounded, 12.0).path(Point::ZERO, 100.0, 50.0);
    let sub = &inverse.subpaths[0];
    near(sub.points[0], 0.0, 12.0);
    near(sub.handles_at(0).outgoing.unwrap(), 6.627, 12.0);
    near(sub.handles_at(1).incoming.unwrap(), 12.0, 6.627);
    near(sub.points[1], 12.0, 0.0);

    // Corners differ one by one; a radius never passes half the short side.
    let mixed = Corners {
        shapes: [
            CornerShape::Rounded,
            CornerShape::None,
            CornerShape::Bevel,
            CornerShape::Fancy,
        ],
        radii: [80.0, 10.0, 10.0, 10.0],
    };
    let sub = &mixed.path(Point::ZERO, 100.0, 50.0).subpaths[0];
    near(sub.points[0], 0.0, 25.0);
    near(sub.points[1], 25.0, 0.0);
    near(sub.points[2], 100.0, 0.0);
    near(sub.points[3], 100.0, 40.0);
    near(sub.points[4], 90.0, 50.0);
    // Decorative corners are drawn square.
    near(sub.points[5], 0.0, 50.0);
    assert!(!mixed.square() && mixed.fancy());
    assert!(uniform(CornerShape::Fancy, 10.0).square());
    assert!(uniform(CornerShape::Rounded, 0.0).square());
}

#[test]
fn only_upright_rectangles_take_corners() {
    let square = |points: &[(f32, f32)]| ShapePath {
        subpaths: vec![SubPath {
            points: points.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
            closed: true,
            handles: Vec::new(),
        }],
        even_odd: false,
    };
    // Any start, either direction.
    let rect = corners::rectangle(&square(&[
        (5.0, 60.0),
        (5.0, 10.0),
        (45.0, 10.0),
        (45.0, 60.0),
    ]));
    assert_eq!(rect, Some(Rect::new(5.0, 10.0, 40.0, 50.0)));
    assert_eq!(
        corners::rectangle(&square(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (12.0, 10.0),
            (0.0, 10.0)
        ])),
        None
    );
    assert_eq!(
        corners::rectangle(&square(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)])),
        None
    );
    assert_eq!(corners::rectangle(&ShapePath::ellipse(10.0, 10.0)), None);
}

fn document() -> LayoutDocument {
    LayoutDocument::new(vec![Page::new("1", 400.0, 400.0)])
}

#[test]
fn items_draw_their_corners_and_keep_their_rectangle() {
    let mut doc = document();
    let mut history = History::default();
    let shape = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(20.0, 20.0, 100.0, 60.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(20.0, 120.0, 100.0, 60.0),
    )
    .unwrap()
    .object;
    let graphic = authoring::graphic_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(20.0, 220.0, 100.0, 50.0),
        "missing.png",
        false,
    )
    .unwrap();
    for id in [shape, frame, graphic] {
        let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.appearance.paint.corners = Some(uniform(CornerShape::Rounded, 10.0));
        object.appearance.paint.fill = Some(Paint::Ink(Ink::black()));
    }
    let resolved = doc.page_objects(0);
    let find = |id| resolved.iter().find(|o| o.id == id).unwrap();

    let LayoutObject::Shape { path, .. } = &find(shape).object else {
        panic!("a shape");
    };
    assert_eq!(path.subpaths[0].points.len(), 8);
    near(path.subpaths[0].points[0], 0.0, 10.0);
    // The document keeps the rectangle.
    let LayoutObject::Shape { path, .. } = &doc.object(shape).unwrap().object else {
        panic!("a shape");
    };
    assert_eq!(path.subpaths[0].points.len(), 4);

    // A text frame's outline is normalized to it; its fill is drawn there.
    let text = find(frame);
    let outline = text.appearance.outline.as_ref().expect("an outline");
    near(outline.subpaths[0].points[0], 0.0, 10.0 / 60.0);
    let fill = text.frame_paint(false).expect("a fill");
    let LayoutObject::Shape { path, .. } = &fill.object else {
        panic!("a shape");
    };
    near(path.subpaths[0].points[1], 10.0, 0.0);
    assert!(doc.object(frame).unwrap().appearance.outline.is_none());

    // A graphic frame clips its image to its corners.
    let LayoutObject::GraphicFrame { clip_path, .. } = &find(graphic).object else {
        panic!("a graphic frame");
    };
    near(clip_path.as_ref().unwrap().subpaths[0].points[1], 0.1, 0.0);
}

#[test]
fn a_styles_corners_belong_to_its_stroke_and_corner_category() {
    let mut doc = document();
    let shape = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 100.0, 60.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    for (enabled, points) in [(Some(true), 12), (None, 4)] {
        doc.styles.objects = vec![ObjectStyle {
            name: "Notched".into(),
            enable_stroke_options: enabled,
            paint: ObjectPaint {
                corners: Some(uniform(CornerShape::Inset, 8.0)),
                ..Default::default()
            },
            ..Default::default()
        }];
        let object = doc.objects.iter_mut().find(|o| o.id == shape).unwrap();
        object.appearance.style = Some("Notched".into());
        let resolved = doc.page_objects(0);
        let LayoutObject::Shape { path, .. } = &resolved[0].object else {
            panic!("a shape");
        };
        assert_eq!(path.subpaths[0].points.len(), points, "{enabled:?}");
    }
}

/// A 200 × 66 pt frame of 10/12 text rounded 12 pt, with its stroke.
fn rounded_frame(stroke: Option<f32>, radius: f32) -> (LayoutDocument, StoryId) {
    let mut doc = schist_layout::blank_a4();
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.family = Some("IBM Plex Sans".into());
    body.point_size = Some(10.0);
    body.leading = Some(schist_layout::styles::Leading::Points(12.0));
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(72.0, 60.0, 200.0, 66.0),
    )
    .unwrap();
    let mut story = schist_layout::Story::new();
    for line in ["One left", "Two", "Three left", "Four", "Five left"] {
        story.push_paragraph(line, "Body");
    }
    doc.stories[frame.story.0 as usize] = story;
    let object = doc
        .objects
        .iter_mut()
        .find(|o| o.id == frame.object)
        .unwrap();
    object.appearance.paint.corners = Some(uniform(CornerShape::Rounded, radius));
    if let Some(width) = stroke {
        object.appearance.paint.stroke = Some(Paint::Ink(Ink::black()));
        object.appearance.paint.stroke_width = Some(width);
        object.appearance.paint.stroke_alignment = Some(StrokeAlignment::Center);
    }
    (doc, frame.story)
}

/// Each line's left edge and baseline, relative to the frame.
fn lines(doc: &LayoutDocument, story: StoryId) -> Vec<(f32, f32)> {
    compose_story(doc, story).frames[0]
        .lines
        .iter()
        .map(|line| (line.bounds.x - 72.0, line.baseline - 60.0))
        .collect()
}

#[test]
fn text_keeps_inside_rounded_corners() {
    let square = lines(&rounded_frame(None, 0.0).0, rounded_frame(None, 0.0).1);
    let base = square[0].1;
    let (doc, story) = rounded_frame(None, 12.0);
    let rounded = lines(&doc, story);
    assert!((rounded[0].0 - 12.0).abs() < 0.01, "{rounded:?}");
    assert!((rounded[0].1 - base).abs() < 0.01, "{rounded:?}");
    assert!(rounded[2].0.abs() < 0.01, "{rounded:?}");
    // A centred 6 pt stroke moves the text half its weight more.
    let (doc, story) = rounded_frame(Some(6.0), 12.0);
    let stroked = lines(&doc, story);
    assert!((stroked[0].1 - base - 3.0).abs() < 0.01, "{stroked:?}");
    assert!((stroked[2].0 - 3.0).abs() < 0.01, "{stroked:?}");
}

#[test]
fn text_wraps_a_rounded_image_frame_by_its_corners() {
    let mut doc = schist_layout::blank_a4();
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(40.0, 40.0, 400.0, 560.0),
    )
    .unwrap();
    let mut story = schist_layout::Story::new();
    for _ in 0..6 {
        story.push_paragraph(
            "Wrapped type keeps its reading order while a picture pushes the measure aside, \
             and every line that meets the obstacle finds the room that remains beside it.",
            "Body",
        );
    }
    doc.stories[frame.story.0 as usize] = story;
    // A 120 pt square rounded 60 pt at every corner is a circle.
    let area = Rect::new(180.0, 140.0, 120.0, 120.0);
    let graphic =
        authoring::graphic_frame(&mut doc, &mut history, 0, area, "missing.png", false).unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == graphic).unwrap();
    object.appearance.paint.corners = Some(uniform(CornerShape::Rounded, 60.0));
    object.appearance.text_wrap = Some(schist_layout::text_wrap::TextWrap {
        mode: schist_layout::text_wrap::WrapMode::Contour,
        ..Default::default()
    });
    let thread = compose_story(&doc, frame.story);
    let lines: Vec<_> = thread.lines().collect();
    let centre = Point::new(240.0, 200.0);
    let mut inside_box = false;
    for line in &lines {
        let (top, bottom) = (line.bounds.y, line.bounds.bottom());
        let (left, right) = (line.bounds.x, line.bounds.right());
        if bottom <= area.y || top >= area.bottom() || right <= area.x || left >= area.right() {
            continue;
        }
        inside_box = true;
        // The circle's half-chord across the line, flattening inscribing it
        // within 0.3 pt.
        let d = if (top..bottom).contains(&centre.y) {
            0.0
        } else {
            (centre.y - top).abs().min((centre.y - bottom).abs())
        };
        let radius: f32 = 60.0 - 0.3;
        if d < radius {
            let half = (radius * radius - d * d).sqrt();
            assert!(
                right <= centre.x - half + 0.01 || left >= centre.x + half - 0.01,
                "{:?} enters the circle",
                line.bounds
            );
        }
    }
    assert!(inside_box, "the wrap follows the corners, not the box");
}
