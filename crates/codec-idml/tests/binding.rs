//! Binding is a spread property; applying/moving a parent cannot move its spine.
//! These cases exercise the public XML rules, not an external application's UI.
use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    affine::Affine,
    authoring,
    parents::{ParentPlacement, ParentSheet},
    structure, History, LayoutDocument, Page, PageBinding, ParentObject, ParentPage, Point, Rect,
    Spread,
};

fn document(count: usize, binding: usize, direction: PageBinding) -> LayoutDocument {
    let mut doc = LayoutDocument::new(
        (0..count)
            .map(|i| Page::new(format!("page {i}"), 200.0 + i as f32 * 10.0, 300.0))
            .collect(),
    );
    doc.facing_pages = true;
    doc.page_binding = direction;
    let mut pages: Vec<_> = (0..count).collect();
    if direction == PageBinding::RightToLeft {
        pages.reverse();
    }
    doc.spreads = vec![Spread {
        pages,
        binding_location: Some(binding),
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    let mut objects = Vec::new();
    for (sheet, name) in ["left", "right"].into_iter().enumerate() {
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 30.0, 40.0, 50.0),
            authoring::Paint::none(),
        )
        .unwrap();
        let mut object = doc.objects.pop().unwrap();
        object.page = sheet;
        object.name = name.into();
        objects.push(ParentObject {
            object,
            overridden_on: Vec::new(),
        });
    }
    let parent = ParentPage {
        name: "A".into(),
        sheets: (0..2)
            .map(|i| ParentSheet {
                page: Page::new("A", 200.0, 300.0),
                origin: Point::new((i as f32 - 1.0) * 200.0, -150.0),
                source: None,
            })
            .collect(),
        placements: (0..count)
            .map(|page| ParentPlacement {
                page,
                sheet: usize::from(!doc.page_is_left(page)),
                transform: Affine::translate(page as f32, page as f32 * 2.0),
                visible: true,
            })
            .collect(),
        applied_to: (0..count).collect(),
        based_on: None,
        objects,
        hidden: false,
    };
    doc.parents.push(parent);
    for page in &mut doc.pages {
        page.master = Some(0);
    }
    for page in 0..count {
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(80.0, 90.0, 15.0, 20.0),
            authoring::Paint::none(),
        )
        .unwrap();
        doc.objects.last_mut().unwrap().name = format!("content {page}");
    }
    doc
}

fn assert_artwork(doc: &LayoutDocument) {
    for page in 0..doc.pages.len() {
        let label = &doc.pages[page].name;
        let original: usize = label.strip_prefix("page ").unwrap().parse().unwrap();
        let objects = doc.page_objects(page);
        assert_eq!(objects.len(), 2);
        let inherited = objects
            .iter()
            .find(|o| o.name == "left" || o.name == "right")
            .unwrap();
        assert_eq!(
            inherited.name,
            if doc.page_is_left(page) {
                "left"
            } else {
                "right"
            }
        );
        let bounds = inherited.visual_bounds();
        assert!((bounds.x - 20.0 - original as f32).abs() < 0.005);
        assert!((bounds.y - 30.0 - original as f32 * 2.0).abs() < 0.005);
        let direct = objects
            .iter()
            .find(|o| o.name == format!("content {original}"))
            .unwrap();
        assert_eq!(direct.bounds, Rect::new(80.0, 90.0, 15.0, 20.0));
    }
}

#[test]
fn every_spine_position_keeps_native_reading_order_geometry_and_parent_side() {
    for direction in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
        for count in 1..=5 {
            for binding in 0..=count {
                let mut doc = document(count, binding, direction);
                let gutter = if count > 1 && binding % 2 == 1 {
                    12.0
                } else {
                    0.0
                };
                doc.spreads[0].gutter = gutter;
                for _ in 0..4 {
                    assert_artwork(&doc);
                    let expected_pages = doc.pages.clone();
                    let written = export::write(&doc);
                    let package = container::read(&written.bytes).unwrap();
                    let path = package
                        .names()
                        .into_iter()
                        .find(|p| p.starts_with("Spreads/"))
                        .unwrap();
                    let root = xml::parse(package.text(path).unwrap()).unwrap();
                    let spread = root.find("Spread").unwrap();
                    assert_eq!(
                        spread.attr("BindingLocation"),
                        Some(binding.to_string().as_str())
                    );
                    let elements: Vec<_> = spread.children_named("Page").collect();
                    for (index, element) in elements.iter().enumerate() {
                        assert_eq!(element.attr("Name"), Some(format!("page {index}").as_str()));
                        let coords = xml::numbers(element.attr("ItemTransform").unwrap());
                        let page = &doc.pages[index];
                        let left = coords[4] + page.width / 2.0 < 0.0;
                        assert_eq!(left, doc.page_is_left(index));
                    }
                    doc = import::read(&written.bytes).unwrap().document;
                    assert_eq!(doc.pages, expected_pages);
                    assert_eq!(doc.page_binding, direction);
                    assert_eq!(doc.spreads[0].binding_location, Some(binding));
                    assert_eq!(doc.spreads[0].gutter, gutter);
                    // Pasteboard origins must advance physically, including RTL.
                    let spread = &doc.spreads[0];
                    for (position, pair) in spread.pages.windows(2).enumerate() {
                        let a = spread.page_origin(&doc.pages, position);
                        let b = spread.page_origin(&doc.pages, position + 1);
                        assert_eq!(b.x - a.x, doc.pages[pair[0]].width + gutter);
                    }
                }
            }
        }
    }
}

#[test]
fn every_page_move_reselects_parent_side_in_one_undo_step_and_survives_native_saves() {
    for direction in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
        for binding in 0..=4 {
            let before = document(4, binding, direction);
            for from in 0..4 {
                for to in 0..4 {
                    if from == to {
                        continue;
                    }
                    let mut doc = before.clone();
                    let mut history = History::default();
                    assert!(structure::move_page(&mut doc, &mut history, from, to));
                    assert_artwork(&doc);
                    assert_eq!(doc.spreads, before.spreads);
                    let after = doc.clone();
                    assert_eq!(history.undo_depth(), 1);
                    assert!(history.undo(&mut doc));
                    assert_eq!(doc, before);
                    assert!(history.redo(&mut doc));
                    assert_eq!(doc, after);
                    for _ in 0..3 {
                        doc = import::read(&export::write(&doc).bytes).unwrap().document;
                        assert_artwork(&doc);
                        assert_eq!(doc.pages, after.pages);
                    }
                }
            }
        }
    }
}

#[test]
fn removing_a_page_preserves_every_surviving_pages_side_and_undo_restores_the_spine() {
    for direction in [PageBinding::LeftToRight, PageBinding::RightToLeft] {
        for binding in 0..=4 {
            let before = document(4, binding, direction);
            for page in 0..4 {
                let mut doc = before.clone();
                let mut history = History::default();
                assert!(structure::remove_page(&mut doc, &mut history, page));
                assert_artwork(&doc);
                let after = doc.clone();
                for _ in 0..3 {
                    doc = import::read(&export::write(&doc).bytes).unwrap().document;
                    assert_artwork(&doc);
                    assert_eq!(doc.pages, after.pages);
                }
                doc = after;
                assert_eq!(history.undo_depth(), 1);
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
            }
        }
    }
}
