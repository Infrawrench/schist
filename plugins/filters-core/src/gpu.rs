//! Shader companions for the CPU filter bodies. Parameters are prepared
//! in the effect logic, after the same slider normalization as the CPU.
//! See `crates/fx/README.md` for the ABI and how to add another effect.

pub use schist_fx::try_shader_rgba as apply;
use schist_fx::ShaderSpec;

macro_rules! shaders {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(pub static $name: ShaderSpec = ShaderSpec {
            name: $file,
            source: include_str!(concat!("shaders/", $file, ".wgsl")),
        };)*
        /// Also used by offline validation, so every registered source is checked.
        pub static SHADERS: &[&ShaderSpec] = &[$(&$name),*];
    };
}

shaders! {
    LENS_CORRECTION => "lens_correction",
    SURFACE => "surface",
    PIXELATE => "pixelate",
    BLUR_EXTRA => "blur_extra",
    DISTORT_EXTRA => "distort_extra",
    MEDIAN_LARGE => "median_large",
    CONVOLVE => "convolve",
    MORPHOLOGY => "morphology",
    BILATERAL => "bilateral",
    MEDIAN => "median",
    MOTION => "motion",
    RADIAL => "radial",
    OIL => "oil",
    FIND_EDGES => "find_edges",
    TRACE_CONTOUR => "trace_contour",
    FACET => "facet",
    ADD_NOISE => "add_noise",
    OFFSET => "offset",
    TWIRL => "twirl",
    RIPPLE => "ripple",
    WAVE => "wave",
    EMBOSS => "emboss",
    FRAGMENT => "fragment",
    CLOUDS => "clouds",
}

/// Small medians use a scratch array; large medians select by float-bit radix
/// with constant register storage. Unsupported sizes retain the CPU body.
pub fn median(
    px: &mut [f32],
    w: usize,
    h: usize,
    r: i32,
    disc: bool,
    channels: usize,
    threshold: f32,
) -> bool {
    if !(1..=100).contains(&r) {
        return false;
    }
    let taps = (2 * r + 1).pow(2) as usize;
    apply(
        px,
        w,
        h,
        if r <= 4 { &MEDIAN } else { &MEDIAN_LARGE },
        &[r as f32, disc as u8 as f32, channels as f32, threshold],
        Some(r as usize),
        taps * if r <= 4 { 8 } else { 32 },
    )
}

pub fn bilateral(px: &mut [f32], w: usize, h: usize, r: i32, threshold: f32, disc: bool) -> bool {
    apply(
        px,
        w,
        h,
        &BILATERAL,
        &[r as f32, threshold, disc as u8 as f32],
        Some(r as usize),
        (2 * r + 1).pow(2) as usize,
    )
}

