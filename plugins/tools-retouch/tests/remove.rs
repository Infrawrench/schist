//! The Remove tool: one stroke takes the object away, as one undo step,
//! through whichever of its paths the stroke and the host call for.

use schist_color::{Depth, Rgba};
use schist_core::{Document, IntRect, Layer, LayerId, SelectOp, TileCoord, TILE_SIZE};
use schist_plugin_api::{
    BackgroundEdit, EditorState, JobControl, Modifiers, OptionValue, Overlay, PointerInput,
    ToolCtx, ToolPlugin,
};
use schist_tools_retouch::remove::{self, Dab, InpaintModel, Method, RemoveOptions};
use schist_tools_retouch::RemoveTool;

fn input(x: f32, y: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    }
}

fn set(doc: &mut Document, layer: LayerId, x: i32, y: i32, c: Rgba) {
    let raster = doc.tree.find_mut(layer).unwrap().as_raster_mut().unwrap();
    let coord = TileCoord::containing(x, y);
    let trect = coord.rect();
    let buf = raster.tiles.get_mut_or_insert(coord, Depth::Eight);
    buf.set(((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize, c);
}

fn pixel(doc: &Document, layer: LayerId, x: i32, y: i32) -> Rgba {
    doc.tree
        .find(layer)
        .unwrap()
        .as_raster()
        .unwrap()
        .tiles
        .pixel(x, y)
}

/// The bottom (photograph) layer.
fn photo(doc: &Document) -> LayerId {
    doc.tree.iter().next().unwrap().id
}

fn get(doc: &Document, x: i32, y: i32) -> Rgba {
    pixel(doc, photo(doc), x, y)
}

/// 120x120 of fine grain over a warm mid-tone, with a dark bar across
/// the middle at y 56..64, x 20..100: the object to remove.
fn scene() -> Document {
    let mut doc = Document::new("t", 120, 120, Depth::Eight);
    let layer = Layer::new_raster("bg");
    let id = layer.id;
    doc.push_layer(layer);
    let mut seed = 0x2545_f491u32;
    for y in 0..120 {
        for x in 0..120 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (seed >> 8) as f32 / (1u32 << 24) as f32;
            let v = 0.55 + (n - 0.5) * 0.12;
            let c = match (56..64).contains(&y) && (20..100).contains(&x) {
                true => Rgba::new(0.05, 0.05, 0.08, 1.0),
                false => Rgba::new(v, v * 0.92, v * 0.8, 1.0),
            };
            set(&mut doc, id, x, y, c);
        }
    }
    doc.active_layer = Some(id);
    doc
}

fn state(size: f32) -> EditorState {
    EditorState {
        brush_size: size,
        ..EditorState::default()
    }
}

fn paint(tool: &mut RemoveTool, doc: &mut Document, state: &mut EditorState, pts: &[(f32, f32)]) {
    let mut ctx = ToolCtx { doc, state };
    tool.on_pointer_down(&mut ctx, input(pts[0].0, pts[0].1));
    for &(x, y) in &pts[1..] {
        tool.on_pointer_move(&mut ctx, input(x, y));
    }
    let last = pts[pts.len() - 1];
    tool.on_pointer_up(&mut ctx, input(last.0, last.1));
}

/// Across the whole bar, the darkest red: 0.05 while it is there.
fn darkest_in_bar(doc: &Document) -> f32 {
    let mut low = 1.0f32;
    for y in 56..64 {
        for x in 20..100 {
            low = low.min(get(doc, x, y).r);
        }
    }
    low
}

const ACROSS: &[(f32, f32)] = &[(18.0, 60.0), (60.0, 60.5), (102.0, 60.0)];

fn line(x0: f32, x1: f32, y: f32, r: f32) -> Vec<Dab> {
    (0..=40)
        .map(|i| Dab {
            x: x0 + (x1 - x0) * i as f32 / 40.0,
            y,
            r,
        })
        .collect()
}

const OPTS: RemoveOptions = RemoveOptions {
    sample_all_layers: false,
};

#[test]
fn one_stroke_removes_the_object_as_one_undo_step() {
    let mut doc = scene();
    let before = doc.history.entries().len();
    let mut tool = RemoveTool::default_tool();
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    assert!(
        darkest_in_bar(&doc) > 0.3,
        "the bar is still there: {}",
        darkest_in_bar(&doc)
    );
    // Away from the stroke nothing moved.
    assert_eq!(get(&doc, 60, 10), get(&scene(), 60, 10));
    assert_eq!(doc.history.entries().len(), before + 1);
    assert_eq!(doc.history.undo_name(), Some("Remove"));
    doc.undo();
    assert!(
        darkest_in_bar(&doc) < 0.1,
        "undo did not bring the bar back"
    );
    assert_eq!(get(&doc, 60, 60), get(&scene(), 60, 60));
}

#[test]
fn strokes_wait_for_enter_when_not_removing_after_each() {
    let mut doc = scene();
    let before = doc.history.entries().len();
    let mut tool = RemoveTool::default_tool();
    tool.set_option("remove-each-stroke", OptionValue::Bool(false));
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, &[(18.0, 60.0), (60.0, 60.0)]);
    paint(&mut tool, &mut doc, &mut st, &[(60.0, 60.0), (102.0, 60.0)]);
    assert_eq!(tool.pending_strokes(), 2);
    assert!(darkest_in_bar(&doc) < 0.1, "removed before Enter");
    let strokes = tool
        .overlays(&doc, &st)
        .iter()
        .filter(|o| matches!(o, Overlay::Stroke { .. }))
        .count();
    assert_eq!(strokes, 2, "both waiting strokes should show");
    tool.on_commit(&mut ToolCtx {
        doc: &mut doc,
        state: &mut st,
    });
    assert_eq!(tool.pending_strokes(), 0);
    assert!(darkest_in_bar(&doc) > 0.3, "Enter did not remove the bar");
    assert_eq!(
        doc.history.entries().len(),
        before + 1,
        "two strokes, one step"
    );
}

