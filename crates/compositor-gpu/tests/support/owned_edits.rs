use schist_color::{Depth, Rgba};
use schist_compositor_gpu::GpuContext;
use schist_core::{Document, IntRect, Layer};
use schist_plugin_api::{
    EditorState, GpuEdit, Modifiers, PluginManifest, PluginRegistry, PointerInput, ToolCtx,
    ToolPlugin,
};

async fn execute(ctx: &GpuContext, request: GpuEdit, doc: &mut Document) {
    let result = ctx
        .run_compute_async(&schist_fx::ComputeJob {
            input: &request.input,
            program: &request.program,
        })
        .await
        .expect("owned edit must execute on GPU");
    let reference = (request.fallback)(&request.input);
    assert_eq!(result.len(), reference.len());
    for (i, (a, b)) in result.iter().zip(reference).enumerate() {
        assert!((a - b).abs() < 1e-4, "owned edit channel {i}: {a} != {b}");
    }
    (request.apply)(doc, result);
}

pub async fn verify(ctx: &GpuContext) {
    let mut doc = Document::new("owned GPU edits", 96, 80, Depth::ThirtyTwo);
    let mut layer = Layer::new_raster("source");
    let rgba = (0..64 * 48)
        .flat_map(|i| {
            [
                ((i % 64) / 8) as f32 / 8.0,
                (i / 64) as f32 / 48.0,
                0.25,
                1.0,
            ]
        })
        .collect::<Vec<_>>();
    schist_core::blit_rgba_f32(
        &mut layer.as_raster_mut().unwrap().tiles,
        doc.depth,
        IntRect::from_size(64, 48),
        &rgba,
    );
    let id = layer.id;
    let original = layer.as_raster().unwrap().tiles.clone();
    doc.tree.layers.push(layer);
    doc.active_layer = Some(id);
    let mut state = EditorState::default();
    let pointer = |x, y| PointerInput {
        x,
        y,
        pressure: 1.0,
        modifiers: Modifiers::default(),
    };
    let mut registry = PluginRegistry::new();
    schist_tools_select::SelectToolsPlugin.register(&mut registry);
    let wand = registry.tool_mut("wand").unwrap();
    let request = wand
        .gpu_pointer_down(&doc, &state, pointer(32.0, 24.0))
        .unwrap();
    execute(ctx, request, &mut doc).await;
    assert!(!doc.selection.is_empty());
    for name in ["select.grow", "select.similar"] {
        let request = schist_commands_core::gpu_selection_command(name, &doc, &state).unwrap();
        execute(ctx, request, &mut doc).await;
    }
    while doc.undo().is_some() {}
    assert!(doc.selection.is_empty());
    let mut tool = schist_tools_transform::TransformTool::default();
    tool.set_async_compute(true);
    tool.on_activate(&mut ToolCtx {
        doc: &mut doc,
        state: &mut state,
    });
    tool.on_pointer_down(
        &mut ToolCtx {
            doc: &mut doc,
            state: &mut state,
        },
        pointer(32.0, 24.0),
    );
    tool.on_pointer_move(
        &mut ToolCtx {
            doc: &mut doc,
            state: &mut state,
        },
        pointer(39.0, 29.0),
    );
    execute(
        ctx,
        tool.take_gpu_edit()
            .expect("asynchronous transform preview"),
        &mut doc,
    )
    .await;
    assert!(
        doc.history.undo_name().is_none(),
        "preview must not create history"
    );
    assert_eq!(
        doc.tree
            .find(id)
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .pixel(7, 5),
        original.pixel(0, 0)
    );
    tool.on_commit(&mut ToolCtx {
        doc: &mut doc,
        state: &mut state,
    });
    execute(
        ctx,
        tool.take_gpu_edit().expect("asynchronous transform Apply"),
        &mut doc,
    )
    .await;
    assert!(doc.undo().is_some());
    assert!(
        doc.history.undo_name().is_none(),
        "Apply creates exactly one history entry"
    );
    let restored = &doc.tree.find(id).unwrap().as_raster().unwrap().tiles;
    for y in 0..80 {
        for x in 0..96 {
            assert_eq!(restored.pixel(x, y), original.pixel(x, y));
        }
    }
    assert_eq!(restored.pixel(70, 70), Rgba::TRANSPARENT);

    struct Required<'a>(&'a GpuContext);
    impl schist_fx::AsyncCompute for Required<'_> {
        async fn compute_async(&self, job: schist_fx::ComputeJob<'_>) -> Option<Vec<f32>> {
            Some(
                self.0
                    .run_compute_async(&job)
                    .await
                    .expect("Image Size GPU operation"),
            )
        }
    }
    let resized =
        schist_tools_transform::ClassicResize::capture(&doc, 47, 39, schist_core::Filter::Bicubic)
            .unwrap()
            .run(&Required(ctx))
            .await;
    resized.apply(&mut doc);
    assert_eq!((doc.width, doc.height), (47, 39));
    assert!(doc.undo().is_some());
    assert_eq!((doc.width, doc.height), (96, 80));
    assert!(doc.history.undo_name().is_none());
}

