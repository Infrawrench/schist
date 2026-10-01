//! Native parent sheets/overlays/overrides, independent of private labels.
use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    affine::{self, Affine},
    blank_a4, ParentPage, Point, Rect, Spread,
};

fn shape(id: &str, name: &str, x: f32, y: f32) -> String {
    format!(
        r#"<Rectangle Self="{id}" Name="{name}" ItemTransform="1 0 0 1 {x} {y}" FillColor="Color/Black"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="0 0"/><PathPointType Anchor="40 0"/><PathPointType Anchor="40 25"/><PathPointType Anchor="0 25"/></PathPointArray></GeometryPathType></PathGeometry></Properties></Rectangle>"#
    )
}
fn native_overlay(local: Affine, origin: Point) -> String {
    let matrix = Affine::translate(origin.x, origin.y)
        .then(&local)
        .then(&Affine::translate(-origin.x, -origin.y));
    [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
        .map(|v| v.to_string())
        .join(" ")
}
fn near(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.005 && (a.y - b.y).abs() < 0.005,
        "{a:?} != {b:?}"
    );
}

#[test]
fn native_parent_sheets_overlays_and_overrides_survive_repeated_saves() {
    for overlay in [
        Affine::IDENTITY,
        Affine::translate(5.0, 7.0),
        Affine::rotate(0.2),
        Affine::skew(0.3, -0.1),
        Affine::scale(-1.0, 1.0).around(100.0, 150.0),
    ] {
        for show in [true, false] {
            let mut doc = blank_a4();
            doc.pages = vec![schist_layout::Page::new("1", 200.0, 300.0); 3];
            doc.spreads = vec![
                Spread::single(0),
                Spread {
                    pages: vec![1, 2],
                    binding_location: None,
                    gutter: 0.0,
                    origin: Point::ZERO,
                },
            ];
            doc.parents.push(ParentPage {
                name: "A-Parent".into(),
                applied_to: vec![0, 1, 2],
                based_on: None,
                objects: Vec::new(),
                hidden: false,
                sheets: Vec::new(),
                placements: Vec::new(),
            });
            for p in &mut doc.pages {
                p.master = Some(0);
            }
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            let master = package
                .names()
                .into_iter()
                .find(|p| p.starts_with("MasterSpreads/"))
                .unwrap()
                .to_owned();
            let master_id = xml::parse(package.text(&master).unwrap())
                .unwrap()
                .find("MasterSpread")
                .unwrap()
                .attr("Self")
                .unwrap()
                .to_owned();
            package.insert(master.clone(),format!(r#"<idPkg:MasterSpread><MasterSpread Self="{master_id}" Name="A-Parent" PageCount="2"><Page Self="ml" Name="A" GeometricBounds="0 0 300 200" ItemTransform="1 0 0 1 -200 -150"/><Page Self="mr" Name="A" GeometricBounds="0 0 300 200" ItemTransform="1 0 0 1 0 -150"/>{}{}</MasterSpread></idPkg:MasterSpread>"#,shape("left","left",-180.0,-130.0),shape("right","right",20.0,-130.0)).into_bytes());
            let spreads: Vec<_> = package
                .names()
                .into_iter()
                .filter(|p| p.starts_with("Spreads/"))
                .map(str::to_owned)
                .collect();
            for (i, path) in spreads.iter().enumerate() {
                let pages = if i == 0 { vec![0] } else { vec![1, 2] };
                let mut text = format!(
                    r#"<idPkg:Spread><Spread Self="s{i}" ShowMasterItems="{}" BindingLocation="{}">"#,
                    i != 0 || show,
                    usize::from(i == 1)
                );
                for page in pages {
                    let x = if page == 1 { -200.0 } else { 0.0 };
                    let origin = Point::new(x, -150.0);
                    let matrix = native_overlay(overlay, origin);
                    text.push_str(&format!(r#"<Page Self="p{page}" AppliedMaster="{master_id}" Name="{}" OverrideList="{}" GeometricBounds="0 0 300 200" ItemTransform="1 0 0 1 {x} -150" MasterPageTransform="{matrix}"/>"#,page+1,if page==2 {"right"} else {""}));
                }
                if i == 1 {
                    text.push_str(&shape("replacement", "replacement", 35.0, -95.0));
                }
                text.push_str("</Spread></idPkg:Spread>");
                package.insert(path.clone(), text.into_bytes());
            }
            doc = import::read(&container::write(&package.into_parts()))
                .unwrap()
                .document;
            for _ in 0..5 {
                assert_eq!(doc.parents.len(), 1);
                assert_eq!(doc.parents[0].sheets.len(), 2);
                assert_eq!(doc.parents[0].objects.len(), 2);
                for page in 0..3 {
                    let objects = doc.page_objects(page);
                    assert_eq!(objects.len(), usize::from(page != 0 || show));
                    if let Some(object) = objects.first() {
                        if page == 2 {
                            assert_eq!(object.name, "replacement");
                            assert_eq!(object.bounds, Rect::new(35.0, 55.0, 40.0, 25.0));
                        } else {
                            assert_eq!(object.name, if page == 0 { "right" } else { "left" });
                            for p in affine::corners(Rect::new(20.0, 20.0, 40.0, 25.0)) {
                                near(
                                    affine::point(object.content_transform(), p),
                                    affine::point(overlay, p),
                                );
                            }
                        }
                    }
                }
                let saved = export::write(&doc);
                let package = container::read(&saved.bytes).unwrap();
                let master = package
                    .names()
                    .into_iter()
                    .find(|p| p.starts_with("MasterSpreads/"))
                    .unwrap();
                let root = xml::parse(package.text(master).unwrap()).unwrap();
                let master = root.find("MasterSpread").unwrap();
                assert_eq!(master.attr("PageCount"), Some("2"));
                assert_eq!(master.children_named("Page").count(), 2);
                let ids: Vec<_> = master
                    .children
                    .iter()
                    .filter(|e| matches!(e.name.as_str(), "Rectangle" | "Polygon"))
                    .map(|r| r.attr("Self").unwrap())
                    .collect();
                for spread in package
                    .names()
                    .into_iter()
                    .filter(|p| p.starts_with("Spreads/"))
                {
                    let root = xml::parse(package.text(spread).unwrap()).unwrap();
                    for page in root.find("Spread").unwrap().children_named("Page") {
                        for reference in page
                            .attr("OverrideList")
                            .unwrap_or_default()
                            .split_whitespace()
                        {
                            assert!(ids.contains(&reference));
                        }
                    }
                }
                doc = import::read(&saved.bytes).unwrap().document;
            }
        }
    }
}

fn hierarchy() -> schist_layout::LayoutDocument {
    use schist_layout::parents::{ParentPlacement, ParentSheet, ParentSource};
    use schist_layout::{authoring, History, ParentObject};
    let mut doc = blank_a4();
    doc.pages.push(doc.pages[0].clone());
    doc.spreads.push(Spread::single(1));
    let mut items = Vec::new();
    for name in ["base kept", "base overridden", "middle", "top"] {
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 30.0, 40.0, 25.0),
            authoring::Paint::none(),
        )
        .unwrap();
        let mut object = doc.objects.pop().unwrap();
        object.name = name.into();
        items.push(ParentObject {
            object,
            overridden_on: Vec::new(),
        });
    }
    let base_override = items[1].object.id;
    for index in 0..3 {
        let source = (index > 0).then(|| ParentSource {
            parent: index - 1,
            sheet: 0,
            transform: if index == 1 {
                Affine::translate(13.0, 7.0)
            } else {
                Affine::rotate(0.3)
            },
            visible: true,
            overrides: if index == 1 {
                vec![base_override]
            } else {
                Vec::new()
            },
        });
        doc.parents.push(ParentPage {
            name: format!("parent {index}"),
            applied_to: if index == 2 { vec![0, 1] } else { Vec::new() },
            based_on: None,
            hidden: false,
            sheets: vec![ParentSheet {
                page: doc.pages[0].clone(),
                origin: Point::new(-300.0 + index as f32 * 70.0, -400.0 + index as f32 * 20.0),
                source,
            }],
            placements: if index == 2 {
                vec![ParentPlacement {
                    page: 1,
                    sheet: 0,
                    transform: Affine::skew(0.2, -0.1),
                    visible: true,
                }]
            } else {
                Vec::new()
            },
            objects: if index == 0 {
                items.drain(..2).collect()
            } else {
                vec![items.remove(0)]
            },
        });
    }
    doc.parents[1].objects[0].overridden_on.push(1);
    for page in &mut doc.pages {
        page.master = Some(2);
    }
    doc
}

#[test]
fn parent_hierarchies_compose_overlays_and_keep_each_override_scope() {
    let mut doc = hierarchy();
    let expected: Vec<Vec<_>> = (0..2)
        .map(|page| {
            doc.page_objects(page)
                .into_iter()
                .map(|o| o.into_owned())
                .collect()
        })
        .collect();
    assert_eq!(
        expected[0]
            .iter()
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>(),
        ["base kept", "middle", "top"]
    );
    assert_eq!(
        expected[1]
            .iter()
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>(),
        ["base kept", "top"]
    );
    for _ in 0..6 {
        let imported = import::read(&export::write(&doc).bytes).unwrap();
        assert!(!imported
            .report
            .skipped
            .iter()
            .any(|m| m == schist_i18n::t("design.idml_parent_cycle")));
        doc = imported.document;
        for (page, expected) in expected.iter().enumerate() {
            let objects = doc.page_objects(page);
            assert_eq!(objects.len(), expected.len());
            for (actual, original) in objects.iter().zip(expected) {
                assert_eq!(actual.name, original.name);
                for p in affine::corners(original.bounds) {
                    near(
                        affine::point(actual.content_transform(), p),
                        affine::point(original.content_transform(), p),
                    );
                }
            }
        }
    }
}

#[test]
fn invalid_parent_cycles_and_override_references_are_reported() {
    let mut doc = hierarchy();
    doc.parents[0].sheets[0].source = Some(schist_layout::parents::ParentSource {
        parent: 2,
        sheet: 0,
        transform: Affine::IDENTITY,
        visible: true,
        overrides: Vec::new(),
    });
    let imported = import::read(&export::write(&doc).bytes).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|m| m == schist_i18n::t("design.idml_parent_cycle")));
    let items = imported.document.page_objects(0);
    let ids: std::collections::BTreeSet<_> = items.iter().map(|o| o.id).collect();
    assert_eq!(
        items.len(),
        ids.len(),
        "a cycle must never duplicate template objects"
    );
    assert!(items.len() <= 4);
    let mut package = container::read(&export::write(&hierarchy()).bytes).unwrap();
    let spread = package
        .names()
        .into_iter()
        .find(|p| p.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let text = package
        .text(&spread)
        .unwrap()
        .replace("OverrideList=\"", "OverrideList=\"unknown-item ");
    package.insert(spread, text.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|m| m == &schist_i18n::tf!("design.idml_parent_override", item = "unknown-item")));
}

