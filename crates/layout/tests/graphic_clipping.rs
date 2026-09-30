use schist_core::IntRect;
use schist_layout::{
    affine::{self, Affine},
    graphics::clip_coverage,
    Point, Rect, ShapePath,
};

#[test]
fn normalized_curves_clip_in_output_coordinates_after_resizing_and_affines() {
    let path = ShapePath::ellipse(1.0, 1.0);
    for frame in [
        Rect::new(-12.0, 8.0, 60.0, 80.0),
        Rect::new(30.0, 20.0, 110.0, 50.0),
    ] {
        for transform in [
            Affine::IDENTITY,
            Affine::rotate(0.57),
            Affine::skew(0.6, -0.2),
            Affine::scale(-1.5, 0.8),
        ] {
            let bounds = affine::bounds(transform, frame);
            let output = IntRect::new(
                bounds.x.floor() as i32 - 2,
                bounds.y.floor() as i32 - 2,
                bounds.right().ceil() as i32 + 2,
                bounds.bottom().ceil() as i32 + 2,
            );
            let mask = clip_coverage(&path, frame, transform, output);
            assert_eq!(mask.len(), (output.width() * output.height()) as usize);
            let inverse = affine::inverse(transform).unwrap();
            let mut antialias = 0;
            for y in output.top..output.bottom {
                for x in output.left..output.right {
                    let p = affine::point(inverse, Point::new(x as f32 + 0.5, y as f32 + 0.5));
                    let radius = ((p.x - frame.center().x) / (frame.width / 2.0))
                        .hypot((p.y - frame.center().y) / (frame.height / 2.0));
                    let alpha =
                        mask[((y - output.top) * output.width() + x - output.left) as usize];
                    // Leave the AA pixel footprint and cubic/circle error out
                    // of exact interior/exterior assertions.
                    if radius < 0.90 {
                        assert_eq!(alpha, 255);
                    }
                    if radius > 1.10 {
                        assert_eq!(alpha, 0);
                    }
                    antialias += usize::from(alpha > 0 && alpha < 255);
                }
            }
            assert!(antialias > 20);
            let area = mask.iter().map(|a| *a as f32 / 255.0).sum::<f32>();
            let expected = std::f32::consts::PI / 4.0
                * frame.width
                * frame.height
                * (transform.a * transform.d - transform.b * transform.c).abs();
            assert!((area / expected - 1.0).abs() < 0.02);
        }
    }
}

#[test]
fn compound_clips_preserve_winding_holes_and_ignore_open_contours() {
    let frame = Rect::new(0.0, 0.0, 100.0, 100.0);
    let output = IntRect::new(0, 0, 100, 100);
    for even_odd in [true, false] {
        let mut path = ShapePath::ellipse(1.0, 1.0);
        let mut inner = ShapePath::ellipse(0.5, 0.5);
        inner.map_points(|p| p + Point::new(0.25, 0.25));
        if !even_odd {
            inner.subpaths[0].points.reverse();
            inner.subpaths[0].handles.reverse();
            for h in &mut inner.subpaths[0].handles {
                std::mem::swap(&mut h.incoming, &mut h.outgoing);
            }
        }
        path.subpaths.extend(inner.subpaths);
        path.even_odd = even_odd;
        let mask = clip_coverage(&path, frame, Affine::IDENTITY, output);
        assert_eq!(mask[50 * 100 + 50], 0);
        assert_eq!(mask[50 * 100 + 10], 255);
        assert_eq!(mask[5 * 100 + 5], 0);
        for sub in &mut path.subpaths {
            sub.closed = false;
        }
        assert!(clip_coverage(&path, frame, Affine::IDENTITY, output)
            .iter()
            .all(|v| *v == 0));
    }
}
