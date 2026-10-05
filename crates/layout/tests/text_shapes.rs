use schist_layout::text_wrap::{TextWrap, WrapMode};
use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, compose::compose_story, text_shape, threading,
    ComposedLine, History, Insets, LayoutDocument, LayoutObject, ObjectId, ParagraphStyle, Point,
    Rect, ShapePath, Story, StoryId, WritingMode,
};

const TEXT: &str = "Shaped frames hold their words inside the outline, line by line, \
so a circle of type reads as a circle and every line keeps to the room the curve allows.";
const FRAME: Rect = Rect::new(100.0, 100.0, 300.0, 300.0);
const CENTER: Point = Point::new(250.0, 250.0);

fn shaped(outline: Option<ShapePath>, paragraphs: usize) -> (LayoutDocument, ObjectId) {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 0, FRAME).unwrap();
    let mut story = Story::new();
    for _ in 0..paragraphs {
        story.push_paragraph(TEXT, "Body");
    }
    doc.stories[frame.story.0 as usize] = story;
    object_mut(&mut doc, frame.object).appearance.outline = outline;
    (doc, frame.object)
}

fn object_mut(doc: &mut LayoutDocument, id: ObjectId) -> &mut schist_layout::PlacedObject {
    doc.objects.iter_mut().find(|o| o.id == id).unwrap()
}

fn set_inset(doc: &mut LayoutDocument, id: ObjectId, inset: f32) {
    let LayoutObject::TextFrame { insets, .. } = &mut object_mut(doc, id).object else {
        panic!()
    };
    *insets = Insets::uniform(inset);
}

fn lines(doc: &LayoutDocument) -> Vec<ComposedLine> {
    compose_story(doc, StoryId(0)).lines().cloned().collect()
}

/// The narrowest half-chord of a circle around `CENTER` over a band.
fn inner_chord(radius: f32, top: f32, bottom: f32) -> Option<f32> {
    let d = (CENTER.y - top).abs().max((CENTER.y - bottom).abs());
    (d < radius).then(|| (radius * radius - d * d).sqrt())
}

/// The widest half-chord of a circle around `CENTER` over a band.
fn outer_chord(radius: f32, top: f32, bottom: f32) -> Option<f32> {
    let d = if (top..=bottom).contains(&CENTER.y) {
        0.0
    } else {
        (CENTER.y - top).abs().min((CENTER.y - bottom).abs())
    };
    (d < radius).then(|| (radius * radius - d * d).sqrt())
}

/// A four-arc Bézier circle bulges up to 0.03% beyond its radius.
fn assert_inside_circle(lines: &[ComposedLine], radius: f32) {
    for line in lines {
        let half = inner_chord(radius + 0.1, line.bounds.y, line.bounds.bottom())
            .unwrap_or_else(|| panic!("{:?} outside the circle's height", line.bounds));
        assert!(line.bounds.x >= CENTER.x - half - 0.01, "{:?}", line.bounds);
        assert!(
            line.bounds.right() <= CENTER.x + half + 0.01,
            "{:?}",
            line.bounds
        );
    }
}

#[test]
fn text_in_an_ellipse_stays_inside_its_outline_and_inset() {
    for inset in [0.0, 8.0, 20.0] {
        let (mut doc, frame) = shaped(Some(ShapePath::ellipse(1.0, 1.0)), 8);
        set_inset(&mut doc, frame, inset);
        let flow = compose_story(&doc, StoryId(0));
        let placed: Vec<_> = flow.lines().cloned().collect();
        assert!(placed.len() > 8, "inset {inset}");
        assert_inside_circle(&placed, 150.0 - inset);
        // The middle of the circle is wide; its top and bottom are narrow.
        assert!(placed.iter().any(|l| l.bounds.width > 220.0 - 2.0 * inset));
        assert!(placed.iter().any(|l| l.bounds.width < 200.0));
        assert!(flow.has_overflow());
        let mut end = 0;
        for line in &placed {
            assert!(line.start >= end);
            end = line.end;
        }
    }
}

