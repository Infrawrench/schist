use schist_layout::{
    affine::{self, Affine},
    authoring, blank_a4,
    graphics::{self, ImageMapping},
    GraphicFit, History, LayoutObject, Link, Point, Rect,
};

#[test]
fn image_mapping_keeps_frame_clipping_and_resizes_in_normalized_coordinates() {
    for matrix in [
        Affine::IDENTITY,
        Affine::rotate(0.6).around(0.5, 0.5),
        Affine::scale(-1.0, 1.0).around(0.5, 0.5),
        Affine::skew(0.4, -0.2),
    ] {
        for frame in [
            Rect::new(10.0, 30.0, 100.0, 60.0),
            Rect::new(-40.0, -20.0, 200.0, 180.0),
        ] {
            for fit in [
                GraphicFit::Fill,
                GraphicFit::Contain,
                GraphicFit::Original,
                GraphicFit::Stretch,
            ] {
                let source = graphics::image_rect(
                    frame,
                    (150, 100),
                    72.0,
                    Some(Rect::new(0.25, 0.1, 0.5, 0.8)),
                    fit,
                    1.0,
                )
                .unwrap();
                let mapping = ImageMapping::new(frame, source, matrix).unwrap();
                // Independently evaluate the normalized transform at points
                // inside and outside the complete source, then the frame clip.
                for x in -2..=12 {
                    for y in -2..=12 {
                        let p = Point::new(
                            source.x + (x as f32 + 0.31) / 10.0 * source.width,
                            source.y + (y as f32 + 0.23) / 10.0 * source.height,
                        );
                        let normalized = Point::new(
                            (p.x - frame.x) / frame.width,
                            (p.y - frame.y) / frame.height,
                        );
                        let q = affine::point(matrix, normalized);
                        let q =
                            Point::new(frame.x + q.x * frame.width, frame.y + q.y * frame.height);
                        let actual = mapping.source_at(q);
                        assert_eq!(actual.is_some(), frame.contains(q) && source.contains(p));
                        if let Some(actual) = actual {
                            assert!(
                                (actual.x - p.x).abs() < 0.0001 && (actual.y - p.y).abs() < 0.0001
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn image_transforms_survive_relink_duplicate_and_legacy_serialization() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let id = authoring::graphic_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(10.0, 20.0, 80.0, 50.0),
        "before.png",
        true,
    )
    .unwrap();
    let matrix = Affine::rotate(0.5).around(0.5, 0.5);
    let LayoutObject::GraphicFrame {
        image_transform,
        clip_path,
        ..
    } = &mut doc.objects[0].object
    else {
        panic!()
    };
    *image_transform = matrix;
    *clip_path = Some(schist_layout::ShapePath::ellipse(1.0, 1.0));
    let original = doc.clone();
    let depth = history.undo_depth();
    assert!(graphics::relink(
        &mut doc,
        &mut history,
        id,
        Link::new("after.png")
    ));
    assert_eq!(history.undo_depth(), depth + 1);
    let LayoutObject::GraphicFrame {
        image_transform, ..
    } = doc.objects[0].object
    else {
        panic!()
    };
    assert_eq!(image_transform, matrix);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    let copy = authoring::duplicate(&mut doc, &mut history, id).unwrap();
    assert_eq!(doc.object(copy).unwrap().object, original.objects[0].object);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
    let mut encoded = serde_json::to_value(&doc.objects[0].object).unwrap();
    encoded["GraphicFrame"]
        .as_object_mut()
        .unwrap()
        .remove("image_transform");
    encoded["GraphicFrame"]
        .as_object_mut()
        .unwrap()
        .remove("clip_path");
    let LayoutObject::GraphicFrame {
        image_transform,
        clip_path,
        ..
    } = serde_json::from_value::<LayoutObject>(encoded).unwrap()
    else {
        panic!()
    };
    assert_eq!(image_transform, Affine::IDENTITY);
    assert_eq!(clip_path, None);
}
