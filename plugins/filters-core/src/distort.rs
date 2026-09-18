//! Filter ▸ Distort. Every one of these is a coordinate remap through
//! [`warp`], so they differ only in the mapping.

use crate::util::{blur_plane, fbm, luma, surface, value_noise, warp, warp_offset};
use crate::{choice, context_filter, param, simple_filter};
use schist_i18n::{choices, t};
use schist_plugin_api::{FilterContext, FilterParam, FilterPlugin, FilterValues};

simple_filter!(
    Twirl,
    "filter.twirl",
    t("filter.twirl.name"),
    t("filter.category.distort"),
    [param(
        "angle",
        t("common.angle"),
        -999.0,
        999.0,
        50.0,
        "\u{b0}"
    )],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let angle = v.get("angle").to_radians();
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius_squared = cx * cx + cy * cy;

        warp_offset(px, w, h, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let distance_squared = dx * dx + dy * dy;
            if distance_squared >= radius_squared {
                return (0.0, 0.0);
            }
            // Normalize before taking the square root: independently rounded
            // lengths amplify coordinate errors at large rotation angles.
            let t = angle * (1.0 - (distance_squared / radius_squared).sqrt()).powi(2);
            let s = t.sin();
            let half_sine = (t * 0.5).sin();
            let c = -2.0 * half_sine * half_sine;
            (dx * c - dy * s, dx * s + dy * c)
        });
    }
);

simple_filter!(
    Ripple,
    "filter.ripple",
    t("filter.ripple.name"),
    t("filter.category.distort"),
    [
        param("amount", t("common.amount"), -999.0, 999.0, 100.0, ""),
        param("size", t("common.size"), 1.0, 64.0, 12.0, " px")
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let amount = v.get("amount") / 100.0;
        let size = v.get("size").max(1.0);
        // Ripple is separable: one horizontal displacement per row and
        // one vertical displacement per column. Prepare sin only once,
        // using the same values on the CPU and every GPU backend.
        let offsets: Vec<f32> = (0..h)
            .chain(0..w)
            .map(|i| ((i as f32 + 0.5) / size).sin() * amount * size * 0.25)
            .collect();
        if crate::gpu::apply(
            px,
            w,
            h,
            &crate::gpu::RIPPLE,
            &offsets,
            Some((amount.abs() * size * 0.25).ceil() as usize + 1),
            16,
        ) {
            return;
        }
        warp_offset(px, w, h, |x, y| {
            (offsets[y as usize], offsets[h + x as usize])
        });
    }
);

/// The three waveforms Wave can add up.
static WAVE_TYPES: &[&str] = &[
    "filter.wave.choice.sine",
    "filter.wave.choice.triangle",
    "filter.wave.choice.square",
];

