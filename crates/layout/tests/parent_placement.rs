use schist_layout::parents::{ParentPlacement, ParentSheet};
use schist_layout::{
    affine::{self, Affine},
    authoring, blank_a4, compose_object, History, ParentObject, ParentPage, Point, Rect, Story,
};

#[test]
fn overlays_transform_inherited_artwork_without_moving_shared_composition_boxes() {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 40.0, 130.0, 100.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text(
        "Shared text remains composed exactly once for this parent frame.",
        "Body",
    );
    doc.pages.push(doc.pages[0].clone());
    let mut object = doc.objects.remove(0);
    object.rotation = 17.0;
    object.transform = Affine::skew(0.2, -0.1);
    let original = object.clone();
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: vec![0, 1],
        based_on: None,
        hidden: false,
        sheets: vec![ParentSheet {
            source: None,
            page: doc.pages[0].clone(),
            origin: Point::ZERO,
        }],
        placements: Vec::new(),
        objects: vec![ParentObject {
            object,
            overridden_on: Vec::new(),
        }],
    });
    let original_lines = compose_object(&doc, &original).unwrap();
    for matrix in [
        Affine::IDENTITY,
        Affine::translate(20.0, -15.0),
        Affine::rotate(0.8),
        Affine::scale(-0.7, 1.2),
        Affine::skew(0.3, 0.1),
    ] {
        doc.parents[0].placements = vec![ParentPlacement {
            page: 1,
            sheet: 0,
            transform: matrix,
            visible: true,
        }];
        let placed = doc.page_objects(1);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].page, 1);
        assert_eq!(placed[0].bounds, original.bounds);
        assert_eq!(compose_object(&doc, &placed[0]).unwrap(), original_lines);
        for p in affine::corners(original.bounds) {
            let expected = affine::point(matrix, affine::point(original.content_transform(), p));
            let actual = affine::point(placed[0].content_transform(), p);
            assert!((actual.x - expected.x).abs() < 0.001 && (actual.y - expected.y).abs() < 0.001);
        }
        assert_eq!(doc.parents[0].objects[0].object, original);
        assert_eq!(
            doc.page_objects(0)[0].content_transform(),
            original.content_transform()
        );
    }
}

#[test]
fn parent_text_uses_each_destination_pages_baseline_grid() {
    let mut doc = blank_a4();
    doc.pages.push(doc.pages[0].clone());
    doc.pages[0].margins.top = 3.0;
    doc.pages[1].margins.top = 9.0;
    doc.grids.document.mode = schist_layout::GridMode::SnapToGrid;
    doc.grids.document.baseline_count = 72.0 / 14.0;
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 40.0, 130.0, 100.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] =
        Story::from_text("Parent text aligns on the destination page grid.", "Body");
    let object = doc.objects.remove(0);
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: vec![0, 1],
        based_on: None,
        hidden: false,
        sheets: Vec::new(),
        placements: Vec::new(),
        objects: vec![ParentObject {
            object,
            overridden_on: Vec::new(),
        }],
    });
    let mut first = Vec::new();
    for page in 0..2 {
        let object = doc.page_objects(page).remove(0);
        let frame = compose_object(&doc, &object).unwrap();
        assert!(!frame.lines.is_empty());
        first.push(frame.lines[0].baseline);
        for line in frame.lines {
            let phase = (line.baseline - doc.pages[page].margins.top) / 14.0;
            assert!((phase - phase.round()).abs() < 0.001);
        }
    }
    assert_ne!(first[0], first[1]);
}
