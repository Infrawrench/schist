use crate::gpu_extra::Graph;
use schist_fx::{ComputeEntry, ComputeShader, ComputeSource as Source, FilterOperation};
use schist_plugin_api::{FilterContext, FilterValues};
use std::sync::Arc;
const INPUT: Source = Source::Input(0);
static TREE: ComputeShader = ComputeShader {
    name: "tree-raster",
    source: include_str!("shaders/tree.wgsl"),
    entry: ComputeEntry::Rgba,
};
static FLAME: ComputeShader = ComputeShader {
    name: "flame",
    source: concat!(
        include_str!("shaders/noise.wgsl"),
        include_str!("shaders/flame.wgsl")
    ),
    entry: ComputeEntry::Rgba,
};
static EXTRUDE_PICK: ComputeShader = ComputeShader {
    name: "extrude-placement",
    source: concat!(
        include_str!("shaders/noise.wgsl"),
        include_str!("shaders/extrude_pick.wgsl")
    ),
    entry: ComputeEntry::Atomic,
};
static EXTRUDE: ComputeShader = ComputeShader {
    name: "extrude-shade",
    source: include_str!("shaders/extrude.wgsl"),
    entry: ComputeEntry::Rgba,
};

pub fn operation(
    id: &str,
    v: &FilterValues,
    context: &FilterContext<'_>,
) -> Option<FilterOperation> {
    if !matches!(id, "filter.tree" | "filter.flame" | "filter.extrude") {
        return None;
    }
    let id = id.to_owned();
    let v = v.clone();
    let path = context.path.map(<[(f32, f32)]>::to_vec);
    Some(FilterOperation::Captured {
        work_per_pixel: 512,
        build: Arc::new(move |w, h| {
            let mut g = Graph::new(w, h)?;
            let result = match id.as_str() {
                "filter.tree" => {
                    let commands = crate::render::tree_commands(w, h, &v);
                    let cols = w.div_ceil(16);
                    let rows = h.div_ceil(16);
                    let mut bins = vec![Vec::new(); cols * rows];
                    for (i, c) in commands.iter().enumerate() {
                        let r = c[2].ceil() as i32;
                        let x = c[0] as i32;
                        let y = c[1] as i32;
                        if x + r < 0 || y + r < 0 || x - r >= w as i32 || y - r >= h as i32 {
                            continue;
                        }
                        for by in
                            (y - r).max(0) as usize / 16..=(y + r).min(h as i32 - 1) as usize / 16
                        {
                            for bx in (x - r).max(0) as usize / 16
                                ..=(x + r).min(w as i32 - 1) as usize / 16
                            {
                                bins[by * cols + bx].push(i);
                            }
                        }
                    }
                    let mut data = vec![0.0; bins.len() * 2];
                    for (i, bin) in bins.iter().enumerate() {
                        data[i * 2] = data.len() as f32;
                        data[i * 2 + 1] = bin.len() as f32;
                        data.extend(bin.iter().map(|&v| v as f32));
                    }
                    let base = data.len();
                    data.extend(commands.iter().flatten());
                    if data.len() > 16_777_216 {
                        return None;
                    }
                    g.p.buffers.push(data);
                    g.stage(
                        &TREE,
                        INPUT,
                        Source::Input(1),
                        vec![cols as f32, base as f32],
                    )
                }
                "filter.flame" => {
                    let count = v.get("count").round().max(1.0) as usize;
                    let length = v.get("height") / 100.0 * h as f32;
                    let width = v.get("width") / 100.0 * (w as f32 / count as f32) * 0.9;
                    let mut args = vec![
                        count as f32,
                        length,
                        width,
                        v.get("angle").to_radians().tan(),
                        v.get("turbulence") / 100.0,
                        v.get("opacity") / 100.0,
                        v.get("seed"),
                    ];
                    for (x, y, nx, ny) in crate::render::flame_roots(w, h, count, path.as_deref()) {
                        args.extend([x, y, nx, ny]);
                    }
                    g.stage(&FLAME, INPUT, INPUT, args)
                }
                _ => {
                    let size = v.get("size").max(2.0) as usize;
                    let count = w
                        .div_ceil(size)
                        .checked_mul(h.div_ceil(size))?
                        .checked_mul(size.checked_mul(size)?)?;
                    if count > 16_777_215 {
                        return None;
                    }
                    let args = vec![
                        size as f32,
                        v.get("depth"),
                        v.get("basis"),
                        v.get("solid"),
                        v.get("type"),
                    ];
                    let winners = g.p.push(
                        &EXTRUDE_PICK,
                        INPUT,
                        INPUT,
                        args.clone(),
                        w * h,
                        [w as u32, h as u32, 4],
                    );
                    g.p.steps.last_mut()?.invocations = count;
                    g.stage(&EXTRUDE, INPUT, winners, args)
                }
            };
            Some(g.finish(result, 512))
        }),
    })
}