simple_filter!(
    Wave,
    "filter.wave",
    t("filter.wave.name"),
    t("filter.category.distort"),
    [
        param(
            "generators",
            t("filter.wave.param.generators"),
            1.0,
            8.0,
            1.0,
            ""
        ),
        param(
            "wavelength",
            t("filter.wave.param.wavelength"),
            1.0,
            400.0,
            60.0,
            " px"
        ),
        param(
            "amplitude",
            t("filter.wave.param.amplitude"),
            0.0,
            200.0,
            15.0,
            " px"
        ),
        param(
            "horizontal",
            t("filter.param.horizontal_scale"),
            0.0,
            100.0,
            100.0,
            "%"
        ),
        param(
            "vertical",
            t("filter.param.vertical_scale"),
            0.0,
            100.0,
            100.0,
            "%"
        ),
        choice("type", t("common.type"), choices!(WAVE_TYPES), 0),
        param("seed", t("filter.param.randomness"), 0.0, 999.0, 1.0, "")
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        // Photoshop's Wave is several wave generators added together,
        // each with its own wavelength and phase inside the ranges the
        // dialog gives -- which is why one generator looks like a ripple
        // and five look like water.
        let generators = v.get("generators").round().max(1.0) as usize;
        let len = v.get("wavelength").max(1.0);
        let amp = v.get("amplitude");
        let hscale = v.get("horizontal") / 100.0;
        let vscale = v.get("vertical") / 100.0;
        let kind = (v.get("type").round().max(0.0) as usize).min(2);
        let seed = v.get("seed") as u32;
        let mut waves = Vec::with_capacity(generators);
        for g in 0..generators {
            let jitter = 0.5 + value_noise(g as f32 * 13.0, 0.0, seed);
            let k = std::f32::consts::TAU / (len * jitter);
            let phase = value_noise(0.0, g as f32 * 7.0, seed) * std::f32::consts::TAU;
            waves.push((k, phase));
        }
        // Square waves displace by a constant either way, which is what
        // gives Wave its torn-paper look; triangles ramp between.
        let shape = move |phase: f32| -> f32 {
            let t = phase.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
            match kind {
                1 => 4.0 * (t - 0.5).abs() - 1.0,
                2 => {
                    if t < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                _ => phase.sin(),
            }
        };
        // These are also separable. Sharing the final offsets avoids
        // backend-dependent trig, remainder and multiply-add rounding.
        let offsets: Vec<f32> = (0..h)
            .map(|i| (i, hscale))
            .chain((0..w).map(|i| (i, vscale)))
            .map(|(i, scale)| {
                let pos = i as f32 + 0.5;
                let mut offset = 0.0;
                for &(k, phase) in &waves {
                    offset += shape(pos * k + phase) * amp / generators as f32;
                }
                offset * scale
            })
            .collect();
        if crate::gpu::apply(
            px,
            w,
            h,
            &crate::gpu::WAVE,
            &offsets,
            Some((amp * vscale).abs().ceil() as usize + 2),
            16,
        ) {
            return;
        }
        warp_offset(px, w, h, |x, y| {
            (offsets[y as usize], offsets[h + x as usize])
        });
    }
);

/// Photoshop's three ZigZags, which are three directions to push in.
static ZIGZAG_STYLES: &[&str] = &[
    "filter.zigzag.choice.around_center",
    "filter.zigzag.choice.out_from_center",
    "filter.zigzag.choice.pond_ripples",
];

/// Spherize and Pinch: a sphere, or a cylinder either way.
static AXIS_MODES: &[&str] = &[
    "filter.choice.axis.normal",
    "filter.choice.axis.horizontal_only",
    "filter.choice.axis.vertical_only",
];

simple_filter!(
    ZigZag,
    "filter.zigzag",
    t("filter.zigzag.name"),
    t("filter.category.distort"),
    [
        param("amount", t("common.amount"), -100.0, 100.0, 30.0, ""),
        param(
            "ridges",
            t("filter.zigzag.param.ridges"),
            1.0,
            20.0,
            5.0,
            ""
        ),
        choice("style", t("common.style"), choices!(ZIGZAG_STYLES), 2)
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let amount = v.get("amount") / 100.0;
        let ridges = v.get("ridges").max(1.0);
        let style = (v.get("style").round().max(0.0) as usize).min(2);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = cx.hypot(cy).max(1.0);
        warp(px, w, h, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let d = dx.hypot(dy);
            if d < 1e-3 {
                return (x, y);
            }
            let phase = d / radius * ridges * std::f32::consts::TAU;
            // The three styles differ in *which way* the ripple pushes.
            match style {
                // Around Center: tangentially, so the picture twists back
                // and forth as the rings go out.
                0 => {
                    let twist = phase.sin() * amount * 0.6 * (1.0 - d / radius).max(0.0);
                    let (s, c) = twist.sin_cos();
                    (cx + dx * c - dy * s, cy + dx * s + dy * c)
                }
                // Out From Center: radially, and always outwards, which
                // reads as a starburst rather than as water.
                1 => {
                    let push = phase.sin().abs() * amount * radius * 0.1;
                    (x + dx / d * push, y + dy / d * push)
                }
                // Pond Ripples: radially, signed, and fading out.
                _ => {
                    let push = phase.sin() * amount * radius * 0.1 * (1.0 - d / radius).max(0.0);
                    (x + dx / d * push, y + dy / d * push)
                }
            }
        });
    }
);

