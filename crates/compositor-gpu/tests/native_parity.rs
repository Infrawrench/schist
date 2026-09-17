//! Native results must come from a real dispatch: a CPU fallback cannot
//! satisfy these tests. Shader validation still runs without an adapter.
use schist_adjustments::{Levels, Params};
use schist_color::{ColorMode, Depth, NativePixel};
use schist_colormgmt::{native_to_rgba, NativeColorTransform};
use schist_compositor::{composite_native_tile_cpu, Compositor, CpuCompositor};
use schist_compositor_gpu::{plan, BatchOut, GpuCompositor};
use schist_core::{
    AdjustmentData, AdjustmentKind, BlendMode, Document, IntRect, Layer, LayerKind, LayerMask,
    StyledRaster, TileCoord, TILE_PIXELS, TILE_SIZE,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn both_compositor_shaders_validate() {
    for source in [
        concat!(
            include_str!("../../pixel-ops/src/blend.wgsl"),
            include_str!("../src/composite_common.wgsl"),
            include_str!("../../adjustments/src/gpu.wgsl"),
            include_str!("../src/composite.wgsl")
        ),
        concat!(
            include_str!("../../pixel-ops/src/blend.wgsl"),
            include_str!("../src/composite_common.wgsl"),
            include_str!("../../adjustments/src/gpu.wgsl"),
            include_str!("../src/composite_native.wgsl")
        ),
    ] {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}

fn doc(mode: ColorMode, depth: Depth) -> Document {
    let mut doc = Document::new("native GPU", 270, 24, depth);
    doc.mode = mode;
    doc.tree.layers.clear();
    doc
}

fn layer(mode: ColorMode, depth: Depth, seed: usize) -> Layer {
    let mut l = Layer::new_raster("samples");
    let tiles = &mut l.as_raster_mut().unwrap().tiles;
    // Straddles negative and positive tile boundaries. Samples include
    // independent K, partial/zero alpha, and out-of-gamut Lab colours.
    for y in 0..7 {
        for x in -3..263 {
            let i = (x + 3 + y * 269) as usize + seed * 31;
            let v = |c: usize| ((i * (13 + c * 6) + c * 19) % 239) as f32 / 238.0;
            let p = NativePixel {
                mode,
                color: [
                    v(0),
                    v(1),
                    v(2),
                    if mode == ColorMode::Cmyk { v(3) } else { 0.0 },
                ],
                alpha: [0.0, 1.0, 0.27, 0.63, 0.89][i % 5],
            };
            tiles
                .get_mut_or_insert_mode(TileCoord::containing(x, y), depth, mode)
                .set_native_pixel((y * TILE_SIZE + x.rem_euclid(TILE_SIZE)) as usize, p);
        }
    }
    l
}

fn mask() -> LayerMask {
    let mut m = LayerMask::new_revealing();
    m.bounds = IntRect::from_xywh(2, 1, 263, 9);
    m.default_value = 173;
    let tile = m.tiles.get_mut_or_insert(TileCoord::containing(0, 0));
    for y in 1..10 {
        for x in 2..256 {
            tile[y * 256 + x] = ((x * 31 + y * 13) % 256) as u8;
        }
    }
    // The part in tile x=1 is deliberately sparse.
    m
}

fn adjustment(kind: AdjustmentKind, params: Params) -> Layer {
    let mut l = Layer::new_raster("adjustment");
    l.kind = LayerKind::Adjustment(AdjustmentData {
        kind,
        raw: Vec::new(),
        params_json: Some(serde_json::to_string(&params).unwrap()),
    });
    l
}

fn close(gpu: &[NativePixel], cpu: &[NativePixel], label: &str) {
    assert_eq!(gpu.len(), cpu.len(), "{label}");
    for (i, (g, c)) in gpu.iter().zip(cpu).enumerate() {
        assert_eq!(g.mode, c.mode);
        for (j, (g, c)) in g
            .color
            .iter()
            .chain([&g.alpha])
            .zip(c.color.iter().chain([&c.alpha]))
            .enumerate()
        {
            assert!(
                g.is_finite() && (g - c).abs() <= 8e-5,
                "{label} pixel {i} sample {j}: GPU={g}, CPU={c}"
            );
        }
    }
}

fn close_display(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}");
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert!((a - b).abs() < 2e-5, "{label} sample {i}: {a} vs {b}");
    }
}

fn equal_display_bytes(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}");
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, b, "{label} sample {i}");
    }
}

fn dispatched(gpu: &GpuCompositor, doc: &Document, coords: &[TileCoord]) -> Vec<Vec<NativePixel>> {
    let p = plan::build(doc).expect("native plan must compile");
    match gpu
        .context()
        .composite_batch(&p, coords, false)
        .expect("native GPU dispatch must succeed")
    {
        BatchOut::Native(tiles) => tiles,
        _ => panic!("native plan lost its native samples"),
    }
}

