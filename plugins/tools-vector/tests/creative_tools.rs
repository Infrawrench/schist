use schist_color::{Depth, Rgba};
use schist_core::{
    model3d::{Model3d, Placement},
    vector_blend::VectorBlend,
    Anchor, Document, Layer, SubPath, VectorPath, VectorShape,
};
use schist_plugin_api::{EditorState, Modifiers, OptionValue, PointerInput, ToolCtx, ToolPlugin};
use schist_tools_vector::{
    blend::{self, BlendTool},
    model3d::ModelTool,
    paths::{ArrowKind, PathSelectTool},
    render_shape,
};

fn p(x: f32, y: f32) -> PointerInput {
    PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    }
}
fn shape(doc: &Document, x: f32) -> Layer {
    let mut path = VectorPath::new("rectangle");
    path.subpaths.push(SubPath {
        closed: true,
        anchors: vec![
            Anchor::corner(x, 20.0),
            Anchor::corner(x + 20.0, 20.0),
            Anchor::corner(x + 20.0, 40.0),
            Anchor::corner(x, 40.0),
        ],
    });
    let shape = VectorShape::new(path, Rgba::WHITE);
    let mut layer = Layer::new_raster("shape");
    layer.as_raster_mut().unwrap().tiles = render_shape(&shape, doc.depth, doc.canvas_rect());
    layer.shape = Some(Box::new(shape));
    layer
}

#[test]
fn blend_creation_mapping_guides_cancel_and_undo() {
    let mut doc = Document::new("blend", 160, 100, Depth::Eight);
    let a = doc.push_layer(shape(&doc, 10.0));
    let b = doc.push_layer(shape(&doc, 110.0));
    let id = blend::create(&mut doc, a, b).unwrap();
    assert!(!doc.tree.find(a).unwrap().visible);
    doc.undo();
    assert!(doc.tree.find(a).unwrap().visible);
    assert!(doc.tree.find(id).is_none());
    doc.redo();
    doc.active_layer = Some(id);
    let original = VectorBlend::from_layer(doc.tree.find(id).unwrap()).unwrap();
    let mut state = EditorState::default();
    let mut tool = BlendTool::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_activate(&mut ctx);
    tool.on_pointer_down(&mut ctx, p(10.0, 20.0));
    tool.on_pointer_move_deferred(&mut ctx, p(14.0, 15.0));
    tool.on_pointer_move_deferred(&mut ctx, p(15.0, 15.0));
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap(),
        original
    );
    assert!(tool.flush_preview(&mut ctx));
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .start
            .path
            .subpaths[0]
            .anchors[0]
            .point,
        (15.0, 15.0)
    );
    assert!(!tool.flush_preview(&mut ctx));
    assert_eq!(
        VectorBlend::from_layer(tool.committed_layer().unwrap()).unwrap(),
        original
    );
    tool.on_document_leave(&mut ctx);
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap(),
        original
    );
    tool.set_option("blend-mode", OptionValue::Choice(3));
    tool.on_pointer_down(&mut ctx, p(10.0, 20.0));
    tool.on_pointer_up(&mut ctx, p(10.0, 20.0));
    tool.on_pointer_down(&mut ctx, p(130.0, 40.0));
    tool.on_pointer_up(&mut ctx, p(130.0, 40.0));
    let mapped = VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap();
    assert_eq!(mapped.mapping[0], [2, 1, 0, 3]);
    ctx.doc.undo();
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap()).unwrap(),
        original
    );
    tool.set_option("blend-mode", OptionValue::Choice(1));
    tool.on_pointer_down(&mut ctx, p(20.0, 60.0));
    tool.on_pointer_move(&mut ctx, p(70.0, 80.0));
    tool.on_pointer_up(&mut ctx, p(120.0, 60.0));
    let spine = VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
        .unwrap()
        .spine
        .unwrap();
    tool.on_pointer_down(&mut ctx, p(70.0, 80.0));
    tool.on_pointer_up(&mut ctx, p(70.0, 70.0));
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .spine
            .unwrap()
            .subpaths[0]
            .anchors[1]
            .point,
        (70.0, 70.0)
    );
    ctx.doc.undo();
    assert_eq!(
        VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .spine
            .unwrap(),
        spine
    );
    tool.set_option("blend-mode", OptionValue::Choice(2));
    tool.on_pointer_down(&mut ctx, p(20.0, 20.0));
    tool.on_pointer_up(&mut ctx, p(120.0, 20.0));
    assert!(VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
        .unwrap()
        .rail
        .is_some());
    ctx.doc.undo();
    assert!(VectorBlend::from_layer(ctx.doc.tree.find(id).unwrap())
        .unwrap()
        .rail
        .is_none());
}