fn spherize_scale(dx: f32, dy: f32, radius: f32, amount: f32) -> Option<f32> {
    let squared = dx * dx + dy * dy;
    let radius_squared = radius * radius;
    if squared >= radius_squared || squared < 1e-6 {
        return None;
    }
    let distance = squared.sqrt();
    // asin(distance / radius) amplifies the rounding of a ratio near one.
    // Keep the hemisphere height in pixel units until after the subtraction;
    // the squared half-pixel coordinates retain precision at the rim.
    let height = (radius_squared - squared).sqrt();
    let bulged = (distance.atan2(height) / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
    Some(1.0 + (bulged / (distance / radius) - 1.0) * amount)
}

simple_filter!(
    Spherize,
    "filter.spherize",
    t("filter.spherize.name"),
    t("filter.category.distort"),
    [
        param("amount", t("common.amount"), -100.0, 100.0, 50.0, "%"),
        choice("mode", t("common.mode"), choices!(AXIS_MODES), 0)
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let amount = v.get("amount") / 100.0;
        // The two axis modes bulge a cylinder rather than a sphere, which
        // is what you want for wrapping a label round a bottle.
        let mode = (v.get("mode").round().max(0.0) as usize).min(2);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = cx.min(cy).max(1.0);
        warp(px, w, h, |x, y| {
            let (dx, dy) = match mode {
                1 => (x - cx, 0.0),
                2 => (0.0, y - cy),
                _ => (x - cx, y - cy),
            };
            let Some(scale) = spherize_scale(dx, dy, radius, amount) else {
                return (x, y);
            };
            (cx + dx * scale, cy + dy * scale)
        });
    }
);

simple_filter!(
    Pinch,
    "filter.pinch",
    t("filter.pinch.name"),
    t("filter.category.distort"),
    [
        param("amount", t("common.amount"), -100.0, 100.0, 50.0, "%"),
        choice("mode", t("common.mode"), choices!(AXIS_MODES), 0)
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let amount = v.get("amount") / 100.0;
        // The two axis modes bulge a cylinder rather than a sphere, which
        // is what you want for wrapping a label round a bottle.
        let mode = (v.get("mode").round().max(0.0) as usize).min(2);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = cx.min(cy).max(1.0);
        warp(px, w, h, |x, y| {
            let (dx, dy) = match mode {
                1 => (x - cx, 0.0),
                2 => (0.0, y - cy),
                _ => (x - cx, y - cy),
            };
            let d = dx.hypot(dy);
            if d >= radius || d < 1e-3 {
                return (x, y);
            }
            let t = d / radius;
            let scale = t.powf(1.0 + amount) / t;
            (cx + dx * scale, cy + dy * scale)
        });
    }
);

/// Which way Polar Coordinates converts.
static POLAR_DIRECTIONS: &[&str] = &[
    "filter.polar.choice.polar_to_rectangular",
    "filter.polar.choice.rectangular_to_polar",
];

simple_filter!(
    PolarCoordinates,
    "filter.polar",
    t("filter.polar.name"),
    t("filter.category.distort"),
    [choice(
        "to_polar",
        t("filter.polar.param.to_polar"),
        choices!(POLAR_DIRECTIONS),
        1
    )],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        let to_polar = v.get("to_polar") >= 0.5;
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = cx.hypot(cy).max(1.0);
        warp(px, w, h, move |x, y| {
            if to_polar {
                // Destination is polar: angle across, radius down.
                let (dx, dy) = (x - cx, y - cy);
                let theta = dy.atan2(dx) + std::f32::consts::PI;
                let r = dx.hypot(dy);
                (
                    theta / std::f32::consts::TAU * w as f32,
                    r / radius * h as f32,
                )
            } else {
                let theta = x / w as f32 * std::f32::consts::TAU - std::f32::consts::PI;
                let r = y / h as f32 * radius;
                (cx + r * theta.cos(), cy + r * theta.sin())
            }
        });
    }
);

/// The shapes anybody actually drags Shear's curve into.
static SHEAR_CURVES: &[&str] = &[
    "filter.shear.choice.bow",
    "filter.shear.choice.s_curve",
    "filter.shear.choice.ramp",
];

/// What happens where a remap sends a pixel off the edge: Shear and
/// Displace offer the same two answers.
static EDGE_UNDEFINED: &[&str] = &[
    "filter.choice.undefined.repeat_edge_pixels",
    "filter.choice.undefined.wrap_around",
];