fn parity(gpu: &GpuCompositor, doc: &Document, coords: &[TileCoord], label: &str) {
    let actual = dispatched(gpu, doc, coords);
    assert_eq!(actual.len(), coords.len());
    for (tile, &coord) in actual.iter().zip(coords) {
        close(tile, &composite_native_tile_cpu(doc, coord), label);
    }
}

struct Tracking {
    gpu: Arc<GpuCompositor>,
    batches: AtomicUsize,
}
impl Compositor for Tracking {
    fn name(&self) -> &'static str {
        "native GPU test"
    }
    fn tile(&self, doc: &Document, coord: TileCoord) -> Vec<f32> {
        self.gpu.tile(doc, coord)
    }
    fn native_tile(&self, doc: &Document, coord: TileCoord) -> Vec<NativePixel> {
        self.native_tiles(doc, &[coord]).pop().unwrap()
    }
    fn native_tiles(&self, doc: &Document, coords: &[TileCoord]) -> Vec<Vec<NativePixel>> {
        self.batches.fetch_add(1, Ordering::Relaxed);
        dispatched(&self.gpu, doc, coords)
    }
}
struct Restore(Arc<dyn Compositor>);
impl Drop for Restore {
    fn drop(&mut self) {
        schist_compositor::set_backend(self.0.clone());
    }
}

