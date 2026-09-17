#![cfg(not(target_arch = "wasm32"))]

use schist_compositor_gpu::GpuContext;
use schist_filters_core::{AddNoise, BoxBlur, GaussianBlur, Median, MotionBlur};
use schist_fx::{BlurJob, FilterOperation, ShaderJob};
use schist_plugin_api::{FilterPlugin, FilterValues};

#[test]
fn complete_async_operations_match_the_actual_filters() {
    let Ok(ctx) = GpuContext::new() else {
        eprintln!("skipping async filter parity: no adapter");
        return;
    };
    let (w, h) = (37, 29);
    let input: Vec<f32> = (0..w * h)
        .flat_map(|i| {
            [
                (i % 19) as f32 / 18.0,
                (i % 7) as f32 / 6.0,
                (i % 11) as f32 / 10.0,
                if i % 13 == 0 { 0.0 } else { 0.6 },
            ]
        })
        .collect();
    let filters: Vec<Box<dyn FilterPlugin>> = vec![
        Box::new(GaussianBlur),
        Box::new(BoxBlur),
        Box::new(MotionBlur),
        Box::new(AddNoise),
        Box::new(Median),
        Box::new(schist_filters_core::pixelate::Mosaic),
        Box::new(schist_filters_core::other::HighPass),
        Box::new(schist_filters_core::Sharpen),
        Box::new(schist_filters_core::UnsharpMask),
        Box::new(schist_filters_core::blurgallery::FieldBlur),
        Box::new(schist_filters_core::blurgallery::IrisBlur),
        Box::new(schist_filters_core::blurgallery::TiltShift),
    ];
    for filter in filters {
        let values = FilterValues::defaults(&filter.params());
        let op = filter.gpu_operation(&values).expect("operation missing");
        let actual = pollster::block_on(async {
            match &op {
                FilterOperation::Program { .. }
                | FilterOperation::Captured { .. }
                | FilterOperation::Sequence(_) => {
                    let program = op.program(w, h).unwrap();
                    ctx.run_compute_async(&schist_fx::ComputeJob {
                        input: &input,
                        program: &program,
                    })
                    .await
                }
                FilterOperation::Blur { radius, passes } => {
                    ctx.run_blur_async(&BlurJob {
                        px: &input,
                        width: w,
                        height: h,
                        radius: *radius,
                        passes: *passes,
                    })
                    .await
                }
                FilterOperation::Shader {
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
                        params,
                        halo: *halo,
                        work_per_pixel: *work_per_pixel,
                    })
                    .await
                }
            }
        })
        .expect("GPU operation declined");
        let mut expected = input.clone();
        filter.apply(&mut expected, w, h, &values);
        for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
            assert!((a - b).abs() < 1e-4, "{} at {i}: {a} != {b}", filter.id());
        }
        assert!(
            pollster::block_on(ctx.filter_async(&op, &input, w, h)).is_none(),
            "small jobs should retain the CPU path"
        );
    }
}
