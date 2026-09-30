use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, blank_a4, History, Ink, LayoutObject, Rect};

#[test]
fn native_fixture_color_references_keep_the_authored_cmyk_builds() {
    let bytes = include_bytes!("../../../fixtures/idml/shapes.idml");
    let package = container::read(bytes).unwrap();
    let resources = xml::parse(package.text("Resources/Graphic.xml").unwrap()).unwrap();
    let colors = resources.find_all("Color");
    let doc = import::read(bytes).unwrap().document;
    let mut checked = 0;
    let mut remaining: Vec<_> = doc.objects.iter().collect();
    for part in package.names().iter().filter(|n| n.starts_with("Spreads/")) {
        let spread = xml::parse(package.text(part).unwrap()).unwrap();
        fn shapes<'a>(element: &'a xml::Element, out: &mut Vec<&'a xml::Element>) {
            for child in &element.children {
                if child.name == "Polygon" {
                    out.push(child);
                } else if child.name == "Group" || child.name == "Spread" {
                    shapes(child, out);
                }
            }
        }
        let mut polygons = Vec::new();
        shapes(&spread, &mut polygons);
        for element in polygons {
            let Some(resource) = colors
                .iter()
                .find(|c| c.attr("Self") == element.attr("FillColor"))
            else {
                continue;
            };
            if resource.attr("Space") != Some("CMYK") {
                continue;
            }
            let name = element.attr("Name").unwrap_or_default();
            let expected: Vec<_> = xml::numbers(resource.attr("ColorValue").unwrap())
                .iter()
                .map(|v| v / 100.0)
                .collect();
            let index=remaining.iter().position(|object| {
                object.name == name && matches!(&object.object, LayoutObject::Shape {fill:Some(ink),..} if ink.source_cmyk.is_some_and(|v|v.as_slice()==expected))
            }).unwrap_or_else(||panic!("lost native color on {name}: {expected:?}"));
            remaining.remove(index);
            checked += 1;
        }
    }
    assert!(checked > 0, "fixture must exercise native CMYK fills");
}

#[test]
fn every_color_space_paint_and_opacity_survives_repeated_saves() {
    for ink in [
        Ink::process("RGB / 赤 & blue", [0.2, 0.4, 0.75]),
        Ink::cmyk("Rich / black", [0.6, 0.4, 0.3, 0.85]),
        Ink::spot("Green / 空", [60.0, -45.0, 35.0]),
    ] {
        for opacity in [0.0, 0.25, 1.0] {
            for (fo, so) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut doc = blank_a4();
                let id = authoring::rectangle(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(10.0, 20.0, 100.0, 50.0),
                    authoring::Paint::none(),
                )
                .unwrap();
                let placed = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
                placed.transparency = opacity;
                let LayoutObject::Shape {
                    fill,
                    stroke,
                    stroke_width,
                    fill_overprint,
                    stroke_overprint,
                    ..
                } = &mut placed.object
                else {
                    panic!()
                };
                *fill = Some(ink.clone());
                *stroke = Some(Ink::black());
                *stroke_width = 3.25;
                *fill_overprint = fo;
                *stroke_overprint = so;
                for _ in 0..4 {
                    doc = import::read(&export::write(&doc).bytes).unwrap().document;
                    let placed = &doc.objects[0];
                    assert_eq!(placed.transparency, opacity);
                    let LayoutObject::Shape {
                        fill: Some(fill),
                        stroke: Some(stroke),
                        stroke_width,
                        fill_overprint,
                        stroke_overprint,
                        ..
                    } = &placed.object
                    else {
                        panic!()
                    };
                    assert_eq!(fill.name, ink.name);
                    assert_eq!(fill.spot, ink.spot);
                    for (a, b) in fill.to_cmyk().iter().zip(ink.to_cmyk()) {
                        assert!((a - b).abs() < 1e-6);
                    }
                    for (a, b) in fill.preview_rgb.iter().zip(ink.preview_rgb) {
                        assert!((a - b).abs() < 1e-5);
                    }
                    assert_eq!(stroke.name, "Black");
                    assert_eq!(*stroke_width, 3.25);
                    assert_eq!((*fill_overprint, *stroke_overprint), (fo, so));
                }
            }
        }
    }
}

#[test]
fn direct_tints_preserve_ink_identity_opacity_and_native_percentages_on_every_save() {
    for ink in [
        Ink::black(),
        Ink::cmyk("Rich", [0.6, 0.4, 0.3, 0.8]),
        Ink::spot("Spot", [50.0, -20.0, 30.0]),
    ] {
        for tint in [0.0, 0.125, 0.5, 1.0] {
            let mut doc = blank_a4();
            authoring::rectangle(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(10.0, 10.0, 30.0, 30.0),
                authoring::Paint::none(),
            )
            .unwrap();
            let shape = &mut doc.objects[0];
            shape.transparency = 0.25;
            let LayoutObject::Shape {
                fill,
                stroke,
                tints,
                ..
            } = &mut shape.object
            else {
                panic!()
            };
            *fill = Some(ink.clone());
            *stroke = Some(ink.clone());
            tints.fill = tint;
            tints.stroke = 1.0 - tint;
            for _ in 0..4 {
                let encoded = export::write(&doc);
                let package = container::read(&encoded.bytes).unwrap();
                let spread = package
                    .names()
                    .into_iter()
                    .find(|p| p.starts_with("Spreads/"))
                    .unwrap();
                let root = xml::parse(package.text(spread).unwrap()).unwrap();
                let shapes = root.find_all("Polygon");
                assert_eq!(shapes[0].number("FillTint"), Some(tint * 100.0));
                assert_eq!(shapes[0].number("StrokeTint"), Some((1.0 - tint) * 100.0));
                doc = import::read(&encoded.bytes).unwrap().document;
                let LayoutObject::Shape {
                    fill: Some(fill),
                    stroke: Some(stroke),
                    tints,
                    ..
                } = &doc.objects[0].object
                else {
                    panic!()
                };
                assert_eq!(tints.fill, tint);
                assert_eq!(tints.stroke, 1.0 - tint);
                assert_eq!(fill.name, ink.name);
                assert_eq!(fill.spot, ink.spot);
                assert_eq!(fill.source_cmyk, ink.source_cmyk);
                assert_eq!(stroke, fill);
                assert_eq!(doc.objects[0].transparency, 0.25);
            }
        }
    }
}

#[test]
fn malformed_native_tints_are_reported_while_inherited_values_remain_unset() {
    for (raw, expected, warning) in [
        ("-1", 1.0, false),
        ("0", 0.0, false),
        ("37.5", 0.375, false),
        ("101", 1.0, true),
        ("-2", 1.0, true),
        ("NaN", 1.0, true),
        ("bad", 1.0, true),
    ] {
        let mut doc = blank_a4();
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(1.0, 2.0, 30.0, 40.0),
            authoring::Paint::filled("Black"),
        )
        .unwrap();
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let path = package
            .names()
            .into_iter()
            .find(|p| p.starts_with("Spreads/"))
            .unwrap()
            .to_owned();
        let xml = package
            .text(&path)
            .unwrap()
            .replace("FillTint=\"100\"", &format!("FillTint=\"{raw}\""));
        package.insert(path, xml.into_bytes());
        let imported = import::read(&container::write(&package.into_parts())).unwrap();
        let LayoutObject::Shape { tints, .. } = &imported.document.objects[0].object else {
            panic!()
        };
        assert_eq!(tints.fill, expected);
        assert_eq!(
            imported
                .report
                .skipped
                .iter()
                .any(|m| m.contains("FillTint")),
            warning
        );
    }
}
