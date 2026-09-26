use schist_color::Rgba;
use schist_core::{
    curves::{curvature_comb, snap_g2, ArcPath, Cubic},
    live_mask::{LiveMask, MaskGradient, MaskSnapshot},
    vector_blend::VectorBlend,
    Anchor, IntRect, Layer, LayerMask, SubPath, TileCoord, VectorPath, VectorShape,
};

fn square(x: f32, size: f32) -> VectorShape {
    let mut path = VectorPath::new("square");
    path.subpaths.push(SubPath {
        closed: true,
        anchors: vec![
            Anchor::corner(x, 0.0),
            Anchor::corner(x + size, 0.0),
            Anchor::corner(x + size, size),
            Anchor::corner(x, size),
        ],
    });
    VectorShape::new(path, Rgba::new(1.0, 0.0, 0.0, 1.0))
}
#[test]
fn arc_length_ignores_cubic_parameter_speed() {
    let path = SubPath {
        closed: false,
        anchors: vec![
            Anchor {
                point: (0.0, 0.0),
                handle_in: (0.0, 0.0),
                handle_out: (0.0, 20.0),
            },
            Anchor {
                point: (120.0, 0.0),
                handle_in: (-60.0, 80.0),
                handle_out: (0.0, 0.0),
            },
        ],
    };
    let arc = ArcPath::new(&path);
    // Integrate each equal-distance interval with many samples. Euclidean
    // endpoint gaps alone underestimate distance on a tight bend.
    let lengths: Vec<_> = (0..8)
        .map(|i| {
            let mut sum = 0.0;
            let mut prev = arc.sample(i as f32 / 8.0).0;
            for j in 1..=100 {
                let p = arc.sample((i as f32 + j as f32 / 100.0) / 8.0).0;
                sum += (p.0 - prev.0).hypot(p.1 - prev.1);
                prev = p;
            }
            sum
        })
        .collect();
    for l in lengths {
        assert!((l - arc.length / 8.0).abs() < 0.05);
    }
}
#[test]
fn g2_snap_matches_signed_curvature_without_moving_anchors() {
    let mut path = SubPath {
        closed: false,
        anchors: vec![
            Anchor::smooth(0.0, 0.0, 10.0, 0.0),
            Anchor {
                point: (30.0, 10.0),
                handle_in: (-5.0, 0.0),
                handle_out: (20.0, 0.0),
            },
            Anchor::smooth(80.0, -10.0, 10.0, 0.0),
        ],
    };
    let anchors: Vec<_> = path.anchors.iter().map(|a| a.point).collect();
    assert!(snap_g2(&mut path, 1, true));
    let left = Cubic::between(path.anchors[0], path.anchors[1]).curvature(1.0);
    let right = Cubic::between(path.anchors[1], path.anchors[2]).curvature(0.0);
    assert!((left - right).abs() < 1e-6, "{left} != {right}");
    assert_eq!(
        anchors,
        path.anchors.iter().map(|a| a.point).collect::<Vec<_>>()
    );
    assert_eq!(path.anchors[1].handle_out, (20.0, 0.0));
    let mut vector = VectorPath::new("curve");
    vector.subpaths.push(path);
    assert!(curvature_comb(&vector, 24, 1000.0)
        .iter()
        .all(|(a, b)| [a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite())));
}
#[test]
fn blend_preserves_endpoints_and_interpolates_color_and_nodes() {
    let start = square(0.0, 10.0);
    let mut end = square(100.0, 20.0);
    end.fill = Rgba::new(0.0, 0.0, 1.0, 0.5);
    let mut blend = VectorBlend::new(start.clone(), end.clone());
    blend.steps = 1;
    let shapes = blend.shapes();
    assert_eq!(shapes.len(), 3);
    assert_eq!(shapes[0].fill, start.fill);
    assert_eq!(shapes[2].fill, end.fill);
    assert_eq!(shapes[1].fill, Rgba::new(0.5, 0.0, 0.5, 0.75));
    assert_eq!(shapes[1].path.subpaths[0].anchors[0].point, (50.0, 0.0));
    blend.bias = 0.2;
    assert!(blend.position(0.5) < 0.5);
    blend.bias = 0.8;
    assert!(blend.position(0.5) > 0.5);
    let mut layer = Layer::new_raster("blend");
    layer.extras = blend.blocks(&layer);
    assert_eq!(VectorBlend::from_layer(&layer), Some(blend));
}
#[test]
fn second_rail_changes_width_and_follows_curved_spine() {
    let mut blend = VectorBlend::new(square(0.0, 10.0), square(100.0, 10.0));
    blend.steps = 1;
    let mut spine = VectorPath::new("spine");
    spine.push_open_anchors(vec![Anchor::corner(0.0, 20.0), Anchor::corner(100.0, 20.0)]);
    let mut rail = spine.clone();
    rail.translate(0.0, 40.0);
    blend.spine = Some(spine);
    blend.rail = Some(rail);
    let shapes = blend.shapes();
    assert!(shapes[1].path.bounds().height() > 30);
    assert!((shapes[1].path.bounds().left - 45).abs() <= 2);
}
#[test]
fn mask_gradient_preserves_paint_and_bounded_defaults() {
    let mut base = LayerMask::new_revealing();
    base.bounds = IntRect::new(10, 10, 20, 20);
    base.tiles
        .get_mut_or_insert(TileCoord::containing(10, 10))
        .fill(128);
    let mut live = LiveMask {
        base: MaskSnapshot::capture(&base),
        gradients: vec![MaskGradient {
            from: (0.0, 0.0),
            to: (100.0, 0.0),
            radial: false,
            reverse: false,
            opacity: 1.0,
        }],
    };
    let first = live.render(IntRect::from_size(100, 100)).unwrap();
    assert_eq!(first.value(15, 15), 20);
    assert_eq!(first.value(50, 50), 129);
    live.gradients[0].reverse = true;
    let second = live.render(IntRect::from_size(100, 100)).unwrap();
    assert_eq!(second.value(15, 15), 108);
    assert_eq!(live.base.restore().unwrap().value(50, 50), 255);
    let mut layer = Layer::new_raster("mask");
    layer.extras = live.blocks(&layer);
    let decoded = LiveMask::from_layer(&layer)
        .unwrap()
        .render(IntRect::from_size(100, 100))
        .unwrap();
    assert_eq!(decoded.value(15, 15), second.value(15, 15));
}
