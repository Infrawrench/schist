#![cfg(not(target_arch = "wasm32"))]

use schist_adjustments::auto::{self, AutoMode};
use schist_compositor_gpu::GpuContext;
use schist_fx::{ComputeJob, ComputeProgram};

fn gpu() -> Option<GpuContext> {
    match GpuContext::new() {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            assert!(
                std::env::var_os("SCHIST_REQUIRE_GPU").is_none(),
                "GPU adapter required: {error}"
            );
            eprintln!("skipping GPU execution: {error}");
            None
        }
    }
}

fn validate(program: &ComputeProgram) {
    for step in &program.steps {
        let source = step.shader.wgsl();
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
            panic!("{}: {}", step.shader.name, error.emit_to_string(&source))
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{}: {}", step.shader.name, error.emit_to_string(&source)));
    }
}

#[test]
fn automatic_corrections_preserve_exact_percentiles_and_transparency() {
    let Some(gpu) = gpu() else {
        return;
    };
    for count in [1usize, 37, 255, 4096, 4097, 131_073] {
        let input: Vec<f32> = (0..count)
            .flat_map(|i| {
                let v = ((i * 3571 + 17) % 10007) as f32 / 9000.0;
                [
                    v - 0.4,
                    v * v * 3.0,
                    1.0 - v,
                    if i % 19 == 3 { 0.0 } else { 0.75 },
                ]
            })
            .collect();
        for mode in [AutoMode::Tone, AutoMode::Contrast, AutoMode::Color] {
            let program = auto::program(count, mode).unwrap();
            validate(&program);
            let output = gpu
                .run_compute(&ComputeJob {
                    input: &input,
                    program: &program,
                })
                .expect("GPU correction declined");
            let mut expected = input.clone();
            assert!(auto::apply_cpu(&mut expected, mode));
            for (i, (&a, &b)) in output.iter().zip(&expected).enumerate() {
                assert!(
                    (a - b).abs() < 2e-5,
                    "{mode:?} {count} pixels at {i}: GPU {a} CPU {b}"
                );
            }
            for (original, result) in input
                .as_chunks::<4>()
                .0
                .iter()
                .zip(output.as_chunks::<4>().0.iter())
            {
                assert_eq!(original[3], result[3]);
                if original[3] == 0.0 {
                    assert_eq!(original, result);
                }
            }
        }
    }
    for pixel in [[1.5, -0.25, 0.8, 0.0], [0.0, -0.0, 0.0, 1.0]] {
        let input = pixel.repeat(17);
        let program = auto::program(17, AutoMode::Color).unwrap();
        let output = gpu
            .run_compute(&ComputeJob {
                input: &input,
                program: &program,
            })
            .unwrap();
        assert!(output.iter().all(|v| v.is_finite()));
        let mut expected = input.clone();
        auto::apply_cpu(&mut expected, AutoMode::Color);
        assert_eq!(output, expected);
    }
}

