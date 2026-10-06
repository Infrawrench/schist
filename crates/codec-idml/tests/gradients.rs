//! Gradient swatches, and gradient fills and strokes on items and text. A
//! real InDesign export fills a rectangle with a two-stop linear gradient
//! whose GradientFillStart is the rectangle's own left-bottom anchor and whose
//! length is its width; the fill is typed with that start in the rectangle's
//! path coordinates. No public file strokes or sets text with a gradient: the
//! stroke reads the specification's GradientStroke attributes as the fill's,
//! and text reads InDesign's defaults ("0 0" with a length of -1) as stating
//! no vector.
use schist_codec_idml::{container, export, import};
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::{authoring, blank_a4, History, Ink, Paint, Point, Rect};

const MULTIPAGE: &[u8] = include_bytes!("../../../fixtures/idml/multipage.idml");

#[test]
fn a_real_exports_gradient_fill_is_typed() {
    let imported = import::read(MULTIPAGE).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Gradient")),
        "{:?}",
        imported.report.skipped
    );
    let fills: Vec<&GradientFill> = imported
        .document
        .objects
        .iter()
        .filter_map(|o| o.appearance.paint.fill_gradient())
        .collect();
    assert_eq!(fills.len(), 1);
    let fill = fills[0];
    assert!(!fill.gradient.radial);
    assert_eq!(fill.gradient.stops.len(), 2);
    assert_eq!(
        (
            fill.gradient.stops[0].location,
            fill.gradient.stops[1].location
        ),
        (0.0, 1.0)
    );
    // The anchor (198.763, 197.105) of a path from (198.763, -48.701):
    // the path's left-bottom corner.
    let start = fill.start.unwrap();
    assert!(start.x.abs() < 0.01, "{start:?}");
    assert!((start.y - 245.806).abs() < 0.01, "{start:?}");
    assert!((fill.length.unwrap() - 245.806).abs() < 0.01);
    assert_eq!(fill.angle, 0.0);
}

#[test]
fn gradient_fills_survive_saves() {
    let mut doc = blank_a4();
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 60.0, 300.0, 200.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let fill = GradientFill {
        gradient: Gradient {
            name: "Glow".into(),
            radial: true,
            stops: vec![
                GradientStop {
                    ink: Ink::cmyk("Warm", [0.0, 0.4, 0.9, 0.0]),
                    location: 0.0,
                    midpoint: 0.5,
                },
                GradientStop {
                    ink: Ink::black(),
                    location: 1.0,
                    midpoint: 0.3,
                },
            ],
        },
        start: Some(Point::new(150.0, 100.0)),
        length: Some(180.0),
        angle: 30.0,
    };
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.fill = Some(Paint::Gradient(Box::new(fill.clone())));
    for _ in 0..3 {
        let bytes = export::write(&doc).bytes;
        let package = container::read(&bytes).unwrap();
        assert!(package
            .text("Resources/Graphic.xml")
            .unwrap()
            .contains("<Gradient "));
        let imported = import::read(&bytes).unwrap();
        doc = imported.document;
        let read: Vec<&GradientFill> = doc
            .objects
            .iter()
            .filter_map(|o| o.appearance.paint.fill_gradient())
            .collect();
        assert_eq!(read, [&fill]);
    }
}

#[test]
fn a_radial_highlight_is_reported() {
    let mut package = container::read(MULTIPAGE).unwrap();
    let part = "Spreads/Spread_u1fd.xml";
    let text = package.text(part).unwrap().replace(
        r#"GradientFillHiliteLength="0""#,
        r#"GradientFillHiliteLength="12""#,
    );
    package.insert(part, text.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(
        imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Gradient highlight not applied")),
        "{:?}",
        imported.report.skipped
    );
}

/// The multipage fixture with one part edited.
fn edited(part: &str, edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut package = container::read(MULTIPAGE).unwrap();
    let text = edit(package.text(part).unwrap().to_string());
    package.insert(part, text.into_bytes());
    container::write(&package.into_parts())
}

const SPREAD: &str = "Spreads/Spread_u1fd.xml";

/// The gradient rectangle, from (198.763, -48.701) to (444.569, 197.105),
/// stroked 6 pt with its own gradient.
fn stroked_rectangle(text: String) -> String {
    text.replace(
        r#"FillColor="Gradient/u27f" StrokeWeight="0" StrokeColor="Swatch/None""#,
        r#"FillColor="Gradient/u27f" StrokeWeight="6" StrokeColor="Gradient/u27f""#,
    )
    .replace(
        r#"GradientFillLength="245.80597014925388" GradientFillAngle="0" GradientStrokeStart="0 0" GradientStrokeLength="0" GradientStrokeAngle="0""#,
        r#"GradientFillLength="245.80597014925388" GradientFillAngle="0" GradientStrokeStart="444.5687843209521 -48.70068296725469" GradientStrokeLength="120" GradientStrokeAngle="-90""#,
    )
}

#[test]
fn a_gradient_stroke_is_typed_where_the_item_says() {
    let imported = import::read(&edited(SPREAD, stroked_rectangle)).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Gradient")),
        "{:?}",
        imported.report.skipped
    );
    let object = imported
        .document
        .objects
        .iter()
        .find(|o| o.appearance.paint.stroke_gradient().is_some())
        .expect("a gradient stroke");
    let paint = &object.appearance.paint;
    assert_eq!(paint.stroke_width, Some(6.0));
    assert!(paint.fill_gradient().is_some());
    let stroke = paint.stroke_gradient().unwrap();
    // The path's top-right anchor, in its own coordinates; running down.
    let start = stroke.start.unwrap();
    assert!((start.x - 245.806).abs() < 0.01, "{start:?}");
    assert!(start.y.abs() < 0.01, "{start:?}");
    assert_eq!((stroke.length, stroke.angle), (Some(120.0), -90.0));
}

