use schist_color::{Depth, Rgba};
use schist_core::{Document, IntRect, Layer, LayerMask, SelectOp};
use schist_plugin_api::{
    BrushDynamics, BrushTip, EditorState, PaintSymmetry, PointerInput, SymmetryMode, ToolCtx,
};

fn input(x: f32, y: f32, pressure: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure,
        modifiers: Default::default(),
    }
}
fn document() -> Document {
    let mut doc = Document::new("pattern", 64, 64, Depth::Eight);
    let id = doc.push_layer(Layer::new_raster("paint"));
    doc.active_layer = Some(id);
    doc
}
fn alpha(doc: &Document, x: i32, y: i32) -> u8 {
    doc.tree.layers[0]
        .as_raster()
        .unwrap()
        .tiles
        .pixel(x, y)
        .to_u8()[3]
}
fn pixels(doc: &Document) -> Vec<u8> {
    (0..64)
        .flat_map(|y| (0..64).map(move |x| alpha(doc, x, y)))
        .collect()
}
fn state(mode: SymmetryMode) -> EditorState {
    EditorState {
        foreground: Rgba::BLACK,
        brush_size: 8.0,
        brush_hardness: 1.0,
        paint_symmetry: PaintSymmetry {
            mode,
            segments: 4,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn paint(doc: &mut Document, state: &mut EditorState, tool: &str, path: &[PointerInput]) {
    let mut tool = schist_tools_paint::tool_for_test(tool).unwrap();
    let mut ctx = ToolCtx { doc, state };
    tool.on_pointer_down(&mut ctx, path[0]);
    for &point in &path[1..] {
        tool.on_pointer_move(&mut ctx, point);
    }
    tool.on_pointer_up(&mut ctx, *path.last().unwrap());
}

#[test]
fn mirrored_pressure_stroke_has_one_undo_and_redo() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Vertical);
    paint(
        &mut doc,
        &mut state,
        "brush",
        &[input(12.5, 12.5, 0.5), input(20.5, 24.5, 1.0)],
    );
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(alpha(&doc, x, y), alpha(&doc, 63 - x, y));
        }
    }
    assert!(alpha(&doc, 12, 12) > 0);
    assert_eq!(
        alpha(&doc, 9, 12),
        0,
        "pressure halves the starting footprint"
    );
    let painted = pixels(&doc);
    doc.undo();
    assert!(pixels(&doc).iter().all(|&a| a == 0));
    assert!(
        !doc.history.can_undo(),
        "all reflected dabs form one history entry"
    );
    doc.redo();
    assert_eq!(pixels(&doc), painted);
}

#[test]
fn radial_pencil_is_rotational_and_overlap_retains_opacity() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Radial);
    state.tool_opacity = 0.5;
    paint(
        &mut doc,
        &mut state,
        "pencil",
        &[input(32.0, 32.0, 1.0), input(47.0, 32.0, 1.0)],
    );
    assert_eq!(
        alpha(&doc, 32, 32),
        128,
        "four overlapping copies never accumulate opacity"
    );
    for (x, y) in [(47, 32), (31, 47), (16, 31), (32, 16)] {
        assert!(alpha(&doc, x, y) > 0);
    }
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(alpha(&doc, x, y), alpha(&doc, 63 - y, x));
        }
    }
}

#[test]
fn reflected_textured_tip_and_scatter_are_symmetric() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Vertical);
    state.brush_size = 20.0;
    state.brush_dynamics = BrushDynamics {
        tip: BrushTip::Bristles,
        scatter: 0.3,
        ..Default::default()
    };
    paint(
        &mut doc,
        &mut state,
        "brush",
        &[input(12.2, 20.4, 1.0), input(17.6, 42.1, 0.8)],
    );
    for y in 0..64 {
        for x in 0..64 {
            assert!((alpha(&doc, x, y) as i32 - alpha(&doc, 63 - x, y) as i32).abs() <= 1);
        }
    }
}

