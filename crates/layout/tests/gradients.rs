//! Gradient fills: where a gradient runs over an item and how its stops
//! mix. An item that states no start begins at its path's left and bottom
//! edges, as long as its path is wide: InDesign's own exports write exactly
//! that, and its PDF of the public paged-media `gradients` sample, whose
//! items state none, runs its linear axis along the width and centres its
//! radial gradient on that corner with the width as radius.
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::{
    authoring, pasteboard, Display, History, Ink, LayoutDocument, Page, Paint, PasteboardView,
    Point, Rect,
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