#[test]
fn a_gradient_strokes_highlight_is_reported() {
    let imported = import::read(&edited(SPREAD, |text| {
        stroked_rectangle(text).replace(
            r#"GradientStrokeHiliteLength="0""#,
            r#"GradientStrokeHiliteLength="12""#,
        )
    }))
    .unwrap();
    assert!(
        imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Gradient highlight not applied")),
        "{:?}",
        imported.report.skipped
    );
}

#[test]
fn gradient_strokes_survive_saves() {
    let mut doc = blank_a4();
    let id = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 60.0, 300.0, 200.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    let stroke = GradientFill {
        gradient: Gradient {
            name: "Edge".into(),
            radial: false,
            stops: vec![
                GradientStop {
                    ink: Ink::cmyk("Warm", [0.0, 0.4, 0.9, 0.0]),
                    location: 0.0,
                    midpoint: 0.5,
                },
                GradientStop {
                    ink: Ink::black(),
                    location: 1.0,
                    midpoint: 0.5,
                },
            ],
        },
        start: Some(Point::new(0.0, 200.0)),
        length: Some(300.0),
        angle: 45.0,
    };
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    object.appearance.paint.stroke = Some(Paint::Gradient(Box::new(stroke.clone())));
    object.appearance.paint.stroke_width = Some(4.0);
    for _ in 0..3 {
        let imported = import::read(&export::write(&doc).bytes).unwrap();
        doc = imported.document;
        let read: Vec<_> = doc
            .objects
            .iter()
            .filter_map(|o| o.appearance.paint.stroke_gradient())
            .collect();
        assert_eq!(read, [&stroke]);
        assert!(doc
            .objects
            .iter()
            .all(|o| o.appearance.paint.fill_gradient().is_none()));
    }
}

const STORY: &str = "Stories/Story_u21b.xml";

/// Every range of the fixture's numbered story with `attributes` added.
fn text_with(attributes: &'static str) -> impl Fn(String) -> String {
    move |text| {
        text.replace(
            r#"FontStyle="Bold" PointSize="36""#,
            &format!(r#"FontStyle="Bold" PointSize="36" {attributes}"#),
        )
    }
}

/// The character style the story's ranges were read into.
fn gradient_style(doc: &schist_layout::LayoutDocument) -> &schist_layout::CharacterStyle {
    doc.styles
        .characters
        .iter()
        .find(|s| s.fill_gradient.is_some())
        .expect("a text gradient")
}

#[test]
fn a_text_gradient_is_typed_and_saved() {
    // InDesign's text defaults for the vector state none.
    let imported = import::read(&edited(
        STORY,
        text_with(
            r#"FillColor="Gradient/u27f" GradientFillStart="0 0" GradientFillLength="-1" StrokeColor="Gradient/u85" StrokeWeight="1""#,
        ),
    ))
    .unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Gradient")),
        "{:?}",
        imported.report.skipped
    );
    let style = gradient_style(&imported.document);
    let fill = style.fill_gradient.as_deref().unwrap();
    assert_eq!((fill.start, fill.length, fill.angle), (None, None, 0.0));
    assert_eq!(fill.gradient.name, "Gradient/u27f");
    // The first stop stands in as the ink for readers of solid colour.
    assert_eq!(style.fill.as_ref(), Some(&fill.gradient.stops[0].ink));
    let stroke = style.stroke_gradient.as_deref().expect("a stroke gradient");
    assert_eq!(stroke.gradient.name, "Gradient/u85");
    let mut saved = Vec::new();
    let mut doc = imported.document;
    for _ in 0..2 {
        let bytes = export::write(&doc).bytes;
        assert!(container::read(&bytes)
            .unwrap()
            .text("Resources/Graphic.xml")
            .unwrap()
            .contains("<Gradient "));
        let imported = import::read(&bytes).unwrap();
        assert!(
            !imported
                .report
                .skipped
                .iter()
                .any(|s| s.contains("Gradient")),
            "{:?}",
            imported.report.skipped
        );
        doc = imported.document;
        let style = gradient_style(&doc);
        saved.push((style.fill_gradient.clone(), style.stroke_gradient.clone()));
    }
    assert_eq!(saved[0], saved[1]);
    let fill = saved[0].0.as_deref().unwrap();
    assert_eq!((fill.start, fill.length, fill.angle), (None, None, 0.0));
    assert_eq!(fill.gradient.stops.len(), 2);
}

#[test]
fn a_text_gradients_stated_start_is_kept_and_reported() {
    let imported = import::read(&edited(
        STORY,
        text_with(
            r#"FillColor="Gradient/u27f" GradientFillStart="10 20" GradientFillLength="50" GradientFillAngle="30""#,
        ),
    ))
    .unwrap();
    let reported = |skipped: &[String]| {
        skipped
            .iter()
            .any(|s| s.contains("Text gradient start not applied: Gradient/u27f"))
    };
    assert!(
        reported(&imported.report.skipped),
        "{:?}",
        imported.report.skipped
    );
    let fill = gradient_style(&imported.document)
        .fill_gradient
        .clone()
        .unwrap();
    assert_eq!(
        (fill.start, fill.length, fill.angle),
        (Some(Point::new(10.0, 20.0)), Some(50.0), 30.0)
    );
    let saved = import::read(&export::write(&imported.document).bytes).unwrap();
    assert!(reported(&saved.report.skipped));
    let again = gradient_style(&saved.document)
        .fill_gradient
        .clone()
        .unwrap();
    assert_eq!(
        (again.start, again.length, again.angle),
        (fill.start, fill.length, fill.angle)
    );
}