#[test]
fn native_gpu_channels_blends_groups_boundaries_and_dispatch() {
    let gpu = match GpuCompositor::new() {
        Ok(gpu) => Arc::new(gpu),
        Err(e) => {
            assert!(!e.starts_with("pipeline creation:"), "{e}");
            assert!(
                std::env::var("SCHIST_REQUIRE_GPU").as_deref() != Ok("1"),
                "{e}"
            );
            eprintln!("skipping native GPU dispatch: {e}");
            return;
        }
    };
    eprintln!("native parity adapter: {}", gpu.describe());
    let coords = [
        TileCoord::containing(-1, 0),
        TileCoord::containing(0, 0),
        TileCoord::containing(256, 0),
        TileCoord::containing(0, 256),
    ];
    let one = &coords[1..2];
    parity(
        &gpu,
        &doc(ColorMode::Cmyk, Depth::Eight),
        one,
        "empty document",
    );
    assert!(dispatched(&gpu, &doc(ColorMode::Lab, Depth::Eight), &[]).is_empty());
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
            let mut d = doc(mode, depth);
            d.push_layer(layer(mode, depth, 1));
            let mut top = layer(mode, depth, 2);
            top.opacity = 0.73;
            top.fill_opacity = 0.61;
            top.mask = Some(mask());
            d.push_layer(top);
            parity(
                &gpu,
                &d,
                &coords,
                &format!("{mode:?} {depth:?} masks and opacity"),
            );
        }
        for &blend in BlendMode::layer_modes() {
            let mut d = doc(mode, Depth::ThirtyTwo);
            d.push_layer(layer(mode, d.depth, 3));
            let mut top = layer(mode, d.depth, 4);
            top.blend = blend;
            top.opacity = 0.83;
            d.push_layer(top);
            parity(&gpu, &d, one, &format!("{mode:?} {blend:?}"));
        }
        let mut d = doc(mode, Depth::Sixteen);
        d.push_layer(layer(mode, d.depth, 5));
        let mut group = Layer::new_group("masked clip base");
        group.blend = BlendMode::Normal;
        group.mask = Some(mask());
        group.opacity = 0.71;
        group.fill_opacity = 0.81;
        if let LayerKind::Group(g) = &mut group.kind {
            g.children.push(layer(mode, d.depth, 6));
        }
        d.push_layer(group);
        let mut clipped = layer(mode, d.depth, 7);
        clipped.clipping = true;
        clipped.mask = Some(mask());
        d.push_layer(clipped);
        let mut adj = adjustment(AdjustmentKind::Invert, Params::Invert);
        adj.clipping = true;
        adj.opacity = 0.41;
        d.push_layer(adj);
        let mut pass = Layer::new_group("pass through");
        pass.blend = BlendMode::PassThrough;
        if let LayerKind::Group(g) = &mut pass.kind {
            g.children.push(layer(mode, d.depth, 8));
        }
        d.push_layer(pass);
        parity(&gpu, &d, &coords, "groups and clipped adjustment");

        // Effects rasters are RGB at their explicit input boundary.
        let mut styled = Layer::new_group("styled RGB group");
        styled.mask = Some(mask());
        styled.styled = Some(Arc::new(StyledRaster {
            tiles: layer(ColorMode::Rgb, Depth::Eight, 9)
                .as_raster()
                .unwrap()
                .tiles
                .clone(),
            bounds: d.canvas_rect(),
            key: 1,
        }));
        d.push_layer(styled);
        parity(&gpu, &d, &coords, "RGB styled source");

        for (kind, params) in [
            (AdjustmentKind::Invert, Params::Invert),
            (AdjustmentKind::Levels, Params::Levels(Levels::default())),
            (
                AdjustmentKind::SolidColor,
                Params::SolidColor {
                    rgba: [0.32, 0.67, 0.19, 1.0],
                },
            ),
            (AdjustmentKind::Threshold, Params::Threshold { level: 0.47 }),
            (AdjustmentKind::Posterize, Params::Posterize { levels: 7 }),
            (
                AdjustmentKind::HueSaturation,
                Params::HueSaturation {
                    hue: 37.0,
                    saturation: 23.0,
                    lightness: -13.0,
                    colorize: false,
                    lightness_desaturates: false,
                    reciprocal_saturation: false,
                    ranges: Vec::new(),
                },
            ),
            (
                AdjustmentKind::BlackWhite,
                Params::BlackWhite {
                    reds: 40.0,
                    yellows: 60.0,
                    greens: 40.0,
                    cyans: 60.0,
                    blues: 20.0,
                    magentas: 80.0,
                },
            ),
        ] {
            let mut d = doc(mode, Depth::ThirtyTwo);
            d.push_layer(layer(mode, d.depth, 10));
            let mut adj = adjustment(kind, params);
            adj.mask = Some(mask());
            adj.opacity = 0.62;
            d.push_layer(adj);
            parity(&gpu, &d, one, &format!("{mode:?} {kind:?}"));
            if kind == AdjustmentKind::Levels {
                let mut before = doc(mode, d.depth);
                before.push_layer(d.tree.layers[0].clone());
                close(
                    &dispatched(&gpu, &d, one)[0],
                    &composite_native_tile_cpu(&before, coords[1]),
                    "identity levels retains original native samples",
                );
            }
            d.tree.layers[1].blend = BlendMode::Color;
            parity(&gpu, &d, one, &format!("{mode:?} {kind:?} color blend"));
        }
    }

    // ICC LUTs quantize their inputs: one ULP of legal CPU/Metal rounding
    // can select adjacent entries (for example around Lab's 0.5 sample).
    // Check native CPU/GPU parity before the transform, then verify every
    // display path against the CMS applied to those exact GPU samples.
    // Neither the native nor the display tolerance needs to be relaxed.
    for mode in [ColorMode::Cmyk, ColorMode::Lab] {
        let mut d = doc(mode, Depth::ThirtyTwo);
        d.push_layer(layer(mode, d.depth, 11));
        let mut profile = moxcms::ColorProfile::new_lab();
        profile.pcs = moxcms::DataColorSpace::Lab;
        for (profile_index, icc) in [None, Some(vec![0; 128]), Some(profile.encode().unwrap())]
            .into_iter()
            .enumerate()
        {
            d.icc_profile = icc;
            let label = format!("display {mode:?} profile {profile_index}");
            let transform = NativeColorTransform::new(mode, d.icc_profile.as_deref()).ok();
            assert_eq!(
                transform.is_some(),
                mode == ColorMode::Lab && profile_index == 2
            );
            let native = dispatched(&gpu, &d, &coords);
            assert_eq!(native.len(), coords.len());
            let expected: Vec<_> = native
                .iter()
                .zip(&coords)
                .map(|(pixels, &coord)| {
                    let cpu = composite_native_tile_cpu(&d, coord);
                    close(pixels, &cpu, &label);
                    let rgba = native_to_rgba(pixels, transform.as_ref());
                    if transform.is_none() {
                        // The fallback conversions are continuous, so their
                        // final RGB values can still be compared directly.
                        close_display(&rgba, &CpuCompositor.tile(&d, coord), &label);
                    }
                    rgba
                })
                .collect();
            close_display(&gpu.tile(&d, coords[1]), &expected[1], &label);

            let rect = IntRect::from_xywh(-3, 1, 267, 5);
            let mut region = Vec::new();
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let t = coords
                        .iter()
                        .position(|&c| c == TileCoord::containing(x, y))
                        .unwrap();
                    let i = (y.rem_euclid(TILE_SIZE) * TILE_SIZE + x.rem_euclid(TILE_SIZE))
                        as usize
                        * 4;
                    region.extend_from_slice(&expected[t][i..i + 4]);
                }
            }
            close_display(&gpu.region_f32(&d, rect), &region, &label);
            let bytes: Vec<_> = region.into_iter().map(schist_color::f32_to_u8).collect();
            equal_display_bytes(
                &gpu.region_rgba8(&d, rect),
                &bytes,
                &format!("{label} region RGBA8"),
            );
            let bytes: Vec<Vec<_>> = expected
                .iter()
                .map(|tile| tile.iter().copied().map(schist_color::f32_to_u8).collect())
                .collect();
            let actual = gpu.tiles_rgba8(&d, &coords);
            assert_eq!(actual.len(), bytes.len(), "{label} tile count");
            for (i, (a, b)) in actual.iter().zip(&bytes).enumerate() {
                equal_display_bytes(a, b, &format!("{label} tile {i} RGBA8"));
            }
        }
    }

    // Same displayed RGB, different separations: both must survive GPU
    // readback, public native dispatch, and merging back to editable tiles.
    let mut d = doc(ColorMode::Cmyk, Depth::ThirtyTwo);
    let mut l = Layer::new_raster("distinct inks");
    let tile = l
        .as_raster_mut()
        .unwrap()
        .tiles
        .get_mut_or_insert_mode(coords[1], d.depth, d.mode);
    for (i, color) in [
        [0.0, 0.0, 0.0, 0.5],
        [0.5, 0.5, 0.5, 0.0],
        [1.2, -0.1, 0.3, 0.71],
    ]
    .into_iter()
    .enumerate()
    {
        tile.set_native_pixel(
            i,
            NativePixel {
                mode: d.mode,
                color,
                alpha: 1.0,
            },
        );
    }
    d.push_layer(l);
    let expected = composite_native_tile_cpu(&d, coords[1]);
    let actual = dispatched(&gpu, &d, one).pop().unwrap();
    assert_eq!(&actual[..3], &expected[..3]);
    assert_eq!(actual[0].to_rgba(), actual[1].to_rgba());
    assert_ne!(actual[0].color, actual[1].color);
    let tracked = Arc::new(Tracking {
        gpu: gpu.clone(),
        batches: AtomicUsize::new(0),
    });
    let _restore = Restore(schist_compositor::backend());
    schist_compositor::set_backend(tracked.clone());
    assert_eq!(
        &schist_compositor::composite_native_tile(&d, coords[1])[..3],
        &expected[..3]
    );
    let merged =
        schist_compositor::composite_region_tiles(&d, IntRect::from_xywh(0, 0, 3, 1), false);
    for (i, expected) in expected[..3].iter().enumerate() {
        assert_eq!(merged.native_pixel(i as i32, 0), *expected);
    }
    assert_eq!(tracked.batches.load(Ordering::Relaxed), 2);
    // The explicit CPU reference must not recurse into the active backend.
    CpuCompositor.tile(&d, coords[1]);
    assert_eq!(tracked.batches.load(Ordering::Relaxed), 2);

    let limit = gpu.context().binding_limit();
    gpu.context().set_binding_limit(TILE_PIXELS * 20);
    parity(&gpu, &d, &coords, "one tile per batch");
    gpu.context().set_binding_limit(TILE_PIXELS * 20 - 1);
    assert!(gpu
        .context()
        .composite_batch(&plan::build(&d).unwrap(), one, false)
        .is_none());
    assert_eq!(
        gpu.native_tile(&d, coords[1]),
        expected,
        "oversized tile fallback"
    );
    gpu.context().set_binding_limit(limit);
    parity(&gpu, &d, one, "successful dispatch after decline");
    d.tree.layers[0].render_offset = (1, 0);
    assert!(plan::build(&d).is_ok());
    assert_eq!(
        gpu.native_tile(&d, coords[1]),
        composite_native_tile_cpu(&d, coords[1])
    );
}