simple_filter!(
    Shear,
    "filter.shear",
    t("filter.shear.name"),
    t("filter.category.distort"),
    [
        param("amount", t("common.amount"), -200.0, 200.0, 40.0, " px"),
        choice(
            "curve",
            t("filter.shear.param.curve"),
            choices!(SHEAR_CURVES),
            0
        ),
        choice(
            "undefined",
            t("filter.param.undefined_areas"),
            choices!(EDGE_UNDEFINED),
            0
        )
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        // Photoshop draws the shear as a curve you drag; the three shapes
        // here are the ones anybody actually drags it into.
        let amount = v.get("amount");
        let curve = (v.get("curve").round().max(0.0) as usize).min(2);
        let wrap = v.get("undefined") >= 0.5;
        let hh = h as f32;
        let ww = w as f32;
        warp(px, w, h, move |x, y| {
            let t = y / hh;
            let shift = match curve {
                1 => (t * std::f32::consts::TAU).sin(),
                2 => t * 2.0 - 1.0,
                _ => (t * std::f32::consts::PI).sin(),
            } * amount;
            let sx = x + shift;
            // What happens at the sides: repeat the edge, which `warp`
            // does by clamping, or bring the other side round.
            (if wrap { sx.rem_euclid(ww) } else { sx }, y)
        });
    }
);

/// How a map that is not the layer's size gets used.
static MAP_FIT: &[&str] = &[
    "filter.displace.choice.stretch_to_fit",
    "filter.displace.choice.tile",
];

/// Filter ▸ Distort ▸ Displace.
///
/// Photoshop reads the displacement out of a file you pick: the red
/// channel moves each pixel horizontally, the green channel vertically,
/// with mid grey meaning "stay". That is exactly what this does when it
/// is given a map -- the dialog has a Choose button for it -- and when
/// it is not, it falls back to a noise field of its own, which is what
/// the filter is most often used for anyway.
pub struct Displace;

impl FilterPlugin for Displace {
    fn gpu_operation(&self, values: &FilterValues) -> Option<schist_fx::FilterOperation> {
        self.gpu_operation_with(values, &FilterContext::default())
    }
    fn gpu_operation_with(
        &self,
        values: &FilterValues,
        context: &FilterContext<'_>,
    ) -> Option<schist_fx::FilterOperation> {
        crate::gpu_programs::displace(values, context)
    }

    fn id(&self) -> &'static str {
        "filter.displace"
    }
    fn name(&self) -> &'static str {
        t("filter.displace.name")
    }
    fn category(&self) -> &'static str {
        t("filter.category.distort")
    }
    fn params(&self) -> Vec<FilterParam> {
        vec![
            param(
                "scale",
                t("filter.param.horizontal_scale"),
                0.0,
                200.0,
                20.0,
                " px",
            ),
            param(
                "vscale",
                t("filter.param.vertical_scale"),
                0.0,
                200.0,
                20.0,
                " px",
            ),
            param("detail", t("filter.param.detail"), 1.0, 64.0, 16.0, " px"),
            param("seed", t("filter.param.randomness"), 0.0, 999.0, 1.0, ""),
            choice("fit", t("filter.displace.param.fit"), choices!(MAP_FIT), 0),
            choice(
                "undefined",
                t("filter.param.undefined_areas"),
                choices!(EDGE_UNDEFINED),
                0,
            ),
        ]
    }

    fn wants_map(&self) -> Option<&'static str> {
        Some(t("filter.displace.map"))
    }

    fn info(&self) -> Option<String> {
        Some(t("filter.displace.msg.info").to_string())
    }

    fn apply(&self, px: &mut [f32], width: usize, height: usize, values: &FilterValues) {
        self.apply_with(px, width, height, values, &FilterContext::default());
    }

    fn apply_with(
        &self,
        px: &mut [f32],
        width: usize,
        height: usize,
        values: &FilterValues,
        context: &FilterContext,
    ) {
        if self
            .gpu_operation_with(values, context)
            .is_some_and(|op| op.apply(px, width, height))
        {
            return;
        }
        let scale = values.get("scale");
        let vscale = values.get("vscale");
        let detail = values.get("detail").max(1.0);
        let seed = values.get("seed") as u32;
        let tile = values.get("fit") >= 0.5;
        let wrap = values.get("undefined") >= 0.5;
        let (ww, hh) = (width as f32, height as f32);
        let map = context.map;
        warp(px, width, height, move |x, y| {
            let (u, v) = match map {
                // Photoshop's convention, and every displacement map
                // ever drawn for it: red is horizontal, green is
                // vertical, and mid grey is no movement at all.
                Some(map) => {
                    let p = if tile {
                        map.tiled(x, y)
                    } else {
                        map.stretched(x / ww, y / hh)
                    };
                    (p[0] - 0.5, p[1] - 0.5)
                }
                None => (
                    fbm(x / detail, y / detail, 11 + seed, 3) - 0.5,
                    fbm(x / detail + 37.0, y / detail - 19.0, 23 + seed, 3) - 0.5,
                ),
            };
            let (sx, sy) = (x + u * scale * 2.0, y + v * vscale * 2.0);
            if wrap {
                (sx.rem_euclid(ww), sy.rem_euclid(hh))
            } else {
                (sx, sy)
            }
        });
    }
}