#[test]
fn padding_and_grouped_transposed_convolution_match_tract() {
    use schist_neural::{Fit, Input, Model, ModelSpec, Range};
    static SPEC: ModelSpec = ModelSpec {
        id: "gpu-pad-convtranspose",
        name: "GPU regression",
        file: "",
        url: None,
        sha256: None,
        bytes: 0,
        input: Input::Frame {
            width: 5,
            height: 4,
            fit: Fit::Stretch,
        },
        range: Range::Unit,
        license: "",
        note: "",
    };
    let Some(gpu) = gpu() else {
        return;
    };
    for (name, bytes) in [
        (
            "indexing",
            include_bytes!("../../neural/tests/fixtures/gpu-indexing.onnx").as_slice(),
        ),
        (
            "pad-convtranspose",
            include_bytes!("../../neural/tests/fixtures/gpu-pad-convtranspose.onnx").as_slice(),
        ),
        (
            "attention",
            include_bytes!("../../neural/tests/fixtures/gpu-attention.onnx").as_slice(),
        ),
        (
            "resize-linear",
            include_bytes!("../../neural/tests/fixtures/gpu-resize-linear.onnx").as_slice(),
        ),
        (
            "resize-cubic",
            include_bytes!("../../neural/tests/fixtures/gpu-resize-cubic.onnx").as_slice(),
        ),
        (
            "resize-nearest",
            include_bytes!("../../neural/tests/fixtures/gpu-resize-nearest.onnx").as_slice(),
        ),
    ] {
        let model = Model::from_bytes(&SPEC, bytes).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let program = model
            .gpu_program()
            .unwrap_or_else(|| panic!("{name} must compile for GPU"));
        validate(program);
        let rgb: Vec<f32> = (0..60)
            .map(|i| ((i * 13 % 67) as f32 - 19.0) / 40.0)
            .collect();
        let input: Vec<f32> = (0..3)
            .flat_map(|c| rgb.as_chunks::<3>().0.iter().map(move |p| p[c]))
            .collect();
        let output = gpu
            .run_compute(&ComputeJob {
                input: &input,
                program,
            })
            .expect("GPU inference declined");
        let expected = model.run_scores(&rgb).unwrap();
        if name == "indexing" {
            assert_eq!(output.len(), expected.len() * 2);
            for (a, b) in output[expected.len()..].iter().zip(&expected) {
                assert!((a + b).abs() < 3e-4);
            }
        } else {
            assert_eq!(output.len(), expected.len(), "{name}");
        }
        for (i, (a, b)) in output.iter().zip(expected).enumerate() {
            assert!((a - b).abs() < 3e-4, "{name} at {i}: GPU {a} tract {b}");
        }
    }
    for id in ["waifu2x-art", "waifu2x-photo"] {
        let model = schist_neural::get(id).unwrap();
        assert!(
            model.gpu_program().is_some(),
            "{id} must compile its complete graph"
        );
    }
}

#[test]
fn immutable_upload_cache_reuses_exact_inputs_and_detects_changes() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut input: Vec<f32> = (0..4096).map(|i| (i % 41) as f32 / 41.0).collect();
    let params = schist_adjustments::Params::Invert;
    let mut program = schist_adjustments::gpu::buffer_program(&params, input.len()).unwrap();
    program.work = usize::MAX;
    let first = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .unwrap();
    let cold = gpu.compute_cache_stats();
    let second = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .unwrap();
    let warm = gpu.compute_cache_stats();
    assert_eq!(first, second);
    assert!(warm.hits > cold.hits);
    input[0] = 0.125;
    let changed = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .unwrap();
    assert_eq!(changed[0], 0.875);
    assert!(gpu.compute_cache_stats().uploads > warm.uploads);
    gpu.clear_compute_cache();
    assert_eq!(gpu.compute_cache_stats().bytes, 0);
}

#[test]
fn nonlocal_effects_read_across_output_bands_and_texture_pages() {
    use schist_fx::{ShaderJob, ShaderSpec};
    static REMAP: ShaderSpec = ShaderSpec {
        name: "paged-regression",
        source: r#"
fn effect(pos: vec2<i32>) -> vec4<f32> {
    return read_pixel(vec2<i32>(i32(image.width) - 1 - pos.x, i32(image.height) - 1 - pos.y));
}
"#,
    };
    let Some(gpu) = gpu() else {
        return;
    };
    for (w, h) in [(37usize, 29usize), (1025, 1025)] {
        gpu.set_binding_limit(w * 16 * 61.min(h / 2));
        let input: Vec<f32> = (0..w * h * 4).map(|i| (i % 7999) as f32 / 7900.0).collect();
        let out = gpu
            .run_shader(&ShaderJob {
                shader: &REMAP,
                px: &input,
                width: w,
                height: h,
                params: &[],
                halo: None,
                work_per_pixel: 1,
            })
            .expect("paged remap declined");
        for (i, p) in out.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(p, &input[(w * h - 1 - i) * 4..(w * h - i) * 4]);
        }
    }
}

