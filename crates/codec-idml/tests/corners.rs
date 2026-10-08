//! Corner options are read and saved corner by corner, as InDesign reads
//! them, with the rectangle saved plain so nothing is rounded twice. Corners
//! Schist leaves square are reported: decorative ones, and corners on an
//! outline with corner points that is not an upright rectangle.
use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, blank_a4, CornerShape, History, LayoutDocument, LayoutObject, PlacedObject, Rect,
};

const ROUNDED: &str = r#"TopLeftCornerOption="RoundedCorner" TopLeftCornerRadius="9" TopRightCornerOption="BevelCorner" TopRightCornerRadius="4" BottomLeftCornerOption="InsetCorner" BottomRightCornerOption="None" BottomRightCornerRadius="12""#;

/// A package holding one shape of `kind` whose element carries `attributes`.
fn package(kind: authoring::ShapeKind, attributes: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        kind,
        authoring::Paint::filled("Black"),
    )
    .unwrap();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let spread = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let text =
        package
            .text(&spread)
            .unwrap()
            .replacen("<Polygon ", &format!("<Polygon {attributes} "), 1);
    package.insert(&spread, text.into_bytes());
    container::write(&package.into_parts())
}

fn shape(doc: &LayoutDocument) -> &PlacedObject {
    doc.objects
        .iter()
        .find(|o| matches!(o.object, LayoutObject::Shape { .. }))
        .unwrap()
}

fn limits(bytes: &[u8]) -> bool {
    import::read(bytes)
        .unwrap()
        .report
        .skipped
        .iter()
        .any(|s| s.contains("unsupported categories"))
}

#[test]
fn corners_are_read_and_saved_corner_by_corner() {
    let bytes = package(authoring::ShapeKind::Rectangle, ROUNDED);
    assert!(!limits(&bytes));
    let mut doc = import::read(&bytes).unwrap().document;
    for _ in 0..2 {
        let shape = shape(&doc);
        let corners = doc.styles.object_paint(shape).corners.expect("corners");
        assert_eq!(
            corners.shapes,
            [
                CornerShape::Rounded,
                CornerShape::Bevel,
                CornerShape::None,
                CornerShape::Inset,
            ]
        );
        // A corner without a radius takes InDesign's 12 pt.
        assert_eq!(corners.radii, [9.0, 4.0, 12.0, 12.0]);
        // The rectangle itself stays plain.
        let LayoutObject::Shape { path, .. } = &shape.object else {
            panic!("a shape");
        };
        assert_eq!(path.subpaths[0].points.len(), 4);
        let saved = export::write(&doc).bytes;
        let package = container::read(&saved).unwrap();
        let spread = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Spreads/"))
            .unwrap()
            .to_owned();
        let xml = package.text(&spread).unwrap();
        assert!(
            xml.contains(r#"TopLeftCornerOption="RoundedCorner""#),
            "{xml}"
        );
        assert!(xml.contains(r#"BottomLeftCornerRadius="12""#), "{xml}");
        assert_eq!(xml.matches("<PathPointType ").count(), 4);
        doc = import::read(&saved).unwrap().document;
    }
}

#[test]
fn the_older_uniform_corner_option_leaves_corners_square() {
    let bytes = package(
        authoring::ShapeKind::Rectangle,
        r#"CornerOption="RoundedCorner" CornerRadius="9""#,
    );
    assert!(!limits(&bytes));
    let doc = import::read(&bytes).unwrap().document;
    assert_eq!(doc.styles.object_paint(shape(&doc)).corners, None);
}

#[test]
fn corners_schist_leaves_square_are_reported() {
    let fancy = package(
        authoring::ShapeKind::Rectangle,
        r#"TopLeftCornerOption="FancyCorner" TopLeftCornerRadius="9""#,
    );
    assert!(limits(&fancy));
    // Kept for saving all the same.
    let doc = import::read(&fancy).unwrap().document;
    let corners = doc.styles.object_paint(shape(&doc)).corners.unwrap();
    assert_eq!(corners.shapes[0], CornerShape::Fancy);

    // A triangle has corner points to round; an oval has none.
    assert!(limits(&package(
        authoring::ShapeKind::Polygon { sides: 3 },
        ROUNDED
    )));
    assert!(!limits(&package(authoring::ShapeKind::Ellipse, ROUNDED)));

    // A value the specification does not name is reported and read square.
    let odd = package(
        authoring::ShapeKind::Rectangle,
        r#"TopLeftCornerOption="Rounded" TopLeftCornerRadius="9""#,
    );
    let imported = import::read(&odd).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|s| s.contains("TopLeftCornerOption")));
    let corners = imported
        .document
        .styles
        .object_paint(shape(&imported.document))
        .corners
        .unwrap();
    assert!(corners.square());
}