#[test]
fn eraser_selection_and_layer_masks_use_destination_coordinates() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Vertical);
    paint(&mut doc, &mut state, "brush", &[input(12.5, 20.5, 1.0)]);
    let before = pixels(&doc);
    doc.selection
        .select_rect(IntRect::from_xywh(0, 0, 32, 64), SelectOp::Replace);
    paint(&mut doc, &mut state, "eraser", &[input(12.5, 20.5, 1.0)]);
    assert_eq!(alpha(&doc, 12, 20), 0);
    assert_eq!(
        alpha(&doc, 51, 20),
        255,
        "reflection outside selection stays untouched"
    );
    doc.undo();
    assert_eq!(pixels(&doc), before);
    let mut mask = LayerMask::new_revealing();
    mask.default_value = 0;
    doc.tree.layers[0].mask = Some(mask);
    paint(&mut doc, &mut state, "pencil", &[input(20.5, 20.5, 1.0)]);
    assert_eq!(
        alpha(&doc, 20, 20),
        255,
        "layer pixels remain editable under a mask"
    );
    let composite =
        schist_compositor::composite_region_rgba8(&doc, IntRect::from_xywh(20, 20, 1, 1));
    assert_eq!(
        composite[3], 0,
        "the layer mask still controls the visible result"
    );
}

#[test]
fn cancel_and_tool_switch_roll_back_all_copies() {
    for deactivate in [false, true] {
        let mut doc = document();
        let mut state = state(SymmetryMode::Radial);
        let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, input(12.5, 20.5, 1.0));
        if deactivate {
            tool.on_deactivate(&mut ctx);
        } else {
            tool.on_cancel(&mut ctx);
        }
        assert!(pixels(&doc).iter().all(|&a| a == 0));
        assert!(!doc.history.can_undo());
    }
}

#[test]
fn center_placement_is_a_draggable_cancelable_view_operation() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Vertical);
    let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
    state.symmetry_positioning = true;
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, input(20.0, 10.0, 1.0));
    tool.on_pointer_move(&mut ctx, input(24.0, 40.0, 1.0));
    assert_eq!(ctx.state.paint_symmetry.center_pixels(64, 64), [24.0, 40.0]);
    tool.on_cancel(&mut ctx);
    assert_eq!(ctx.state.paint_symmetry.center, [0.5, 0.5]);
    ctx.state.symmetry_positioning = true;
    tool.on_pointer_down(&mut ctx, input(20.0, 10.0, 1.0));
    tool.on_pointer_up(&mut ctx, input(24.0, 40.0, 1.0));
    assert!(!ctx.state.symmetry_positioning);
    assert_eq!(ctx.state.paint_symmetry.center_pixels(64, 64), [24.0, 40.0]);
    assert!(tool
        .overlays(ctx.doc, ctx.state)
        .iter()
        .any(|overlay| matches!(overlay,
        schist_plugin_api::Overlay::GuideLine { x1, y1, x2, y2 }
        if (*x1, *y1, *x2, *y2) == (24.0, 0.0, 24.0, 64.0))));
    assert!(!doc.history.can_undo());
    paint(&mut doc, &mut state, "brush", &[input(20.0, 20.0, 1.0)]);
    assert!(alpha(&doc, 28, 20) > 0, "reflection follows the moved axis");
}

#[test]
fn seamless_corner_dabs_wrap_without_off_canvas_tiles_or_opacity_buildup() {
    let mut doc = document();
    let mut state = state(SymmetryMode::None);
    state.seamless_painting = true;
    state.tool_opacity = 0.5;
    paint(&mut doc, &mut state, "brush", &[input(0.0, 0.0, 1.0)]);
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
        assert_eq!(alpha(&doc, x, y), 128);
    }
    assert_eq!(alpha(&doc, -1, 0), 0);
    assert_eq!(alpha(&doc, 64, 63), 0);
    assert_eq!((doc.width, doc.height), (64, 64));
    let before = pixels(&doc);
    doc.undo();
    assert!(pixels(&doc).iter().all(|&a| a == 0));
    doc.redo();
    assert_eq!(pixels(&doc), before);
}