#[test]
fn resident_filter_stack_and_auxiliary_displacement_match_cpu() {
    use schist_fx::{ComputeJob, FilterOperation};
    use schist_plugin_api::{FilterContext, FilterImage, FilterPlugin, FilterValues};
    let Some(gpu) = gpu() else {
        return;
    };
    let (w, h) = (37, 29);
    let input: Vec<f32> = (0..w * h * 4)
        .map(|i| {
            if i % 4 == 3 {
                [0.0, 0.01, 0.75, 1.0][i / 4 % 4]
            } else {
                ((i * 19) % 157) as f32 / 140.0 - 0.1
            }
        })
        .collect();
    let filters: Vec<Box<dyn FilterPlugin>> = vec![
        Box::new(schist_filters_core::BoxBlur),
        Box::new(schist_filters_core::other::HighPass),
        Box::new(schist_filters_core::other::Offset),
    ];
    let mut operations = Vec::new();
    let mut expected = input.clone();
    for filter in filters {
        let values = FilterValues::defaults(&filter.params());
        operations.push(filter.gpu_operation(&values).unwrap());
        filter.apply(&mut expected, w, h, &values);
    }
    let program = FilterOperation::Sequence(operations).program(w, h).unwrap();
    validate(&program);
    let out = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .unwrap();
    for (a, b) in out.iter().zip(expected) {
        assert!((a - b).abs() < 1e-4, "stack: {a} != {b}");
    }
    let filter = schist_filters_core::distort::Displace;
    let map = FilterImage {
        width: 11,
        height: 7,
        pixels: (0..11 * 7 * 4).map(|i| (i % 97) as f32 / 96.0).collect(),
    };
    for map in [None, Some(&map)] {
        for tile in [0.0, 1.0] {
            for wrap in [0.0, 1.0] {
                let mut values = FilterValues::defaults(&filter.params());
                values.set("fit", tile);
                values.set("undefined", wrap);
                let context = FilterContext {
                    map,
                    ..Default::default()
                };
                let program = filter
                    .gpu_operation_with(&values, &context)
                    .unwrap()
                    .program(w, h)
                    .unwrap();
                validate(&program);
                let mut expected = input.clone();
                filter.apply_with(&mut expected, w, h, &values, &context);
                let out = gpu
                    .run_compute(&ComputeJob {
                        input: &input,
                        program: &program,
                    })
                    .unwrap();
                for (a, b) in out
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0.iter())
                {
                    assert!((a[3] - b[3]).abs() < 5e-4);
                    for c in 0..3 {
                        assert!(
                            (a[c] * a[3] - b[c] * b[3]).abs() < 5e-4,
                            "displace: {a:?} != {b:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn gallery_families_and_complete_blurs_match_cpu() {
    use schist_plugin_api::{FilterContext, FilterValues, PluginManifest, PluginRegistry};
    let Some(gpu) = gpu() else {
        return;
    };
    let mut registry = PluginRegistry::new();
    schist_filters_core::CoreFiltersPlugin.register(&mut registry);
    let ids = [
        "tree",
        "flame",
        "extrude",
        "blur",
        "blur_more",
        "sharpen_edges",
        "sharpen_more",
        "diffuse_glow",
        "despeckle",
        "dust_scratches",
        "custom",
        "hsb_hsl",
        "ntsc_colors",
        "deinterlace",
        "average",
        "ripple",
        "wave",
        "camera_raw",
        "craquelure",
        "grain",
        "mosaic_tiles",
        "patchwork",
        "stained_glass",
        "lens_flare",
        "lighting_effects",
        "picture_frame",
        "bump_map",
        "normal_map",
        "glowing_edges",
        "solarize",
        "wind",
        "tiles",
        "diffuse",
        "oil_paint",
        "bas_relief",
        "chalk_charcoal",
        "charcoal",
        "chrome",
        "conte_crayon",
        "graphic_pen",
        "halftone_pattern",
        "note_paper",
        "photocopy",
        "plaster",
        "reticulation",
        "stamp",
        "torn_edges",
        "water_paper",
        "cutout",
        "dry_brush",
        "film_grain",
        "fresco",
        "colored_pencil",
        "neon_glow",
        "paint_daubs",
        "palette_knife",
        "plastic_wrap",
        "poster_edges",
        "rough_pastels",
        "smudge_stick",
        "sponge",
        "underpainting",
        "watercolor",
        "accented_edges",
        "angled_strokes",
        "crosshatch",
        "dark_strokes",
        "ink_outlines",
        "spatter",
        "sprayed_strokes",
        "sumi_e",
        "reduce_noise",
        "lens_blur",
        "smart_sharpen",
        "color_halftone",
    ];
    for (w, h) in [(1, 7), (29, 23)] {
        let input: Vec<f32> = (0..w * h * 4)
            .map(|i| {
                if i % 4 == 3 {
                    [0.0, 0.000002, 0.75, 1.0][i / 4 % 4]
                } else {
                    ((i * 71) % 307) as f32 / 280.0 - 0.05
                }
            })
            .collect();
        let backdrop: Vec<f32> = input.iter().rev().copied().collect();
        let context = FilterContext {
            backdrop: Some(&backdrop),
            foreground: schist_color::Rgba::new(0.2, 0.1, 0.7, 1.0),
            background: schist_color::Rgba::new(0.8, 0.6, 0.3, 1.0),
            ..Default::default()
        };
        for id in ids {
            let filter = registry
                .filters()
                .find(|f| f.id() == format!("filter.{id}"))
                .unwrap();
            for variant in 0..3 {
                let mut values = FilterValues::defaults(&filter.params());
                if variant > 0 {
                    if id == "lens_blur" {
                        values.set("shape", variant as f32);
                        values.set("curvature", 35.0);
                        values.set("rotation", 31.0);
                        values.set("depth", variant as f32);
                        values.set("noise", 17.0);
                        values.set("threshold", 41.0);
                        values.set("invert_depth", 1.0);
                    } else if id == "camera_raw" {
                        for (key, value) in [
                            ("temperature", -23.0),
                            ("tint", 31.0),
                            ("exposure", 0.7),
                            ("contrast", 17.0),
                            ("highlights", -33.0),
                            ("shadows", 42.0),
                            ("whites", 27.0),
                            ("blacks", -26.0),
                            ("clarity", 45.0),
                            ("dehaze", -12.0),
                            ("vibrance", 32.0),
                            ("saturation", -28.0),
                            ("noise", 63.0),
                            ("sharpening", 54.0),
                            ("vignette", -35.0),
                        ] {
                            values.set(key, value);
                        }
                    } else if id == "reduce_noise" {
                        values.set("jpeg", 1.0);
                        values.set("sharpen", 31.0);
                        values.set("colour", 65.0);
                    } else if id == "smart_sharpen" {
                        values.set("remove", ((variant - 1) * 2) as f32);
                        values.set("angle", 33.0);
                    } else {
                        continue;
                    }
                }
                let operation = filter
                    .gpu_operation_with(&values, &context)
                    .unwrap_or_else(|| panic!("missing {id}"));
                let program = operation.program(w, h).unwrap();
                validate(&program);
                let mut expected = input.clone();
                filter.apply_with(&mut expected, w, h, &values, &context);
                let out = gpu
                    .run_compute(&ComputeJob {
                        input: &input,
                        program: &program,
                    })
                    .unwrap_or_else(|| panic!("GPU {id} declined"));
                for (i, (a, b)) in out.iter().zip(&expected).enumerate() {
                    assert!(
                        (a - b).abs() < 5e-4,
                        "{id} {w}x{h} variant {variant} at {i}: GPU {a}, CPU {b}"
                    );
                }
            }
        }
    }
}

#[test]
fn connected_selections_match_flood_fill_through_long_paths() {
    use schist_core::selection_gpu::{self, ColorMatch};
    let Some(gpu) = gpu() else {
        return;
    };
    for (w, h) in [(1usize, 65usize), (63, 65), (257, 259)] {
        for pattern in 0..3 {
            let allowed: Vec<bool> = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    match pattern {
                        0 => true,
                        1 => y % 2 == 0 || (y % 4 == 1 && x == w - 1) || (y % 4 == 3 && x == 0),
                        _ => (i * 3571 % 733) % 5 != 0,
                    }
                })
                .collect();
            let input: Vec<f32> = allowed
                .iter()
                .flat_map(|&on| [if on { 0.25 } else { 0.9 }, 0.3, 0.4, 1.0])
                .collect();
            let mut seeds = vec![0.0; w * h];
            seeds[0] = 1.0;
            if pattern == 2 {
                seeds[w * h - 1] = 1.0;
            }
            let mut expected: Vec<bool> = seeds.iter().map(|&v| v > 0.0).collect();
            let mut stack: Vec<_> = expected
                .iter()
                .enumerate()
                .filter_map(|(i, &v)| v.then_some(i))
                .collect();
            while let Some(i) = stack.pop() {
                for neighbor in [
                    (i % w > 0).then(|| i - 1),
                    (i % w + 1 < w).then(|| i + 1),
                    (i >= w).then(|| i - w),
                    (i + w < w * h).then(|| i + w),
                ]
                .into_iter()
                .flatten()
                {
                    if allowed[neighbor] && !expected[neighbor] {
                        expected[neighbor] = true;
                        stack.push(neighbor);
                    }
                }
            }
            let program = selection_gpu::program(
                w,
                h,
                ColorMatch::Rgb {
                    color: [0.25, 0.3, 0.4],
                    tolerance: 0.01,
                },
                Some(&seeds),
            )
            .unwrap();
            validate(&program);
            let result = gpu
                .run_compute(&ComputeJob {
                    input: &input,
                    program: &program,
                })
                .expect("GPU flood declined");
            assert_eq!(
                result.iter().map(|&v| v > 0.0).collect::<Vec<_>>(),
                expected,
                "{w}x{h}, pattern {pattern}"
            );
        }
    }
}

/// Optional checks for the catalogue's downloadable models; ordinary CI uses
/// the small committed operator fixtures and bundled Waifu2x weights.
#[test]
fn downloaded_catalogue_graphs_compile() {
    let Some(directory) = std::env::var_os("SCHIST_GPU_MODEL_DIR") else {
        return;
    };
    for id in ["depth", "segment", "embed-image"] {
        let path = std::path::Path::new(&directory).join(format!("{id}.onnx"));
        let bytes = std::fs::read(path).unwrap();
        let spec = schist_neural::CATALOG.iter().find(|s| s.id == id).unwrap();
        let model = schist_neural::Model::from_bytes(spec, &bytes).unwrap();
        let program = model
            .gpu_program()
            .unwrap_or_else(|| panic!("{id} graph declined"));
        validate(program);
        if std::env::var_os("SCHIST_GPU_MODEL_EXECUTE").is_some() {
            let gpu = gpu().expect("GPU required for catalogue inference");
            let (w, h) = spec.input.dims();
            let rgb: Vec<f32> = (0..w * h * 3)
                .map(|i| (i * 11 % 1009) as f32 / 1008.0)
                .collect();
            let input: Vec<f32> = (0..3)
                .flat_map(|c| {
                    rgb.as_chunks::<3>()
                        .0
                        .iter()
                        .map(move |p| match spec.range {
                            schist_neural::Range::Unit => p[c],
                            schist_neural::Range::Byte => p[c] * 255.0,
                            schist_neural::Range::Standard { mean, sd } => (p[c] - mean[c]) / sd[c],
                        })
                })
                .collect();
            let output = gpu
                .run_compute(&ComputeJob {
                    input: &input,
                    program,
                })
                .unwrap_or_else(|| panic!("{id} inference declined"));
            let expected = model.run_scores(&rgb).unwrap();
            for (i, (a, b)) in output.iter().zip(expected).enumerate() {
                assert!(
                    (a - b).abs() < 3e-4 * (1.0 + b.abs()),
                    "{id} output {i}: {a} != {b}"
                );
            }
        }
    }
}

#[test]
fn cms_clut_and_cicp_transforms_preserve_the_reference() {
    use schist_colormgmt::{ColorTransform, Intent, Profile};
    let Some(gpu) = gpu() else {
        return;
    };
    let mut lab_profile = moxcms::ColorProfile::new_lab();
    lab_profile.pcs = moxcms::DataColorSpace::Lab;
    let native = schist_colormgmt::NativeColorTransform::new(
        schist_color::ColorMode::Lab,
        Some(&lab_profile.encode().unwrap()),
    )
    .unwrap();
    let samples = (0..257)
        .map(|i| schist_color::NativePixel {
            mode: schist_color::ColorMode::Lab,
            color: [
                i as f32 / 256.0,
                ((i * 73) % 257) as f32 / 256.0,
                ((i * 37) % 257) as f32 / 256.0,
                0.0,
            ],
            alpha: 0.75,
        })
        .collect::<Vec<_>>();
    let input = samples
        .iter()
        .flat_map(|p| p.color[..3].iter().copied())
        .collect::<Vec<_>>();
    let program = native
        .to_rgb_program(samples.len())
        .expect("native Lab conversion must compile");
    let actual = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .expect("native Lab conversion");
    let expected = schist_colormgmt::native_to_rgba(&samples, Some(&native));
    for (a, b) in actual
        .as_chunks::<3>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0.iter())
    {
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() < 3e-4);
        }
    }
    let inverse = native
        .from_rgb_program(samples.len())
        .expect("inverse native Lab conversion must compile");
    let back = gpu
        .run_compute(&ComputeJob {
            input: &actual,
            program: &inverse,
        })
        .expect("inverse native Lab conversion");
    let rgba = actual
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 0.75])
        .collect::<Vec<_>>();
    let reference =
        schist_colormgmt::rgba_to_native(schist_color::ColorMode::Lab, &rgba, Some(&native));
    for (a, b) in back.as_chunks::<3>().0.iter().zip(reference) {
        for (a, b) in a.iter().zip(b.color) {
            assert!((a - b).abs() < 3e-4);
        }
    }
    let mut profiles = Vec::new();
    for transfer in [
        moxcms::TransferCharacteristics::Bt709,
        moxcms::TransferCharacteristics::Smpte2084,
        moxcms::TransferCharacteristics::Hlg,
        moxcms::TransferCharacteristics::Bt470Bg,
    ] {
        let profile = moxcms::ColorProfile::new_from_cicp(moxcms::CicpProfile {
            color_primaries: moxcms::CicpColorPrimaries::Bt709,
            transfer_characteristics: transfer,
            matrix_coefficients: moxcms::MatrixCoefficients::Identity,
            full_range: true,
        });
        profiles.push(Profile::from_bytes(&profile.encode().unwrap()).unwrap());
    }
    profiles.push(Profile::from_bytes(&moxcms::ColorProfile::new_lab().encode().unwrap()).unwrap());
    let mut clut = moxcms::ColorProfile::new_srgb();
    let mut values = Vec::new();
    for r in 0..3 {
        for g in 0..3 {
            for b in 0..3 {
                values.extend(
                    [
                        (r as f32 * 0.35 + g as f32 * 0.1 + b as f32 * 0.025) * 65535.0,
                        (r as f32 * 0.1 + g as f32 * 0.35 + b as f32 * 0.05) * 65535.0,
                        (r as f32 * 0.025 + g as f32 * 0.05 + b as f32 * 0.3) * 65535.0,
                    ]
                    .map(|v| v.round() as u16),
                );
            }
        }
    }
    let table = moxcms::LutWarehouse::Multidimensional(moxcms::LutMultidimensionalType {
        num_input_channels: 3,
        num_output_channels: 3,
        grid_points: [3, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        clut: Some(moxcms::LutStore::Store16(values)),
        a_curves: vec![moxcms::ToneReprCurve::Parametric(vec![1.0]); 3],
        b_curves: vec![moxcms::ToneReprCurve::Parametric(vec![1.0]); 3],
        m_curves: vec![],
        matrix: moxcms::Matrix3d::IDENTITY,
        bias: Default::default(),
    });
    clut.lut_a_to_b_perceptual = Some(table.clone());
    clut.lut_a_to_b_colorimetric = Some(table.clone());
    clut.lut_a_to_b_saturation = Some(table);
    profiles.push(Profile::from_bytes(&clut.encode().unwrap()).unwrap());
    let input: Vec<f32> = (0..8192)
        .flat_map(|i| {
            [
                (i * 29 % 1027) as f32 / 1024.0 - 0.001,
                (i * 47 % 1009) as f32 / 1008.0,
                (i * 61 % 1031) as f32 / 1028.0,
                if i % 7 == 0 { 0.0 } else { 0.75 },
            ]
        })
        .collect();
    for (i, profile) in profiles.iter().enumerate() {
        for reverse in [false, true] {
            if reverse && i == profiles.len() - 1 {
                continue;
            }
            let srgb = Profile::srgb();
            let (src, dst) = if reverse {
                (&srgb, profile)
            } else {
                (profile, &srgb)
            };
            let transform = ColorTransform::new(src, dst, Intent::Perceptual).unwrap();
            let operation = transform
                .gpu_operation()
                .unwrap_or_else(|| panic!("profile {i} reverse {reverse} has no GPU operation"));
            let program = operation.program(input.len() / 4, 1).unwrap();
            validate(&program);
            let output = gpu
                .run_compute(&ComputeJob {
                    input: &input,
                    program: &program,
                })
                .unwrap();
            let mut expected = input.clone();
            transform.apply(&mut expected);
            for (j, (a, b)) in output.iter().zip(expected).enumerate() {
                assert!(
                    (a - b).abs() < 3e-4,
                    "profile {i} reverse {reverse} channel {j}: {a} != {b}"
                );
            }
            assert!(output
                .as_chunks::<4>()
                .0
                .iter()
                .zip(input.as_chunks::<4>().0.iter())
                .all(|(a, b)| a[3] == b[3]));
        }
    }
}

