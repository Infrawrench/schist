use crate::gpu_extra::Graph;
use schist_fx::{ComputeEntry, ComputeShader, ComputeSource as Source, FilterOperation};
use schist_plugin_api::{FilterContext, FilterValues};
use std::sync::Arc;

static MISC: ComputeShader = ComputeShader {
    name: "misc-filters",
    source: concat!(
        include_str!("shaders/noise.wgsl"),
        include_str!("shaders/gallery_common.wgsl"),
        include_str!("shaders/misc.wgsl")
    ),
    entry: ComputeEntry::Rgba,
};
static AVERAGE: ComputeShader =
    ComputeShader::new("average-reduction", include_str!("shaders/average.wgsl"));
const INPUT: Source = Source::Input(0);

pub fn operation(
    id: &str,
    v: &FilterValues,
    context: &FilterContext<'_>,
) -> Option<FilterOperation> {
    let id = match id {
        "filter.blur"
        | "filter.blur_more"
        | "filter.sharpen_more"
        | "filter.sharpen_edges"
        | "filter.diffuse_glow"
        | "filter.despeckle"
        | "filter.dust_scratches"
        | "filter.custom"
        | "filter.hsb_hsl"
        | "filter.deinterlace"
        | "filter.ntsc_colors"
        | "filter.average"
        | "filter.ripple"
        | "filter.wave" => id.to_owned(),
        _ => return None,
    };
    let v = v.clone();
    let bg = context.bg();
    Some(FilterOperation::Captured {
        work_per_pixel: 128,
        build: Arc::new(move |w, h| {
            let mut g = Graph::new(w, h)?;
            let result = match id.as_str() {
                "filter.blur" | "filter.blur_more" => {
                    g.blur(INPUT, if id == "filter.blur" { 0.6 } else { 1.7 })
                }
                "filter.sharpen_more" => {
                    let first = g.effect(
                        &crate::gpu::CONVOLVE,
                        INPUT,
                        vec![
                            3.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0,
                        ],
                    );
                    g.effect(
                        &crate::gpu::CONVOLVE,
                        first,
                        vec![
                            3.0, 1.0, 0.0, 0.0, 0.0, -0.5, 0.0, -0.5, 3.0, -0.5, 0.0, -0.5, 0.0,
                        ],
                    )
                }
                "filter.sharpen_edges" => {
                    let low = g.blur(INPUT, 1.5);
                    g.stage(&MISC, INPUT, low, vec![0.0])
                }
                "filter.diffuse_glow" => {
                    let bright = g.stage(&MISC, INPUT, INPUT, vec![1.0, v.get("clear") / 20.0]);
                    let low = g.blur(bright, 4.0 + v.get("glow") / 20.0 * 12.0);
                    g.stage(
                        &MISC,
                        INPUT,
                        low,
                        vec![
                            2.0,
                            v.get("graininess") / 10.0,
                            v.get("glow") / 20.0,
                            bg[0],
                            bg[1],
                            bg[2],
                        ],
                    )
                }
                "filter.despeckle" | "filter.dust_scratches" => {
                    let (r, t) = if id == "filter.despeckle" {
                        (1.0, 0.04)
                    } else {
                        (v.get("radius").round().max(1.0), v.get("threshold") / 255.0)
                    };
                    if r > 100.0 {
                        return None;
                    }
                    g.effect(
                        if r <= 4.0 {
                            &crate::gpu::MEDIAN
                        } else {
                            &crate::gpu::MEDIAN_LARGE
                        },
                        INPUT,
                        vec![r, u8::from(id == "filter.dust_scratches") as f32, 3.0, t],
                    )
                }
                "filter.custom" => {
                    let mut args = vec![
                        5.0,
                        v.get("scale").abs().max(1e-3),
                        v.get("offset") / 255.0,
                        1.0,
                    ];
                    for y in 0..5 {
                        for x in 0..5 {
                            args.push(v.get(&format!("k{y}{x}")));
                        }
                    }
                    g.effect(&crate::gpu::CONVOLVE, INPUT, args)
                }
                "filter.hsb_hsl" => g.stage(
                    &MISC,
                    INPUT,
                    INPUT,
                    vec![3.0, v.get("mode").round().clamp(0.0, 3.0)],
                ),
                "filter.deinterlace" => g.stage(
                    &MISC,
                    INPUT,
                    INPUT,
                    vec![4.0, v.get("field"), v.get("fill")],
                ),
                "filter.ntsc_colors" => g.stage(&MISC, INPUT, INPUT, vec![5.0]),
                "filter.average" => {
                    let groups = (w * h).div_ceil(1024);
                    let sums = g.p.push(
                        &AVERAGE,
                        INPUT,
                        INPUT,
                        vec![(w * h) as f32],
                        groups * 3,
                        [w as u32, h as u32, 4],
                    );
                    g.stage(&MISC, INPUT, sums, vec![6.0, groups as f32])
                }
                "filter.ripple" => {
                    let amount = v.get("amount") / 100.0;
                    let size = v.get("size").max(1.0);
                    let offsets = (0..h)
                        .chain(0..w)
                        .map(|i| ((i as f32 + 0.5) / size).sin() * amount * size * 0.25)
                        .collect();
                    g.effect(&crate::gpu::RIPPLE, INPUT, offsets)
                }
                "filter.wave" => {
                    let generators = v.get("generators").round().max(1.0) as usize;
                    let len = v.get("wavelength").max(1.0);
                    let amp = v.get("amplitude");
                    let seed = v.get("seed") as u32;
                    let kind = v.get("type").round().clamp(0.0, 2.0) as u32;
                    let waves: Vec<_> = (0..generators)
                        .map(|i| {
                            let jitter = 0.5 + crate::util::value_noise(i as f32 * 13.0, 0.0, seed);
                            (
                                std::f32::consts::TAU / (len * jitter),
                                crate::util::value_noise(0.0, i as f32 * 7.0, seed)
                                    * std::f32::consts::TAU,
                            )
                        })
                        .collect();
                    let offsets = (0..h)
                        .map(|i| (i, v.get("horizontal") / 100.0))
                        .chain((0..w).map(|i| (i, v.get("vertical") / 100.0)))
                        .map(|(i, scale)| {
                            let mut offset = 0.0;
                            for &(k, phase) in &waves {
                                let p = (i as f32 + 0.5) * k + phase;
                                let t = p.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                                let value = match kind {
                                    1 => 4.0 * (t - 0.5).abs() - 1.0,
                                    2 => {
                                        if t < 0.5 {
                                            1.0
                                        } else {
                                            -1.0
                                        }
                                    }
                                    _ => p.sin(),
                                };
                                offset += value * amp / generators as f32;
                            }
                            offset * scale
                        })
                        .collect();
                    g.effect(&crate::gpu::WAVE, INPUT, offsets)
                }
                _ => return None,
            };
            Some(g.finish(result, 128))
        }),
    })
}
