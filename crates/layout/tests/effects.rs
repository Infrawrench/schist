//! Blend modes and drop shadows. InDesign's PDF of the public paged-media
//! `effects` sample writes each item's BlendMode as PDF's own, which blends
//! subtractive inks on their complements; its drop shadows are soft masks
//! whose edge, measured across sizes of 6 and 24 pt, is one curve scaled by
//! the size.
use schist_layout::effects::{blur, BlendMode, DropShadow};

fn near(a: f32, b: f32, tolerance: f32, what: &str) {
    assert!((a - b).abs() <= tolerance, "{what}: {a} vs {b}");
}

#[test]
fn inks_blend_on_their_complements() {
    // 50 % magenta over a plate with 100 % cyan and no magenta.
    let cases = [
        (BlendMode::Normal, 0.0, 0.5),
        // Multiply keeps the cyan beneath and adds the magenta.
        (BlendMode::Multiply, 1.0, 0.5),
        // Screen and Lighten clear toward paper.
        (BlendMode::Screen, 0.0, 0.0),
        (BlendMode::Lighten, 0.0, 0.0),
        // Darken keeps the darker of each.
        (BlendMode::Darken, 1.0, 0.5),
        // Difference of no ink from solid ink clears it.
        (BlendMode::Difference, 0.0, 0.5),
    ];
    for (mode, cyan, magenta) in cases {
        near(
            mode.blend_ink(1.0, 0.0),
            cyan,
            1e-5,
            &format!("{mode:?} cyan"),
        );
        near(
            mode.blend_ink(0.0, 0.5),
            magenta,
            1e-5,
            &format!("{mode:?} magenta"),
        );
    }
    // Overlay of 50 % over 50 % stays 50 %; multiply of two halves is 75 %.
    near(BlendMode::Overlay.blend_ink(0.5, 0.5), 0.5, 1e-5, "overlay");
    near(
        BlendMode::Multiply.blend_ink(0.5, 0.5),
        0.75,
        1e-5,
        "multiply",
    );
    assert!(BlendMode::Multiply.separable());
    assert!(!BlendMode::Luminosity.separable());
}

#[test]
fn a_blurred_edge_follows_indesigns_shadow_profile() {
    // A straight edge at 200 px, blurred as a shadow of `size` pixels.
    for size in [24.0f32, 60.0] {
        let width = 400;
        let mut line: Vec<f32> = (0..width)
            .map(|x| if x >= 200 { 1.0 } else { 0.0 })
            .collect();
        // One row is enough: the vertical pass leaves a uniform column alone
        // apart from its ends, so blur a tall strip and read its middle.
        let height = 200;
        let mut data = Vec::with_capacity(width * height);
        for _ in 0..height {
            data.extend_from_slice(&line);
        }
        blur(&mut data, width, height, size / 2.0);
        line.copy_from_slice(&data[height / 2 * width..(height / 2 + 1) * width]);
        // The mask InDesign drew for the sample's shadows, against the
        // distance from the edge in sizes.
        for (d, measured) in [
            (-0.8, 0.054),
            (-0.6, 0.127),
            (-0.4, 0.222),
            (-0.2, 0.352),
            (0.0, 0.5),
            (0.2, 0.652),
            (0.4, 0.778),
            (0.6, 0.875),
            (0.8, 0.949),
        ] {
            let x = 200.0 - 0.5 + d * size;
            let (i, t) = (x.floor() as usize, x - x.floor());
            let value = line[i] * (1.0 - t) + line[i + 1] * t;
            near(value, measured, 0.03, &format!("size {size}, {d} sizes"));
        }
    }
}

#[test]
fn shadows_default_as_the_specification_says() {
    let shadow = DropShadow::default();
    assert_eq!(shadow.blend, BlendMode::Multiply);
    assert_eq!(
        (shadow.x_offset, shadow.y_offset, shadow.size),
        (7.0, 7.0, 5.0)
    );
    assert_eq!(shadow.opacity, 0.75);
    assert!(shadow.knocked_out);
    assert_eq!(shadow.ink(), schist_layout::Ink::black());
}

#[test]
fn the_canvas_casts_shapes_and_frames_shadows() {
    use schist_layout::{
        authoring, pasteboard, Display, History, Ink, LayoutDocument, Page, Paint, PasteboardView,
        Rect,
    };
    let mut doc = LayoutDocument::new(vec![Page::new("1", 400.0, 300.0)]);
    let shape = authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 100.0, 60.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::filled("Black"),
    )
    .unwrap();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 120.0, 200.0, 100.0),
    )
    .unwrap()
    .object;
    for id in [shape, frame] {
        let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.appearance.drop_shadow = Some(DropShadow::default());
    }
    let object = doc.objects.iter_mut().find(|o| o.id == frame).unwrap();
    object.appearance.paint.fill = Some(Paint::Ink(Ink::cmyk("Pale", [0.1, 0.0, 0.0, 0.0])));
    object.appearance.paint.stroke = Some(Paint::Ink(Ink::black()));
    object.appearance.paint.stroke_width = Some(1.0);
    let view = PasteboardView::default();
    let plan = pasteboard(&doc, &view).unwrap();
    let shadows = |id| -> Vec<_> {
        plan.objects()
            .filter_map(|display| match display {
                Display::Shape { object, shadow, .. } if *object == id => Some(shadow.clone()),
                _ => None,
            })
            .collect()
    };
    let cast = shadows(shape);
    assert_eq!(cast.len(), 1);
    let shadow = cast[0].as_ref().expect("the shape casts its shadow");
    assert!((shadow.offset.x - 7.0 * view.scale).abs() < 1e-4);
    assert!((shadow.sigma - 2.5 * view.scale).abs() < 1e-4);
    assert!((shadow.color[3] - 0.75).abs() < 1e-4);
    // A frame casts its shadow once, from its fill.
    let cast = shadows(frame);
    assert_eq!(cast.len(), 2, "fill and stroke");
    assert!(cast[0].is_some() && cast[1].is_none());
}