#[test]
fn complete_tiled_neural_filters_match_cpu_and_preserve_alpha() {
    let Some(gpu) = gpu() else {
        return;
    };
    for id in ["dejpeg", "detail"] {
        let model = schist_neural::get(id).unwrap();
        for (w, h) in [(1, 7), (137, 119)] {
            let input: Vec<f32> = (0..w * h)
                .flat_map(|i| {
                    [
                        (i * 13 % 71) as f32 / 70.0,
                        (i * 17 % 83) as f32 / 82.0,
                        (i * 19 % 97) as f32 / 96.0,
                        if i % 11 == 0 { 0.0 } else { 0.7 },
                    ]
                })
                .collect();
            let mut expected: Vec<f32> = input
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            schist_neural::run_tiled(&model, &mut expected, w, h, 0.63);
            let program = model.rgba_operation(0.63).unwrap().program(w, h).unwrap();
            validate(&program);
            let output = gpu
                .run_compute(&ComputeJob {
                    input: &input,
                    program: &program,
                })
                .unwrap();
            for ((pixel, original), expected) in output
                .as_chunks::<4>()
                .0
                .iter()
                .zip(input.as_chunks::<4>().0)
                .zip(expected.as_chunks::<3>().0)
            {
                for c in 0..3 {
                    assert!(
                        (pixel[c] - expected[c]).abs() < 3e-4,
                        "{id} {w}x{h}: {pixel:?} != {expected:?}"
                    );
                }
                assert_eq!(pixel[3], original[3]);
            }
        }
    }
}