#[test]
fn rectangular_outlines_and_rotation_do_not_change_composition() {
    let (plain, _) = shaped(None, 4);
    let (boxed, _) = shaped(Some(authoring::path_for(ShapeKind::Rectangle, 1.0, 1.0)), 4);
    assert_eq!(lines(&boxed), lines(&plain));
    // Composition happens in the frame's own box; affines only place it.
    let (doc, frame) = shaped(Some(ShapePath::ellipse(1.0, 1.0)), 4);
    let expected = lines(&doc);
    for rotation in [15.0, -90.0] {
        let mut rotated = doc.clone();
        object_mut(&mut rotated, frame).rotation = rotation;
        assert_eq!(lines(&rotated), expected);
    }
}

/// A 300pt circle with a 120pt circle in its middle.
fn ring(even_odd: bool) -> ShapePath {
    let mut outer = ShapePath::ellipse(1.0, 1.0);
    let mut inner = ShapePath::ellipse(0.4, 0.4);
    inner.map_points(|p| Point::new(p.x + 0.3, p.y + 0.3));
    outer.subpaths.extend(inner.subpaths);
    outer.even_odd = even_odd;
    outer
}

#[test]
fn compound_outlines_follow_their_fill_rule() {
    let hole = |line: &ComposedLine| outer_chord(60.0, line.bounds.y, line.bounds.bottom());
    let (doc, _) = shaped(Some(ring(true)), 8);
    let placed = lines(&doc);
    assert_inside_circle(&placed, 150.0);
    for line in &placed {
        if let Some(half) = hole(line) {
            // Flattening inscribes the hole within 0.25pt.
            assert!(
                line.bounds.right() <= CENTER.x - half + 0.5
                    || line.bounds.x >= CENTER.x + half - 0.5,
                "{:?} crosses the hole",
                line.bounds
            );
        }
    }
    assert!(placed
        .iter()
        .any(|l| hole(l).is_some() && l.bounds.right() < CENTER.x));
    assert!(placed
        .iter()
        .any(|l| hole(l).is_some() && l.bounds.x > CENTER.x));
    // Same-direction rings under the nonzero rule fill the middle.
    let (doc, _) = shaped(Some(ring(false)), 8);
    assert!(lines(&doc).iter().any(|l| hole(l)
        .is_some_and(|half| l.bounds.x < CENTER.x - half && l.bounds.right() > CENTER.x + half)));
}

#[test]
fn shaped_frames_wrap_around_other_items_and_keep_their_shape_when_ignoring_wrap() {
    let (doc, frame) = shaped(Some(ShapePath::ellipse(1.0, 1.0)), 8);
    let shape_only = lines(&doc);
    let mut wrapped = doc.clone();
    let rect = Rect::new(220.0, 200.0, 60.0, 100.0);
    let id = authoring::rectangle(
        &mut wrapped,
        &mut History::default(),
        0,
        rect,
        authoring::Paint::none(),
    )
    .unwrap();
    object_mut(&mut wrapped, id).appearance.text_wrap = Some(TextWrap {
        mode: WrapMode::BoundingBox,
        ..Default::default()
    });
    let placed = lines(&wrapped);
    assert_inside_circle(&placed, 150.0);
    for line in &placed {
        assert!(
            !(line.bounds.y < rect.bottom() - 0.01
                && line.bounds.bottom() > rect.y + 0.01
                && line.bounds.x < rect.right() - 0.01
                && line.bounds.right() > rect.x + 0.01),
            "{:?} enters the wrap",
            line.bounds
        );
    }
    assert_ne!(placed, shape_only);
    object_mut(&mut wrapped, frame).appearance.ignore_wrap = true;
    assert_eq!(lines(&wrapped), shape_only);
}

#[test]
fn shaped_text_threads_on_without_losing_text() {
    let (mut doc, first) = shaped(Some(ShapePath::ellipse(1.0, 1.0)), 6);
    let second = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 450.0, 500.0, 360.0),
    )
    .unwrap();
    assert!(threading::link(
        &mut doc,
        &mut History::default(),
        first,
        second.object
    ));
    let flow = compose_story(&doc, StoryId(0));
    assert!(!flow.has_overflow());
    let total = doc.stories[0].text().len();
    assert_eq!(flow.frames.last().unwrap().consumed_to, total);
    let first_lines = flow.frames[0].lines.clone();
    assert!(!first_lines.is_empty());
    assert_inside_circle(&first_lines, 150.0);
    assert!(!flow.frames[1].lines.is_empty());
}

