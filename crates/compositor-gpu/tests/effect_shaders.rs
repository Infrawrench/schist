//! Run each shader through its real filter body, against that body's CPU
//! fallback. Tiny forced jobs cover edges; production-sized jobs verify
//! normal offloading. Neither can pass by silently declining GPU work.
use schist_compositor_gpu::{GpuCompositor, GpuContext};
use schist_filters_core as filters;
use schist_fx::{FxBackend, ShaderJob, ShaderSpec};
use schist_plugin_api::{FilterContext, FilterPlugin, FilterValues};
use std::sync::{Arc, Mutex};

struct Tracking {
    ctx: Arc<GpuContext>,
    production: bool,
    seen: Mutex<Vec<&'static str>>,
}
impl FxBackend for Tracking {
    fn name(&self) -> &'static str {
        "tracked shader test"
    }
    fn shader(&self, job: &ShaderJob<'_>) -> Option<Vec<f32>> {
        let out = if self.production {
            schist_compositor_gpu::GpuFx::new(self.ctx.clone()).shader(job)
        } else {
            self.ctx.run_shader(job)
        }
        .unwrap_or_else(|| {
            panic!(
                "{} {}x{} halo={:?}",
                job.shader.name, job.width, job.height, job.halo
            )
        });
        self.seen.lock().unwrap().push(job.shader.source);
        Some(out)
    }
}
// Discover the real filter body's normalized cost, so the production
// cases remain above the offload threshold when parameters change.
#[derive(Default)]
struct CostProbe(Mutex<Vec<usize>>);
impl FxBackend for CostProbe {
    fn name(&self) -> &'static str {
        "shader cost probe"
    }
    fn shader(&self, job: &ShaderJob<'_>) -> Option<Vec<f32>> {
        self.0.lock().unwrap().push(job.work_per_pixel);
        Some(job.px.to_vec())
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
fn compare_pixels(g: &[f32], c: &[f32], label: &str) -> Result<(), String> {
    if g.len() != c.len() {
        return Err(format!(
            "{label}: GPU length {} != CPU length {}",
            g.len(),
            c.len()
        ));
    }
    for (i, (&a, &b)) in g.iter().zip(c).enumerate() {
        if !a.is_finite() || !b.is_finite() {
            return Err(format!("{label} at {i}: non-finite output"));
        }
        let alpha = i / 4 * 4 + 3;
        // At the unpremultiply cutoff a one-ULP alpha difference can
        // choose zero RGB on one device and straight RGB on another.
        // Compare the actual color contribution for nearly invisible
        // pixels; keep the strict straight-alpha check everywhere else.
        let transcendental = [
            "filter.spin_blur",
            "filter.zigzag",
            "filter.spherize",
            "filter.pinch",
            "filter.polar",
            "filter.shear",
            "filter.lens_correction",
            "filter.glass",
            "filter.ocean_ripple",
            "filter.adaptive_wide_angle",
        ]
        .iter()
        .any(|name| label.starts_with(name));
        let difference = if i % 4 != 3 && (g[alpha].max(c[alpha]) < 1e-5 || transcendental) {
            (a * g[alpha] - b * c[alpha]).abs()
        } else {
            (a - b).abs()
        };
        if difference > if transcendental { 5e-4 } else { 1e-4 } {
            return Err(format!(
                "{label} at {i}: gpu {a}, cpu {b}, alpha {} / {}, difference {difference}",
                g[alpha], c[alpha]
            ));
        }
    }
    Ok(())
}
type Case = (Box<dyn FilterPlugin>, Vec<(&'static str, f32)>);
fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    for filter in [
        Box::new(schist_filters_core::pixelate::Crystallize) as Box<dyn FilterPlugin>,
        Box::new(schist_filters_core::pixelate::Pointillize),
        Box::new(schist_filters_core::pixelate::Mezzotint),
    ] {
        cases.push((filter, vec![]));
    }
    cases.push((
        Box::new(schist_filters_core::lens::LensCorrection),
        vec![
            ("distortion", 30.0),
            ("red", 20.0),
            ("blue", -17.0),
            ("angle", 13.0),
            ("vertical", 15.0),
            ("horizontal", -9.0),
            ("vignette", 40.0),
        ],
    ));
    for kind in 0..6 {
        cases.push((
            Box::new(schist_filters_core::distort::Glass),
            vec![("texture", kind as f32)],
        ));
    }
    cases.push((Box::new(schist_filters_core::distort::OceanRipple), vec![]));
    for kind in 0..4 {
        cases.push((
            Box::new(schist_filters_core::texture::Texturizer),
            vec![("texture", kind as f32), ("invert", 1.0)],
        ));
    }
    macro_rules! case {
        ($filter:expr $(, $key:literal => $value:expr)* $(,)?) => {
            cases.push((Box::new($filter), vec![$(($key, $value)),*]));
        };
    }
    for angle in [0.0, 30.0, 90.0, -73.0] {
        case!(filters::MotionBlur, "distance" => 12.0, "angle" => angle);
    }
    for radius in [1.0, 2.0, 4.0, 7.0] {
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
    case!(filters::distort::Twirl, "angle" => -137.0);
    case!(filters::distort::Ripple, "amount" => -237.0);
    case!(filters::distort::Ripple, "amount" => 17.0, "size" => 7.0);
    case!(filters::distort::Wave, "generators" => 3.0, "horizontal" => 18.0, "vertical" => 30.0, "seed" => 71.0);
    case!(filters::render::Clouds);
    case!(filters::render::DifferenceClouds);
    case!(filters::render::Fibers);
    case!(filters::blurgallery::SpinBlur);
    case!(filters::blurgallery::PathBlur,"curve"=>37.0,"taper"=>72.0,"angle"=>45.0);
    for kind in 0..6 {
        case!(filters::blurgallery::ShapeBlur,"shape"=>kind as f32,"radius"=>4.0);
    }
    for mode in 0..3 {
        case!(filters::blurgallery::SmartBlur,"mode"=>mode as f32);
    }
    case!(filters::distort::ZigZag);
    for mode in 0..3 {
        for amount in [-100.0, 50.0, 100.0] {
            case!(filters::distort::Spherize, "mode" => mode as f32, "amount" => amount);
        }
    }
    case!(filters::distort::Pinch);
    case!(filters::distort::PolarCoordinates);
    case!(filters::distort::Shear,"undefined"=>1.0);
    case!(filters::lens::AdaptiveWideAngle);
    cases
}

#[test]
fn effect_bodies_match_the_cpu_including_bands_and_alpha() {
    for shader in filters::gpu::SHADERS {
        let source = shader.wgsl();
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}: {}", shader.name, e.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}: {e}", shader.name));
    }
    let Some(gpu) = gpu() else { return };
    let _restore = Restore(schist_fx::backend());
    let force = Arc::new(Tracking {
        ctx: gpu.context().clone(),
        production: false,
        seen: Mutex::new(Vec::new()),
    });
    let production = Arc::new(Tracking {
        ctx: gpu.context().clone(),
        production: true,
        seen: Mutex::new(Vec::new()),
    });
    let original_limit = force.ctx.binding_limit();
    let mut failures = Vec::new();
    for (width, height, banded, normal_offload) in [
        (37, 29, false, false),
        (1, 9, false, false),
        (9, 1, false, false),
        (67, 49, true, false),
        (1025, 3, false, false),
        (0, 0, false, true),
    ] {
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
            let (w, h) = if normal_offload {
                let probe = Arc::new(CostProbe::default());
                schist_fx::set_backend(probe.clone());
                filter.apply_with(&mut pixels(37, 29), 37, 29, &values, &context);
                let costs = probe.0.lock().unwrap();
                let cost = *costs.iter().min().expect("filter attempted a shader");
                let area = 8_000_000usize.div_ceil(cost.max(1));
                let w = ((area as f64).sqrt().ceil() as usize).max(67) | 1;
                (w, area.div_ceil(w).max(49))
            } else {
                (width, height)
            };
            let input = pixels(w, h);
            let mut cpu = input.clone();
            schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
            filter.apply_with(&mut cpu, w, h, &values, &context);
            // Nonlocal samplers need the full source. Every other effect
            // crosses band boundaries; global procedural coordinates must
            // not reset at the start of each band.
            let local = !matches!(
                filter.id(),
                "filter.radial_blur"
                    | "filter.twirl"
                    | "filter.spin_blur"
                    | "filter.path_blur"
                    | "filter.zigzag"
                    | "filter.spherize"
                    | "filter.pinch"
                    | "filter.crystallize"
                    | "filter.pointillize"
                    | "filter.polar"
                    | "filter.glass"
                    | "filter.lens_correction"
                    | "filter.adaptive_wide_angle"
            ) && !(filter.id() == "filter.offset" && values.get("undefined") == 2.0);
            force.ctx.set_binding_limit(if banded && local {
                w * 16 * 39
            } else {
                original_limit
            });
            let backend = if normal_offload { &production } else { &force };
            let before = backend.seen.lock().unwrap().len();
            schist_fx::set_backend(backend.clone());
            let mut result = input.clone();
            filter.apply_with(&mut result, w, h, &values, &context);
            assert!(
                backend.seen.lock().unwrap().len() > before,
                "{} did not dispatch",
                filter.id()
            );
            if let Err(error) = compare_pixels(
                &result,
                &cpu,
                &format!(
                    "{} {w}x{h} production={normal_offload} {values:?}",
                    filter.id()
                ),
            ) {
                failures.push(error);
            }
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
    assert!(
        failures.is_empty(),
        "filter parity failures:\n{}",
        failures.join("\n")
    );
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
