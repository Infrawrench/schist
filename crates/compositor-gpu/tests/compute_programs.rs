#![cfg(not(target_arch = "wasm32"))]
use schist_color::{ColorMode, Depth, NativePixel, Rgba};
use schist_compositor_gpu::GpuContext;
use schist_core::{
    resample::{self, Affine, Filter},
    IntRect, LayerStyle, SelectOp, Selection, TileCoord, TileMap, TILE_SIZE,
};
use schist_fx::{ComputeJob, FxBackend};
use std::sync::{Arc, Mutex};

struct Tracking {
    ctx: GpuContext,
    seen: Mutex<Vec<&'static str>>,
}
impl FxBackend for Tracking {
    fn compute_available(&self, _: usize) -> bool {
        true
    }
    fn name(&self) -> &'static str {
        "forced compute"
    }
    fn compute(&self, job: &ComputeJob<'_>) -> Option<Vec<f32>> {
        let out = self.ctx.run_compute(job).unwrap_or_else(|| {
            for step in &job.program.steps {
                let source = step.shader.wgsl();
                let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| {
                    panic!("{}: {}", step.shader.name, e.emit_to_string(&source))
                });
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::empty(),
                )
                .validate(&module)
                .unwrap_or_else(|e| panic!("{}: {e}", step.shader.name));
            }
            panic!(
                "GPU declined {:?}",
                job.program
                    .steps
                    .iter()
                    .map(|s| s.shader.name)
                    .collect::<Vec<_>>()
            );
        });
        self.seen
            .lock()
            .unwrap()
            .extend(job.program.steps.iter().map(|s| s.shader.name));
        Some(out)
    }
}
struct Restore(Arc<dyn FxBackend>);
impl Drop for Restore {
    fn drop(&mut self) {
        schist_fx::set_backend(self.0.clone());
    }
}
#[track_caller]
fn close(a: &[f32], b: &[f32], t: f32) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert!((a - b).abs() <= t, "at {i}: GPU={a} CPU={b}");
    }
}
#[track_caller]
fn close_pixels(a: &[f32], b: &[f32], t: f32) {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    for (a, b) in a
        .as_chunks_mut::<5>()
        .0
        .iter_mut()
        .zip(b.as_chunks_mut::<5>().0.iter_mut())
    {
        // Compare the color contributed to compositing. Unpremultiplying a
        // faint shadow amplifies backend rounding (Metal vs. Vulkan) without
        // changing its visible color. Alpha is still compared independently.
        for c in 0..4 {
            a[c] *= a[4];
            b[c] *= b[4];
        }
    }
    close(&a, &b, t);
}
fn mask(s: &Selection, rect: IntRect) -> Vec<f32> {
    (rect.top..rect.bottom)
        .flat_map(|y| (rect.left..rect.right).map(move |x| s.coverage(x, y) as f32))
        .collect()
}
fn tile_data(s: &TileMap, rect: IntRect) -> Vec<f32> {
    (rect.top..rect.bottom)
        .flat_map(|y| {
            (rect.left..rect.right).flat_map(move |x| {
                let p = s.native_pixel(x, y);
                [p.color[0], p.color[1], p.color[2], p.color[3], p.alpha]
            })
        })
        .collect()
}

