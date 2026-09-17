use schist_compositor_gpu::GpuContext;
use schist_fx::{ShaderJob, ShaderSpec};

pub fn twirl_tolerance(angle: f32) -> f32 {
    // Rotation magnifies float radius errors in proportion to the angle.
    // Preserve the original bound through a half-turn; even at the slider's
    // +/-999 degree extremes this stays below one 8-bit color step (1/255).
    5e-4 * (angle.abs() / 180.0).max(1.0)
}

pub async fn verify(ctx: &GpuContext) {
    static TRIG: ShaderSpec = ShaderSpec {
        name: "trig-extrema",
        source: r#"
fn effect(pos: vec2<i32>) -> vec4<f32> {
    let angle = read_pixel(pos).r;
    let sc = precise_sin_cos(angle);
    let half_sine = precise_sin_cos(angle * 0.5).x;
    return vec4<f32>(sc, -2.0 * half_sine * half_sine, 1.0);
}
"#,
    };
    let mut angles: Vec<f32> = (-4096..=4096)
        .map(|i| i as f32 * 999.0f32.to_radians() / 4096.0)
        .collect();
    // Twirl's almost-zero boundary offsets and Halftone's exact extrema caused
    // visible Windows WARP failures despite only small native trig errors.
    angles.extend([-0.00002, -0.000001, 0.0, 0.000001, 0.00002]);
    angles.extend((-128..=128).map(|i| i as f32 / 4.0 * std::f32::consts::TAU));
    angles.extend(
        (0..=16384)
            .step_by(17)
            .map(|i| i as f32 / 4.0 * std::f32::consts::TAU),
    );
    let input: Vec<f32> = angles.iter().flat_map(|&a| [a, 0.0, 0.0, 1.0]).collect();
    let result = ctx
        .run_shader_async(&ShaderJob {
            shader: &TRIG,
            px: &input,
            width: angles.len(),
            height: 1,
            params: &[],
            halo: Some(0),
            work_per_pixel: 1,
        })
        .await
        .expect("trigonometric regression must execute on GPU");
    for (&angle, actual) in angles.iter().zip(result.as_chunks::<4>().0) {
        let (s, c) = angle.sin_cos();
        let half_sine = (angle * 0.5).sin();
        for (i, expected) in [s, c, -2.0 * half_sine * half_sine].into_iter().enumerate() {
            let tolerance = if i == 2 { 5e-7 } else { 2e-7 };
            assert!(
                (actual[i] - expected).abs() <= tolerance,
                "angle {angle}, channel {i}: GPU {} != CPU {expected}",
                actual[i]
            );
            if i < 2 && expected.abs() == 1.0 {
                assert_eq!(actual[i], expected, "exact extremum at {angle}");
            }
        }
    }

    use schist_plugin_api::{FilterContext, FilterPlugin, FilterValues};
    let filter = schist_filters_core::sketch::HalftonePattern;
    let context = FilterContext::default();
    let input = [1.0, 1.0, 1.0, 0.6].repeat(16 * 16);
    for pattern in 0..3 {
        let mut values = FilterValues::defaults(&filter.params());
        values.set("pattern", pattern as f32);
        let mut expected = input.clone();
        filter.apply_with(&mut expected, 16, 16, &values, &context);
        let program = filter
            .gpu_operation_with(&values, &context)
            .unwrap()
            .program(16, 16)
            .unwrap();
        let actual = ctx
            .run_compute_async(&schist_fx::ComputeJob {
                input: &input,
                program: &program,
            })
            .await
            .expect("Halftone extrema must execute on GPU");
        assert_eq!(actual, expected, "Halftone pattern {pattern} extrema");
    }
}