#[test]
fn direct_selection_restores_pixels_even_after_shell_refresh() {
    let mut doc = Document::new("shape", 160, 100, Depth::Eight);
    let id = doc.push_layer(shape(&doc, 10.0));
    let mut tool = PathSelectTool::new(ArrowKind::Direct);
    let mut state = EditorState::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_pointer_down(&mut ctx, p(10.0, 20.0));
    tool.on_pointer_move(&mut ctx, p(50.0, 10.0));
    // The editor refreshes shape rasters during a gesture; exercise the same
    // transition before committing, rather than only changing path metadata.
    let tiles = render_shape(
        ctx.doc.tree.find(id).unwrap().shape.as_ref().unwrap(),
        ctx.doc.depth,
        ctx.doc.canvas_rect(),
    );
    ctx.doc
        .tree
        .find_mut(id)
        .unwrap()
        .as_raster_mut()
        .unwrap()
        .tiles = tiles;
    assert_eq!(
        tool.committed_layer()
            .unwrap()
            .shape
            .as_ref()
            .unwrap()
            .path
            .subpaths[0]
            .anchors[0]
            .point,
        (10.0, 20.0)
    );
    tool.on_pointer_up(&mut ctx, p(50.0, 10.0));
    ctx.doc.undo();
    let layer = ctx.doc.tree.find(id).unwrap();
    assert_eq!(
        layer.shape.as_ref().unwrap().path.subpaths[0].anchors[0].point,
        (10.0, 20.0)
    );
    assert!(layer.as_raster().unwrap().tiles.pixel(12, 22).a > 0.99);
    assert!(layer.as_raster().unwrap().tiles.pixel(42, 15).a < 0.01);
    ctx.doc.redo();
    assert_eq!(
        ctx.doc
            .tree
            .find(id)
            .unwrap()
            .shape
            .as_ref()
            .unwrap()
            .path
            .subpaths[0]
            .anchors[0]
            .point,
        (50.0, 10.0)
    );
}

#[test]
fn model_drag_is_relative_to_start_and_undo_restores_source() {
    let mut doc = Document::new("3d", 100, 100, Depth::Eight);
    let mesh = schist_model3d::import(b"v -1 -1 0\nv 1 -1 0\nv 0 1 0\nf 1 2 3\n", "obj").unwrap();
    let model = Model3d {
        mesh,
        placement: Placement::fitted(100, 100),
    };
    let id = doc
        .push_layer(schist_model3d::layer(&model, doc.depth, doc.canvas_rect(), "model").unwrap());
    let mut tool = ModelTool::default();
    let mut state = EditorState::default();
    let mut ctx = ToolCtx {
        doc: &mut doc,
        state: &mut state,
    };
    tool.on_activate(&mut ctx);
    tool.on_pointer_down(&mut ctx, p(50.0, 50.0));
    tool.on_pointer_move(&mut ctx, p(60.0, 50.0));
    tool.on_pointer_move(&mut ctx, p(70.0, 50.0));
    tool.on_pointer_up(&mut ctx, p(70.0, 50.0));
    assert_eq!(
        Model3d::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .placement
            .rotation[1],
        10.0
    );
    ctx.doc.undo();
    assert_eq!(
        Model3d::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .placement,
        model.placement
    );
    tool.set_option("model-mode", OptionValue::Choice(1));
    tool.on_pointer_down(&mut ctx, p(50.0, 50.0));
    tool.on_pointer_move(&mut ctx, p(75.0, 50.0));
    tool.on_cancel(&mut ctx);
    assert_eq!(
        Model3d::from_layer(ctx.doc.tree.find(id).unwrap())
            .unwrap()
            .placement,
        model.placement
    );
    assert!(
        ctx.doc
            .tree
            .find(id)
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .pixel(50, 50)
            .a
            > 0.99
    );
}
