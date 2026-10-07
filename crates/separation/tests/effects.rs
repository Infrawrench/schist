//! Blend modes and drop shadows on the plates, as InDesign's PDF of the
//! public paged-media `effects` sample sets them: a 50 % magenta rectangle
//! over a cyan one in each blend mode, and a paper rectangle casting a 75 %
//! black shadow 6 pt right and down, 6 pt soft, under Multiply.
use schist_layout::{
    authoring,
    effects::{BlendMode, DropShadow},
    History, Ink, LayoutDocument, LayoutObject, ObjectId, Page, PaintTints, Rect,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};

const CYAN: usize = 0;
const MAGENTA: usize = 1;
const BLACK: usize = 3;

fn rectangle(doc: &mut LayoutDocument, area: Rect, ink: Ink) -> ObjectId {
    let id = authoring::shape(
        doc,
        &mut History::default(),
        0,
        area,
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    let LayoutObject::Shape { path, .. } = &object.object else {
        unreachable!()
    };
    object.object = LayoutObject::Shape {
        path: path.clone(),
        fill: Some(ink),
        stroke: None,
        stroke_width: 0.0,
        fill_overprint: false,
        stroke_overprint: false,
        tints: PaintTints::default(),
    };
    object.bounds = area;
    id
}

fn plates(doc: &LayoutDocument, dpi: f32) -> impl Fn(usize, i32, i32) -> f32 {
    let result = separate_page(doc, 0, OutputSettings::at(dpi), &NoGraphics).unwrap();
    move |plate, x, y| {
        let plate = result.plan.process[plate];
        result.separation.plate(plate).unwrap().at(x, y)
    }
}

fn near(a: f32, b: f32, tolerance: f32, what: &str) {
    assert!((a - b).abs() <= tolerance, "{what}: {a} vs {b}");
}

#[test]
fn blend_modes_mix_with_what_lies_beneath() {
    for (mode, cyan, magenta) in [
        (None, 0.0, 0.5),
        (Some(BlendMode::Multiply), 1.0, 0.5),
        (Some(BlendMode::Screen), 0.0, 0.0),
        (Some(BlendMode::Darken), 1.0, 0.5),
        (Some(BlendMode::Lighten), 0.0, 0.0),
        // Drawn Normal.
        (Some(BlendMode::Luminosity), 0.0, 0.5),
    ] {
        let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
        rectangle(
            &mut doc,
            Rect::new(50.0, 50.0, 200.0, 150.0),
            Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]),
        );
        let top = rectangle(
            &mut doc,
            Rect::new(150.0, 100.0, 200.0, 150.0),
            Ink::cmyk("Magenta 50", [0.0, 0.5, 0.0, 0.0]),
        );
        doc.objects
            .iter_mut()
            .find(|o| o.id == top)
            .unwrap()
            .appearance
            .blend_mode = mode;
        let at = plates(&doc, 72.0);
        let what = format!("{mode:?}");
        near(at(CYAN, 200, 150), cyan, 0.01, &format!("{what} cyan"));
        near(
            at(MAGENTA, 200, 150),
            magenta,
            0.01,
            &format!("{what} magenta"),
        );
        // Beside the underlay the magenta lies on paper.
        let alone = if mode == Some(BlendMode::Screen) || mode == Some(BlendMode::Lighten) {
            0.0
        } else {
            0.5
        };
        near(at(MAGENTA, 300, 220), alone, 0.01, &format!("{what} alone"));
    }
}

/// A paper rectangle at (100, 100), 200 × 100 pt, casting `shadow`, over a
/// cyan page-wide rectangle.
fn shadowed(shadow: DropShadow, opacity: f32) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    rectangle(
        &mut doc,
        Rect::new(0.0, 0.0, 400.0, 300.0),
        Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]),
    );
    let id = rectangle(
        &mut doc,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        Ink::cmyk("Paper", [0.0; 4]),
    );
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.drop_shadow = Some(shadow);
    object.transparency = opacity;
    doc
}

fn sample_shadow() -> DropShadow {
    DropShadow {
        x_offset: 6.0,
        y_offset: 6.0,
        size: 6.0,
        ..DropShadow::default()
    }
}

#[test]
fn a_paper_item_casts_indesigns_shadow() {
    // At 144 dpi a point is two pixels; pixel x covers x/2 to (x + 1)/2 pt.
    let at = plates(&shadowed(sample_shadow(), 1.0), 144.0);
    let row = 300; // 150 pt, the rectangle's middle
                   // The shadow's right edge is 306 pt; InDesign's mask is half there,
                   // 0.778 of the way in 0.4 sizes inside it and 0.054 0.8 sizes outside,
                   // all at 75 %.
    for (pt, mask) in [(306.0f32, 0.5), (303.6, 0.778), (310.8, 0.054)] {
        let x = (pt * 2.0).round() as i32;
        let value = (at(BLACK, x - 1, row) + at(BLACK, x, row)) / 2.0;
        near(value, 0.75 * mask, 0.04, &format!("{pt} pt"));
    }
    // The paper rectangle knocks the shadow and the cyan out beneath it.
    near(at(BLACK, 400, row), 0.0, 0.001, "under the item");
    near(at(CYAN, 400, row), 0.0, 0.001, "paper over cyan");
    // Multiply leaves the cyan under the shadow as it was.
    near(at(CYAN, 610, row), 1.0, 0.001, "cyan under the shadow");
    // Nothing well clear of it.
    near(at(BLACK, 700, row), 0.0, 0.001, "clear");
}

#[test]
fn a_transparent_item_hides_its_shadow_unless_told_not_to() {
    // Deep inside the shadow, under a half-transparent paper item. Knocked
    // out, the item's shape hides the shadow and half the cyan shows.
    let at = plates(&shadowed(sample_shadow(), 0.5), 72.0);
    near(at(BLACK, 250, 170), 0.0, 0.01, "knocked out");
    near(at(CYAN, 250, 170), 0.5, 0.01, "cyan through the item");
    // Not knocked out, the shadow shows through: 75 % at the item's 50 %,
    // then half of that through the paper.
    let at = plates(
        &shadowed(
            DropShadow {
                knocked_out: false,
                ..sample_shadow()
            },
            0.5,
        ),
        72.0,
    );
    near(
        at(BLACK, 250, 170),
        0.75 * 0.5 * 0.5,
        0.01,
        "showing through",
    );
}
