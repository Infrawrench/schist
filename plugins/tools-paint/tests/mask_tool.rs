use schist_color::{Depth, Rgba};
use schist_core::{live_mask::LiveMask, Document, Layer};
use schist_plugin_api::{EditorState, Modifiers, OptionValue, PointerInput, ToolCtx, ToolPlugin};
use schist_tools_paint::mask::MaskTool;
fn p(x: f32, y: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    }
}
#[test]
fn mask_creation_paint_cancel_undo_and_gradients_share_one_tool() {
    let mut doc = Document::new("test", 100, 100, Depth::Eight);
    let id = doc.push_layer(Layer::new_raster("image"));
    let mut state = EditorState {
        foreground: Rgba::new(0.8, 0.2, 0.3, 1.0),
        brush_size: 12.0,
        brush_hardness: 1.0,
        ..Default::default()
    };
    let mut tool = MaskTool::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, p(30.0, 30.0));
    tool.on_pointer_up(&mut ctx, p(30.0, 30.0));
    assert_eq!(
        ctx.doc
            .tree
            .find(id)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .value(30, 30),
        0
    );
    assert_eq!(ctx.state.foreground, Rgba::new(0.8, 0.2, 0.3, 1.0));
    assert!(ctx.doc.undo().is_some());
    assert!(ctx.doc.tree.find(id).unwrap().mask.is_none());
    assert!(ctx.doc.redo().is_some());
    tool.set_option("mask-mode", OptionValue::Choice(1));
    tool.on_pointer_down(&mut ctx, p(0.0, 0.0));
    tool.on_pointer_up(&mut ctx, p(100.0, 0.0));
    assert!(LiveMask::from_layer(ctx.doc.tree.find(id).unwrap()).is_some());
    assert_eq!(
        ctx.doc
            .tree
            .find(id)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .value(30, 30),
        0
    );
    tool.on_pointer_down(&mut ctx, p(100.0, 0.0));
    tool.on_pointer_move_deferred(&mut ctx, p(50.0, 0.0));
    assert!(tool.flush_preview(&mut ctx));
    assert!(!tool.flush_preview(&mut ctx));
    tool.on_cancel(&mut ctx);
    let live = LiveMask::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap();
    assert_eq!(live.gradients[0].to, (100.0, 0.0));
    tool.set_option("mask-mode", OptionValue::Choice(0));
    tool.on_pointer_down(&mut ctx, p(70.0, 70.0));
    tool.on_pointer_up(&mut ctx, p(70.0, 70.0));
    let live = LiveMask::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap();
    assert_eq!(live.base.restore().unwrap().value(70, 70), 0);
    tool.set_option("mask-mode", OptionValue::Choice(1));
    tool.on_pointer_down(&mut ctx, p(100.0, 0.0));
    tool.on_pointer_up(&mut ctx, p(80.0, 0.0));
    let mask = ctx.doc.tree.find(id).unwrap().mask.as_ref().unwrap();
    assert_eq!(mask.value(30, 30), 0);
    assert_eq!(mask.value(70, 70), 0);
    assert!(ctx.doc.undo().is_some());
    assert_eq!(
        LiveMask::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .gradients[0]
            .to,
        (100.0, 0.0)
    );
}
#[test]
fn locked_layers_and_cancel_do_not_create_masks() {
    let mut doc = Document::new("test", 40, 40, Depth::Eight);
    let id = doc.push_layer(Layer::new_raster("image"));
    let mut state = EditorState::default();
    let mut tool = MaskTool::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, p(10.0, 10.0));
    tool.on_document_leave(&mut ctx);
    assert!(ctx.doc.tree.find(id).unwrap().mask.is_none());
    ctx.doc.tree.find_mut(id).unwrap().locked = true;
    tool.on_pointer_down(&mut ctx, p(10.0, 10.0));
    tool.on_pointer_up(&mut ctx, p(10.0, 10.0));
    assert!(ctx.doc.tree.find(id).unwrap().mask.is_none());
}

#[test]
fn selected_gradient_does_not_follow_a_different_layer() {
    let mut doc = Document::new("layers", 64, 64, Depth::Eight);
    let first = doc.push_layer(Layer::new_raster("first"));
    let mut state = EditorState::default();
    let mut tool = MaskTool::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.set_option("mask-mode", OptionValue::Choice(1));
    tool.on_pointer_down(&mut ctx, p(0.0, 0.0));
    tool.on_pointer_up(&mut ctx, p(64.0, 0.0));
    let original = ctx.doc.tree.find(first).unwrap();
    let mut second = Layer::new_raster("second");
    second.mask = original.mask.clone();
    second.extras = original.extras.clone();
    let second = ctx.doc.push_layer(second);
    assert!(!tool.on_key(&mut ctx, "delete", None, Modifiers::default()));
    tool.set_option("mask-reverse", OptionValue::Bool(true));
    tool.on_option_changed(&mut ctx, "mask-reverse");
    let recipe = LiveMask::from_layer(ctx.doc.tree.find(second).unwrap()).unwrap();
    assert_eq!(recipe.gradients.len(), 1);
    assert!(!recipe.gradients[0].reverse);
}