#[test]
fn asynchronous_raw_development_includes_super_ccd_and_every_orientation() {
    use schist_codec_raw::{
        Cfa, CfaColor, DevelopOptions, Format, Orientation, RawData, RawImage, Rect,
    };
    let Some(gpu) = gpu() else {
        return;
    };
    let bayer = [
        CfaColor::Red,
        CfaColor::Green,
        CfaColor::Green,
        CfaColor::Blue,
    ];
    let mut cases = Vec::new();
    for staggered in [false, true] {
        let fw = 19;
        let crop = Rect {
            x: 3,
            y: 1,
            width: fw << u32::from(!staggered),
            height: 29,
        };
        let (w, h) = (crop.x + crop.width + 2, crop.y + crop.height + 2);
        let cfa = Cfa::super_ccd(staggered, fw, bayer, (crop.x, crop.y));
        let input = (0..w * h).map(|i| (i * 17 % 311) as f32 / 290.0).collect();
        let mut raw = RawImage::new(Format::Raf, w, h, 1, RawData::F32(input), cfa);
        raw.crop = crop;
        raw.white_level = 1.0;
        raw.black_levels = [0.02; 4];
        raw.wb_coeffs = [1.1, 1.0, 0.9, 1.0];
        cases.push(raw);
    }
    let (w, h) = (41, 35);
    let raw = RawImage::new(
        Format::Dng,
        w,
        h,
        1,
        RawData::F32((0..w * h).map(|i| (i * 23 % 173) as f32 / 172.0).collect()),
        Cfa::Bayer(bayer),
    );
    cases.push(raw);
    for mut raw in cases {
        for orientation in 1..=8 {
            raw.orientation = Orientation::from_exif(orientation);
            for quality in [
                schist_codec_raw::demosaic::Quality::Fast,
                schist_codec_raw::demosaic::Quality::Best,
            ] {
                let options = DevelopOptions {
                    quality,
                    ..Default::default()
                };
                let expected = schist_codec_raw::develop(&raw, &options).unwrap();
                // Require successful submission: AsyncCompute fallback cannot hide a missing kernel.
                struct Required<'a>(&'a GpuContext);
                impl schist_fx::AsyncCompute for Required<'_> {
                    async fn compute_async<'a>(&'a self, job: ComputeJob<'a>) -> Option<Vec<f32>> {
                        validate(job.program);
                        Some(
                            self.0
                                .run_compute_async(&job)
                                .await
                                .expect("RAW GPU declined"),
                        )
                    }
                }
                let output = pollster::block_on(schist_codec_raw::develop::develop_async(
                    &raw,
                    &options,
                    &Required(&gpu),
                ))
                .unwrap();
                assert_eq!(
                    (output.width, output.height),
                    (expected.width, expected.height)
                );
                for (i, (a, b)) in output.rgb.iter().zip(expected.rgb).enumerate() {
                    assert!(
                        (a - b).abs() < 1e-4,
                        "{:?} {quality:?} orientation {orientation} at {i}: {a} != {b}",
                        raw.cfa
                    );
                }
            }
        }
    }
}

