use schist_codec_idml::{container, export, import};
use schist_layout::{blank_a4, LayerId, LayoutLayer, LayoutObject, Story};

#[test]
fn flattened_groups_inherit_visibility_and_nearest_layer_through_repeated_saves() {
    for depth in [1, 2, 5] {
        for hidden_level in 0..=depth + 1 {
            for master in [false, true] {
                let mut doc = blank_a4();
                doc.layers = vec![LayerId(0), LayerId(1), LayerId(2)];
                doc.layer_properties = (0..3)
                    .map(|id| LayoutLayer {
                        id: LayerId(id),
                        name: format!("Layer {id}"),
                        visible: id != 1,
                        locked: false,
                        ignore_wrap: false,
                    })
                    .collect();
                doc.stories.push(Story::from_text("Visible text", "Body"));
                let mut package = container::read(&export::write(&doc).bytes).unwrap();
                let story_id = schist_codec_idml::designmap::DesignPackage::open(&package)
                    .unwrap()
                    .listed_of(schist_codec_idml::designmap::PartKind::Story)[0]
                    .id
                    .clone();
                let mut items = String::new();
                for level in 0..depth {
                    // The closest group's layer beats its ancestors. Child
                    // objects may name a still nearer explicit layer.
                    let layer = if level + 1 == depth { 1 } else { 2 };
                    items.push_str(&format!(
                        r#"<Group Self="g{level}" ItemLayer="SchistLayer{layer}" Visible="{}">"#,
                        level != hidden_level
                    ));
                }
                for (name, extra, kind) in [
                    ("inherited", "", "Rectangle"),
                    ("explicit", "ItemLayer=\"SchistLayer2\"", "Rectangle"),
                    ("text", "ItemLayer=\"SchistLayer0\"", "TextFrame"),
                    ("graphic", "ItemLayer=\"SchistLayer0\"", "Rectangle"),
                ] {
                    items.push_str(&format!(r#"<{kind} Self="{name}" Name="{name}" {extra} Visible="{}" ParentStory="{story_id}" FillColor="Color/Black" GeometricBounds="20 20 60 120">"#, hidden_level != depth));
                    items.push_str(r#"<Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="20 20"/><PathPointType Anchor="120 20"/><PathPointType Anchor="120 60"/><PathPointType Anchor="20 60"/></PathPointArray></GeometryPathType></PathGeometry></Properties>"#);
                    if name == "graphic" {
                        items.push_str(r#"<Image Self="i" ActualPpi="72 72" ItemTransform="1 0 0 1 20 20"><Properties><GraphicBounds Left="0" Top="0" Right="100" Bottom="40"/></Properties><Link Self="link" LinkResourceURI="file:art.png" StoredState="Normal"/></Image>"#);
                    }
                    items.push_str(&format!("</{kind}>"));
                }
                items.push_str(&"</Group>".repeat(depth));
                let spread = package
                    .names()
                    .into_iter()
                    .find(|n| n.starts_with("Spreads/"))
                    .unwrap()
                    .to_owned();
                let mut master_attr = String::new();
                if master {
                    let map = package.text("designmap.xml").unwrap().replace(
                        "</Document>",
                        "<idPkg:MasterSpread src=\"MasterSpreads/visibility.xml\"/></Document>",
                    );
                    package.insert("designmap.xml", map.into_bytes());
                    package.insert("MasterSpreads/visibility.xml", format!(r#"<idPkg:MasterSpread><MasterSpread Self="m" Name="Master"><Page Self="mp" GeometricBounds="0 0 842 595"/>{items}</MasterSpread></idPkg:MasterSpread>"#).into_bytes());
                    master_attr = "AppliedMaster=\"m\"".into();
                    items.clear();
                }
                package.insert(spread, format!(r#"<idPkg:Spread><Spread Self="s"><Page Self="p" GeometricBounds="0 0 842 595" {master_attr}/>{items}</Spread></idPkg:Spread>"#).into_bytes());
                let mut doc = import::read(&container::write(&package.into_parts()))
                    .unwrap()
                    .document;
                for _ in 0..4 {
                    let objects: Vec<_> = if master {
                        doc.parents[0].objects.iter().map(|o| &o.object).collect()
                    } else {
                        doc.objects.iter().collect()
                    };
                    assert_eq!(objects.len(), 4);
                    for object in objects {
                        assert_eq!(object.hidden, hidden_level <= depth);
                        let expected = match object.name.as_str() {
                            "inherited" => 1,
                            "explicit" => 2,
                            _ => 0,
                        };
                        assert_eq!(doc.object_layer(object.id), LayerId(expected));
                        assert_eq!(
                            object.transparency, 1.0,
                            "visibility must not erase opacity"
                        );
                    }
                    assert_eq!(
                        doc.page_objects(0).len(),
                        if hidden_level <= depth { 0 } else { 3 }
                    );
                    doc = import::read(&export::write(&doc).bytes).unwrap().document;
                }
            }
        }
    }
}

#[test]
fn public_academic_hidden_parent_groups_never_paint_corner_squares() {
    let mut doc = import::read(include_bytes!(
        "../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"
    ))
    .unwrap()
    .document;
    for _ in 0..4 {
        let shapes: Vec<_> = doc
            .parents
            .iter()
            .flat_map(|p| &p.objects)
            .filter(|o| matches!(o.object.object, LayoutObject::Shape { .. }))
            .collect();
        assert_eq!(
            shapes.len(),
            24,
            "hidden source artwork must remain recoverable"
        );
        assert_eq!(shapes.iter().filter(|o| o.object.hidden).count(), 8);
        assert_eq!(
            shapes
                .iter()
                .filter(|o| !doc.layer_visible(doc.object_layer(o.object.id)))
                .count(),
            16
        );
        for page in 0..doc.pages.len() {
            assert!(doc
                .page_objects(page)
                .iter()
                .all(|o| !matches!(o.object, LayoutObject::Shape { .. })));
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}
