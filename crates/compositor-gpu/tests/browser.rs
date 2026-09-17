//! Actual WebGPU execution, with no CPU fallback accepted as a passing result.
#![cfg(target_arch = "wasm32")]

use schist_color::{Depth, Rgba};
use schist_compositor::{Compositor, CpuCompositor};
use schist_compositor_gpu::{plan, BatchOut, GpuContext};
use schist_core::{Document, Layer, TileCoord};
use schist_fx::{BlurJob, ShaderJob};
use schist_plugin_api::{FilterPlugin, FilterValues};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn pixels(w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .flat_map(|i| {
            [
                (i % 17) as f32 / 16.0,
                (i % 29) as f32 / 28.0,
                (i % 7) as f32 / 6.0,
                if i % 13 == 0 { 0.0 } else { 0.75 },
            ]
        })
        .collect()
}

fn close(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert!((a - b).abs() < 1e-4, "channel {i}: {a} != {b}");
    }
}

#[wasm_bindgen_test(async)]
async fn composite_snapshot_and_viewport_match_cpu() {
    let ctx = GpuContext::new_async()
        .await
        .expect("a WebGPU adapter is required");
    let mut doc = Document::new("WebGPU", 256, 256, Depth::ThirtyTwo);
    let mut layer = Layer::new_raster("pixels");
    let coord = TileCoord { tx: 0, ty: 0 };
    let tile = layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(coord, doc.depth);
    tile.set(0, Rgba::new(0.8, 0.3, 0.1, 0.75));
    doc.tree.layers.push(layer);
    let expected = CpuCompositor.tiles_rgba8(&doc, &[coord]);
    let snapshot = plan::build(&doc).unwrap().snapshot();
    doc.tree.layers.clear(); // In-flight work must retain the original pixels.
    let Some(BatchOut::Rgba8(actual)) = ctx
        .composite_batch_async(&snapshot.plan(), &[coord], true)
        .await
    else {
        panic!("compositing declined");
    };
    assert_eq!(actual, expected);
    let grid = vec![Some(std::sync::Arc::new(actual[0].clone()))];
    let params = schist_compositor::viewport::ViewportParams {
        width: 80,
        height: 60,
        origin: (8.0, 4.0),
        zoom: 0.8,
        scale_factor: 1.0,
        rotation: 0.2,
        canvas: doc.canvas_rect(),
        grid_origin: (0, 0),
        grid_cols: 1,
        grid_rows: 1,
        surround: 0x343434,
    };
    let actual = ctx
        .render_viewport_async(&params, &grid)
        .await
        .expect("viewport declined");
    let expected = schist_compositor::viewport::render_viewport_cpu(&params, &grid);
    assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1));
}

#[wasm_bindgen_test(async)]
async fn concurrent_banded_blur_and_shader_match_cpu() {
    let ctx = GpuContext::new_async()
        .await
        .expect("a WebGPU adapter is required");
    let (w, h) = (35, 31);
    let input = pixels(w, h);
    ctx.set_binding_limit(w * 16 * 17);
    let blur = BlurJob {
        px: &input,
        width: w,
        height: h,
        radius: 2,
        passes: 3,
    };
    let filter = schist_filters_core::AddNoise;
    let values = FilterValues::defaults(&filter.params());
    let schist_fx::FilterOperation::Shader {
        shader,
        params,
        halo,
        work_per_pixel,
    } = filter.gpu_operation(&values).unwrap()
    else {
        panic!()
    };
    let noise = ShaderJob {
        px: &input,
        width: w,
        height: h,
        shader,
        params: &params,
        halo,
        work_per_pixel,
    };
    let (blurred, noisy) = futures::join!(ctx.run_blur_async(&blur), ctx.run_shader_async(&noise));
    let mut expected = input.clone();
    schist_fx::blur_rgba_cpu(&mut expected, w, h, 2, 3);
    close(&blurred.expect("blur declined"), &expected);
    let mut expected = input.clone();
    filter.apply(&mut expected, w, h, &values);
    close(&noisy.expect("shader declined"), &expected);
}

#[wasm_bindgen_test(async)]
async fn cancelled_readback_does_not_poison_the_next_job() {
    let ctx = GpuContext::new_async()
        .await
        .expect("a WebGPU adapter is required");
    let input = pixels(32, 24);
    let job = BlurJob {
        px: &input,
        width: 32,
        height: 24,
        radius: 2,
        passes: 3,
    };
    let mut cancelled = Box::pin(ctx.run_blur_async(&job));
    assert!(futures::poll!(&mut cancelled).is_pending());
    drop(cancelled);
    let output = ctx
        .run_blur_async(&job)
        .await
        .expect("job after cancellation declined");
    let mut expected = input;
    schist_fx::blur_rgba_cpu(&mut expected, 32, 24, 2, 3);
    close(&output, &expected);
}

#[wasm_bindgen_test(async)]
async fn resident_programs_and_context_filters_match_cpu() {
    let ctx = GpuContext::new_async()
        .await
        .expect("WebGPU adapter required");
    let (w, h) = (37, 29);
    let input = pixels(w, h);
    let filters: Vec<Box<dyn FilterPlugin>> = vec![
        Box::new(schist_filters_core::UnsharpMask),
        Box::new(schist_filters_core::blurgallery::IrisBlur),
        Box::new(schist_filters_core::pixelate::Mosaic),
        Box::new(schist_filters_core::render::Clouds),
    ];
    let context = schist_plugin_api::FilterContext {
        foreground: Rgba::new(0.1, 0.7, 0.3, 1.0),
        background: Rgba::new(0.9, 0.2, 0.8, 1.0),
        ..Default::default()
    };
    for filter in filters {
        let values = FilterValues::defaults(&filter.params());
        let operation = filter.gpu_operation_with(&values, &context).unwrap();
        let actual = match operation {
            schist_fx::FilterOperation::Program { build, params, .. } => {
                let program = build(w, h, &params).unwrap();
                ctx.run_compute_async(&schist_fx::ComputeJob {
                    input: &input,
                    program: &program,
                })
                .await
            }
            schist_fx::FilterOperation::Shader {
                shader,
                params,
                halo,
                work_per_pixel,
            } => {
                ctx.run_shader_async(&ShaderJob {
                    px: &input,
                    width: w,
                    height: h,
                    shader,
                    params: &params,
                    halo,
                    work_per_pixel,
                })
                .await
            }
            _ => panic!("unexpected operation"),
        }
        .expect("GPU operation declined");
        let mut expected = input.clone();
        filter.apply_with(&mut expected, w, h, &values, &context);
        close(&actual, &expected);
    }
    let mut document = Document::new("moving layer", 512, 512, Depth::ThirtyTwo);
    let mut layer = Layer::new_raster("shift");
    layer
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::ThirtyTwo)
        .set(255, Rgba::new(0.7, 0.1, 0.2, 0.8));
    layer.render_offset = (13, 257);
    document.tree.layers.push(layer);
    let coords = [TileCoord { tx: 1, ty: 1 }];
    let expected = CpuCompositor.tiles_rgba8(&document, &coords);
    let snapshot = plan::build(&document).unwrap().snapshot();
    let Some(BatchOut::Rgba8(actual)) = ctx
        .composite_batch_async(&snapshot.plan(), &coords, true)
        .await
    else {
        panic!("offset composite declined")
    };
    assert_eq!(actual, expected);
}
