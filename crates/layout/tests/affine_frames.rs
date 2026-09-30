use schist_layout::{
    affine::{self, Affine},
    authoring, blank_a4, compose_object, History, Point, Rect, Story,
};

#[test]
fn frame_affines_preserve_composition_and_move_duplicate_and_undo_together() {
    for matrix in [
        Affine::rotate(0.4),
        Affine::skew(0.5, -0.1),
        Affine::scale(-2.0, 0.7),
    ] {
        let mut doc = blank_a4();
        let mut history = History::default();
        let frame = authoring::text_frame(
            &mut doc,
            &mut history,
            0,
            Rect::new(80.0, 60.0, 120.0, 90.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text(
            "Text wraps in the local frame, including é and 空. ".repeat(5),
            "Body",
        );
        let expected = compose_object(&doc, doc.object(frame.object).unwrap()).unwrap();
        doc.objects[0].transform = matrix;
        doc.objects[0].rotation = 21.0;
        let actual = compose_object(&doc, doc.object(frame.object).unwrap()).unwrap();
        assert_eq!(actual, expected);
        let before = doc.clone();
        let original = doc.objects[0].clone();
        let copy = authoring::duplicate(&mut doc, &mut history, frame.object).unwrap();
        let duplicate = doc.object(copy).unwrap();
        assert_eq!(duplicate.transform, matrix);
        let delta = duplicate.bounds.origin() - original.bounds.origin();
        for local in [Point::ZERO, Point::new(7.0, 9.0), Point::new(100.0, 70.0)] {
            let a = affine::point(
                original.content_transform(),
                original.bounds.origin() + local,
            );
            let b = affine::point(
                duplicate.content_transform(),
                duplicate.bounds.origin() + local,
            );
            assert!((b.x - a.x - delta.x).abs() < 0.0001 && (b.y - a.y - delta.y).abs() < 0.0001);
        }
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert_eq!(doc.object(copy).unwrap().transform, matrix);
        let mut encoded = serde_json::to_value(&original).unwrap();
        encoded.as_object_mut().unwrap().remove("transform");
        let legacy: schist_layout::PlacedObject = serde_json::from_value(encoded).unwrap();
        assert_eq!(legacy.transform, Affine::IDENTITY);
    }
}

#[test]
fn editing_an_affine_curve_preserves_untouched_anchors_and_one_step_undo() {
    for matrix in [
        Affine::rotate(0.4),
        Affine::skew(0.7, -0.2),
        Affine::scale(-1.3, 0.6),
    ] {
        let mut doc = blank_a4();
        let mut history = History::default();
        let mut path = schist_layout::ShapePath::ellipse(60.0, 30.0);
        path.map_points(|p| p + Point::new(80.0, 70.0));
        let id = authoring::path_shape(&mut doc, &mut history, 0, path, authoring::Paint::none())
            .unwrap();
        doc.objects[0].transform = matrix;
        doc.objects[0].rotation = 17.0;
        let before = doc.clone();
        let anchors = |object: &schist_layout::PlacedObject| {
            let schist_layout::LayoutObject::Shape { path, .. } = &object.object else {
                panic!()
            };
            path.subpaths[0]
                .points
                .iter()
                .map(|p| affine::point(object.content_transform(), *p + object.bounds.origin()))
                .collect::<Vec<_>>()
        };
        let points = anchors(&doc.objects[0]);
        let at = authoring::PointRef {
            subpath: 0,
            index: 0,
            part: authoring::PointPart::Anchor,
        };
        let depth = history.undo_depth();
        assert!(!authoring::move_point(
            &mut doc,
            &mut history,
            id,
            at,
            points[0]
        ));
        assert_eq!(doc, before);
        let target = points[0] + Point::new(-70.0, 110.0);
        assert!(authoring::move_point(
            &mut doc,
            &mut history,
            id,
            at,
            target
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        let actual = anchors(&doc.objects[0]);
        for (a, b) in actual
            .iter()
            .zip(std::iter::once(&target).chain(points[1..].iter()))
        {
            assert!((a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001);
        }
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
    }
}
