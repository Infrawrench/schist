//! Run each shader through its real filter body, against that body's CPU
//! fallback. Tiny forced jobs make edge cases affordable and ensure that
//! a passing test cannot be a silently declined GPU operation.
use schist_compositor_gpu::{GpuCompositor, GpuContext};
use schist_filters_core as filters;
use schist_fx::{FxBackend, ShaderJob, ShaderSpec};
use schist_plugin_api::{FilterContext, FilterPlugin, FilterValues};
use std::sync::{Arc, Mutex};

struct Forced {
    ctx: Arc<GpuContext>,
    seen: Mutex<Vec<&'static str>>,
}
impl FxBackend for Forced {
    fn name(&self) -> &'static str {
        "forced shader test"
    }
    fn shader(&self, job: &ShaderJob<'_>) -> Option<Vec<f32>> {
        let out = self.ctx.run_shader(job).expect(job.shader.name);
        self.seen.lock().unwrap().push(job.shader.source);
        Some(out)
    }
}
struct Restore(Arc<dyn FxBackend>);
impl Drop for Restore {
    fn drop(&mut self) {
        schist_fx::set_backend(self.0.clone());
    }
}
fn gpu() -> Option<GpuCompositor> {
    match GpuCompositor::new() {
        Ok(g) => Some(g),
        Err(e) => {
            assert!(!e.starts_with("pipeline creation:"), "{e}");
            eprintln!("skipping effect GPU tests: {e}");
            None
        }
    }
}
fn pixels(w: usize, h: usize) -> Vec<f32> {
    let mut state = 1234567u32;
    (0..w * h * 4)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            if i % 4 == 3 {
                [0.0, 1.0, 0.0000005, 0.000002, 0.25, 0.7][(state % 6) as usize]
            } else {
                (state % 2048) as f32 / 1024.0 - 0.25
            }
        })
        .collect()
}
fn close(g: &[f32], c: &[f32], label: &str) {
    assert_eq!(g.len(), c.len(), "{label}");
    for (i, (&a, &b)) in g.iter().zip(c).enumerate() {
        assert!(
            a.is_finite() && b.is_finite(),
            "{label} at {i}: non-finite output"
        );
        let alpha = i / 4 * 4 + 3;
        // At the unpremultiply cutoff a one-ULP alpha difference can
        // choose zero RGB on one device and straight RGB on another.
        // Compare the actual color contribution for nearly invisible
        // pixels; keep the strict straight-alpha check everywhere else.
        let difference = if i % 4 != 3 && g[alpha].max(c[alpha]) < 1e-5 {
            (a * g[alpha] - b * c[alpha]).abs()
        } else {
            (a - b).abs()
        };
        assert!(
            difference <= 1e-4,
            "{label} at {i}: gpu {a}, cpu {b}, difference {difference}"
        );
    }
}
type Case = (Box<dyn FilterPlugin>, Vec<(&'static str, f32)>);
fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    macro_rules! case {
        ($filter:expr $(, $key:literal => $value:expr)* $(,)?) => {
            cases.push((Box::new($filter), vec![$(($key, $value)),*]));
        };
    }
    for angle in [0.0, 30.0, 90.0, -73.0] {
        case!(filters::MotionBlur, "distance" => 12.0, "angle" => angle);
    }
    for radius in [1.0, 2.0, 4.0] {
        case!(filters::Median, "radius" => radius);
        case!(filters::other::DustAndScratches, "radius" => radius, "threshold" => 10.0);
    }
    for distribution in [0.0, 1.0] {
        for mono in [0.0, 1.0] {
            case!(filters::AddNoise, "distribution" => distribution, "monochrome" => mono);
        }
    }
    for preserve in [0.0, 1.0] {
        case!(filters::other::Maximum, "radius" => 5.0, "preserve" => preserve);
        case!(filters::other::Minimum, "radius" => 5.0, "preserve" => preserve);
    }
    for mode in [0.0, 1.0, 2.0] {
        case!(filters::other::Offset, "x" => -7.0, "y" => 3.0, "undefined" => mode);
        case!(filters::distort::Wave, "generators" => 3.0, "type" => mode);
    }
    for method in [0.0, 1.0] {
        case!(filters::other::RadialBlur, "method" => method, "x" => 37.0, "y" => 64.0);
    }
    case!(filters::other::SurfaceBlur, "threshold" => 125.0);
    case!(filters::other::ReduceNoise, "detail" => 10.0, "jpeg" => 1.0);
    case!(filters::other::Despeckle);
    case!(filters::other::SharpenMore);
    case!(filters::other::Custom, "k00" => -2.0, "k24" => 3.0, "scale" => 3.0, "offset" => 23.0);
    case!(filters::stylize::FindEdges);
    case!(filters::stylize::TraceContour, "edge" => 0.0);
    case!(filters::stylize::TraceContour, "edge" => 1.0);
    case!(filters::stylize::OilPaint, "levels" => 64.0);
    case!(filters::stylize::OilPaint, "levels" => 2.0, "bristle" => 0.0);
    case!(filters::stylize::Emboss, "angle" => -40.0);
    case!(filters::pixelate::Facet);
    case!(filters::pixelate::Fragment);
    case!(filters::distort::Twirl);
    case!(filters::distort::Ripple, "amount" => -237.0);
    case!(filters::render::Clouds);
    case!(filters::render::DifferenceClouds);
    case!(filters::render::Fibers);
    cases
}

#[test]
fn effect_bodies_match_the_cpu_including_bands_and_alpha() {
    let Some(gpu) = gpu() else { return };
    let _restore = Restore(schist_fx::backend());
    let force = Arc::new(Forced {
        ctx: gpu.context().clone(),
        seen: Mutex::new(Vec::new()),
    });
    let original_limit = force.ctx.binding_limit();
    for (w, h, banded) in [
        (37, 29, false),
        (1, 9, false),
        (9, 1, false),
        (67, 49, true),
        (1025, 3, false),
    ] {
        let input = pixels(w, h);
        for (filter, pairs) in cases() {
            let mut values = FilterValues::defaults(&filter.params());
            for (k, v) in pairs {
                values.set(k, v);
            }
            let context = FilterContext {
                foreground: schist_color::Rgba::new(0.1, 0.7, 0.3, 1.0),
                background: schist_color::Rgba::new(0.9, 0.2, 0.8, 1.0),
                ..FilterContext::default()
            };
            let mut cpu = input.clone();
            schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
            filter.apply_with(&mut cpu, w, h, &values, &context);
            // Nonlocal samplers need the full source. Every other effect
            // crosses band boundaries; global procedural coordinates must
            // not reset at the start of each band.
            let local = !matches!(filter.id(), "filter.radial_blur" | "filter.twirl")
                && !(filter.id() == "filter.offset" && values.get("undefined") == 2.0);
            force.ctx.set_binding_limit(if banded && local {
                w * 16 * 39
            } else {
                original_limit
            });
            let before = force.seen.lock().unwrap().len();
            schist_fx::set_backend(force.clone());
            let mut result = input.clone();
            filter.apply_with(&mut result, w, h, &values, &context);
            assert!(
                force.seen.lock().unwrap().len() > before,
                "{} did not dispatch",
                filter.id()
            );
            close(&result, &cpu, &format!("{} {w}x{h}", filter.id()));
        }
    }
    let seen = force.seen.lock().unwrap();
    for shader in filters::gpu::SHADERS {
        assert!(
            seen.contains(&shader.source),
            "{} is not wired into a filter",
            shader.name
        );
    }
}

static IDENTITY: ShaderSpec = ShaderSpec {
    name: "same-name",
    source: "fn effect(p: vec2<i32>) -> vec4<f32> { return read_pixel(p); }",
};
static ZERO: ShaderSpec = ShaderSpec {
    name: "same-name",
    source: "fn effect(p: vec2<i32>) -> vec4<f32> { return vec4<f32>(0.0); }",
};
static INVALID: ShaderSpec = ShaderSpec {
    name: "invalid",
    source: "this is not WGSL",
};

#[test]
fn shader_failures_limits_and_cache_do_not_poison_other_effects() {
    let Some(gpu) = gpu() else { return };
    let ctx = gpu.context();
    let px = pixels(19, 23);
    let mut job = ShaderJob {
        shader: &IDENTITY,
        px: &px,
        width: 19,
        height: 23,
        params: &[],
        halo: Some(0),
        work_per_pixel: 1,
    };
    assert!(
        gpu.fx().shader(&job).is_none(),
        "small jobs should stay on the CPU"
    );
    for _ in 0..2 {
        job.shader = &INVALID;
        assert!(ctx.run_shader(&job).is_none());
        job.shader = &IDENTITY;
        assert_eq!(ctx.run_shader(&job).unwrap(), px);
        job.shader = &ZERO;
        assert_eq!(ctx.run_shader(&job).unwrap(), vec![0.0; px.len()]);
    }
    job.shader = &IDENTITY;
    ctx.set_binding_limit(19 * 16 * 5);
    assert_eq!(ctx.run_shader(&job).unwrap(), px);
    job.halo = None;
    assert!(
        ctx.run_shader(&job).is_none(),
        "nonlocal source cannot be banded"
    );
    job.halo = Some(3);
    assert!(ctx.run_shader(&job).is_none(), "halo leaves no output rows");
    job.halo = Some(0);
    job.px = &px[..px.len() - 1];
    assert!(ctx.run_shader(&job).is_none());
    job.px = &px;
    job.params = &[f32::NAN];
    assert!(ctx.run_shader(&job).is_none());
    job.params = &[];
    job.width = usize::MAX;
    assert!(ctx.run_shader(&job).is_none());
}
