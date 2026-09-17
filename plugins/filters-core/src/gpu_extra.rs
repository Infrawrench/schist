//! Resident graphs for the remaining gallery and compound effects.
use schist_fx::{
    ComputeEntry, ComputeProgram, ComputeShader, ComputeSource as Source, FilterOperation,
    ShaderSpec,
};
use schist_plugin_api::{FilterContext, FilterValues};
use std::sync::Arc;

const INPUT: Source = Source::Input(0);
macro_rules! kernel {
    ($name:ident, $file:literal) => {
        static $name: ComputeShader = ComputeShader {
            name: $file,
            source: concat!(
                include_str!("shaders/noise.wgsl"),
                include_str!("shaders/gallery_common.wgsl"),
                include_str!(concat!("shaders/", $file, ".wgsl"))
            ),
            entry: ComputeEntry::Rgba,
        };
    };
}
kernel!(PLANE, "gallery_plane");
kernel!(ARTISTIC, "artistic");
kernel!(BRUSH, "brush");
kernel!(OTHER, "other");
kernel!(CAMERA, "camera_raw");

pub(super) struct Graph {
    pub p: ComputeProgram,
    pub w: usize,
    pub h: usize,
}
impl Graph {
    pub fn new(w: usize, h: usize) -> Option<Self> {
        let n = w.checked_mul(h)?.checked_mul(4)?;
        if n == 0 || n > u32::MAX as usize {
            return None;
        }
        Some(Self {
            p: ComputeProgram {
                buffers: vec![],
                steps: vec![],
                result: INPUT,
                work: 0,
            },
            w,
            h,
        })
    }
    pub fn stage(
        &mut self,
        shader: &ComputeShader,
        source: Source,
        auxiliary: Source,
        params: Vec<f32>,
    ) -> Source {
        let result = self.p.push(
            shader,
            source,
            auxiliary,
            params,
            self.w * self.h * 4,
            [self.w as u32, self.h as u32, 4],
        );
        self.p.steps.last_mut().unwrap().invocations = self.w * self.h;
        result
    }
    pub fn effect(&mut self, shader: &ShaderSpec, source: Source, params: Vec<f32>) -> Source {
        self.stage(
            &ComputeShader {
                name: shader.name,
                source: shader.source,
                entry: ComputeEntry::Rgba,
            },
            source,
            source,
            params,
        )
    }
    pub fn blur(&mut self, source: Source, radius: f32) -> Source {
        if radius < 0.5 {
            source
        } else {
            self.p.rgba_blur(
                source,
                self.w,
                self.h,
                (radius / 3f32.sqrt()).round().max(1.0) as usize,
                3,
            )
        }
    }
    pub fn plane(&mut self, source: Source) -> Source {
        self.stage(&PLANE, source, source, vec![0.0])
    }
    pub fn edge(&mut self, source: Source) -> Source {
        self.stage(&PLANE, source, source, vec![1.0])
    }
    pub fn gradient(&mut self, source: Source) -> Source {
        self.stage(&PLANE, source, source, vec![2.0])
    }
    pub fn streak(&mut self, source: Source, length: f32, direction: (f32, f32)) -> Source {
        let n = direction.0.hypot(direction.1).max(1e-6);
        self.stage(
            &PLANE,
            source,
            source,
            vec![
                3.0,
                length.max(1.0).round(),
                direction.0 / n,
                direction.1 / n,
            ],
        )
    }
    pub fn flatten(&mut self, source: Source, radius: f32, tolerance: f32) -> Source {
        if radius < 0.5 {
            source
        } else {
            self.stage(
                &PLANE,
                source,
                source,
                vec![4.0, radius.round().max(1.0), tolerance.max(0.01)],
            )
        }
    }
    pub fn finish(mut self, result: Source, work: usize) -> ComputeProgram {
        self.p.result = result;
        self.p.work = self.w.saturating_mul(self.h).saturating_mul(work);
        self.p
    }
}