#[path = "support/owned_edits.rs"]
mod owned_edits;

#[test]
fn asynchronous_tool_edits_preserve_preview_and_undo_contracts() {
    let Some(gpu) = gpu() else {
        return;
    };
    pollster::block_on(owned_edits::verify(&gpu));
    pollster::block_on(owned_edits::export(&gpu));
}

#[test]
fn complete_raw_exposure_and_decoded_cache_match_cpu() {
    let Some(gpu) = gpu() else {
        return;
    };
    pollster::block_on(owned_edits::raw_development(&gpu));
}

#[test]
fn native_cmyk_clut_preserves_all_four_channels() {
    use moxcms::{
        ColorProfile, DataColorSpace, LutMultidimensionalType, LutStore, LutWarehouse, Matrix3d,
        ToneReprCurve,
    };
    use schist_color::{ColorMode, NativePixel};
    let Some(gpu) = gpu() else {
        return;
    };
    let mut profile = ColorProfile::new_srgb();
    profile.color_space = DataColorSpace::Cmyk;
    profile.cicp = None;
    let values = (0..81usize)
        .flat_map(|i| {
            let k = (i % 3) as f32 / 2.0;
            let y = (i / 3 % 3) as f32 / 2.0;
            let m = (i / 9 % 3) as f32 / 2.0;
            let c = (i / 27) as f32 / 2.0;
            [
                (1.0 - c) * (1.0 - k) * 0.4,
                (1.0 - m) * (1.0 - k) * 0.5,
                (1.0 - y) * (1.0 - k) * 0.3,
            ]
            .map(|v| (v * 65535.0).round() as u16)
        })
        .collect();
    profile.lut_a_to_b_perceptual = Some(LutWarehouse::Multidimensional(LutMultidimensionalType {
        num_input_channels: 4,
        num_output_channels: 3,
        grid_points: [3, 3, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        clut: Some(LutStore::Store16(values)),
        a_curves: vec![ToneReprCurve::Parametric(vec![1.0]); 4],
        b_curves: vec![ToneReprCurve::Parametric(vec![1.0]); 3],
        m_curves: vec![],
        matrix: Matrix3d::IDENTITY,
        bias: Default::default(),
    }));
    let bytes = profile.encode().unwrap();
    let transform =
        schist_colormgmt::NativeColorTransform::new(ColorMode::Cmyk, Some(&bytes)).unwrap();
    let samples = (0..4097)
        .map(|i| NativePixel {
            mode: ColorMode::Cmyk,
            color: [i % 257, i * 13 % 257, i * 37 % 257, i * 47 % 257].map(|n| n as f32 / 256.0),
            alpha: 0.3,
        })
        .collect::<Vec<_>>();
    let input = samples.iter().flat_map(|p| p.color).collect::<Vec<_>>();
    let program = transform
        .to_rgb_program(samples.len())
        .expect("4D native CLUT must compile");
    let output = gpu
        .run_compute(&ComputeJob {
            input: &input,
            program: &program,
        })
        .expect("native CMYK GPU conversion");
    let expected = schist_colormgmt::native_to_rgba(&samples, Some(&transform));
    for (i, (a, b)) in output
        .as_chunks::<3>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0.iter())
        .enumerate()
    {
        for c in 0..3 {
            assert!(
                (a[c] - b[c]).abs() < 3e-4,
                "CMYK {i}.{c}: {} != {}",
                a[c],
                b[c]
            );
        }
    }
}