pub async fn raw_development(ctx: &GpuContext) {
    use schist_codecs_common::raw::{develop_rgba, develop_rgba_async, RawQuality};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let bytes = include_bytes!("../../../codec-raw/tests/fixtures/gpu-development.dng");
    struct Required<'a>(&'a GpuContext, AtomicUsize);
    impl schist_fx::AsyncCompute for Required<'_> {
        async fn compute_async(&self, job: schist_fx::ComputeJob<'_>) -> Option<Vec<f32>> {
            self.1.fetch_add(1, Ordering::Relaxed);
            Some(
                self.0
                    .run_compute_async(&job)
                    .await
                    .expect("RAW GPU stage must execute"),
            )
        }
    }
    let backend = Required(ctx, AtomicUsize::new(0));
    for exposure in [-5.0, -0.75, 0.0, 2.0, 5.0] {
        let settings = schist_core::RawSettings {
            exposure,
            temperature: 13.0,
            tint: -7.0,
            ..Default::default()
        };
        let expected = develop_rgba(bytes, settings, RawQuality::Best).unwrap();
        let actual = develop_rgba_async(bytes, settings, RawQuality::Best, &backend)
            .await
            .unwrap();
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        for (i, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                (a - b).abs() < 3e-4,
                "RAW exposure {exposure} at {i}: {a} != {b}"
            );
        }
    }
    assert_eq!(
        backend.1.load(Ordering::Relaxed),
        10,
        "each development includes sensor and histogram/encoding GPU programs"
    );
    let first = schist_codec_raw::decode_cached(bytes).unwrap();
    let second = schist_codec_raw::decode_cached(bytes).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    let mut changed = bytes.to_vec();
    let n = changed.len();
    changed[n - 2] ^= 64;
    let modified = schist_codec_raw::decode_cached(&changed).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first, &modified));
}

pub async fn export(ctx: &GpuContext) {
    let mut doc = Document::new("export snapshot", 273, 267, Depth::ThirtyTwo);
    let mut layer = Layer::new_raster("HDR source");
    let input = (0..273 * 267)
        .flat_map(|i| {
            [
                ((i * 13) % 97) as f32 / 50.0 - 0.1,
                ((i * 7) % 37) as f32 / 36.0,
                0.125,
                if i % 19 == 0 { 0.0 } else { 0.8 },
            ]
        })
        .collect::<Vec<_>>();
    schist_core::blit_rgba_f32(
        &mut layer.as_raster_mut().unwrap().tiles,
        doc.depth,
        doc.canvas_rect(),
        &input,
    );
    layer.render_offset = (-7, 9);
    doc.tree.layers.push(layer);
    let expected = schist_compositor::composite_region_f32_cpu(&doc, doc.canvas_rect());
    let tiles = ctx
        .flatten_async(&doc)
        .await
        .expect("export composite must run on GPU");
    for y in 0..267 {
        for x in 0..273 {
            let p = tiles.pixel(x, y);
            let i = (y as usize * 273 + x as usize) * 4;
            for (a, b) in [p.r, p.g, p.b, p.a].iter().zip(&expected[i..i + 4]) {
                assert!((a - b).abs() < 1e-4);
            }
        }
    }
    let mut flat = Layer::new_raster("flattened");
    flat.as_raster_mut().unwrap().tiles = tiles;
    doc.tree.layers = vec![flat];
    let options = schist_plugin_api::ExportOptions {
        bit_depth: 16,
        dither: false,
        ..Default::default()
    };
    use schist_plugin_api::CodecPlugin;
    let bytes = schist_codecs_common::PngCodec
        .export_with(&doc, &options)
        .unwrap();
    let restored = schist_codecs_common::PngCodec.import(&bytes).unwrap();
    assert_eq!(
        (restored.width, restored.height, restored.depth),
        (273, 267, Depth::Sixteen)
    );
}