context_filter!(
    DiffuseGlow,
    "filter.diffuse_glow",
    t("filter.diffuse_glow.name"),
    t("filter.category.distort"),
    [
        param(
            "graininess",
            t("filter.param.graininess"),
            0.0,
            10.0,
            6.0,
            ""
        ),
        param(
            "glow",
            t("filter.diffuse_glow.param.glow"),
            0.0,
            20.0,
            10.0,
            ""
        ),
        param(
            "clear",
            t("filter.diffuse_glow.param.clear"),
            0.0,
            20.0,
            15.0,
            ""
        )
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues, ctx: &FilterContext| {
        // Light bleeding out of the highlights through a grainy diffusion
        // filter, which is what a stocking over the lens does. Clear
        // Amount is the threshold: below it nothing glows, which is what
        // keeps the shadows from fogging. The glow is the background
        // colour, as Photoshop's is -- white by default, which is why
        // nobody notices until they change it.
        let graininess = v.get("graininess") / 10.0;
        let glow = v.get("glow") / 20.0;
        let clear = v.get("clear") / 20.0;
        let mut bright: Vec<f32> = px
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (luma(p) - clear).max(0.0) / (1.0 - clear).max(1e-3))
            .collect();
        blur_plane(&mut bright, w, h, 4.0 + glow * 12.0);
        for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let grain = (value_noise(x, y, 5237) - 0.5) * graininess * 0.35;
            let lift = (bright[i] * (0.6 + glow * 2.0) + grain).clamp(0.0, 1.0);
            let colour = ctx.bg();
            for (c, v) in p.iter_mut().take(3).enumerate() {
                // Screened towards the glow colour rather than added, so
                // the light saturates the way light does instead of
                // clipping.
                *v = (colour[c] - (colour[c] - *v) * (1.0 - lift)).clamp(0.0, 1.0);
            }
        }
    }
);

simple_filter!(
    Glass,
    "filter.glass",
    t("filter.glass.name"),
    t("filter.category.distort"),
    [
        param(
            "distortion",
            t("filter.glass.param.distortion"),
            0.0,
            20.0,
            5.0,
            ""
        ),
        param(
            "smoothness",
            t("filter.param.smoothness"),
            1.0,
            15.0,
            3.0,
            ""
        ),
        choice(
            "texture",
            t("filter.param.texture"),
            choices!(GLASS_TEXTURES),
            0
        ),
        param(
            "scaling",
            t("filter.param.scaling"),
            50.0,
            200.0,
            100.0,
            "%"
        )
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        // Seen through a sheet of textured glass: the texture is a height
        // field, and every pixel is fetched from wherever that height
        // field's slope bends the light. Smoothness is the polish on the
        // glass.
        let distortion = v.get("distortion");
        let smoothness = v.get("smoothness");
        let kind = v.get("texture").round().max(0.0) as u32;
        let scaling = (v.get("scaling") / 100.0 * 10.0).max(1.0);
        let height = |x: f32, y: f32| -> f32 {
            match kind {
                // Frosted: fine noise, softened by Smoothness.
                0 => fbm(
                    x / (smoothness * 2.0).max(1.0),
                    y / (smoothness * 2.0).max(1.0),
                    337,
                    3,
                ),
                // Blocks: a grid of panes.
                1 => {
                    let (u, vv) = ((x / scaling).fract(), (y / scaling).fract());
                    ((u - 0.5).abs() + (vv - 0.5).abs()) * 0.7
                }
                // Canvas and tiny lens use the shared surface generator.
                _ => surface(kind - 2, x, y, scaling, 149),
            }
        };
        warp(px, w, h, |x, y| {
            let gx = height(x + 1.0, y) - height(x - 1.0, y);
            let gy = height(x, y + 1.0) - height(x, y - 1.0);
            (x + gx * distortion * 1.2, y + gy * distortion * 1.2)
        });
    }
);