#[test]
fn escape_drops_strokes_waiting_for_enter() {
    let mut doc = scene();
    let mut tool = RemoveTool::default_tool();
    tool.set_option("remove-each-stroke", OptionValue::Bool(false));
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut st,
    };
    tool.on_cancel(&mut ctx);
    tool.on_commit(&mut ctx);
    assert!(darkest_in_bar(&doc) < 0.1);
}

#[test]
fn thin_strokes_use_spot_healing_and_thick_ones_the_fill() {
    let doc = scene();
    let thin = remove::prepare(&doc, &[line(30.0, 90.0, 30.0, 0.75)], OPTS).unwrap();
    assert_eq!(thin.method(), Method::SpotHeal);
    let thick = remove::prepare(&doc, &[line(30.0, 90.0, 60.0, 8.0)], OPTS).unwrap();
    assert_eq!(thick.method(), Method::Fill);
    // The window is the hole plus context, inside the canvas.
    let w = thick.window();
    assert!(w.left <= 30 - 8 && w.right >= 90 + 8, "{w:?}");
    assert_eq!(w.intersect(&doc.canvas_rect()), w);
}

#[test]
fn a_thin_stroke_is_healed_from_its_sides() {
    // A one-pixel scratch on the flat-ish field.
    let mut doc = scene();
    let id = photo(&doc);
    for x in 30..90 {
        set(&mut doc, id, x, 30, Rgba::new(1.0, 1.0, 1.0, 1.0));
    }
    let mut tool = RemoveTool::default_tool();
    let mut st = state(2.0);
    paint(&mut tool, &mut doc, &mut st, &[(29.0, 30.5), (91.0, 30.5)]);
    for x in 30..90 {
        let p = get(&doc, x, 30);
        assert!((p.r - 0.55).abs() < 0.08, "scratch left at x={x}: {p:?}");
    }
}

#[test]
fn without_the_model_the_classical_fill_runs_and_says_so() {
    let mut doc = scene();
    let prepared = remove::prepare(&doc, &[line(18.0, 102.0, 60.0, 8.0)], OPTS).unwrap();
    assert_eq!(prepared.method(), Method::Fill);
    let control = JobControl::new();
    let finished = remove::run_with(prepared, &control, InpaintModel::Missing).unwrap();
    assert_eq!(control.missing_model(), Some("inpaint"));
    assert!(remove::apply(&mut doc, finished, "Remove"));
    assert!(darkest_in_bar(&doc) > 0.3, "{}", darkest_in_bar(&doc));
    // With the model (built in natively) there is nothing to report.
    let control = JobControl::new();
    let prepared = remove::prepare(&scene(), &[line(18.0, 102.0, 60.0, 8.0)], OPTS).unwrap();
    remove::run(prepared, &control).unwrap();
    assert_eq!(control.missing_model(), None);
}

#[test]
fn a_cancelled_removal_writes_nothing() {
    let doc = scene();
    let prepared = remove::prepare(&doc, &[line(18.0, 102.0, 60.0, 8.0)], OPTS).unwrap();
    let control = JobControl::new();
    control.cancel();
    assert!(remove::run(prepared, &control).is_none());
}