#[test]
fn seamless_stroke_crosses_boundary_without_drawing_a_line_across_canvas() {
    let mut doc = document();
    let mut state = state(SymmetryMode::None);
    state.seamless_painting = true;
    paint(
        &mut doc,
        &mut state,
        "pencil",
        &[input(60.0, 20.0, 1.0), input(68.0, 20.0, 1.0)],
    );
    assert!(alpha(&doc, 63, 20) > 0 && alpha(&doc, 0, 20) > 0 && alpha(&doc, 4, 20) > 0);
    assert_eq!(alpha(&doc, 32, 20), 0);
    doc.selection
        .select_rect(IntRect::from_xywh(0, 0, 32, 64), SelectOp::Replace);
    paint(&mut doc, &mut state, "eraser", &[input(64.0, 20.0, 1.0)]);
    assert_eq!(alpha(&doc, 0, 20), 0);
    assert!(alpha(&doc, 63, 20) > 0);
}

#[test]
fn repeated_view_coordinates_produce_the_same_radial_pattern() {
    let render = |x, y| {
        let mut doc = document();
        let mut state = state(SymmetryMode::Radial);
        state.paint_symmetry.segments = 6;
        state.seamless_painting = true;
        paint(&mut doc, &mut state, "brush", &[input(x, y, 1.0)]);
        pixels(&doc)
    };
    assert_eq!(render(12.5, 20.5), render(76.5, -43.5));
}

#[test]
fn large_dab_on_tiny_pattern_stays_bounded() {
    let mut doc = Document::new("tiny", 1, 1, Depth::Eight);
    let id = doc.push_layer(Layer::new_raster("paint"));
    doc.active_layer = Some(id);
    let mut state = state(SymmetryMode::Radial);
    state.seamless_painting = true;
    state.brush_size = 500.0;
    paint(&mut doc, &mut state, "brush", &[input(-4.5, 10.5, 1.0)]);
    assert_eq!(alpha(&doc, 0, 0), 255);
    assert_eq!(alpha(&doc, 1, 0), 0);
}

#[test]
fn positioning_in_a_repeat_places_the_source_center_without_painting() {
    let mut doc = document();
    let mut state = state(SymmetryMode::Radial);
    state.seamless_painting = true;
    state.symmetry_positioning = true;
    let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, input(76.0, -44.0, 1.0));
    tool.on_pointer_up(&mut ctx, input(76.0, -44.0, 1.0));
    assert_eq!(ctx.state.paint_symmetry.center_pixels(64, 64), [12.0, 20.0]);
    assert!(!ctx.doc.history.can_undo());
    assert!(pixels(ctx.doc).iter().all(|&a| a == 0));
}

#[test]
fn large_textured_dabs_match_an_independent_unwrapped_periodic_oracle() {
    // Paint the complete unwrapped footprint once, then fold all of its
    // pixels into the source canvas with maximum coverage. Faraway tip
    // detail must contribute even beyond the nearest nine repetitions.
    for tip in [BrushTip::Grain, BrushTip::Bristles] {
        let make_doc = || {
            let mut doc = Document::new("small pattern", 7, 5, Depth::Eight);
            let id = doc.push_layer(Layer::new_raster("paint"));
            doc.active_layer = Some(id);
            doc
        };
        let mut source = make_doc();
        let mut repeated = make_doc();
        let mut state = state(SymmetryMode::None);
        state.brush_size = 120.0;
        state.brush_hardness = 0.7;
        state.tool_opacity = 0.6;
        state.brush_dynamics.tip = tip;
        let point = input(2.3, 1.7, 0.8);
        paint(&mut source, &mut state, "brush", &[point]);
        state.seamless_painting = true;
        paint(&mut repeated, &mut state, "brush", &[point]);
        for y in 0..5 {
            for x in 0..7 {
                let mut expected = 0;
                for oy in -12..=12 {
                    for ox in -12..=12 {
                        expected = expected.max(alpha(&source, x + ox * 7, y + oy * 5));
                    }
                }
                assert_eq!(alpha(&repeated, x, y), expected, "{tip:?} at {x},{y}");
            }
        }
    }
}

