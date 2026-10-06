//! Gradient fills and strokes: where a gradient runs over an item and how
//! its stops mix. An item that states no start begins at its path's left and
//! bottom edges, as long as its path is wide: InDesign's own exports write
//! exactly that, and its PDF of the public paged-media `gradients` sample,
//! whose items state none, runs its linear axis along the width and centres
//! its radial gradient on that corner with the width as radius. A stroke runs
//! the same way, and text over the frame it is set in: no public sample shows
//! either, so both are the specification read as fills are.
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::pasteboard::TextPart;
use schist_layout::{
    authoring, pasteboard, CharacterStyle, Display, History, Ink, LayoutDocument, Page, Paint,
    ParagraphStyle, PasteboardView, Point, Rect, Story, StyleSet,
};

fn gradient(inks: &[Ink], radial: bool) -> Gradient {
    let last = (inks.len() - 1) as f32;
    Gradient {
        name: "Test".into(),
        radial,
        stops: inks
            .iter()
            .enumerate()
            .map(|(index, ink)| GradientStop {
                ink: ink.clone(),
                location: index as f32 / last,
                midpoint: 0.5,
            })
            .collect(),
    }
}

fn fill(gradient: Gradient) -> GradientFill {
    GradientFill {
        gradient,
        start: None,
        length: None,
        angle: 0.0,
    }
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 1e-3, "{what}: {a} vs {b}");
}

#[test]
fn an_item_without_a_start_runs_from_its_bottom_left_over_its_width() {
    let bounds = Rect::new(0.0, 0.0, 360.0, 200.0);
    let linear = fill(gradient(&[Ink::black(), Ink::white()], false));
    let vector = linear.vector(bounds);
    near(vector.0.x, 0.0, "start x");
    near(vector.0.y, 200.0, "start y");
    near(vector.2, 360.0, "length");
    for (x, t) in [(0.0, 0.0), (90.0, 0.25), (360.0, 1.0), (500.0, 1.0)] {
        near(linear.position(Point::new(x, 37.0), vector), t, "along");
    }
    let radial = fill(gradient(&[Ink::white(), Ink::black()], true));
    let vector = radial.vector(bounds);
    near(
        radial.position(Point::new(0.0, 200.0), vector),
        0.0,
        "centre",
    );
    near(
        radial.position(Point::new(180.0, 200.0), vector),
        0.5,
        "half",
    );
    near(radial.position(Point::new(0.0, 20.0), vector), 0.5, "up");
    near(
        radial.position(Point::new(360.0, 0.0), vector),
        1.0,
        "beyond",
    );
}

#[test]
fn a_stated_start_length_and_angle_place_the_gradient() {
    let mut up = fill(gradient(&[Ink::black(), Ink::white()], false));
    up.start = Some(Point::new(10.0, 110.0));
    up.length = Some(100.0);
    // Counter-clockwise from the x axis: up the page.
    up.angle = 90.0;
    let vector = up.vector(Rect::new(0.0, 0.0, 50.0, 120.0));
    near(up.position(Point::new(10.0, 110.0), vector), 0.0, "start");
    near(
        up.position(Point::new(40.0, 60.0), vector),
        0.5,
        "half way up",
    );
    near(up.position(Point::new(10.0, 10.0), vector), 1.0, "end");
}

#[test]
fn stops_mix_evenly_at_their_midpoint() {
    let cyan = Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]);
    let magenta = Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0]);
    let yellow = Ink::cmyk("Yellow", [0.0, 0.0, 1.0, 0.0]);
    let mut three = gradient(&[cyan, magenta, yellow], false);
    assert!(three.valid());
    assert_eq!(three.mix(0.0), (0, 0, 0.0));
    let (a, b, f) = three.mix(0.25);
    assert_eq!((a, b), (0, 1));
    near(f, 0.5, "half way to the second stop");
    let (a, b, f) = three.mix(0.75);
    assert_eq!((a, b), (1, 2));
    near(f, 0.5, "half way to the third stop");
    assert_eq!(three.mix(1.0), (2, 2, 0.0));
    // A midpoint a quarter of the way along mixes evenly there.
    three.stops[1].midpoint = 0.25;
    let (_, _, f) = three.mix(0.125);
    near(f, 0.5, "even at the midpoint");
    let (_, _, f) = three.mix(0.375);
    assert!(f > 0.75, "{f}");
    // Out of order stops are refused.
    three.stops.swap(0, 2);
    assert!(!three.valid());
}

