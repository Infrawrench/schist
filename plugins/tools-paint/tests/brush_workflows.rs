use schist_color::{Depth, Rgba};
use schist_core::{Document, Layer};
use schist_plugin_api::{BrushDynamics, BrushTip, EditorState, Modifiers, PointerInput, ToolCtx};

fn point(x: f32, y: f32, pressure: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure,
        modifiers: Modifiers::default(),
    }
}

fn document() -> Document {
    let mut doc = Document::new("brush test", 180, 140, Depth::Eight);
    let layer = Layer::new_raster("paint");
    let id = layer.id;
    doc.push_layer(layer);
    doc.active_layer = Some(id);
    doc
}

fn alpha(doc: &Document, x: i32, y: i32) -> f32 {
    doc.tree
        .find(doc.active_layer.unwrap())
        .unwrap()
        .as_raster()
        .unwrap()
        .tiles
        .pixel(x, y)
        .a
}

fn pixels(doc: &Document) -> Vec<u8> {
    (0..140)
        .flat_map(|y| (0..180).map(move |x| (alpha(doc, x, y) * 255.0).round() as u8))
        .collect()
}

fn paint(
    doc: &mut Document,
    state: &mut EditorState,
    path: &[PointerInput],
    release: PointerInput,
) {
    let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
    let mut ctx = ToolCtx { doc, state };
    tool.on_pointer_down(&mut ctx, path[0]);
    for &input in &path[1..] {
        tool.on_pointer_move(&mut ctx, input);
    }
    tool.on_pointer_up(&mut ctx, release);
}

#[test]
fn textured_tips_and_scatter_change_pixels_reproducibly() {
    let make = |tip, scatter| {
        let mut doc = document();
        let mut state = EditorState {
            brush_size: 36.0,
            brush_hardness: 1.0,
            brush_dynamics: BrushDynamics {
                tip,
                scatter,
                ..Default::default()
            },
            ..Default::default()
        };
        paint(
            &mut doc,
            &mut state,
            &[point(70.0, 60.0, 1.0)],
            point(70.0, 60.0, 1.0),
        );
        pixels(&doc)
    };
    let round = make(BrushTip::Round, 0.0);
    let grain = make(BrushTip::Grain, 0.0);
    let bristles = make(BrushTip::Bristles, 0.0);
    assert_ne!(round, grain);
    assert_ne!(grain, bristles);
    assert_ne!(round, bristles);
    assert_eq!(grain, make(BrushTip::Grain, 0.0));
    assert!(
        grain.iter().map(|&v| v as u32).sum::<u32>() < round.iter().map(|&v| v as u32).sum::<u32>()
    );
    assert_ne!(round, make(BrushTip::Round, 1.0));
    assert_eq!(make(BrushTip::Grain, 1.0), make(BrushTip::Grain, 1.0));
}

#[test]
fn pressure_curve_changes_size_and_zero_pressure_does_not_paint() {
    let make = |gamma, pressure| {
        let mut doc = document();
        let mut state = EditorState {
            brush_size: 40.0,
            brush_hardness: 1.0,
            brush_dynamics: BrushDynamics {
                pressure_gamma: gamma,
                ..Default::default()
            },
            ..Default::default()
        };
        paint(
            &mut doc,
            &mut state,
            &[point(70.0, 60.0, pressure)],
            point(70.0, 60.0, pressure),
        );
        pixels(&doc).into_iter().filter(|&v| v > 0).count()
    };
    assert_eq!(make(1.0, 0.0), 0);
    assert!(make(0.5, 0.5) > make(1.0, 0.5));
    assert!(make(1.0, 0.5) > make(2.0, 0.5));
    assert_eq!(make(0.5, 1.0), make(2.0, 1.0));
}

