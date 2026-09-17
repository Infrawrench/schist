use crate::gpu_extra::Graph;
use schist_fx::{ComputeEntry, ComputeShader, ComputeSource as Source, FilterOperation};
use schist_plugin_api::{FilterContext, FilterValues};
use std::sync::Arc;

static SURFACE: ComputeShader = ComputeShader {
    name: "surface-render",
    source: concat!(
        include_str!("shaders/noise.wgsl"),
        include_str!("shaders/gallery_common.wgsl"),
        include_str!("shaders/surface_render.wgsl")
    ),
    entry: ComputeEntry::Rgba,
};
static CELLS: ComputeShader =
    ComputeShader::new("cell-average", include_str!("shaders/cells.wgsl"));
const INPUT: Source = Source::Input(0);

pub fn operation(
    id: &str,
    v: &FilterValues,
    context: &FilterContext<'_>,
) -> Option<FilterOperation> {
    let id = match id {
        "filter.craquelure"
        | "filter.grain"
        | "filter.mosaic_tiles"
        | "filter.patchwork"
        | "filter.stained_glass"
        | "filter.lens_flare"
        | "filter.lighting_effects"
        | "filter.picture_frame"
        | "filter.bump_map"
        | "filter.normal_map"
        | "filter.glowing_edges"
        | "filter.solarize"
        | "filter.wind"
        | "filter.tiles"
        | "filter.diffuse"
        | "filter.oil_paint" => id.to_owned(),
        _ => return None,
    };
    let v = v.clone();
    let (fg, bg) = (context.fg(), context.bg());
    Some(FilterOperation::Captured {
        work_per_pixel: 256,
        build: Arc::new(move |w, h| {
            let mut g = Graph::new(w, h)?;
            let result = match id.as_str() {
                "filter.craquelure" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    vec![
                        0.0,
                        v.get("spacing").max(2.0),
                        v.get("depth") / 10.0,
                        v.get("brightness") / 10.0,
                    ],
                ),
                "filter.grain" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    vec![
                        1.0,
                        v.get("intensity") / 100.0,
                        v.get("contrast") / 100.0,
                        v.get("kind").round().clamp(0.0, 9.0),
                    ],
                ),
                "filter.mosaic_tiles" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    vec![
                        2.0,
                        v.get("size").max(2.0),
                        v.get("grout"),
                        v.get("lighten") / 10.0,
                    ],
                ),
                "filter.patchwork" => {
                    let size = (2.0 + v.get("size") * 3.0).round().max(2.0) as usize;
                    let shape = [w as u32, h as u32, 4];
                    let means = g.p.push(
                        &CELLS,
                        INPUT,
                        INPUT,
                        vec![size as f32, 2.0],
                        w.div_ceil(size) * h.div_ceil(size) * 4,
                        shape,
                    );
                    g.p.push(
                        &CELLS,
                        means,
                        INPUT,
                        vec![size as f32, 3.0, v.get("relief") / 25.0],
                        w * h * 4,
                        shape,
                    )
                }
                "filter.stained_glass" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    vec![
                        3.0,
                        v.get("size").max(2.0),
                        v.get("border"),
                        v.get("light") / 10.0,
                    ],
                ),
                "filter.lens_flare" => {
                    let ghosts: &[[f32; 3]] = match v.get("lens").round().clamp(0.0, 3.0) as u32 {
                        0 => &[
                            [0.35, 0.10, 0.6],
                            [0.70, 0.06, 0.9],
                            [1.30, 0.05, 0.8],
                            [1.70, 0.08, 0.5],
                            [2.10, 0.04, 1.1],
                        ],
                        1 => &[[0.55, 0.16, 0.8], [1.45, 0.20, 0.6]],
                        2 => &[[0.80, 0.07, 1.0], [1.25, 0.05, 0.7]],
                        _ => &[
                            [0.45, 0.22, 0.5],
                            [0.95, 0.10, 0.9],
                            [1.55, 0.14, 0.4],
                            [2.30, 0.06, 0.7],
                        ],
                    };
                    let mut args = vec![
                        4.0,
                        v.get("x") / 100.0 * w as f32,
                        v.get("y") / 100.0 * h as f32,
                        v.get("brightness") / 100.0,
                    ];
                    args.extend(ghosts.iter().flatten());
                    g.stage(&SURFACE, INPUT, INPUT, args)
                }
                "filter.lighting_effects" => {
                    let plane = g.plane(INPUT);
                    let height = g.blur(plane, 1.2);
                    let a = v.get("angle").to_radians();
                    g.stage(
                        &SURFACE,
                        height,
                        INPUT,
                        vec![
                            5.0,
                            v.get("type").round().clamp(0.0, 2.0),
                            v.get("intensity") / 100.0,
                            v.get("ambience") / 100.0,
                            v.get("gloss") / 100.0,
                            v.get("height") / 100.0,
                            ((w * w + h * h) as f32).sqrt() * (v.get("spread") / 100.0).max(0.05),
                            a.cos(),
                            a.sin(),
                            v.get("x") / 100.0 * w as f32,
                            v.get("y") / 100.0 * h as f32,
                        ],
                    )
                }
                "filter.picture_frame" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    vec![
                        6.0,
                        v.get("style").round().clamp(0.0, 4.0),
                        (v.get("width") / 100.0 * w.min(h) as f32).max(1.0),
                        v.get("tone") / 100.0,
                        v.get("relief") / 100.0,
                    ],
                ),
                "filter.bump_map" | "filter.normal_map" => {
                    let plane = g.plane(INPUT);
                    let plane = g.blur(
                        plane,
                        [0.4, 1.6, 4.0][v.get("blur").round().clamp(0.0, 2.0) as usize],
                    );
                    let height = g.stage(
                        &SURFACE,
                        plane,
                        plane,
                        vec![
                            7.0,
                            v.get("contrast") / 100.0,
                            u8::from(v.get("invert") >= 0.5) as f32,
                        ],
                    );
                    g.stage(
                        &SURFACE,
                        height,
                        INPUT,
                        vec![
                            if id == "filter.bump_map" { 8.0 } else { 9.0 },
                            v.get("strength") / 100.0 * 20.0,
                        ],
                    )
                }
                "filter.glowing_edges" => {
                    let smooth = g.blur(INPUT, (v.get("smoothness") - 1.0) * 0.35);
                    let edge = g.stage(&SURFACE, smooth, smooth, vec![10.0]);
                    g.stage(
                        &SURFACE,
                        INPUT,
                        edge,
                        vec![11.0, v.get("brightness") * v.get("width").max(1.0) * 0.25],
                    )
                }
                "filter.solarize" => g.stage(&SURFACE, INPUT, INPUT, vec![12.0]),
                "filter.wind" => {
                    let edge = g.stage(&SURFACE, INPUT, INPUT, vec![10.0]);
                    let method = v.get("method").round().clamp(0.0, 2.0);
                    g.stage(
                        &SURFACE,
                        INPUT,
                        edge,
                        vec![
                            13.0,
                            v.get("strength").max(1.0) * if method == 1.0 { 2.5 } else { 1.0 },
                            u8::from(v.get("direction") >= 0.5) as f32,
                            u8::from(method == 2.0) as f32,
                        ],
                    )
                }
                "filter.tiles" => g.stage(
                    &SURFACE,
                    INPUT,
                    INPUT,
                    [
                        vec![
                            14.0,
                            v.get("count").max(2.0) as usize as f32,
                            v.get("offset") / 100.0,
                            v.get("fill").round().clamp(0.0, 4.0),
                        ],
                        fg.to_vec(),
                        bg.to_vec(),
                    ]
                    .concat(),
                ),
                "filter.diffuse" => {
                    let r = v.get("amount").max(1.0);
                    let mut args = vec![15.0, r, v.get("mode").round().clamp(0.0, 3.0)];
                    for k in 0..8 {
                        let a = k as f32 * std::f32::consts::TAU / 8.0;
                        args.extend([(a.cos() * r) as i32 as f32, (a.sin() * r) as i32 as f32]);
                    }
                    g.stage(&SURFACE, INPUT, INPUT, args)
                }
                "filter.oil_paint" => {
                    let levels = v.get("levels").max(2.0) as usize;
                    if levels > 64 {
                        return None;
                    }
                    let a = v.get("angle").to_radians();
                    g.effect(
                        &crate::gpu::OIL,
                        INPUT,
                        vec![
                            v.get("radius").max(1.0) as i32 as f32,
                            levels as f32,
                            v.get("bristle") / 10.0,
                            v.get("shine") / 10.0,
                            a.cos(),
                            a.sin(),
                        ],
                    )
                }
                _ => return None,
            };
            Some(g.finish(result, 256))
        }),
    })
}