/// Whole operations are shared by synchronous native callers and async browser hosts.
pub fn operation(
    id: &str,
    v: &schist_plugin_api::FilterValues,
) -> Option<schist_fx::FilterOperation> {
    use schist_fx::FilterOperation;
    if let Some(program) = crate::gpu_programs::operation(id, v) {
        return Some(program);
    }
    let (shader, params, halo, work): (&'static ShaderSpec, Vec<f32>, Option<usize>, usize) =
        match id {
            "filter.glass" => (
                &SURFACE,
                vec![
                    0.0,
                    v.get("distortion"),
                    v.get("smoothness"),
                    v.get("texture").round().max(0.0),
                    (v.get("scaling") / 100.0 * 10.0).max(1.0),
                ],
                Some((v.get("distortion").abs() * 3.0).ceil() as usize + 2),
                128,
            ),
            "filter.ocean_ripple" => (
                &SURFACE,
                vec![1.0, v.get("size").max(1.0) * 6.0, v.get("magnitude")],
                Some(v.get("magnitude").abs().ceil() as usize + 2),
                64,
            ),
            "filter.texturizer" => {
                let (x, y) = crate::sketch::light_of(v.get("light"));
                (
                    &SURFACE,
                    vec![
                        2.0,
                        v.get("texture").round().max(0.0),
                        (v.get("scaling") / 100.0 * 8.0).max(1.0),
                        v.get("relief") / 50.0,
                        x,
                        y,
                        u8::from(v.get("invert") >= 0.5) as f32,
                    ],
                    Some(0),
                    128,
                )
            }
            "filter.lens_correction" => {
                let distortion = v.get("distortion") / 100.0;
                let red = 1.0 + v.get("red") / 50.0 * 0.006;
                let blue = 1.0 + v.get("blue") / 50.0 * 0.006;
                let vertical = v.get("vertical") / 100.0;
                let horizontal = v.get("horizontal") / 100.0;
                let angle = v.get("angle").to_radians();
                let scale = (v.get("scale") / 100.0).max(0.05);
                let corrected = distortion != 0.0
                    || red != 1.0
                    || blue != 1.0
                    || vertical != 0.0
                    || horizontal != 0.0
                    || angle != 0.0
                    || (scale - 1.0).abs() > 1e-6;
                (
                    &LENS_CORRECTION,
                    vec![
                        distortion,
                        red,
                        blue,
                        v.get("vignette") / 100.0,
                        vertical,
                        horizontal,
                        angle.sin(),
                        angle.cos(),
                        scale,
                        (v.get("midpoint") / 100.0).clamp(0.05, 1.0),
                        u8::from(corrected) as f32,
                    ],
                    None,
                    64,
                )
            }
            "filter.find_edges" => (&FIND_EDGES, vec![], Some(1), 18),
            "filter.trace_contour" => (
                &TRACE_CONTOUR,
                vec![v.get("level"), u8::from(v.get("edge") >= 0.5) as f32],
                Some(1),
                3,
            ),
            "filter.emboss" => {
                let a = v.get("angle").to_radians();
                let step = v.get("height").max(1.0);
                let dx = (a.cos() * step) as i32;
                let dy = (a.sin() * step) as i32;
                (
                    &EMBOSS,
                    vec![
                        dx as f32,
                        dy as f32,
                        v.get("amount") / 100.0 * v.get("height"),
                    ],
                    Some(dy.unsigned_abs() as usize),
                    6,
                )
            }
            "filter.minimum" | "filter.maximum" => {
                let r = v.get("radius").round().max(1.0) as usize;
                (
                    &MORPHOLOGY,
                    vec![
                        r as f32,
                        u8::from(id == "filter.maximum") as f32,
                        u8::from(v.get("preserve") >= 0.5) as f32,
                    ],
                    Some(r),
                    (2 * r + 1).pow(2),
                )
            }
            "filter.offset" => {
                let x = v.get("x").round();
                let y = v.get("y").round();
                let mode = v.get("undefined").round().clamp(0.0, 2.0);
                (
                    &OFFSET,
                    vec![x, y, mode],
                    if mode == 2.0 {
                        None
                    } else {
                        Some(y.abs() as usize)
                    },
                    4,
                )
            }
            "filter.crystallize" => (
                &PIXELATE,
                vec![1.0, v.get("size").round().max(1.0)],
                None,
                24,
            ),
            "filter.pointillize" => (
                &PIXELATE,
                vec![2.0, v.get("size").round().max(2.0)],
                None,
                180,
            ),
            "filter.mezzotint" => {
                let kind = v.get("type").round().clamp(0.0, 9.0) as usize;
                (
                    &PIXELATE,
                    vec![
                        3.0,
                        v.get("grain").max(1.0) * [0.6, 1.0, 1.4, 2.2][kind % 4],
                        kind as f32,
                    ],
                    Some(0),
                    40,
                )
            }
            "filter.facet" => (&FACET, vec![], Some(1), 81),
            "filter.fragment" => {
                let d = v.get("offset").round();
                (&FRAGMENT, vec![d], Some(d.abs() as usize), 4)
            }
            "filter.spin_blur" => (
                &BLUR_EXTRA,
                vec![
                    0.0,
                    v.get("angle").to_radians(),
                    v.get("x") / 100.0,
                    v.get("y") / 100.0,
                    v.get("radius") / 100.0,
                    (v.get("feather") / 100.0).max(1e-3),
                ],
                None,
                96,
            ),
            "filter.path_blur" => {
                let speed = v.get("speed") / 100.0 * 60.0;
                let angle = v.get("angle").to_radians();
                let curve = v.get("curve") / 100.0;
                let taper = v.get("taper") / 100.0;
                let mut args = vec![1.0, speed];
                for s in 0..24 {
                    let t = s as f32 / 23.0;
                    let a = angle + curve * t * std::f32::consts::FRAC_PI_2;
                    let d = t * speed;
                    args.extend_from_slice(&[a.cos() * d, a.sin() * d, 1.0 - taper * t]);
                }
                (&BLUR_EXTRA, args, Some(speed.abs().ceil() as usize + 2), 96)
            }
            "filter.shape_blur" => {
                let r = v.get("radius").round().max(1.0) as i32;
                let kind = (v.get("shape").round().max(0.0) as usize).min(5);
                let mut args = vec![2.0, 0.0];
                for y in -r..=r {
                    for x in -r..=r {
                        if crate::blurgallery::in_shape(
                            kind,
                            x as f32 / r as f32,
                            y as f32 / r as f32,
                        ) {
                            args.extend_from_slice(&[x as f32, y as f32]);
                        }
                    }
                }
                let n = (args.len() - 2) / 2;
                args[1] = n as f32;
                (&BLUR_EXTRA, args, Some(r as usize), n)
            }
            "filter.smart_blur" => {
                let r = v.get("radius").round().max(1.0);
                (
                    &BLUR_EXTRA,
                    vec![
                        3.0,
                        r,
                        v.get("threshold") / 100.0 * 0.6,
                        v.get("mode").round().clamp(0.0, 2.0),
                    ],
                    Some(r as usize),
                    (2 * r as usize + 1).pow(2),
                )
            }
            "filter.zigzag" => (
                &DISTORT_EXTRA,
                vec![
                    0.0,
                    v.get("amount") / 100.0,
                    v.get("ridges").max(1.0),
                    v.get("style").round().clamp(0.0, 2.0),
                ],
                None,
                48,
            ),
            "filter.spherize" | "filter.pinch" => (
                &DISTORT_EXTRA,
                vec![
                    if id == "filter.spherize" { 1.0 } else { 2.0 },
                    v.get("amount") / 100.0,
                    v.get("mode").round().clamp(0.0, 2.0),
                ],
                None,
                40,
            ),
            "filter.polar" => (&DISTORT_EXTRA, vec![3.0, v.get("to_polar")], None, 40),
            "filter.shear" => (
                &DISTORT_EXTRA,
                vec![
                    4.0,
                    v.get("amount"),
                    v.get("curve").round().clamp(0.0, 2.0),
                    v.get("undefined"),
                ],
                Some(1),
                24,
            ),
            "filter.adaptive_wide_angle" => {
                let projection = v.get("projection").round().clamp(0.0, 2.0);
                let half =
                    ((43.27 / v.get("crop").max(0.1)) / (2.0 * v.get("focal").max(1.0))).atan();
                let edge = match projection as u32 {
                    0 => half,
                    1 => half.tan(),
                    _ => 2.0 * (half / 2.0).sin(),
                };
                (
                    &DISTORT_EXTRA,
                    vec![
                        5.0,
                        projection,
                        (v.get("scale") / 100.0).max(0.05),
                        half.tan(),
                        edge,
                    ],
                    None,
                    48,
                )
            }
            _ => return None,
        };
    Some(FilterOperation::Shader {
        shader,
        params,
        halo,
        work_per_pixel: work,
    })
}

pub fn apply_operation(
    operation: &schist_fx::FilterOperation,
    pixels: &mut [f32],
    w: usize,
    h: usize,
) -> bool {
    match operation {
        schist_fx::FilterOperation::Program {
            build,
            params,
            work_per_pixel,
        } => {
            if !schist_fx::backend()
                .compute_available(w.saturating_mul(h).saturating_mul(*work_per_pixel))
            {
                return false;
            }
            let Some(program) = build(w, h, params) else {
                return false;
            };
            match schist_fx::try_compute(pixels, &program) {
                Some(out) if out.len() == pixels.len() => {
                    pixels.copy_from_slice(&out);
                    true
                }
                _ => false,
            }
        }
        schist_fx::FilterOperation::Shader {
            shader,
            params,
            halo,
            work_per_pixel,
        } => apply(pixels, w, h, shader, params, *halo, *work_per_pixel),
        schist_fx::FilterOperation::Blur { .. } => false,
    }
}

/// Context is encoded into owned parameters before the browser host yields.
pub fn operation_with(
    id: &str,
    v: &schist_plugin_api::FilterValues,
    context: &schist_plugin_api::FilterContext<'_>,
) -> Option<schist_fx::FilterOperation> {
    let mut params = match id {
        "filter.clouds" | "filter.difference_clouds" => vec![
            if id == "filter.clouds" { 0.0 } else { 1.0 },
            v.get("scale").max(4.0),
            1.0,
            v.get("detail").max(1.0) as u32 as f32,
            v.get("seed") as u32 as f32,
        ],
        "filter.fibers" => vec![
            2.0,
            v.get("variance").max(1.0),
            v.get("strength").max(1.0),
            4.0,
            (977 + v.get("seed") as u32) as f32,
        ],
        _ => return operation(id, v),
    };
    let work_per_pixel = params[3] as usize * 16;
    params.extend_from_slice(&context.fg());
    params.extend_from_slice(&context.bg());
    Some(schist_fx::FilterOperation::Shader {
        shader: &CLOUDS,
        params,
        halo: Some(0),
        work_per_pixel,
    })
}
