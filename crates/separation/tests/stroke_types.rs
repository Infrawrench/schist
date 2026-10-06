//! Dashed, dotted and striped strokes reach the plates laid along the
//! item's path, their gap colour filling the rest of the band a solid
//! stroke would cover with its own tint and overprint, as decoration gaps
//! do. The patterns are the public IDML specification's stroke styles
//! (tables 130 to 132) and its page items' StrokeDashAndGap, GapColor,
//! GapTint and OverprintGap; no public InDesign PDF draws a patterned item
//! stroke to measure.
use schist_layout::{
    authoring, decorations::DecorationStroke, History, Ink, LayoutDocument, LayoutObject,
    ObjectPaint, Page, Paint, PaintTints, Rect, StrokeType,
};
use schist_separation::{separate_page, NoGraphics, OutputSettings};
use schist_text_engine::{DecorationCap, DecorationDashes, DecorationFit, TextDecorationPattern};

const AREA: Rect = Rect::new(100.0, 100.0, 200.0, 100.0);
const CYAN: usize = 0;
const MAGENTA: usize = 1;
const BLACK: usize = 3;

/// An unfilled rectangle with a black stroke `width` wide, running
/// clockwise from its top-left corner, with `paint`'s stroke options.
fn document(width: f32, paint: ObjectPaint) -> LayoutDocument {
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
        fill: None,
        stroke: Some(Ink::black()),
        stroke_width: width,
        fill_overprint: false,
        stroke_overprint: false,
        tints: PaintTints::default(),
    };
    object.appearance.paint = paint;
    doc
}

fn plates(doc: &LayoutDocument) -> impl Fn(usize, i32, i32) -> f32 {
    let result = separate_page(doc, 0, OutputSettings::at(72.0), &NoGraphics).unwrap();
    move |plate, x, y| {
        let plate = result.plan.process[plate];
        result.separation.plate(plate).unwrap().at(x, y)
    }
}

fn cyan() -> Paint {
    Paint::Ink(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]))
}

fn dashed(gap: Option<Paint>) -> ObjectPaint {
    ObjectPaint {
        stroke_type: Some(StrokeType::Dashed),
        dash_and_gap: Some(vec![12.0, 6.0]),
        gap,
        ..Default::default()
    }
}

#[test]
fn dashes_and_their_gap_colour_partition_the_band() {
    let at = plates(&document(6.0, dashed(Some(cyan()))));
    // Along the top edge, from the top-left corner: a dash 0 to 12 pt, a
    // gap to 18, a dash to 30.
    assert!(at(BLACK, 105, 99) > 0.99 && at(CYAN, 105, 99) < 0.01);
    assert!(at(BLACK, 114, 99) < 0.01 && at(CYAN, 114, 99) > 0.99);
    assert!(at(BLACK, 120, 101) > 0.99);
    // Only the band: nothing beyond the stroke's 3 pt either side.
    assert!(at(CYAN, 114, 96) < 0.01 && at(CYAN, 114, 103) < 0.01);
    // No gap colour leaves the gaps clear.
    let at = plates(&document(6.0, dashed(None)));
    assert!(at(BLACK, 114, 99) < 0.01 && at(CYAN, 114, 99) < 0.01);
}

#[test]
fn a_gap_takes_its_own_tint_and_overprint() {
    let mut paint = dashed(Some(cyan()));
    paint.gap_tint = Some(0.5);
    let at = plates(&document(6.0, paint.clone()));
    assert!((at(CYAN, 114, 99) - 0.5).abs() < 0.02);

    // A magenta fill under the gap: knocked out, unless the gap overprints.
    let filled = |overprint: bool| {
        let mut doc = document(
            6.0,
            ObjectPaint {
                overprint_gap: Some(overprint),
                ..dashed(Some(cyan()))
            },
        );
        if let LayoutObject::Shape { fill, .. } = &mut doc.objects[0].object {
            *fill = Some(Ink::cmyk("Magenta", [0.0, 1.0, 0.0, 0.0]));
        }
        plates(&doc)
    };
    let knocked = filled(false);
    assert!(knocked(MAGENTA, 114, 101) < 0.01 && knocked(CYAN, 114, 101) > 0.99);
    let overprinted = filled(true);
    assert!(overprinted(MAGENTA, 114, 101) > 0.99 && overprinted(CYAN, 114, 101) > 0.99);
}

#[test]
fn dots_and_stripes_reach_the_plates() {
    let dotted = ObjectPaint {
        stroke_type: Some(StrokeType::Style(DecorationStroke {
            name: "Dots".into(),
            fitting: DecorationFit::None,
            pattern: TextDecorationPattern::Dots(vec![12.0]),
        })),
        gap: Some(cyan()),
        ..Default::default()
    };
    let at = plates(&document(6.0, dotted));
    // Dots 6 pt across, centred 12 pt apart from the corner.
    assert!(at(BLACK, 112, 99) > 0.9 && at(CYAN, 112, 99) < 0.1);
    assert!(at(BLACK, 118, 99) < 0.01 && at(CYAN, 118, 99) > 0.99);

    let striped = ObjectPaint {
        stroke_type: Some(StrokeType::Style(DecorationStroke {
            name: "Thick Thin".into(),
            fitting: DecorationFit::None,
            pattern: TextDecorationPattern::Stripes(vec![0.0, 50.0, 75.0, 100.0]),
        })),
        ..Default::default()
    };
    let at = plates(&document(8.0, striped));
    // Clockwise, the stroke's left edge is outside: 0 to 50 % runs 4 pt to
    // 0 pt out, 75 to 100 % from 2 to 4 pt in.
    assert!(at(BLACK, 150, 96) > 0.99 && at(BLACK, 150, 99) > 0.99);
    assert!(at(BLACK, 150, 100) < 0.01);
    assert!(at(BLACK, 150, 102) > 0.99 && at(BLACK, 150, 104) < 0.01);
}

#[test]
fn a_frames_stroke_is_patterned_too() {
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    authoring::text_frame(&mut doc, &mut History::default(), 0, AREA).unwrap();
    let frame = doc.objects.last_mut().unwrap();
    frame.bounds = AREA;
    frame.appearance.paint = ObjectPaint {
        stroke: Some(Paint::Ink(Ink::black())),
        stroke_width: Some(6.0),
        stroke_type: Some(StrokeType::Style(DecorationStroke {
            name: "Long".into(),
            fitting: DecorationFit::None,
            pattern: TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![12.0, 6.0],
                cap: DecorationCap::Butt,
            }),
        })),
        ..Default::default()
    };
    let at = plates(&doc);
    assert!(at(BLACK, 105, 99) > 0.99);
    assert!(at(BLACK, 114, 99) < 0.01);
}
