//! Stroke caps, joins and alignment reach the plates as InDesign's PDF of the
//! public paged-media `strokes-fills` sample draws them: a 6 pt stroke on a
//! 200 × 100 pt rectangle sits across the path when centred, and inside or
//! outside it moves the path, fill and stroke alike, half the weight that way
//! (the PDF's inside rectangle is 194 × 94 pt from 3 pt in, its outside one
//! 206 × 106 pt with square corners); corners mitre by default.
use schist_layout::{
    authoring, History, Ink, LayoutDocument, LayoutObject, ObjectPaint, Page, PaintTints, Point,
    Rect, ShapePath, StrokeAlignment, StrokeCap, StrokeJoin, SubPath,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

const AREA: Rect = Rect::new(100.0, 100.0, 200.0, 100.0);

/// A cyan rectangle with an overprinted 6 pt black stroke, so the fill shows
/// under the stroke wherever the fill reaches.
fn document(options: ObjectPaint) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        AREA,
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    let path = match &object.object {
        LayoutObject::Shape { path, .. } => path.clone(),
        _ => unreachable!(),
    };
    object.bounds = AREA;
    object.object = LayoutObject::Shape {
        path,
        fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        stroke: Some(Ink::black()),
        stroke_width: 6.0,
        fill_overprint: false,
        stroke_overprint: true,
        tints: PaintTints::default(),
    };
    object.appearance.paint = options;
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

fn aligned(alignment: StrokeAlignment) -> ObjectPaint {
    ObjectPaint {
        stroke_alignment: Some(alignment),
        ..Default::default()
    }
}

#[test]
fn a_centred_stroke_straddles_the_path() {
    let at = plates(&document(ObjectPaint::default()));
    // The left edge at x = 100: the band runs 97 to 103.
    assert!(at(BLACK, 98, 150) > 0.9);
    assert!(at(BLACK, 102, 150) > 0.9);
    assert!(at(BLACK, 96, 150) < 0.05 && at(BLACK, 104, 150) < 0.05);
    // The fill reaches the path.
    assert!(at(CYAN, 101, 150) > 0.9);
    assert!(at(CYAN, 98, 150) < 0.05);
}

#[test]
fn an_inside_stroke_and_its_fill_move_in() {
    let at = plates(&document(aligned(StrokeAlignment::Inside)));
    assert!(at(BLACK, 98, 150) < 0.05, "nothing outside the path");
    assert!(at(BLACK, 100, 150) > 0.9 && at(BLACK, 105, 150) > 0.9);
    assert!(at(BLACK, 107, 150) < 0.05);
    // The fill starts half the weight in.
    assert!(at(CYAN, 101, 150) < 0.05);
    assert!(at(CYAN, 104, 150) > 0.9);
}

#[test]
fn an_outside_stroke_and_its_fill_move_out() {
    let at = plates(&document(aligned(StrokeAlignment::Outside)));
    assert!(at(BLACK, 94, 150) > 0.9 && at(BLACK, 99, 150) > 0.9);
    assert!(at(BLACK, 92, 150) < 0.05, "the band is the weight wide");
    assert!(at(BLACK, 101, 150) < 0.05, "nothing inside the path");
    // The fill reaches half the weight out, under the stroke.
    assert!(at(CYAN, 98, 150) > 0.9);
    assert!(at(CYAN, 96, 150) < 0.05);
    // Square outer corners: the outset path mitres.
    assert!(at(BLACK, 95, 95) > 0.9);
}

#[test]
fn corners_mitre_unless_asked_to_round() {
    // Just outside the corner's 3 pt radius at (97.5, 97.5).
    let mitred = plates(&document(ObjectPaint::default()));
    assert!(mitred(BLACK, 97, 97) > 0.9);
    let rounded = plates(&document(ObjectPaint {
        stroke_join: Some(StrokeJoin::Round),
        ..Default::default()
    }));
    assert!(rounded(BLACK, 97, 97) < 0.3);
}

#[test]
fn open_ends_take_their_caps() {
    let line = |cap: StrokeCap| {
        let mut doc = document(ObjectPaint {
            stroke_cap: Some(cap),
            ..Default::default()
        });
        let object = &mut doc.objects[0];
        object.bounds = Rect::new(100.0, 250.0, 200.0, 0.0);
        if let LayoutObject::Shape {
            path, stroke_width, ..
        } = &mut object.object
        {
            *path = ShapePath {
                subpaths: vec![SubPath {
                    points: vec![Point::new(0.0, 0.0), Point::new(200.0, 0.0)],
                    handles: Vec::new(),
                    closed: false,
                }],
                even_odd: false,
            };
            *stroke_width = 10.0;
        }
        plates(&doc)
    };
    let butt = line(StrokeCap::Butt);
    assert!(butt(BLACK, 98, 250) < 0.05);
    let projecting = line(StrokeCap::Projecting);
    // The square reaches 95: (95.5, 246.5) is inside it.
    assert!(projecting(BLACK, 97, 250) > 0.9 && projecting(BLACK, 95, 246) > 0.9);
    let round = line(StrokeCap::Round);
    assert!(round(BLACK, 96, 250) > 0.9);
    // (95.5, 246.5) is 5.7 pt from the end: outside the 5 pt round cap.
    assert!(round(BLACK, 95, 246) < 0.3);
}