#[test]
fn programs_match_real_layer_styles_masks_and_affine_callers() {
    let _restore = Restore(schist_fx::backend());
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().expect("compute tests need an adapter"),
        seen: Mutex::new(vec![]),
    });
    for (w, h, r) in [(1, 17, 8.0), (37, 29, 0.7), (37, 29, 4.1), (17, 1, 13.0)] {
        let input: Vec<f32> = (0..w * h).map(|i| (i % 23) as f32 / 22.0).collect();
        let mut a = input.clone();
        let mut b = input;
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        schist_layer_fx::gaussian_alpha(&mut b, w, h, r);
        schist_fx::set_backend(gpu.clone());
        schist_layer_fx::gaussian_alpha(&mut a, w, h, r);
        close(&a, &b, 1e-5);
    }
    let mut style = LayerStyle::default();
    style.drop_shadow.enabled = true;
    style.inner_shadow.enabled = true;
    style.outer_glow.enabled = true;
    style.inner_glow.enabled = true;
    style.bevel.enabled = true;
    style.satin.enabled = true;
    style.stroke.enabled = true;
    let rect = IntRect::new(-7, -4, 23, 19);
    let pixel = |x, y| {
        if rect.contains(x, y) {
            Rgba::new(0.2, 0.5, 0.7, if (x + y) % 5 == 0 { 0.6 } else { 1.0 })
        } else {
            Rgba::TRANSPARENT
        }
    };
    schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
    let expected = schist_layer_fx::render_content(rect, pixel, &style, 0.7).unwrap();
    schist_fx::set_backend(gpu.clone());
    let actual = schist_layer_fx::render_content(rect, pixel, &style, 0.7).unwrap();
    close_pixels(
        &tile_data(&actual.tiles, rect.inflated(40)),
        &tile_data(&expected.tiles, rect.inflated(40)),
        1e-4,
    );
    // Photoshop's spread/range/noise follows a different path from the
    // original intensity-based glow, including at negative coordinates.
    for spread in [0.0, 0.21, 1.0] {
        let mut photoshop = style;
        for effect in [&mut photoshop.outer_glow, &mut photoshop.inner_glow] {
            effect.settings.size = 12.0;
            effect.settings.spread = spread;
            effect.settings.falloff = schist_core::style::GlowFalloff::Photoshop {
                range: 0.5,
                noise: 0.05,
            };
        }
        photoshop.inner_shadow.settings.blend = schist_core::BlendMode::Dissolve;
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = schist_layer_fx::render_content(rect, pixel, &photoshop, 0.7).unwrap();
        schist_fx::set_backend(gpu.clone());
        let actual = schist_layer_fx::render_content(rect, pixel, &photoshop, 0.7).unwrap();
        close_pixels(
            &tile_data(&actual.tiles, rect.inflated(40)),
            &tile_data(&expected.tiles, rect.inflated(40)),
            1e-4,
        );
    }
    for (i, bevel) in [
        schist_core::BevelStyle_::InnerBevel,
        schist_core::BevelStyle_::OuterBevel,
        schist_core::BevelStyle_::Emboss,
        schist_core::BevelStyle_::PillowEmboss,
    ]
    .into_iter()
    .enumerate()
    {
        style.bevel.settings.style = bevel;
        style.bevel.settings.soften = 1.3;
        style.bevel.settings.depth = -0.7;
        style.blur.enabled = true;
        style.blur.settings.radius = 1.7;
        style.blur.settings.preserve_alpha = i % 2 == 0;
        style.inner_glow.settings.technique = schist_core::Technique::Precise;
        style.outer_glow.settings.technique = schist_core::Technique::Precise;
        style.inner_glow.settings.from_edge = i % 2 == 0;
        style.gradient_overlay.enabled = true;
        style.gradient_overlay.settings.shape = if i % 2 == 0 {
            schist_core::GradientShape::Radial
        } else {
            schist_core::GradientShape::Linear
        };
        style.gradient_overlay.settings.reverse = true;
        style.color_overlay.enabled = true;
        style.color_overlay.settings.blend = schist_core::BlendMode::Color;
        style.color_overlay.settings.opacity = 0.3;
        style.stroke.settings.position = if i % 2 == 0 {
            schist_core::StrokePosition::Inside
        } else {
            schist_core::StrokePosition::Center
        };
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = schist_layer_fx::render_content(rect, pixel, &style, 0.4).unwrap();
        schist_fx::set_backend(gpu.clone());
        let actual = schist_layer_fx::render_content(rect, pixel, &style, 0.4).unwrap();
        close_pixels(
            &tile_data(&actual.tiles, rect.inflated(50)),
            &tile_data(&expected.tiles, rect.inflated(50)),
            1e-4,
        );
    }
    let canvas = IntRect::new(-30, -20, 310, 260);
    let mut original = Selection::new();
    original.select_ellipse(IntRect::new(-11, -7, 276, 241), SelectOp::Replace);
    original.select_rect(IntRect::new(117, 80, 143, 96), SelectOp::Subtract);
    for mode in 0..6 {
        let canvas = if mode == 5 {
            IntRect::new(-30, -20, 640, 550)
        } else {
            canvas
        };
        let mut original = original.clone();
        if mode == 5 {
            original.select_ellipse(IntRect::new(-11, -7, 570, 500), SelectOp::Replace);
        }
        let run = |s: &mut Selection| match mode {
            0 => s.feather(8.3),
            1 => s.expand(7, canvas),
            2 => s.contract(7, canvas),
            3 => s.smooth(7, canvas),
            4 => s.border(14, canvas),
            _ => *s = s.transformed(&Affine::rotate(0.21).around(130.0, 100.0), canvas),
        };
        let mut a = original.clone();
        let mut b = original.clone();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        run(&mut b);
        let before = gpu.seen.lock().unwrap().len();
        schist_fx::set_backend(gpu.clone());
        run(&mut a);
        assert!(
            gpu.seen.lock().unwrap().len() > before,
            "selection mode {mode} did not offload"
        );
        close(
            &mask(&a, canvas.inflated(40)),
            &mask(&b, canvas.inflated(40)),
            1.0,
        );
    }
    for mode in [ColorMode::Rgb, ColorMode::Cmyk, ColorMode::Lab] {
        let mut input = TileMap::new_in_mode(mode);
        for y in -7..237 {
            for x in -11..309 {
                let coord = TileCoord::containing(x, y);
                let tile = input.get_mut_or_insert(coord, Depth::ThirtyTwo);
                let mut p = NativePixel::transparent(mode);
                p.color = [
                    (x.rem_euclid(19) as f32) / 18.0,
                    (y.rem_euclid(13) as f32) / 12.0,
                    0.31,
                    0.67,
                ];
                p.alpha = if (x + y) % 11 == 0 { 0.0 } else { 0.73 };
                tile.set_native_pixel(
                    (y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE)) as usize,
                    p,
                );
            }
        }
        for filter in [Filter::Nearest, Filter::Bilinear, Filter::Bicubic] {
            for matrix in [
                Affine::rotate(0.21).around(130.0, 100.0),
                Affine::scale(1.23, 0.93).then(&Affine::translate(0.5, -0.5)),
                Affine::scale(0.23, 0.19),
            ] {
                let clip = IntRect::new(-100, -100, 450, 350);
                schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
                let expected =
                    resample::transform_tiles(&input, &matrix, Depth::ThirtyTwo, filter, clip);
                schist_fx::set_backend(gpu.clone());
                let actual =
                    resample::transform_tiles(&input, &matrix, Depth::ThirtyTwo, filter, clip);
                // Coverage is discrete; use exactly representable translations in the boundary probe below.
                close_pixels(
                    &tile_data(&actual, clip),
                    &tile_data(&expected, clip),
                    0.0002,
                );
            }
        }
    }
    // Destructive edits preserve their direct formulas, including curves between LUT entries.
    for kind in [
        schist_core::AdjustmentKind::Curves,
        schist_core::AdjustmentKind::Levels,
        schist_core::AdjustmentKind::Vibrance,
        schist_core::AdjustmentKind::Exposure,
        schist_core::AdjustmentKind::ColorBalance,
    ] {
        let mut params = schist_adjustments::Params::default_for(kind);
        params.set_param("vibrance", 55.0);
        params.set_param("exposure", 1.3);
        let mut a: Vec<f32> = (0..4096).map(|i| i as f32 / 3500.0 - 0.08).collect();
        let mut b = a.clone();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        params.apply_buffer(&mut b);
        schist_fx::set_backend(gpu.clone());
        params.apply_buffer(&mut a);
        close(&a, &b, 0.0002);
    }
    let profile = |p: moxcms::ColorProfile| {
        schist_colormgmt::Profile::from_bytes(&p.encode().unwrap()).unwrap()
    };
    for (src, dst) in [
        (
            profile(moxcms::ColorProfile::new_adobe_rgb()),
            schist_colormgmt::Profile::srgb(),
        ),
        (
            schist_colormgmt::Profile::srgb(),
            profile(moxcms::ColorProfile::new_adobe_rgb()),
        ),
        (
            profile(moxcms::ColorProfile::new_aces_cg_linear()),
            schist_colormgmt::Profile::srgb(),
        ),
        (
            schist_colormgmt::Profile::srgb(),
            profile(moxcms::ColorProfile::new_aces_cg_linear()),
        ),
        (
            schist_colormgmt::Profile::srgb(),
            schist_colormgmt::Profile::display_p3(),
        ),
        (
            schist_colormgmt::Profile::display_p3(),
            schist_colormgmt::Profile::srgb(),
        ),
    ] {
        let transform = schist_colormgmt::ColorTransform::new(
            &src,
            &dst,
            schist_colormgmt::Intent::RelativeColorimetric,
        )
        .unwrap();
        let mut a: Vec<f32> = (0..8192)
            .map(|i| (i * 197 % 1009) as f32 / 1008.0)
            .collect();
        let mut b = a.clone();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        transform.apply(&mut b);
        let before = gpu.seen.lock().unwrap().len();
        schist_fx::set_backend(gpu.clone());
        transform.apply(&mut a);
        assert!(
            gpu.seen.lock().unwrap().len() > before,
            "ICC path did not execute"
        );
        close(&a, &b, 0.0003);
    }
    {
        use schist_codec_raw::demosaic::{demosaic, Quality};
        use schist_codec_raw::develop::{develop, DevelopOptions};
        use schist_codec_raw::{Cfa, CfaColor, Format, Orientation, RawData, RawImage, Rect};
        // Exceed the RAW band size and compare the seam as well as the short final band.
        let (w, h) = (2048, 2050);
        let input: Vec<f32> = (0..w * h).map(|i| (i * 37 % 257) as f32 / 256.0).collect();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = demosaic(&input, w, h, &Cfa::RGGB, Quality::Fast).unwrap();
        schist_fx::set_backend(gpu.clone());
        let actual = demosaic(&input, w, h, &Cfa::RGGB, Quality::Fast).unwrap();
        close(&actual, &expected, 0.00002);
        for cfa in [
            Cfa::RGGB,
            Cfa::BGGR,
            Cfa::Pattern {
                width: 3,
                height: 2,
                colors: vec![
                    CfaColor::Red,
                    CfaColor::Green,
                    CfaColor::Blue,
                    CfaColor::Green,
                    CfaColor::Blue,
                    CfaColor::Red,
                ],
            },
        ] {
            for quality in [Quality::Fast, Quality::Best] {
                eprintln!("RAW parity: {cfa:?}, {quality:?}");
                let (w, h) = (37, 29);
                let input: Vec<f32> = (0..w * h)
                    .map(|i| ((i * 197 % 1009) as f32 / 1008.0) * 1.7 - 0.02)
                    .collect();
                schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
                let b = demosaic(&input, w, h, &cfa, quality).unwrap();
                schist_fx::set_backend(gpu.clone());
                let a = demosaic(&input, w, h, &cfa, quality).unwrap();
                close(&a, &b, 0.00002);
                let mut raw = RawImage::new(Format::Dng, w, h, 1, RawData::F32(input), cfa.clone());
                raw.white_level = 1.2;
                raw.black_levels = [0.02; 4];
                raw.wb_coeffs = [1.2, 1.0, 0.8, 1.0];
                raw.crop = Rect {
                    x: 3,
                    y: 1,
                    width: 31,
                    height: 23,
                };
                raw.color_matrix = Some([
                    [0.6844, -0.0996, -0.0856],
                    [-0.3876, 1.1761, 0.2396],
                    [-0.0593, 0.1772, 0.6198],
                ]);
                for orient in 1..=8 {
                    raw.orientation = Orientation::from_exif(orient);
                    let options = DevelopOptions {
                        quality,
                        ..Default::default()
                    };
                    schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
                    let b = develop(&raw, &options).unwrap();
                    schist_fx::set_backend(gpu.clone());
                    let a = develop(&raw, &options).unwrap();
                    assert_eq!((a.width, a.height), (b.width, b.height));
                    close(&a.rgb, &b.rgb, 0.0001);
                }
            }
        }
    }
    {
        use schist_vector::{rasterize, FillRule, PathBuilder};
        let mut path = PathBuilder::new();
        path.ellipse(IntRect::new(-12, -8, 80, 64));
        path.rect(IntRect::new(21, 6, 107, 29));
        let path = path.build(0.1);
        let rect = IntRect::new(-18, -15, 115, 79);
        for rule in [FillRule::EvenOdd, FillRule::NonZero] {
            schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
            let b = rasterize(&path, rect, rule);
            schist_fx::set_backend(gpu.clone());
            let a = rasterize(&path, rect, rule);
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1));
        }
    }
    for id in [
        "detail",
        "dejpeg",
        "portrait",
        "waifu2x-art",
        "waifu2x-photo",
    ] {
        let model = schist_neural::get(id).unwrap();
        assert!(model.gpu_program().is_some(), "{id} compiler declined");
        let (w, h) = model.spec.input.dims();
        let input: Vec<f32> = (0..w * h * 3)
            .map(|i| (i * 197 % 1009) as f32 / 1008.0)
            .collect();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let b = model.run_tile(&input).unwrap();
        let before = gpu.seen.lock().unwrap().len();
        schist_fx::set_backend(gpu.clone());
        let a = model.run_tile(&input).unwrap();
        assert!(
            gpu.seen.lock().unwrap().len() > before,
            "{id} did not offload"
        );
        close(&a, &b, 0.0003);
    }
    {
        let model = schist_neural::get("colorize").unwrap();
        assert!(model.gpu_program().is_some());
        let (w, h) = model.spec.input.dims();
        let input: Vec<f32> = (0..w * h * 3).map(|i| (i % 193) as f32 / 192.0).collect();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let b = model.run_scores(&input).unwrap();
        schist_fx::set_backend(gpu.clone());
        let a = model.run_scores(&input).unwrap();
        close(&a, &b, 0.0003);
        assert!(schist_neural::get("inpaint")
            .unwrap()
            .gpu_program()
            .is_some());
    }
    {
        let rect = IntRect::new(0, 0, 39, 31);
        let mut tiles = TileMap::new();
        let tile = tiles.get_mut_or_insert(TileCoord { tx: 0, ty: 0 }, Depth::ThirtyTwo);
        for y in 0..31 {
            for x in 0..39 {
                tile.set(
                    y * 256 + x,
                    Rgba::new(x as f32 / 39.0, y as f32 / 31.0, 0.4, 0.8),
                );
            }
        }
        let hole: Vec<bool> = (0..39 * 31)
            .map(|i| (13..25).contains(&(i % 39)) && (9..21).contains(&(i / 39)))
            .collect();
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let b = schist_tools_retouch::fill::inpaint(&tiles, rect, &hole);
        schist_fx::set_backend(gpu.clone());
        let a = schist_tools_retouch::fill::inpaint(&tiles, rect, &hole);
        let flatten = |v: Vec<Rgba>| {
            v.into_iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect::<Vec<_>>()
        };
        close(&flatten(a), &flatten(b), 0.0002);
    }
    let seen = gpu.seen.lock().unwrap();
    for name in [
        "alpha-box",
        "alpha-offset",
        "signed-distance",
        "mask-morph",
        "affine-resample",
        "vector-coverage",
        "neural-tensor",
        "healing-diffusion",
        "healing-patch-score",
    ] {
        assert!(seen.contains(&name), "missing {name}");
    }
}

#[test]
fn programs_reject_forward_references_and_oversized_allocations() {
    use schist_fx::{ComputeProgram, ComputeSource};
    let ctx = GpuContext::new().unwrap();
    let mut p = ComputeProgram::single(
        &schist_fx::plane::ALPHA_OFFSET,
        vec![0.0, 0.0],
        4,
        [2, 2, 1],
        4,
    );
    p.steps[0].source = ComputeSource::Step(0);
    assert!(ctx
        .run_compute(&ComputeJob {
            input: &[1.0; 4],
            program: &p
        })
        .is_none());
    p.steps[0].source = ComputeSource::Input(0);
    p.steps[0].output_len = usize::MAX;
    assert!(ctx
        .run_compute(&ComputeJob {
            input: &[1.0; 4],
            program: &p
        })
        .is_none());
}
