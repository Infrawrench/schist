use schist_codec_idml::{container, export, import};
use schist_layout::{authoring, blank_a4, History, LayoutObject, Point, ShapePath};

#[test]
fn every_contour_and_handle_survives_repeated_native_idml_roundtrips() {
    for closed in [false, true] {
        let mut doc = blank_a4();
        let mut path = ShapePath::ellipse(234.5, 98.75);
        let mut second = ShapePath::ellipse(56.25, 19.5);
        second.map_points(|p| p + Point::new(83.0, 33.0));
        path.subpaths.extend(second.subpaths);
        path.subpaths[1].closed = closed;
        path.map_points(|p| p + Point::new(73.5, 119.25));
        let id = authoring::path_shape(
            &mut doc,
            &mut History::default(),
            0,
            path.clone(),
            authoring::Paint::none(),
        )
        .unwrap();
        let original = doc.object(id).unwrap().bounds;
        for _ in 0..8 {
            let encoded = export::write(&doc);
            let package = container::read(&encoded.bytes).unwrap();
            let spread = package
                .names()
                .into_iter()
                .find(|name| name.starts_with("Spreads/"))
                .unwrap();
            let xml = package.text(spread).unwrap();
            assert_eq!(xml.matches("<GeometryPathType ").count(), 2);
            assert_eq!(xml.matches("LeftDirection=").count(), 8);
            assert_eq!(xml.matches("RightDirection=").count(), 8);
            doc = import::read(&encoded.bytes).unwrap().document;
            let object = &doc.objects[0];
            for (a, b) in [
                (object.bounds.x, original.x),
                (object.bounds.y, original.y),
                (object.bounds.width, original.width),
                (object.bounds.height, original.height),
            ] {
                assert!(
                    (a - b).abs() < 0.001,
                    "repeated save changed bounds: {a} {b}"
                );
            }
            let LayoutObject::Shape { path: read, .. } = &object.object else {
                panic!("not a path")
            };
            assert_eq!(read.subpaths.len(), 2);
            for (a, b) in path.subpaths.iter().zip(&read.subpaths) {
                assert_eq!(a.closed, b.closed);
                assert_eq!(a.points.len(), b.points.len());
                for i in 0..a.points.len() {
                    let ah = a.handles_at(i);
                    let bh = b.handles_at(i);
                    for (a, b) in [
                        (a.points[i], b.points[i]),
                        (ah.incoming.unwrap(), bh.incoming.unwrap()),
                        (ah.outgoing.unwrap(), bh.outgoing.unwrap()),
                    ] {
                        let b = b + object.bounds.origin();
                        assert!(
                            (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001,
                            "handle drift: {a:?} {b:?}"
                        );
                    }
                }
            }
        }
    }
}