#[test]
fn revisiting_a_different_sheet_of_a_parent_is_not_a_cycle() {
    use schist_layout::parents::{ParentPlacement, ParentSheet, ParentSource};
    let mut doc = hierarchy();
    doc.pages.truncate(1);
    doc.spreads = vec![Spread::single(0)];
    let mut top = doc.parents.pop().unwrap().objects.remove(0);
    top.object.page = 1;
    let width = doc.pages[0].width;
    doc.parents[0].objects.truncate(1);
    doc.parents[0].objects.push(top);
    doc.parents[0].sheets = (0..2)
        .map(|sheet| ParentSheet {
            page: doc.pages[0].clone(),
            origin: Point::new((sheet as f32 - 1.0) * width, 0.0),
            source: (sheet == 1).then_some(ParentSource {
                parent: 1,
                sheet: 0,
                transform: Affine::translate(10.0, 0.0),
                visible: true,
                overrides: Vec::new(),
            }),
        })
        .collect();
    doc.parents[1].sheets[0].origin = Point::new(-width, 0.0);
    doc.parents[1].sheets[0].source = Some(ParentSource {
        parent: 0,
        sheet: 0,
        transform: Affine::translate(20.0, 0.0),
        visible: true,
        overrides: Vec::new(),
    });
    doc.parents[0].applied_to = vec![0];
    doc.parents[0].placements = vec![ParentPlacement {
        page: 0,
        sheet: 1,
        transform: Affine::IDENTITY,
        visible: true,
    }];
    doc.pages[0].master = Some(0);
    for _ in 0..5 {
        let items = doc.page_objects(0);
        assert_eq!(
            items.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            ["base kept", "middle", "top"]
        );
        for (object, x) in items.iter().zip([50.0, 30.0, 20.0]) {
            assert!((object.visual_bounds().x - x).abs() < 0.005);
        }
        let back = import::read(&export::write(&doc).bytes).unwrap();
        assert!(!back
            .report
            .skipped
            .iter()
            .any(|w| w == schist_i18n::t("design.idml_parent_cycle")));
        doc = back.document;
    }
}