#[test]
fn translated_layers_cross_tiles_with_masks_in_every_color_mode() {
    let gpu = GpuCompositor::new().expect("GPU adapter required");
    let coords: Vec<_> = (-1..=1)
        .flat_map(|ty| (-1..=2).map(move |tx| TileCoord { tx, ty }))
        .collect();
    for mode in [ColorMode::Rgb, ColorMode::Cmyk, ColorMode::Lab] {
        for offset in [(1, 1), (-13, -7), (256, 0), (275, 261), (-257, 255)] {
            let mut d = doc(mode, Depth::ThirtyTwo);
            let mut l = layer(mode, d.depth, 2);
            l.render_offset = offset;
            l.mask = Some(mask());
            d.tree.layers.push(l);
            let plan = plan::build(&d).unwrap();
            let result = gpu
                .context()
                .composite_batch(&plan, &coords, false)
                .expect("offset GPU declined");
            match result {
                BatchOut::Native(tiles) => {
                    for (coord, tile) in coords.iter().zip(tiles) {
                        close(
                            &tile,
                            &composite_native_tile_cpu(&d, *coord),
                            "translated native",
                        );
                    }
                }
                BatchOut::F32(tiles) => {
                    for (coord, tile) in coords.iter().zip(tiles) {
                        close_display(&tile, &CpuCompositor.tile(&d, *coord), "translated RGB");
                    }
                }
                _ => panic!("unexpected format"),
            }
        }
    }
}
