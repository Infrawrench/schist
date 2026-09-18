use crate::gpu_extra::Graph;
use schist_fx::{ComputeEntry, ComputeShader, ComputeSource as Source, FilterOperation};
use schist_plugin_api::{FilterContext, FilterValues};
use std::sync::Arc;

static SKETCH: ComputeShader = ComputeShader {
    name: "sketch",
    source: concat!(
        include_str!("shaders/noise.wgsl"),
        include_str!("shaders/gallery_common.wgsl"),
        include_str!("shaders/sketch.wgsl")
    ),
    entry: ComputeEntry::Rgba,
};
const INPUT: Source = Source::Input(0);

pub fn operation(
    id: &str,
    v: &FilterValues,
    context: &FilterContext<'_>,
) -> Option<FilterOperation> {
    let id = match id {
        "filter.bas_relief"
        | "filter.chalk_charcoal"
        | "filter.charcoal"
        | "filter.chrome"
        | "filter.conte_crayon"
        | "filter.graphic_pen"
        | "filter.halftone_pattern"
        | "filter.note_paper"
        | "filter.photocopy"
        | "filter.plaster"
        | "filter.reticulation"
        | "filter.stamp"
        | "filter.torn_edges"
        | "filter.water_paper" => id.to_owned(),
        _ => return None,
    };
    let v = v.clone();
    let colors = [vec![100.0], context.fg().to_vec(), context.bg().to_vec()].concat();
    Some(FilterOperation::Captured {
        work_per_pixel: 256,
        build: Arc::new(move |w, h| {
            let mut g = Graph::new(w, h)?;
            let plane = g.plane(INPUT);
            let output = match id.as_str() {
                "filter.bas_relief" => {
                    let plane = g.blur(plane, v.get("smoothness") * 0.25);
                    let (x, y) = crate::sketch::light_of(v.get("light"));
                    g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![0.0, x, y, v.get("detail") * 0.9],
                    )
                }
                "filter.chalk_charcoal" => g.stage(
                    &SKETCH,
                    plane,
                    plane,
                    vec![
                        1.0,
                        v.get("charcoal") / 20.0,
                        v.get("chalk") / 20.0,
                        0.5 + v.get("pressure") / 5.0,
                    ],
                ),
                "filter.charcoal" => {
                    let edge = g.edge(plane);
                    let smear = g.streak(plane, v.get("thickness") * 2.0, (0.707, 0.707));
                    g.stage(
                        &SKETCH,
                        smear,
                        edge,
                        vec![2.0, v.get("detail") / 5.0, v.get("balance") / 100.0],
                    )
                }
                "filter.chrome" => {
                    let plane = g.blur(plane, 0.5 + v.get("smoothness") * 0.5);
                    let relief = g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![0.0, 0.707, -0.707, 6.0 + v.get("detail") * 3.0],
                    );
                    g.stage(&SKETCH, relief, relief, vec![3.0, v.get("detail")])
                }
                "filter.conte_crayon" => g.stage(
                    &SKETCH,
                    plane,
                    plane,
                    vec![
                        4.0,
                        v.get("foreground") / 15.0,
                        v.get("background") / 15.0,
                        v.get("texture").round().max(0.0),
                        (v.get("scaling") / 100.0 * 6.0).max(1.0),
                        v.get("relief") / 50.0,
                    ],
                ),
                "filter.graphic_pen" => {
                    let direction = crate::brush::direction_of(v.get("direction"));
                    let plane = g.streak(plane, v.get("length") * 0.5, direction);
                    g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![5.0, direction.0, direction.1, v.get("balance") / 100.0],
                    )
                }
                "filter.halftone_pattern" => g.stage(
                    &SKETCH,
                    plane,
                    plane,
                    vec![
                        6.0,
                        v.get("size").max(1.0) * 4.0,
                        v.get("contrast") / 50.0,
                        v.get("pattern").round().clamp(0.0, 2.0),
                    ],
                ),
                "filter.note_paper" => {
                    let plane = g.blur(plane, 1.5);
                    let relief = g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![0.0, 0.707, -0.707, 6.0 * v.get("relief") / 25.0],
                    );
                    g.stage(
                        &SKETCH,
                        plane,
                        relief,
                        vec![7.0, v.get("balance") / 50.0, v.get("graininess") / 20.0],
                    )
                }
                "filter.photocopy" => {
                    let local = g.blur(plane, 1.0 + v.get("detail") * 0.5);
                    g.stage(&SKETCH, plane, local, vec![8.0, v.get("darkness") / 50.0])
                }
                "filter.plaster" => {
                    let plane = g.blur(plane, v.get("smoothness") * 0.8);
                    let height =
                        g.stage(&SKETCH, plane, plane, vec![11.0, v.get("balance") / 50.0]);
                    let height = g.blur(height, 2.0 + v.get("smoothness"));
                    let (x, y) = crate::sketch::light_of(v.get("light"));
                    let relief = g.stage(&SKETCH, height, height, vec![0.0, x, y, 14.0]);
                    g.stage(&SKETCH, relief, height, vec![9.0])
                }
                "filter.reticulation" => g.stage(
                    &SKETCH,
                    plane,
                    plane,
                    vec![
                        10.0,
                        1.0 + v.get("density") / 50.0 * 8.0,
                        v.get("black") / 50.0,
                        v.get("white") / 50.0,
                    ],
                ),
                "filter.stamp" => {
                    let plane = g.blur(plane, v.get("smoothness") * 0.4);
                    g.stage(&SKETCH, plane, plane, vec![11.0, v.get("balance") / 50.0])
                }
                "filter.torn_edges" => {
                    let plane = g.blur(plane, v.get("smoothness") * 0.3);
                    g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![12.0, v.get("balance") / 50.0, v.get("contrast") / 25.0],
                    )
                }
                "filter.water_paper" => {
                    let streak = g.stage(
                        &SKETCH,
                        plane,
                        plane,
                        vec![13.0, (v.get("fiber") * 0.3).max(1.0).round()],
                    );
                    let result = g.stage(
                        &SKETCH,
                        INPUT,
                        streak,
                        vec![14.0, v.get("brightness") / 100.0, v.get("contrast") / 100.0],
                    );
                    return Some(g.finish(result, 256));
                }
                _ => return None,
            };
            let result = g.stage(&SKETCH, output, INPUT, colors.clone());
            Some(g.finish(result, 256))
        }),
    })
}
