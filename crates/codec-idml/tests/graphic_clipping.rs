use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    affine::{self, Affine},
    authoring, blank_a4, History, LayoutObject, Point, Rect, ShapePath,
};

fn outline(object: &LayoutObject) -> &ShapePath {
    let LayoutObject::GraphicFrame {
        clip_path: Some(path),
        ..
    } = object
    else {
        panic!("missing clip")
    };
    path
}
fn near(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.002 && (a.y - b.y).abs() < 0.002,
        "{a:?} != {b:?}"
    );
}

#[test]
fn curved_and_compound_frame_outlines_survive_native_saves_without_labels() {
    for compound in [false, true] {
        for outer in [
            Affine::IDENTITY,
            Affine::skew(0.4, -0.1),
            Affine::rotate(0.7),
            Affine::scale(-1.2, 0.8),
        ] {
            let mut doc = blank_a4();
            authoring::graphic_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(123.0, 117.0, 150.0, 80.0),
                "art.png",
                false,
            )
            .unwrap();
            let object = &mut doc.objects[0];
            object.transform = outer;
            let LayoutObject::GraphicFrame {
                clip_path,
                image_transform,
                ..
            } = &mut object.object
            else {
                panic!()
            };
            let mut shape = ShapePath::ellipse(1.0, 1.0);
            if compound {
                let mut inner = ShapePath::ellipse(0.4, 0.4);
                inner.map_points(|p| p + Point::new(0.3, 0.3));
                inner.subpaths[0].points.reverse();
                inner.subpaths[0].handles.reverse();
                for h in &mut inner.subpaths[0].handles {
                    std::mem::swap(&mut h.incoming, &mut h.outgoing);
                }
                shape.subpaths.extend(inner.subpaths);
            }
            *clip_path = Some(shape);
            *image_transform = Affine::rotate(-0.4).around(0.5, 0.5);
            let original = object.clone();
            for _ in 0..6 {
                let encoded = export::write(&doc);
                let mut package = container::read(&encoded.bytes).unwrap();
                let spread = package
                    .names()
                    .into_iter()
                    .find(|n| n.starts_with("Spreads/"))
                    .unwrap()
                    .to_owned();
                let mut text = package.text(&spread).unwrap().to_owned();
                let root = xml::parse(&text).unwrap();
                let frame = root.find("Rectangle").unwrap();
                assert_eq!(
                    frame
                        .child("Properties")
                        .unwrap()
                        .child("PathGeometry")
                        .unwrap()
                        .children_named("GeometryPathType")
                        .count(),
                    1 + usize::from(compound)
                );
                // Metadata must not be necessary for native cubic handles.
                let start = text.find("<Label>").unwrap();
                let end = start + text[start..].find("</Label>").unwrap() + 8;
                text.replace_range(start..end, "");
                package.insert(spread, text.into_bytes());
                doc = import::read(&container::write(&package.into_parts()))
                    .unwrap()
                    .document;
                let actual = &doc.objects[0];
                for (a, b) in outline(&original.object)
                    .subpaths
                    .iter()
                    .zip(&outline(&actual.object).subpaths)
                {
                    assert_eq!(a.closed, b.closed);
                    assert_eq!(a.points.len(), b.points.len());
                    let placed_point = |o: &schist_layout::PlacedObject, p: Point| {
                        affine::point(
                            o.content_transform(),
                            Point::new(
                                o.bounds.x + p.x * o.bounds.width,
                                o.bounds.y + p.y * o.bounds.height,
                            ),
                        )
                    };
                    for i in 0..a.points.len() {
                        let ah = a.handles_at(i);
                        let bh = b.handles_at(i);
                        for (p, q) in [
                            (a.points[i], b.points[i]),
                            (ah.incoming.unwrap(), bh.incoming.unwrap()),
                            (ah.outgoing.unwrap(), bh.outgoing.unwrap()),
                        ] {
                            near(placed_point(&original, p), placed_point(actual, q));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn public_oval_image_fixture_retains_curves_and_exact_metadata_on_repeated_saves() {
    let mut doc = import::read(include_bytes!("../../../fixtures/idml/placeholders.idml"))
        .unwrap()
        .document;
    let object = doc
        .objects
        .iter()
        .find(|o| o.name == "Placeholder in Shape<PH><PACM>")
        .unwrap();
    let name = object.name.clone();
    let original = object.object.clone();
    let path = outline(&original);
    assert_eq!(path.subpaths.len(), 1);
    assert_eq!(path.subpaths[0].points.len(), 4);
    assert!(path.subpaths[0]
        .handles
        .iter()
        .all(|h| h.incoming.is_some() && h.outgoing.is_some()));
    near(path.bounds().origin(), Point::ZERO);
    near(
        Point::new(path.bounds().width, path.bounds().height),
        Point::new(1.0, 1.0),
    );
    for _ in 0..6 {
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(
            doc.objects.iter().find(|o| o.name == name).unwrap().object,
            original
        );
    }
}
