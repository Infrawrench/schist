//! Gradient swatches and gradient fills. A real InDesign export fills a
//! rectangle with a two-stop linear gradient whose GradientFillStart is the
//! rectangle's own left-bottom anchor and whose length is its width; the
//! fill is typed with that start in the rectangle's path coordinates.
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