#[test]
fn background_hosts_get_an_edit_instead_of_a_change() {
    let mut doc = scene();
    let mut tool = RemoveTool::default_tool();
    tool.set_background_edits(true);
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    assert!(
        darkest_in_bar(&doc) < 0.1,
        "a background host's edit ran inline"
    );
    let edit: BackgroundEdit = tool.take_background_edit().expect("an edit");
    assert!(tool.take_background_edit().is_none());
    assert!(!edit.overlay.is_empty(), "the stroke should stay on screen");
    // The host's three stages, by hand: prepare on the UI thread, run on
    // a worker, apply back on the UI thread.
    let run = (edit.prepare)(&doc).unwrap();
    let control = JobControl::new();
    let worker = control.clone();
    let apply = std::thread::spawn(move || run(&worker))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(control.progress(), 1.0);
    assert!(apply(&mut doc));
    assert!(darkest_in_bar(&doc) > 0.3);
    // Prepared again from the changed document, it still runs: what a
    // host does when an undo lands while a removal is running.
    doc.undo();
    assert!((edit.prepare)(&doc).is_some());
}

#[test]
fn sampling_all_layers_writes_into_an_empty_layer() {
    let mut doc = scene();
    let bottom = photo(&doc);
    let empty = Layer::new_raster("empty");
    let empty_id = empty.id;
    doc.push_layer(empty);
    doc.active_layer = Some(empty_id);
    let mut tool = RemoveTool::default_tool();
    tool.set_option("remove-sample-all", OptionValue::Bool(true));
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    // The bar is untouched on its own layer and covered on the new one.
    assert!(pixel(&doc, bottom, 60, 60).r < 0.1);
    let cover = pixel(&doc, empty_id, 60, 60);
    assert!(cover.a > 0.99 && cover.r > 0.3, "{cover:?}");
    // Outside the stroke the empty layer stays empty.
    assert_eq!(pixel(&doc, empty_id, 60, 10).a, 0.0);
}

#[test]
fn sampling_all_layers_with_no_pixel_layer_active_makes_one() {
    let mut doc = scene();
    let count = doc.tree.iter().count();
    let group = Layer::new_group("group");
    let group_id = group.id;
    doc.push_layer(group);
    doc.active_layer = Some(group_id);
    let mut tool = RemoveTool::default_tool();
    let mut st = state(16.0);
    // Without Sample All Layers there is nowhere to write.
    let before = doc.history.entries().len();
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    assert_eq!(doc.history.entries().len(), before);
    tool.set_option("remove-sample-all", OptionValue::Bool(true));
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    assert_eq!(doc.tree.iter().count(), count + 2);
    let made = doc.active_layer.unwrap();
    assert_ne!(made, group_id);
    assert_eq!(doc.tree.find(made).unwrap().name, "Removed");
    assert!(pixel(&doc, made, 60, 60).r > 0.3);
    // One undo takes the layer and its pixels away together.
    doc.undo();
    assert_eq!(doc.tree.iter().count(), count + 1);
}

#[test]
fn a_selection_confines_the_removal() {
    let mut doc = scene();
    doc.selection
        .select_rect(IntRect::new(0, 0, 60, 120), SelectOp::Replace);
    let mut tool = RemoveTool::default_tool();
    let mut st = state(16.0);
    paint(&mut tool, &mut doc, &mut st, ACROSS);
    assert!(
        get(&doc, 40, 60).r > 0.3,
        "inside the selection was not removed"
    );
    assert!(
        get(&doc, 80, 60).r < 0.1,
        "outside the selection was changed"
    );
}

#[test]
fn pen_pressure_sizes_the_stroke() {
    let doc = scene();
    let mut tool = RemoveTool::default_tool();
    let mut doc2 = scene();
    let mut st = state(40.0);
    let mut ctx = ToolCtx {
        doc: &mut doc2,
        state: &mut st,
    };
    let light = PointerInput {
        pressure: 0.25,
        ..input(60.0, 60.0)
    };
    tool.on_pointer_down(&mut ctx, light);
    let dabs = tool
        .overlays(&doc, &state(40.0))
        .into_iter()
        .find_map(|o| match o {
            Overlay::Stroke { dabs } => Some(dabs),
            _ => None,
        })
        .unwrap();
    assert!((dabs[0].2 - 5.0).abs() < 1e-4, "{dabs:?}");
}
