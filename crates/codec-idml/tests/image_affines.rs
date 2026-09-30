use schist_codec_idml::{container, export, import, xml};
use schist_layout::{affine, graphics, LayoutObject, Point, Rect};

fn apply(m: [f32; 6], p: Point) -> Point {
    Point::new(
        m[0] * p.x + m[2] * p.y + m[4],
        m[1] * p.x + m[3] * p.y + m[5],
    )
}
fn matrix(element: &xml::Element) -> [f32; 6] {
    xml::numbers(element.attr("ItemTransform").unwrap())
        .try_into()
        .unwrap()
}
fn near(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.02 && (a.y - b.y).abs() < 0.02,
        "{a:?} != {b:?}"
    );
}

#[test]
fn independent_image_and_frame_transforms_survive_without_private_metadata() {
    for inner in [
        [1.2, 0.0, 0.0, 0.7, -12.0, -7.0],
        [0.0, 0.8, -1.2, 0.0, 100.0, -30.0],
        [-1.1, 0.2, 0.4, 0.6, 80.0, -20.0],
    ] {
        for outer in [
            [1.0, 0.0, 0.0, 1.0, 120.0, 140.0],
            [0.7, 0.3, -0.4, 1.2, 200.0, 160.0],
        ] {
            let mut package =
                container::read(&export::write(&schist_layout::blank_a4()).bytes).unwrap();
            let spread = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Spreads/"))
                .unwrap()
                .to_owned();
            let i = inner.map(|n| n.to_string()).join(" ");
            let o = outer.map(|n| n.to_string()).join(" ");
            package.insert(spread, format!(r#"<idPkg:Spread><Spread Self="s"><Page Self="p" GeometricBounds="0 0 842 595" ItemTransform="1 0 0 1 0 0"/><Rectangle Self="r" GeometricBounds="-20 -30 40 90" ItemTransform="{o}"><Image Self="i" ActualPpi="144 144" ItemTransform="{i}"><Properties><GraphicBounds Left="-7" Top="13" Right="193" Bottom="113"/></Properties><Link Self="l" LinkResourceURI="file:art.png" StoredState="Normal"/></Image></Rectangle></Spread></idPkg:Spread>"#).into_bytes());
            let mut bytes = container::write(&package.into_parts());
            for _ in 0..5 {
                let imported = import::read(&bytes).unwrap();
                assert!(!imported
                    .report
                    .skipped
                    .iter()
                    .any(|s| s == schist_i18n::t("design.idml_image_transform")));
                let doc = imported.document;
                let object = &doc.objects[0];
                let LayoutObject::GraphicFrame {
                    link,
                    fit,
                    crop,
                    scale,
                    image_transform,
                    ..
                } = &object.object
                else {
                    panic!()
                };
                let info = link.info.unwrap();
                let mapped = graphics::image_rect(
                    object.bounds,
                    (info.width, info.height),
                    info.dpi,
                    *crop,
                    *fit,
                    *scale,
                )
                .unwrap();
                let inner_map = graphics::image_affine(object.bounds, *image_transform).unwrap();
                let exported = export::write(&doc);
                assert!(
                    !exported.warnings.iter().any(|w| w
                        == &schist_i18n::tf!("design.idml_graphic_unwritten", name = object.name)),
                    "{:?}",
                    exported.warnings
                );
                let mut package = container::read(&exported.bytes).unwrap();
                let spread = package
                    .names()
                    .into_iter()
                    .find(|n| n.starts_with("Spreads/"))
                    .unwrap()
                    .to_owned();
                let text = package.text(&spread).unwrap().to_owned();
                let root = xml::parse(&text).unwrap();
                let frame = root.find("Rectangle").unwrap();
                let image = frame.child("Image").unwrap();
                let bounds = image.find("GraphicBounds").unwrap();
                for u in [0.0, 0.25, 0.5, 1.0] {
                    for v in [0.0, 0.5, 1.0] {
                        let expected = apply(
                            outer,
                            apply(inner, Point::new(-7.0 + u * 200.0, 13.0 + v * 100.0)),
                        );
                        let actual = affine::point(
                            object.content_transform(),
                            affine::point(
                                inner_map,
                                Point::new(
                                    mapped.x + u * mapped.width,
                                    mapped.y + v * mapped.height,
                                ),
                            ),
                        );
                        near(actual, expected);
                        let native = Point::new(
                            bounds.number("Right").unwrap() * u,
                            bounds.number("Bottom").unwrap() * v,
                        );
                        near(apply(matrix(frame), apply(matrix(image), native)), expected);
                    }
                }
                // A third-party save can discard our label. Native geometry must
                // still preserve every point, including a nonzero image origin.
                let start = text.find("<Label>").unwrap();
                let end = start + text[start..].find("</Label>").unwrap() + "</Label>".len();
                let mut text = text;
                text.replace_range(start..end, "");
                package.insert(spread, text.into_bytes());
                bytes = container::write(&package.into_parts());
            }
        }
    }
}

#[test]
fn fitting_intent_and_affine_metadata_survive_together() {
    for fit in [
        schist_layout::GraphicFit::Fill,
        schist_layout::GraphicFit::Contain,
        schist_layout::GraphicFit::Original,
        schist_layout::GraphicFit::Stretch,
    ] {
        for crop in [
            None,
            Some(Rect::new(0.2, 0.1, 0.5, 0.7)),
            Some(Rect::new(-0.4, -0.2, 1.8, 1.2)),
        ] {
            let mut doc = schist_layout::blank_a4();
            schist_layout::authoring::graphic_frame(
                &mut doc,
                &mut Default::default(),
                0,
                Rect::new(20.0, 30.0, 150.0, 100.0),
                "art.png",
                false,
            )
            .unwrap();
            let LayoutObject::GraphicFrame {
                fit: f,
                crop: c,
                image_transform,
                ..
            } = &mut doc.objects[0].object
            else {
                panic!()
            };
            *f = fit;
            *c = crop;
            *image_transform = affine::Affine::rotate(0.7).around(0.5, 0.5);
            let original = doc.objects[0].object.clone();
            for _ in 0..5 {
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
                assert_eq!(doc.objects[0].object, original);
            }
        }
    }
}