#[test]
fn the_canvas_shows_a_gradient_across_a_shape() {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 50.0, 360.0, 200.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.fill = Some(Paint::Gradient(Box::new(fill(gradient(
        &[Ink::black(), Ink::white()],
        false,
    )))));
    let plan = pasteboard(&doc, &PasteboardView::default()).unwrap();
    let gradient = plan
        .objects()
        .find_map(|display| match display {
            Display::Shape {
                gradient: Some(gradient),
                fill,
                ..
            } => {
                assert!(fill.is_none());
                Some(gradient.clone())
            }
            _ => None,
        })
        .expect("a gradient shape");
    let Display::Shape { path, .. } = plan
        .objects()
        .find(|d| matches!(d, Display::Shape { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let area = path.bounds();
    let mid = area.y + area.height / 2.0;
    let left = gradient.preview(area.x + 1.0, mid);
    let right = gradient.preview(area.right() - 1.0, mid);
    let middle = gradient.preview(area.x + area.width / 2.0, mid);
    assert!(left[0] < 0.1 && right[0] > 0.9, "{left:?} {right:?}");
    assert!((middle[0] - 0.5).abs() < 0.1, "{middle:?}");
}

#[test]
fn the_canvas_strokes_a_shape_with_a_gradient() {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 50.0, 360.0, 200.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.stroke = Some(Paint::Gradient(Box::new(fill(gradient(
        &[Ink::black(), Ink::white()],
        false,
    )))));
    object.appearance.paint.stroke_width = Some(10.0);
    // A stroke with a gradient is a stroke: it reaches into a frame's text
    // as a solid one does.
    let resolved = doc.styles.object_paint(object);
    assert_eq!(
        resolved.drawn_stroke(),
        Some((10.0, schist_layout::StrokeAlignment::Center))
    );
    let plan = pasteboard(&doc, &PasteboardView::default()).unwrap();
    let Some(Display::Shape {
        path,
        stroke: Some((_, width)),
        stroke_gradient: Some(gradient),
        gradient: None,
        ..
    }) = plan.objects().find(|d| matches!(d, Display::Shape { .. }))
    else {
        panic!("a gradient stroke");
    };
    near(*width, 10.0, "weight");
    // The stroke runs over the path from its left edge across its width.
    let area = path.bounds();
    let left = gradient.preview(area.x, area.y);
    let right = gradient.preview(area.right(), area.y);
    let middle = gradient.preview(area.x + area.width / 2.0, area.bottom());
    assert!(left[0] < 0.01 && right[0] > 0.99, "{left:?} {right:?}");
    assert!((middle[0] - 0.5).abs() < 0.01, "{middle:?}");
}

#[test]
fn a_text_gradient_runs_over_its_frame_from_the_bottom_left() {
    let mut ramp = fill(gradient(&[Ink::black(), Ink::white()], false));
    let vector = ramp.text_vector((360.0, 200.0));
    near(vector.0.x, 0.0, "start x");
    near(vector.0.y, 200.0, "start y");
    near(vector.2, 360.0, "length");
    near(ramp.position(Point::new(90.0, 20.0), vector), 0.25, "along");
    // A stated length and angle are drawn; a stated start is not.
    ramp.start = Some(Point::new(-500.0, 900.0));
    ramp.length = Some(100.0);
    ramp.angle = 90.0;
    let vector = ramp.text_vector((360.0, 200.0));
    near(vector.0.y, 200.0, "start y");
    near(ramp.position(Point::new(300.0, 150.0), vector), 0.5, "up");
}

#[test]
fn a_text_gradient_inherits_with_its_paint() {
    let ramp = Box::new(fill(gradient(&[Ink::black(), Ink::white()], false)));
    let red = Ink::cmyk("Red", [0.0, 1.0, 1.0, 0.0]);
    let mut styles = StyleSet::with_defaults();
    styles.add_paragraph(ParagraphStyle {
        name: "Ramp".into(),
        fill: Some(Ink::black()),
        fill_gradient: Some(ramp.clone()),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Red".into(),
        fill: Some(red.clone()),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Glow".into(),
        fill: Some(Ink::black()),
        fill_gradient: Some(ramp.clone()),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Red over glow".into(),
        based_on: Some("Glow".into()),
        fill: Some(red.clone()),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Under glow".into(),
        based_on: Some("Glow".into()),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Unfilled".into(),
        fill_disabled: true,
        ..Default::default()
    });
    let base = styles
        .resolve_paragraph("Ramp")
        .character(styles.resolve_character("Default"));
    assert_eq!(base.fill_gradient.as_ref(), Some(&ramp));
    let run = |name: &str| styles.resolve_character(name).with_paint_defaults(&base);
    // A nearer ink replaces the gradient; a nearer gradient replaces an ink.
    let over = run("Red");
    assert_eq!((over.fill, over.fill_gradient), (Some(red.clone()), None));
    assert_eq!(run("Glow").fill_gradient.as_ref(), Some(&ramp));
    assert_eq!(run("Red over glow").fill_gradient, None);
    assert_eq!(run("Under glow").fill_gradient.as_ref(), Some(&ramp));
    assert!(run("Unfilled").fill_disabled);
    assert_eq!(run("Unfilled").fill_gradient, None);
    let plain = styles
        .resolve_paragraph("Default")
        .character(styles.resolve_character("Glow"));
    assert_eq!(plain.fill_gradient.as_ref(), Some(&ramp));
}

#[test]
fn the_canvas_paints_text_with_its_gradient_over_the_frame() {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 50.0, 360.0, 200.0),
    )
    .unwrap();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Ramp".into(),
        point_size: Some(24.0),
        fill: Some(Ink::black()),
        fill_gradient: Some(Box::new(fill(gradient(
            &[Ink::black(), Ink::white()],
            false,
        )))),
        underline: Some(true),
        ..Default::default()
    });
    doc.stories[frame.story.0 as usize] = Story::from_text("Gradient", "Ramp");
    let plan = pasteboard(&doc, &PasteboardView::default()).unwrap();
    let gradients = plan
        .objects()
        .find_map(|d| match d {
            Display::Text { gradients, .. } if !gradients.is_empty() => Some(gradients.clone()),
            _ => None,
        })
        .expect("a line with a gradient");
    // The glyphs and their underline in the text's colour; no stroke.
    let parts: Vec<_> = gradients.iter().map(|g| (g.run, g.part)).collect();
    assert!(parts.contains(&(0, TextPart::Fill)), "{parts:?}");
    assert!(parts.contains(&(0, TextPart::Underline)), "{parts:?}");
    assert!(!parts.iter().any(|(_, part)| *part == TextPart::Stroke));
    let Some(Display::Frame { rect, .. }) =
        plan.objects().find(|d| matches!(d, Display::Frame { .. }))
    else {
        panic!("the frame");
    };
    // Black at the frame's left edge, paper at its right, wherever the
    // line's glyphs end.
    let gradient = &gradients[0].gradient;
    let mid = rect.y + 10.0;
    let left = gradient.preview(rect.x, mid);
    let right = gradient.preview(rect.right(), mid);
    let quarter = gradient.preview(rect.x + rect.width / 4.0, mid);
    assert!(left[0] < 0.01 && right[0] > 0.99, "{left:?} {right:?}");
    assert!((quarter[0] - 0.25).abs() < 0.01, "{quarter:?}");
}
