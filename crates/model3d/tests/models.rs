use schist_color::Depth;
use schist_core::{Affine, Document, IntRect};
use schist_model3d::{import, layer, render, Model3d, Placement};

const OBJ: &[u8] = b"v -1 -1 0 1 0 0\nv 1 -1 0 1 0 0\nv 0 1 0 1 0 0\nf -3 -2 -1\n";
fn model() -> Model3d {
    Model3d {
        mesh: import(OBJ, "obj").unwrap(),
        placement: Placement::fitted(64, 64),
    }
}
#[test]
fn obj_negative_indices_and_concave_polygons() {
    let mesh = import(OBJ, "obj").unwrap();
    assert_eq!(mesh.triangles.len(), 1);
    assert!(mesh.vertices[0].normal[2] > 0.9);
    let concave = b"v 0 0 0\nv 2 0 0\nv 2 2 0\nv 1 1 0\nv 0 2 0\nf 1 2 3 4 5\n";
    assert_eq!(import(concave, "obj").unwrap().triangles.len(), 3);
    assert!(import(b"v NaN 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3", "obj").is_err());
    assert!(import(b"v 0 0 0\nf 1 2 99", "obj").is_err());
}
#[test]
fn rendering_responds_to_lighting_rotation_scale_and_depth() {
    let mut model = model();
    let canvas = IntRect::from_size(64, 64);
    let first = render(&model, Depth::Eight, canvas).unwrap();
    assert!(first.pixel(32, 32).a > 0.9);
    assert!(first.pixel(2, 2).a < 0.01);
    model.placement.light_intensity = 0.0;
    model.placement.ambient = 0.1;
    let dark = render(&model, Depth::Eight, canvas).unwrap();
    assert!(first.pixel(32, 32).r > dark.pixel(32, 32).r + 0.2);
    model.placement.rotation[1] = 90.0;
    let rotated = render(&model, Depth::Eight, canvas).unwrap();
    assert!(rotated.pixel(32, 32).a < 0.01);
    model.placement.rotation[1] = 0.0;
    model.placement.scale = 4.0;
    let small = render(&model, Depth::Eight, canvas).unwrap();
    assert!(small.pixel(24, 40).a < 0.01);
    assert!(first.pixel(24, 40).a > 0.9);
    // Two coincident projected triangles; the front green one must win,
    // independent of file order.
    let obj=b"v -1 -1 0 1 0 0\nv 1 -1 0 1 0 0\nv 0 1 0 1 0 0\nv -1 -1 0.5 0 1 0\nv 1 -1 0.5 0 1 0\nv 0 1 0.5 0 1 0\nf 4 5 6\nf 1 2 3";
    model.mesh = import(obj, "obj").unwrap();
    model.placement = Placement::fitted(64, 64);
    let pixel = render(&model, Depth::Eight, canvas).unwrap().pixel(32, 32);
    assert!(pixel.g > pixel.r + 0.2);
}
#[test]
fn retained_model_survives_psd_move_transform_undo_and_raster_edit() {
    let mut doc = Document::new("model", 64, 64, Depth::Eight);
    let model = model();
    let id = doc.push_layer(layer(&model, doc.depth, doc.canvas_rect(), "model").unwrap());
    let mut edit = doc.begin_edit("move");
    edit.translate_layer(id, 5, 3);
    edit.commit();
    let moved = Model3d::from_layer(doc.tree.find(id).unwrap()).unwrap();
    assert_eq!(moved.placement.transform.tx, 5.0);
    doc.undo();
    assert_eq!(
        Model3d::from_layer(doc.tree.find(id).unwrap())
            .unwrap()
            .placement
            .transform
            .tx,
        0.0
    );
    doc.redo();
    let canvas = doc.canvas_rect();
    let mut edit = doc.begin_edit("transform");
    edit.transform_layer(
        id,
        &Affine::scale(0.5, 0.8),
        schist_core::Filter::Bilinear,
        canvas,
    );
    edit.commit();
    let transformed = Model3d::from_layer(doc.tree.find(id).unwrap()).unwrap();
    assert_eq!(transformed.placement.transform.tx, 2.5);
    assert_eq!(transformed.placement.transform.a, 0.5);
    // The GUI captures an identity source, then commits a composed transform.
    let prepared = schist_core::filter_stack::LayerTransform::prepare(
        doc.tree.find(id).unwrap(),
        &Affine::IDENTITY,
        schist_core::Filter::Bilinear,
    )
    .unwrap();
    let prepared = prepared
        .then(&Affine::translate(7.0, 0.0), schist_core::Filter::Bilinear)
        .unwrap();
    let mut preview = doc.tree.find(id).unwrap().clone();
    preview.extras = prepared.extras;
    assert_eq!(
        Model3d::from_layer(&preview)
            .unwrap()
            .placement
            .transform
            .tx,
        9.5
    );
    let encoded = schist_codec_psd::write_psd(&doc).unwrap();
    let reopened = schist_codec_psd::read_psd(&encoded).unwrap();
    let saved = Model3d::from_layer(&reopened.tree.layers[0]).unwrap();
    assert_eq!(saved.placement, transformed.placement);
    assert_eq!(saved.mesh.vertices.len(), 3);
    let mut edit = doc.begin_edit("paint");
    edit.writable_tile(id, schist_core::TileCoord::containing(0, 0))
        .unwrap()
        .set(0, schist_color::Rgba::WHITE);
    edit.commit();
    assert!(Model3d::from_layer(doc.tree.find(id).unwrap()).is_none());
    doc.undo();
    assert!(Model3d::from_layer(doc.tree.find(id).unwrap()).is_some());
    let mut stroke = schist_core::StrokeEdit::new("brush");
    stroke
        .writable_tile(&mut doc, id, schist_core::TileCoord::containing(0, 0))
        .unwrap()
        .set(0, schist_color::Rgba::WHITE);
    stroke.commit(&mut doc);
    assert!(Model3d::from_layer(doc.tree.find(id).unwrap()).is_none());
    doc.undo();
    assert!(Model3d::from_layer(doc.tree.find(id).unwrap()).is_some());
    let layer = doc.tree.find(id).unwrap();
    let stack = schist_core::filter_stack::FilterStack::new(doc.canvas_rect());
    let extras = stack
        .blocks(layer, &layer.as_raster().unwrap().tiles)
        .unwrap();
    assert!(!extras
        .iter()
        .any(|block| block.key == schist_core::model3d::BLOCK));
}
#[test]
fn binary_stl_and_glb_import() {
    let mut stl = vec![0; 80];
    stl.extend(1u32.to_le_bytes());
    for f in [0f32, 0., 1., -1., -1., 0., 1., -1., 0., 0., 1., 0.] {
        stl.extend(f.to_le_bytes());
    }
    stl.extend(0u16.to_le_bytes());
    assert_eq!(import(&stl, "stl").unwrap().triangles.len(), 1);
    assert!(import(&stl[..stl.len() - 1], "stl").is_err());
    let mut binary = Vec::new();
    for f in [-1f32, -1., 0., 1., -1., 0., 0., 1., 0.] {
        binary.extend(f.to_le_bytes());
    }
    let json = serde_json::json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-1,-1,0],"max":[1,1,0]}],"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,1]}}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],"nodes":[{"mesh":0,"scale":[2,1,1]}],"scenes":[{"nodes":[0]}],"scene":0});
    let mut json = serde_json::to_vec(&json).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    glb.extend(((12 + 8 + json.len() + 8 + binary.len()) as u32).to_le_bytes());
    glb.extend((json.len() as u32).to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json);
    glb.extend((binary.len() as u32).to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(binary);
    let mesh = import(&glb, "glb").unwrap();
    assert_eq!(mesh.triangles.len(), 1);
    assert_eq!(mesh.vertices[0].color, [0.0, 1.0, 0.0, 1.0]);
    assert!(mesh.vertices[1].position[0] > mesh.vertices[2].position[1]);
    glb.truncate(glb.len() - 1);
    assert!(import(&glb, "glb").is_err());
}