/// Glass has its own texture list: the first two are its own, the rest
/// are the surfaces the Texture group uses.
static GLASS_TEXTURES: &[&str] = &[
    "filter.glass.choice.frosted",
    "filter.glass.choice.blocks",
    "filter.choice.surface.canvas",
    "filter.choice.surface.sandstone",
    "filter.choice.surface.burlap",
    "filter.choice.surface.brick",
];

simple_filter!(
    OceanRipple,
    "filter.ocean_ripple",
    t("filter.ocean_ripple.name"),
    t("filter.category.distort"),
    [
        param(
            "size",
            t("filter.ocean_ripple.param.size"),
            1.0,
            15.0,
            9.0,
            ""
        ),
        param(
            "magnitude",
            t("filter.ocean_ripple.param.magnitude"),
            0.0,
            20.0,
            9.0,
            ""
        )
    ],
    |px: &mut [f32], w: usize, h: usize, v: &FilterValues| {
        // Randomly spaced ripples, as though the image were under moving
        // water. Distort ▸ Ripple is regular and periodic; this one takes
        // its offsets from a noise field, which is why it looks wet
        // rather than corrugated.
        let size = v.get("size").max(1.0) * 6.0;
        let magnitude = v.get("magnitude");
        warp(px, w, h, |x, y| {
            let a = fbm(x / size, y / size, 1049, 2) - 0.5;
            let b = fbm(x / size + 31.0, y / size + 17.0, 1049, 2) - 0.5;
            (x + a * magnitude * 2.0, y + b * magnitude * 2.0)
        });
    }
);

pub fn register(registry: &mut schist_plugin_api::PluginRegistry) {
    registry.register_filter(Box::new(Twirl));
    registry.register_filter(Box::new(Ripple));
    registry.register_filter(Box::new(Wave));
    registry.register_filter(Box::new(ZigZag));
    registry.register_filter(Box::new(Spherize));
    registry.register_filter(Box::new(Pinch));
    registry.register_filter(Box::new(PolarCoordinates));
    registry.register_filter(Box::new(Shear));
    registry.register_filter(Box::new(Displace));
    registry.register_filter(Box::new(DiffuseGlow));
    registry.register_filter(Box::new(Glass));
    registry.register_filter(Box::new(OceanRipple));
}

#[cfg(test)]
mod tests {
    use super::spherize_scale;

    #[test]
    fn spherize_keeps_subpixel_precision_near_the_rim() {
        // The first point exposed CPU/Metal drift on a 449x446 image.
        // Compare against f64 geometry, independently of either f32 backend.
        let radius = 223.0;
        for (dx, dy) in [
            (-185.0, -124.5),
            (185.0, 124.5),
            (222.5, 0.0),
            (0.0, -222.5),
        ] {
            for amount in [-1.0, 0.5, 1.0] {
                let scale = spherize_scale(dx, dy, radius, amount).unwrap();
                let t = (dx as f64).hypot(dy as f64) / radius as f64;
                let reference =
                    1.0 + (t.asin() / std::f64::consts::FRAC_PI_2 / t - 1.0) * amount as f64;
                for (centre, delta) in [(224.5, dx), (223.0, dy)] {
                    let actual = (centre + delta * scale) as f64;
                    let expected = centre as f64 + delta as f64 * reference;
                    assert!(
                        (actual - expected).abs() < 0.00004,
                        "({dx}, {dy}), amount {amount}: {actual} != {expected}"
                    );
                }
            }
        }
        assert!(spherize_scale(0.0, 0.0, radius, 1.0).is_none());
        assert!(spherize_scale(radius, 0.0, radius, 1.0).is_none());
    }
}