fn asymmetric_bitmap_state(mode: SymmetryMode) -> EditorState {
    let mut state = state(mode);
    state.brush_size = 24.0;
    state.brush_bitmap = Some(std::sync::Arc::new(schist_plugin_api::BrushBitmap {
        width: 5,
        height: 3,
        pixels: vec![0, 0, 255, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255],
    }));
    state.brush_dynamics = BrushDynamics {
        tip: BrushTip::Bitmap,
        rotation: 31.0,
        tilt_rotation: true,
        pressure_opacity: true,
        ..Default::default()
    };
    state.pen_tilt = Some([30.0, 30.0]);
    state.tool_opacity = 0.8;
    state
}

#[test]
fn symmetry_transforms_rotated_tilted_bitmap_pixels_and_preserves_pressure_opacity() {
    // Independently transform the completed unsymmetrical raster. An
    // asymmetric mask exposes reflection handedness and rotation order.
    for tool in ["brush", "pencil"] {
        let mut source = document();
        let mut base = asymmetric_bitmap_state(SymmetryMode::None);
        let point = input(12.0, 20.0, 0.6);
        paint(&mut source, &mut base, tool, &[point]);
        assert!(pixels(&source).iter().any(|&a| a > 100));
        assert!(pixels(&source).iter().all(|&a| a <= 123));
        for mode in [SymmetryMode::Vertical, SymmetryMode::Radial] {
            let mut actual = document();
            let mut state = asymmetric_bitmap_state(mode);
            paint(&mut actual, &mut state, tool, &[point]);
            for y in 0..64 {
                for x in 0..64 {
                    let expected = if mode == SymmetryMode::Vertical {
                        alpha(&source, x, y).max(alpha(&source, 63 - x, y))
                    } else {
                        [
                            alpha(&source, x, y),
                            alpha(&source, y, 63 - x),
                            alpha(&source, 63 - x, 63 - y),
                            alpha(&source, 63 - y, x),
                        ]
                        .into_iter()
                        .max()
                        .unwrap()
                    };
                    assert!(
                        (alpha(&actual, x, y) as i32 - expected as i32).abs() <= 1,
                        "{tool} {mode:?} at {x},{y}: expected {expected}, got {}",
                        alpha(&actual, x, y)
                    );
                }
            }
        }
    }
}

#[test]
fn seamless_rotated_bitmap_wraps_corner_detail_beyond_the_round_radius() {
    let mut source = document();
    let mut repeated = document();
    let mut state = state(SymmetryMode::None);
    state.brush_size = 120.0;
    let mut mask = vec![0; 64];
    mask[63] = 255;
    state.brush_bitmap = Some(std::sync::Arc::new(schist_plugin_api::BrushBitmap {
        width: 8,
        height: 8,
        pixels: mask,
    }));
    state.brush_dynamics.tip = BrushTip::Bitmap;
    state.brush_dynamics.rotation = 45.0;
    let point = input(32.0, 0.0, 1.0);
    paint(&mut source, &mut state, "brush", &[point]);
    state.seamless_painting = true;
    paint(&mut repeated, &mut state, "brush", &[point]);
    assert!(pixels(&repeated).iter().any(|&a| a > 200));
    // Fold the unbounded raster into the canvas; rotated square corners
    // extend beyond the radius used for round and procedural tips.
    for y in 0..64 {
        for x in 0..64 {
            let mut expected = 0;
            for oy in -3..=3 {
                for ox in -3..=3 {
                    expected = expected.max(alpha(&source, x + ox * 64, y + oy * 64));
                }
            }
            assert_eq!(alpha(&repeated, x, y), expected, "bitmap at {x},{y}");
        }
    }
}
