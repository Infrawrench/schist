//! Corner options reach the plates: a rectangle's rounded, inverse rounded,
//! bevelled and inset corners cut its fill, and its stroke follows them.
use schist_layout::{
    authoring, CornerShape, Corners, History, Ink, LayoutDocument, LayoutObject, ObjectPaint, Page,
    PaintTints, Rect,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

/// A cyan 200 × 100 pt rectangle at (100, 100) with 30 pt corners and an
/// overprinted black stroke.
fn document(shape: CornerShape, stroke: f32) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let area = Rect::new(100.0, 100.0, 200.0, 100.0);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        area,
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    let path = match &object.object {
        LayoutObject::Shape { path, .. } => path.clone(),
        _ => unreachable!(),
    };
    object.bounds = area;
    object.object = LayoutObject::Shape {
        path,
        fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        stroke: Some(Ink::black()),
        stroke_width: stroke,
        fill_overprint: false,
        stroke_overprint: true,
        tints: PaintTints::default(),
    };
    object.appearance.paint = ObjectPaint {
        corners: Some(Corners {
            shapes: [shape; 4],
            radii: [30.0; 4],
        }),
        ..Default::default()
    };
    doc
}

fn plates(doc: &LayoutDocument) -> impl Fn(usize, i32, i32) -> f32 {
    let result = separate_page(doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    move |plate, x, y| {
        let plate = result.plan.process[plate];
        result.separation.plate(plate).unwrap().at(x, y)
    }
}

const CYAN: usize = 0;
const BLACK: usize = 3;

#[test]
fn corners_cut_the_fill() {
    // Pixels are named by their top-left; (x, y) covers x..x+1.
    let rounded = plates(&document(CornerShape::Rounded, 0.0));
    // The arc crosses the diagonal 8.79 pt from the corner.
    assert!(rounded(CYAN, 104, 104) < 0.05);
    assert!(rounded(CYAN, 112, 112) > 0.9);
    assert!(rounded(CYAN, 100, 150) > 0.9, "the edges stay");

    // Inverse rounded: a quarter disc about the corner is gone.
    let inverse = plates(&document(CornerShape::InverseRounded, 0.0));
    assert!(inverse(CYAN, 110, 110) < 0.05);
    assert!(inverse(CYAN, 101, 127) < 0.05);
    assert!(inverse(CYAN, 125, 125) > 0.9);
    assert!(inverse(CYAN, 101, 135) > 0.9);

    // Bevel: the cut runs from 30 pt down the side to 30 pt along the top.
    let bevel = plates(&document(CornerShape::Bevel, 0.0));
    assert!(bevel(CYAN, 110, 110) < 0.05);
    assert!(bevel(CYAN, 118, 118) > 0.9);

    // Inset: a 30 pt square notch.
    let inset = plates(&document(CornerShape::Inset, 0.0));
    assert!(inset(CYAN, 125, 125) < 0.05);
    assert!(inset(CYAN, 135, 105) > 0.9 && inset(CYAN, 105, 135) > 0.9);
    // The far corner is notched too.
    assert!(inset(CYAN, 295, 195) < 0.05);
}

#[test]
fn the_stroke_follows_the_corners() {
    let at = plates(&document(CornerShape::Rounded, 4.0));
    // On the arc: 30 pt from its centre at (130, 130).
    assert!(at(BLACK, 108, 108) > 0.9);
    assert!(at(BLACK, 101, 101) < 0.05, "nothing at the square corner");
    assert!(
        at(BLACK, 99, 150) > 0.9,
        "the straight edges stroke as before"
    );
}
