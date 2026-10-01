use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, CharacterStyle, History, Ink, LayoutObject, Rect};

#[test]
fn named_tints_retain_native_base_references_and_inherited_percentages() {
    for base in [
        Ink::cmyk("Base / 青", [0.25, 0.5, 0.75, 1.0]),
        Ink::spot("Spot / 青", [50.0, 20.0, -30.0]),
    ] {
        for value in [0.0, 0.125, 0.5, 1.0] {
            let tint = base.named_tint("Tint / 空 & light", value).unwrap();
            let mut doc = schist_layout::blank_a4();
            doc.inks.extend([base.clone(), tint.clone()]);
            authoring::rectangle(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(1.0, 2.0, 30.0, 40.0),
                authoring::Paint::filled(&tint.name),
            )
            .unwrap();
            if let LayoutObject::Shape { tints, stroke, .. } = &mut doc.objects[0].object {
                tints.fill = 1.0;
                *stroke = Some(tint.clone());
            }
            doc.styles.add_character(CharacterStyle {
                name: "Tint type".into(),
                fill: Some(tint.clone()),
                fill_tint: None,
                ..Default::default()
            });
            for _ in 0..5 {
                let encoded = export::write(&doc);
                let package = container::read(&encoded.bytes).unwrap();
                let root = xml::parse(package.text("Resources/Graphic.xml").unwrap()).unwrap();
                let native = root.find_all("Tint");
                assert_eq!(native.len(), 1);
                assert_eq!(native[0].number("TintValue"), Some(value * 100.0));
                let colors = root.find_all("Color");
                let referenced = colors
                    .iter()
                    .find(|c| c.attr("Self") == native[0].attr("BaseColor"))
                    .unwrap();
                assert_eq!(referenced.attr("Name"), Some(base.name.as_str()));
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
                assert_eq!(fill, &tint);
                assert_eq!(stroke, &tint);
                assert_eq!(tints.fill, 1.0);
                assert_eq!(
                    doc.styles.character("Tint type").unwrap().fill.as_ref(),
                    Some(&tint)
                );
                assert_eq!(doc.inks.iter().filter(|ink| ink.tint.is_some()).count(), 1);
            }
        }
    }
}

#[test]
fn native_tint_resources_resolve_in_either_order_and_invalid_bases_or_values_are_reported() {
    for reverse in [false, true] {
        for (base_ref, value, valid) in [
            ("opaque-color", "25", true),
            ("opaque-color", "0", true),
            ("missing", "25", false),
            ("opaque-tint", "25", false),
            ("opaque-color", "-1", false),
            ("opaque-color", "NaN", false),
            ("opaque-color", "101", false),
        ] {
            let mut doc = schist_layout::blank_a4();
            authoring::rectangle(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(1.0, 2.0, 30.0, 40.0),
                authoring::Paint::filled("Black"),
            )
            .unwrap();
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            let base = r#"<Color Self="opaque-color" Name="Native base" Model="Spot" Space="Lab" ColorValue="50 20 -30"/>"#;
            let tint = format!(
                r#"<Tint Self="opaque-tint" Name="Native tint" BaseColor="{base_ref}" TintValue="{value}"/>"#
            );
            let resources = if reverse {
                format!("{tint}{base}")
            } else {
                format!("{base}{tint}")
            };
            let graphics = package
                .text("Resources/Graphic.xml")
                .unwrap()
                .replace("</idPkg:Graphic>", &format!("{resources}</idPkg:Graphic>"));
            package.insert("Resources/Graphic.xml", graphics.into_bytes());
            let path = package
                .names()
                .into_iter()
                .find(|p| p.starts_with("Spreads/"))
                .unwrap()
                .to_owned();
            let spread = package.text(&path).unwrap();
            let root = xml::parse(spread).unwrap();
            let fill = root.find_all("Polygon")[0].attr("FillColor").unwrap();
            let spread = spread.replace(
                &format!("FillColor=\"{fill}\""),
                "FillColor=\"opaque-tint\"",
            );
            package.insert(path, spread.into_bytes());
            let read = import::read(&container::write(&package.into_parts())).unwrap();
            let LayoutObject::Shape { fill, .. } = &read.document.objects[0].object else {
                panic!()
            };
            assert_eq!(fill.is_some(), valid, "{base_ref} {value}");
            if let Some(fill) = fill {
                assert_eq!(fill.tint.as_ref().unwrap().base_name, "Native base");
                assert_eq!(fill.tint_amount(), value.parse::<f32>().unwrap() / 100.0);
            } else {
                assert!(!read.report.skipped.is_empty());
            }
        }
    }
}