pub fn operation(
    id: &str,
    values: &FilterValues,
    context: &FilterContext<'_>,
) -> Option<FilterOperation> {
    if let Some(op) = crate::gpu_procedural::operation(id, values, context) {
        return Some(op);
    }
    if let Some(op) = crate::gpu_misc::operation(id, values, context) {
        return Some(op);
    }
    if let Some(op) = crate::gpu_sketch::operation(id, values, context) {
        return Some(op);
    }
    if let Some(op) = crate::gpu_surface::operation(id, values, context) {
        return Some(op);
    }
    let id = match id {
        "filter.camera_raw"
        | "filter.cutout"
        | "filter.dry_brush"
        | "filter.film_grain"
        | "filter.fresco"
        | "filter.colored_pencil"
        | "filter.neon_glow"
        | "filter.paint_daubs"
        | "filter.palette_knife"
        | "filter.plastic_wrap"
        | "filter.poster_edges"
        | "filter.rough_pastels"
        | "filter.smudge_stick"
        | "filter.sponge"
        | "filter.underpainting"
        | "filter.watercolor"
        | "filter.accented_edges"
        | "filter.angled_strokes"
        | "filter.crosshatch"
        | "filter.dark_strokes"
        | "filter.ink_outlines"
        | "filter.spatter"
        | "filter.sprayed_strokes"
        | "filter.sumi_e"
        | "filter.reduce_noise"
        | "filter.lens_blur"
        | "filter.smart_sharpen"
        | "filter.color_halftone" => id.to_owned(),
        _ => return None,
    };
    let values = values.clone();
    let (fg, bg) = (context.fg(), context.bg());
    let backdrop = context.backdrop.map(<[f32]>::to_vec);
    Some(FilterOperation::Captured {
        work_per_pixel: 256,
        build: Arc::new(move |w, h| {
            let mut g = Graph::new(w, h)?;
            let v = &values;
            let result = match id.as_str() {
                "filter.camera_raw" => {
                    let mut result = g.stage(
                        &CAMERA,
                        INPUT,
                        INPUT,
                        vec![
                            0.0,
                            v.get("temperature") / 100.0,
                            v.get("tint") / 100.0,
                            2f32.powf(v.get("exposure")),
                            v.get("contrast") / 100.0,
                            v.get("highlights") / 100.0,
                            v.get("shadows") / 100.0,
                            v.get("whites") / 100.0,
                            v.get("blacks") / 100.0,
                        ],
                    );
                    for (key, radius) in [("clarity", 12.0), ("dehaze", 48.0)] {
                        let amount = v.get(key) / 100.0;
                        if amount != 0.0 {
                            let low = g.blur(result, radius);
                            result = g.stage(&CAMERA, result, low, vec![1.0, amount]);
                        }
                    }
                    if v.get("vibrance") != 0.0 || v.get("saturation") != 0.0 {
                        result = g.stage(
                            &CAMERA,
                            result,
                            result,
                            vec![2.0, v.get("vibrance") / 100.0, v.get("saturation") / 100.0],
                        );
                    }
                    let noise = v.get("noise") / 100.0;
                    if noise > 0.0 {
                        let low = g.effect(
                            &crate::gpu::BILATERAL,
                            result,
                            vec![2.0, (0.06 + 0.12 * (1.0 - noise)).max(1e-3), 0.0],
                        );
                        result = g.stage(&CAMERA, result, low, vec![3.0, noise]);
                    }
                    if v.get("sharpening") > 0.0 {
                        let low = g.blur(result, 1.0);
                        result =
                            g.stage(&CAMERA, result, low, vec![4.0, v.get("sharpening") / 100.0]);
                    }
                    if v.get("vignette") != 0.0 {
                        result = g.stage(
                            &CAMERA,
                            result,
                            result,
                            vec![5.0, v.get("vignette") / 100.0],
                        );
                    }
                    result
                }
                "filter.cutout" => {
                    let flat = g.flatten(
                        INPUT,
                        1.0 + v.get("simplicity"),
                        0.08 * v.get("fidelity").clamp(1.0, 3.0),
                    );
                    g.stage(
                        &ARTISTIC,
                        flat,
                        flat,
                        vec![0.0, v.get("levels").max(2.0), 0.09],
                    )
                }
                "filter.dry_brush" => {
                    let flat = g.flatten(INPUT, 1.0 + v.get("size"), 0.05 + v.get("detail") * 0.02);
                    g.stage(
                        &ARTISTIC,
                        flat,
                        flat,
                        vec![1.0, 12.0 - v.get("detail") * 0.6, v.get("texture")],
                    )
                }
                "filter.film_grain" => g.stage(
                    &ARTISTIC,
                    INPUT,
                    INPUT,
                    vec![
                        2.0,
                        v.get("grain") / 20.0,
                        1.0 - v.get("highlight") / 20.0,
                        v.get("intensity") / 10.0,
                    ],
                ),
                "filter.fresco" | "filter.watercolor" => {
                    let plane = g.plane(INPUT);
                    let edge = g.edge(plane);
                    let (r, t, mode) = if id == "filter.fresco" {
                        (1.5 + v.get("size"), 0.06 + v.get("detail") * 0.015, 3.0)
                    } else {
                        (2.0 + (14.0 - v.get("detail")) * 0.4, 0.12, 14.0)
                    };
                    let flat = g.flatten(INPUT, r, t);
                    g.stage(
                        &ARTISTIC,
                        flat,
                        edge,
                        vec![mode, v.get("texture"), v.get("shadow") / 10.0],
                    )
                }
                "filter.colored_pencil" => {
                    let plane = g.plane(INPUT);
                    let edge = g.edge(plane);
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        edge,
                        [
                            vec![
                                4.0,
                                v.get("width").max(1.0),
                                v.get("pressure") / 15.0,
                                v.get("paper") / 50.0,
                            ],
                            bg.to_vec(),
                        ]
                        .concat(),
                    )
                }
                "filter.neon_glow" => {
                    let plane = g.plane(INPUT);
                    let edge = g.edge(plane);
                    let glow = g.blur(edge, v.get("size"));
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        glow,
                        [vec![5.0, v.get("brightness") / 50.0], fg.to_vec()].concat(),
                    )
                }
                "filter.paint_daubs" => {
                    let plane = g.plane(INPUT);
                    let gradient = g.gradient(plane);
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        gradient,
                        vec![
                            6.0,
                            v.get("size").round().max(1.0),
                            v.get("sharpness") / 40.0,
                        ],
                    )
                }
                "filter.palette_knife" => {
                    let flat = g.flatten(
                        INPUT,
                        1.0 + v.get("size") / 6.0,
                        0.1 / v.get("detail").max(1.0),
                    );
                    let color = g.stage(&ARTISTIC, flat, flat, vec![0.0, 6.0, 0.08]);
                    g.blur(color, v.get("softness") * 0.25)
                }
                "filter.plastic_wrap" => {
                    let plane = g.plane(INPUT);
                    let height = g.blur(plane, v.get("smoothness") * 0.4);
                    let gradient = g.gradient(height);
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        gradient,
                        vec![8.0, v.get("strength") / 20.0, v.get("detail") / 15.0],
                    )
                }
                "filter.poster_edges" => {
                    let plane = g.plane(INPUT);
                    let edge = g.edge(plane);
                    let edge = g.blur(edge, v.get("thickness") * 0.4);
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        edge,
                        vec![9.0, 2.0 + v.get("levels"), v.get("intensity") / 10.0],
                    )
                }
                "filter.rough_pastels" => {
                    let mut plane = g.plane(INPUT);
                    if v.get("length") > 0.5 {
                        plane = g.streak(plane, v.get("length") * 0.4, (0.94, 0.34));
                    }
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        plane,
                        vec![
                            10.0,
                            v.get("texture").round().max(0.0),
                            v.get("scaling") / 100.0 * 6.0,
                            v.get("relief") / 50.0,
                            v.get("detail"),
                        ],
                    )
                }
                "filter.smudge_stick" => {
                    let plane = g.plane(INPUT);
                    let smear = g.streak(plane, 1.0 + v.get("length") * 1.5, (0.87, 0.5));
                    g.stage(
                        &ARTISTIC,
                        INPUT,
                        smear,
                        vec![
                            11.0,
                            1.0 - v.get("highlight") / 20.0,
                            v.get("intensity") / 10.0,
                        ],
                    )
                }
                "filter.sponge" => {
                    let flat = g.flatten(INPUT, v.get("smoothness") * 0.4, 0.12);
                    g.stage(
                        &ARTISTIC,
                        flat,
                        flat,
                        vec![12.0, 1.0 + v.get("size"), v.get("definition") / 25.0],
                    )
                }
                "filter.underpainting" => {
                    let flat = g.flatten(INPUT, 2.0 + v.get("size") / 3.0, 0.15);
                    g.stage(
                        &ARTISTIC,
                        flat,
                        INPUT,
                        vec![
                            13.0,
                            v.get("texture").round().max(0.0),
                            v.get("scaling") / 100.0 * 6.0,
                            v.get("relief") / 50.0,
                            v.get("coverage") / 40.0,
                        ],
                    )
                }
                "filter.accented_edges" => {
                    let plane = g.plane(INPUT);
                    let smooth = if v.get("smoothness") > 1.0 {
                        g.blur(plane, v.get("smoothness") * 0.2)
                    } else {
                        plane
                    };
                    let edge = g.edge(smooth);
                    let edge = g.blur(edge, v.get("width") * 0.25);
                    g.stage(
                        &BRUSH,
                        INPUT,
                        edge,
                        vec![0.0, v.get("width"), v.get("brightness") / 50.0],
                    )
                }
                "filter.angled_strokes" => {
                    let plane = g.plane(INPUT);
                    let stroked = g.stage(
                        &BRUSH,
                        plane,
                        plane,
                        vec![
                            1.0,
                            (v.get("length") * 0.3).max(1.0).round(),
                            v.get("balance") / 100.0,
                            v.get("sharpness") / 10.0,
                        ],
                    );
                    g.stage(&BRUSH, stroked, INPUT, vec![3.0, 0.0, 1.0])
                }
                "filter.crosshatch" => {
                    let mut plane = g.plane(INPUT);
                    for pass in 0..v.get("strength").round().max(1.0) as usize {
                        plane = g.stage(
                            &BRUSH,
                            plane,
                            plane,
                            vec![
                                2.0,
                                (v.get("length") * 0.3).max(1.0).round(),
                                0.6 + 0.2 * pass as f32,
                            ],
                        );
                    }
                    g.stage(
                        &BRUSH,
                        plane,
                        INPUT,
                        vec![3.0, v.get("sharpness") / 20.0, 1.0],
                    )
                }
                "filter.dark_strokes" => {
                    let plane = g.plane(INPUT);
                    let strokes = g.streak(plane, 4.0, (0.707, 0.707));
                    g.stage(
                        &BRUSH,
                        INPUT,
                        strokes,
                        vec![
                            4.0,
                            v.get("balance") / 10.0,
                            v.get("black") / 10.0,
                            v.get("white") / 10.0,
                        ],
                    )
                }
                "filter.ink_outlines" => {
                    let plane = g.plane(INPUT);
                    let edge = g.edge(plane);
                    let strokes = g.streak(edge, v.get("length") * 0.25, (0.707, 0.707));
                    g.stage(
                        &BRUSH,
                        INPUT,
                        strokes,
                        vec![5.0, v.get("dark") / 50.0, v.get("light") / 50.0],
                    )
                }
                "filter.spatter" => g.stage(
                    &BRUSH,
                    INPUT,
                    INPUT,
                    vec![6.0, v.get("radius") * (16.0 - v.get("smoothness")) / 15.0],
                ),
                "filter.sprayed_strokes" => {
                    let (dx, dy) = crate::brush::direction_of(v.get("direction"));
                    g.stage(
                        &BRUSH,
                        INPUT,
                        INPUT,
                        vec![7.0, v.get("length"), v.get("radius"), dx, dy],
                    )
                }
                "filter.sumi_e" => {
                    let plane = g.plane(INPUT);
                    let plane = g.blur(plane, v.get("width") * 0.15);
                    let ink = g.streak(plane, v.get("width") * 0.5, (0.707, 0.707));
                    g.stage(
                        &BRUSH,
                        ink,
                        INPUT,
                        vec![8.0, v.get("pressure") / 15.0, v.get("contrast") / 40.0],
                    )
                }
                "filter.reduce_noise" => {
                    let radius = v.get("strength").max(0.5).round();
                    let threshold = (0.25 * (1.0 - v.get("detail") / 100.0)).max(1e-3);
                    let mut result =
                        g.effect(&crate::gpu::BILATERAL, INPUT, vec![radius, threshold, 0.0]);
                    let color = v.get("colour") / 100.0;
                    if color > 0.0 {
                        let smooth = g.blur(result, 1.0 + color * 4.0);
                        result = g.stage(&OTHER, result, smooth, vec![0.0, color]);
                    }
                    if v.get("jpeg") >= 0.5 {
                        result = g.stage(&OTHER, result, result, vec![1.0]);
                    }
                    if v.get("sharpen") > 0.0 {
                        let low = g.blur(result, 1.0);
                        result = g.stage(
                            &OTHER,
                            result,
                            low,
                            vec![2.0, 0.02, v.get("sharpen") / 100.0 * 1.5],
                        );
                    }
                    result
                }
                "filter.lens_blur" => {
                    let radius = v.get("radius").round().max(1.0) as i32;
                    let blades = v.get("shape").round().clamp(0.0, 6.0) as usize;
                    let curvature = v.get("curvature") / 100.0;
                    let threshold = v.get("threshold") / 100.0;
                    let basic = blades == 0 && curvature <= 0.0 && (threshold - 0.75).abs() < 0.01;
                    let args = aperture(
                        radius,
                        blades,
                        curvature,
                        v.get("rotation").to_radians(),
                        v.get("brightness") / 100.0,
                        threshold,
                        basic,
                    );
                    let blur = g.stage(&OTHER, INPUT, INPUT, args);
                    let source = v.get("depth").round().clamp(0.0, 2.0);
                    let mut params = vec![
                        5.0,
                        if source == 2.0 && backdrop.is_none() {
                            0.0
                        } else {
                            source
                        },
                        v.get("focal") / 100.0,
                        u8::from(v.get("invert_depth") >= 0.5) as f32,
                        v.get("noise") / 100.0,
                    ];
                    if source == 2.0 {
                        if let Some(backdrop) = &backdrop {
                            if backdrop.len() != w * h * 4 {
                                return None;
                            }
                            params.extend_from_slice(backdrop);
                        }
                    }
                    g.stage(&OTHER, blur, INPUT, params)
                }
                "filter.smart_sharpen" => {
                    let radius = v.get("radius");
                    let low = match v.get("remove").round().clamp(0.0, 2.0) as u32 {
                        0 => g.blur(INPUT, radius),
                        1 => g.stage(
                            &OTHER,
                            INPUT,
                            INPUT,
                            aperture(radius.round().max(1.0) as i32, 0, 0.0, 0.0, 0.0, 0.75, true),
                        ),
                        _ => {
                            let a = v.get("angle").to_radians();
                            g.stage(
                                &OTHER,
                                INPUT,
                                INPUT,
                                vec![6.0, radius.round().max(1.0), a.cos(), a.sin()],
                            )
                        }
                    };
                    g.stage(
                        &OTHER,
                        INPUT,
                        low,
                        vec![2.0, v.get("noise") / 100.0 * 0.25, v.get("amount") / 100.0],
                    )
                }
                "filter.color_halftone" => {
                    let mut args = vec![7.0, v.get("radius").max(2.0)];
                    for key in ["c1", "c2", "c3"] {
                        let a = v.get(key).to_radians();
                        args.extend([a.sin(), a.cos()]);
                    }
                    g.stage(&OTHER, INPUT, INPUT, args)
                }
                _ => return None,
            };
            Some(g.finish(result, 256))
        }),
    })
}

fn aperture(
    radius: i32,
    blades: usize,
    curvature: f32,
    rotation: f32,
    boost: f32,
    threshold: f32,
    basic: bool,
) -> Vec<f32> {
    let mut args = vec![3.0, boost, threshold, u8::from(basic) as f32];
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let reach = if blades == 0 {
                radius as f32
            } else {
                let step = std::f32::consts::TAU / (blades + 2) as f32;
                let inner =
                    (((dy as f32).atan2(dx as f32) - rotation).rem_euclid(step) - step / 2.0).cos()
                        / (step / 2.0).cos();
                let flat = radius as f32 / inner.max(1e-3);
                flat + (radius as f32 - flat) * curvature
            };
            if (dx as f32).hypot(dy as f32) <= reach {
                args.extend([dx as f32, dy as f32]);
            }
        }
    }
    args
}