#[test]
fn vertical_text_in_a_shaped_frame_is_reported() {
    let (mut doc, _) = shaped(Some(ShapePath::ellipse(1.0, 1.0)), 0);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Vertical".into(),
        writing_mode: Some(WritingMode::VerticalRightToLeft),
        ..Default::default()
    });
    doc.stories[0] = Story::from_text(TEXT, "Vertical");
    assert!(compose_story(&doc, StoryId(0)).frames[0].wrap.ignored);
    doc.stories[0] = Story::from_text(TEXT, "Body");
    assert!(!compose_story(&doc, StoryId(0)).frames[0].wrap.ignored);
}

#[test]
fn attaching_text_to_a_closed_shape_is_one_undo_step() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let ellipse = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(50.0, 60.0, 200.0, 100.0),
        ShapeKind::Ellipse,
        authoring::Paint::none(),
    )
    .unwrap();
    let square = authoring::rectangle(
        &mut doc,
        &mut history,
        0,
        Rect::new(300.0, 60.0, 80.0, 80.0),
        authoring::Paint::none(),
    )
    .unwrap();
    let line = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(50.0, 300.0, 200.0, 40.0),
        ShapeKind::Line,
        authoring::Paint::none(),
    )
    .unwrap();
    let paint = |doc: &LayoutDocument, id| doc.styles.object_paint(doc.object(id).unwrap());
    let ellipse_paint = paint(&doc, ellipse);
    let mut history = History::default();
    let before = doc.clone();
    let frame = text_shape::attach(&mut doc, &mut history, ellipse).unwrap();
    assert_eq!(frame.object, ellipse);
    assert_eq!(history.undo_depth(), 1);
    let object = doc.object(ellipse).unwrap();
    assert!(matches!(
        object.object,
        LayoutObject::TextFrame {
            text_path: None,
            ..
        }
    ));
    assert_eq!(object.bounds, before.object(ellipse).unwrap().bounds);
    let LayoutObject::Shape { path, .. } = &before.object(ellipse).unwrap().object else {
        panic!()
    };
    let mut expected = path.clone();
    expected.map_points(|p| Point::new(p.x / 200.0, p.y / 100.0));
    assert_eq!(object.appearance.outline, Some(expected));
    assert_eq!(paint(&doc, ellipse), ellipse_paint);
    assert_eq!(doc.stories.len(), before.stories.len() + 1);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);

    // A rectangle becomes an ordinary frame; open paths and frames refuse.
    text_shape::attach(&mut doc, &mut history, square).unwrap();
    assert_eq!(doc.object(square).unwrap().appearance.outline, None);
    let unchanged = doc.clone();
    assert!(text_shape::attach(&mut doc, &mut history, line).is_none());
    assert!(text_shape::attach(&mut doc, &mut history, square).is_none());
    object_mut(&mut doc, ellipse).locked = true;
    let locked = doc.clone();
    assert!(text_shape::attach(&mut doc, &mut history, ellipse).is_none());
    assert_eq!(doc, locked);
    object_mut(&mut doc, ellipse).locked = false;
    assert_eq!(doc, unchanged);
}

#[test]
fn box_detection_requires_a_straight_axis_aligned_frame() {
    let rectangle = authoring::path_for(ShapeKind::Rectangle, 40.0, 20.0);
    assert!(text_shape::is_box(&rectangle, 40.0, 20.0));
    assert!(!text_shape::is_box(&rectangle, 40.0, 21.0));
    assert!(!text_shape::is_box(
        &ShapePath::ellipse(40.0, 20.0),
        40.0,
        20.0
    ));
    // The same corners in a crossing order are a bow tie, not a box.
    let mut bow = rectangle;
    bow.subpaths[0].points.swap(1, 2);
    assert!(!text_shape::is_box(&bow, 40.0, 20.0));
}