#[test]
fn stationary_pressure_changes_paint_immediately_with_smoothing_enabled() {
    for stabilization in [0.0, 12.0] {
        let mut doc = document();
        let mut state = EditorState {
            brush_size: 30.0,
            brush_hardness: 1.0,
            brush_dynamics: BrushDynamics {
                stabilization,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
        let mut ctx = ToolCtx {
            doc: &mut doc,
            state: &mut state,
        };
        tool.on_pointer_down(&mut ctx, point(50.5, 50.5, 0.0));
        assert_eq!(alpha(ctx.doc, 50, 50), 0.0);
        tool.on_pointer_move(&mut ctx, point(50.5, 50.5, 0.25));
        assert!(alpha(ctx.doc, 50, 50) > 0.9);
        assert_eq!(alpha(ctx.doc, 60, 50), 0.0);
        tool.on_pointer_move(&mut ctx, point(50.5, 50.5, 1.0));
        assert!(alpha(ctx.doc, 60, 50) > 0.9);
        tool.on_cancel(&mut ctx);
        assert!(pixels(&doc).iter().all(|&a| a == 0));
    }
}

#[test]
fn pressure_and_scatter_are_independent_of_pointer_event_batching() {
    for stabilization in [0.0, 12.0] {
        let draw = |step: usize| {
            let mut doc = document();
            let mut state = EditorState {
                brush_size: 20.0,
                brush_dynamics: BrushDynamics {
                    scatter: 0.8,
                    stabilization,
                    ..Default::default()
                },
                ..Default::default()
            };
            let path: Vec<_> = (0..=100)
                .step_by(step)
                .map(|i| point(30.0 + i as f32, 65.0, 0.25 + 0.75 * i as f32 / 100.0))
                .collect();
            paint(&mut doc, &mut state, &path, point(130.0, 65.0, 0.0));
            pixels(&doc)
        };
        let sparse = draw(100);
        let dense = draw(5);
        assert!(
            sparse.iter().zip(&dense).all(|(a, b)| a.abs_diff(*b) <= 1),
            "event batching changed stroke at smoothing {stabilization}"
        );
    }
}

#[test]
fn smoothing_reduces_jitter_and_release_flushes_short_and_long_tails() {
    let draw = |stabilization| {
        let mut doc = document();
        let mut state = EditorState {
            brush_size: 4.0,
            brush_hardness: 1.0,
            brush_dynamics: BrushDynamics {
                stabilization,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut path = vec![point(20.0, 60.0, 1.0)];
        path.extend((1..=18).map(|i| {
            point(
                20.0 + i as f32 * 5.0,
                60.0 + if i % 2 == 0 { -8.0 } else { 8.0 },
                1.0,
            )
        }));
        paint(&mut doc, &mut state, &path, point(140.0, 60.0, 0.0));
        doc
    };
    let rough = draw(0.0);
    let smooth = draw(12.0);
    let variance = |doc: &Document| {
        let mut weighted = 0.0;
        let mut mass = 0.0;
        for y in 40..80 {
            for x in 40..100 {
                let a = alpha(doc, x, y);
                weighted += a * (y as f32 + 0.5 - 60.0).powi(2);
                mass += a;
            }
        }
        weighted / mass
    };
    assert!(variance(&smooth) < variance(&rough) * 0.5);
    assert!(alpha(&smooth, 139, 60) > 0.9, "release location was lost");
    let mut doc = document();
    let mut state = EditorState {
        brush_size: 2.0,
        brush_dynamics: BrushDynamics {
            stabilization: 64.0,
            spacing: 2.0,
            ..Default::default()
        },
        ..Default::default()
    };
    paint(
        &mut doc,
        &mut state,
        &[point(20.5, 60.5, 1.0)],
        point(23.5, 60.5, 0.0),
    );
    assert!(alpha(&doc, 23, 60) > 0.9, "sub-spacing tail was lost");
}

#[test]
fn textured_stroke_obeys_opacity_and_is_one_undoable_edit() {
    let mut doc = document();
    let mut state = EditorState {
        foreground: Rgba::BLACK,
        tool_opacity: 0.5,
        brush_size: 20.0,
        brush_dynamics: BrushDynamics {
            tip: BrushTip::Grain,
            scatter: 0.2,
            stabilization: 8.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let before = pixels(&doc);
    let history = doc.history.entries().len();
    let path = [
        point(40.0, 60.0, 1.0),
        point(100.0, 60.0, 1.0),
        point(40.0, 60.0, 1.0),
    ];
    paint(&mut doc, &mut state, &path, path[2]);
    let after = pixels(&doc);
    assert_ne!(before, after);
    assert!(after.iter().all(|&a| a <= 128));
    assert_eq!(doc.history.entries().len(), history + 1);
    doc.undo();
    assert_eq!(pixels(&doc), before);
    doc.redo();
    assert_eq!(pixels(&doc), after);
}

#[test]
fn cancelled_and_malformed_strokes_do_not_leave_paint_or_history() {
    let mut doc = document();
    let mut state = EditorState {
        brush_dynamics: BrushDynamics {
            tip: BrushTip::Bristles,
            stabilization: 20.0,
            scatter: 1.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let before = pixels(&doc);
    let history = doc.history.entries().len();
    let mut tool = schist_tools_paint::tool_for_test("brush").unwrap();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, point(40.0, 60.0, 1.0));
    tool.on_pointer_move(&mut ctx, point(100.0, 60.0, 1.0));
    tool.on_cancel(&mut ctx);
    tool.on_pointer_down(&mut ctx, point(f32::NAN, 60.0, 1.0));
    tool.on_pointer_up(&mut ctx, point(40.0, 60.0, 1.0));
    assert_eq!(pixels(&doc), before);
    assert_eq!(doc.history.entries().len(), history);
}

#[test]
fn selection_clips_scattered_textured_strokes() {
    let mut doc = document();
    doc.selection.apply_shape(
        schist_core::IntRect::new(50, 50, 90, 80),
        schist_core::SelectOp::Replace,
        |_, _| 128,
    );
    let mut state = EditorState {
        brush_size: 30.0,
        brush_dynamics: BrushDynamics {
            tip: BrushTip::Grain,
            scatter: 1.0,
            stabilization: 8.0,
            ..Default::default()
        },
        ..Default::default()
    };
    paint(
        &mut doc,
        &mut state,
        &[point(20.0, 65.0, 1.0)],
        point(130.0, 65.0, 0.0),
    );
    assert!(pixels(&doc).iter().any(|&a| a > 0));
    for y in 0..140 {
        for x in 0..180 {
            let a = alpha(&doc, x, y);
            if (50..90).contains(&x) && (50..80).contains(&y) {
                assert!(a <= 128.0 / 255.0 + 0.001);
            } else {
                assert_eq!(a, 0.0);
            }
        }
    }
}