#[test]
fn psd_round_trip_preserves_blend_and_painted_gradient_mask() {
    use schist_core::{
        live_mask::{LiveMask, MaskGradient, MaskSnapshot},
        vector_blend::VectorBlend,
        Anchor, Layer, LayerMask, VectorPath, VectorShape,
    };
    let mut path = VectorPath::new("shape");
    path.push_open_anchors(vec![Anchor::corner(0.0, 0.0), Anchor::corner(10.0, 0.0)]);
    let start = VectorShape::new(path, schist_color::Rgba::WHITE);
    let mut end = start.clone();
    end.path.translate(30.0, 20.0);
    let blend = VectorBlend::new(start, end);
    let mut layer = Layer::new_raster("blend");
    layer.extras = blend.blocks(&layer);
    let mut base = LayerMask::new_revealing();
    base.bounds = IntRect::from_size(64, 64);
    base.tiles
        .get_mut_or_insert(schist_core::TileCoord::containing(0, 0))
        .fill(200);
    let live = LiveMask {
        base: MaskSnapshot::capture(&base),
        gradients: vec![MaskGradient {
            from: (0.0, 0.0),
            to: (64.0, 0.0),
            radial: false,
            reverse: false,
            opacity: 1.0,
        }],
    };
    layer.mask = live.render(base.bounds);
    layer.extras = live.blocks(&layer);
    let mut doc = Document::new("retained", 64, 64, Depth::Eight);
    doc.push_layer(layer);
    let doc = schist_codec_psd::read_psd(&schist_codec_psd::write_psd(&doc).unwrap()).unwrap();
    let layer = &doc.tree.layers[0];
    assert_eq!(VectorBlend::from_layer(layer), Some(blend));
    let loaded = LiveMask::from_layer(layer).unwrap();
    assert_eq!(loaded.gradients, live.gradients);
    assert_eq!(loaded.base.restore().unwrap().value(25, 30), 200);
    assert_eq!(
        loaded.render(doc.canvas_rect()).unwrap().value(25, 30),
        layer.mask.as_ref().unwrap().value(25, 30)
    );
}
