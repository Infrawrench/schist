use schist_layout::{
    affine::{self, Affine},
    authoring::{self, Paint},
    compose_object,
    parents::{ParentPlacement, ParentSheet},
    pasteboard, History, LayoutDocument, Page, ParentObject, ParentPage, PasteboardView, Point,
    Rect, Spread, Story,
};

#[test]
fn crossing_text_keeps_its_source_composition_grid_and_only_moves_final_artwork() {
    let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 2]);
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        ..Spread::single(0)
    }];
    let text = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(75.0, 10.0, 65.0, 60.0),
    )
    .unwrap();
    doc.stories[text.story.0 as usize] = Story::from_text("Crossing é ffi across a spread", "Body");
    doc.pages[0].margins.top = 7.0;
    doc.pages[1].margins.top = 32.0;
    doc.grids.document.mode = schist_layout::GridMode::SnapToGrid;
    for transform in [
        Affine::IDENTITY,
        Affine::skew(0.3, 0.0),
        Affine::scale(1.2, 0.8),
    ] {
        doc.objects[0].transform = transform;
        let before = compose_object(&doc, &doc.objects[0]).unwrap();
        let artwork = doc.page_artwork(1, doc.pages[1].bleed_rect());
        let placed = artwork
            .iter()
            .find(|object| object.id == text.object)
            .unwrap();
        assert_eq!(placed.page, 0);
        assert_eq!(placed.bounds, doc.objects[0].bounds);
        assert_eq!(compose_object(&doc, placed).unwrap(), before);
        for point in affine::corners(placed.bounds) {
            let source = affine::point(doc.objects[0].content_transform(), point);
            let dest = affine::point(placed.content_transform(), point);
            assert!((dest.x - source.x + 100.0).abs() < 0.0001);
            assert!((dest.y - source.y).abs() < 0.0001);
        }
        for page in 0..2 {
            let board = pasteboard(
                &doc,
                &PasteboardView {
                    page: Some(page),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(board.pages.len(), 1);
            assert_eq!(board.objects().filter(|d| d.frame().is_some()).count(), 1);
            let texts: Vec<_> = board
                .objects()
                .filter_map(|d| match d {
                    schist_layout::Display::Text { start, end, .. } => Some((*start, *end)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                texts,
                before
                    .lines
                    .iter()
                    .map(|l| (l.start, l.end))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn parent_instances_cross_gutters_once_and_keep_overrides_scoped_to_the_source_page() {
    let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 3]);
    doc.spreads = vec![
        Spread {
            pages: vec![0, 1],
            ..Spread::single(0)
        },
        Spread::single(2),
    ];
    authoring::rectangle(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(80.0, 20.0, 40.0, 30.0),
        Paint::filled("Black"),
    )
    .unwrap();
    let object = doc.objects.pop().unwrap();
    let id = object.id;
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: vec![0, 1, 2],
        based_on: None,
        hidden: false,
        sheets: vec![ParentSheet {
            page: doc.pages[0].clone(),
            origin: Point::ZERO,
            source: None,
        }],
        placements: (0..3)
            .map(|page| ParentPlacement {
                page,
                sheet: 0,
                transform: Affine::IDENTITY,
                visible: true,
            })
            .collect(),
        objects: vec![ParentObject {
            object,
            overridden_on: vec![],
        }],
    });
    for page in &mut doc.pages {
        page.master = Some(0);
    }
    let expected = [vec![0], vec![0, 1], vec![2]];
    for (page, sources) in expected.iter().enumerate() {
        let artwork = doc.page_artwork(page, doc.pages[page].bleed_rect());
        assert_eq!(artwork.iter().map(|o| o.page).collect::<Vec<_>>(), *sources);
        assert!(artwork.iter().all(|o| o.id == id));
    }
    // Suppressing only the right-page instance must not remove the part of the
    // left-page instance that extends onto the right page.
    doc.parents[0].objects[0].overridden_on = vec![1];
    assert_eq!(
        doc.page_artwork(1, doc.pages[1].bleed_rect())
            .iter()
            .map(|o| o.page)
            .collect::<Vec<_>>(),
        vec![0]
    );
    doc.parents[0].objects[0].overridden_on.push(0);
    assert!(doc.page_artwork(1, doc.pages[1].bleed_rect()).is_empty());
    assert_eq!(doc.page_artwork(2, doc.pages[2].bleed_rect()).len(), 1);
}

#[test]
fn inherited_text_crossing_a_gutter_keeps_the_source_instances_baseline_grid() {
    let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 2]);
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        ..Spread::single(0)
    }];
    doc.pages[0].margins.top = 7.0;
    doc.pages[1].margins.top = 32.0;
    doc.grids.document.mode = schist_layout::GridMode::SnapToGrid;
    let text = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(75.0, 10.0, 65.0, 60.0),
    )
    .unwrap();
    doc.stories[text.story.0 as usize] = Story::from_text("Parent crossover é ffi", "Body");
    let object = doc.objects.pop().unwrap();
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: vec![0],
        based_on: None,
        hidden: false,
        sheets: vec![ParentSheet {
            page: doc.pages[0].clone(),
            origin: Point::ZERO,
            source: None,
        }],
        placements: vec![],
        objects: vec![ParentObject {
            object,
            overridden_on: vec![],
        }],
    });
    let own = doc.page_objects(0);
    let expected = compose_object(&doc, &own[0]).unwrap();
    let crossed = doc.page_artwork(1, doc.pages[1].bleed_rect());
    assert_eq!(crossed.len(), 1);
    assert_eq!(compose_object(&doc, &crossed[0]).unwrap(), expected);
    let mut wrong_page = crossed[0].clone().into_owned();
    wrong_page.page = 1;
    assert_ne!(
        compose_object(&doc, &wrong_page).unwrap(),
        expected,
        "test must distinguish the two destination grids"
    );
}
