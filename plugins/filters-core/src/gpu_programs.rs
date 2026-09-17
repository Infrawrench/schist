use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, FilterOperation};
use schist_plugin_api::FilterValues;
static COMPOUND: ComputeShader = ComputeShader {
    name: "compound-filter",
    source: include_str!("shaders/compound.wgsl"),
};
pub fn operation(id: &str, v: &FilterValues) -> Option<FilterOperation> {
    let mut params = match id {
        "filter.mosaic" => vec![10.0, v.get("size").round().max(1.0)],
        "filter.high_pass" => vec![0.0, v.get("radius"), 0.0, 0.0],
        "filter.unsharp_mask" => vec![
            1.0,
            v.get("radius"),
            v.get("amount") / 100.0,
            v.get("threshold") / 255.0,
        ],
        "filter.sharpen" => vec![1.0, 1.0, v.get("amount") / 100.0, 0.0],
        "filter.field_blur" => {
            let a = v.get("angle").to_radians();
            vec![
                2.0,
                v.get("blur"),
                0.0,
                a.cos(),
                a.sin(),
                v.get("position") / 100.0,
                (v.get("spread") / 100.0).max(0.01),
            ]
        }
        "filter.iris_blur" => vec![
            3.0,
            v.get("blur"),
            0.0,
            v.get("x") / 100.0,
            v.get("y") / 100.0,
            v.get("radius") / 100.0,
            v.get("roundness") / 100.0,
            v.get("feather") / 100.0,
        ],
        "filter.tilt_shift" => {
            let a = v.get("angle").to_radians();
            vec![
                4.0,
                v.get("blur"),
                0.0,
                -a.sin(),
                a.cos(),
                v.get("position") / 100.0,
                v.get("band") / 100.0,
                v.get("feather") / 100.0,
            ]
        }
        _ => return None,
    };
    params.shrink_to_fit();
    let work_per_pixel = if params[0] == 10.0 {
        16
    } else {
        ((params[1].max(1.0) as usize) * 12 + 32) * if params[0] >= 2.0 { 3 } else { 1 }
    };
    Some(FilterOperation::Program {
        build,
        params,
        work_per_pixel,
    })
}
fn build(w: usize, h: usize, args: &[f32]) -> Option<ComputeProgram> {
    let len = w.checked_mul(h)?.checked_mul(4)?;
    if len == 0 {
        return None;
    }
    let original = ComputeSource::Input(0);
    let mut p = ComputeProgram {
        buffers: vec![],
        steps: vec![],
        result: original,
        work: 0,
    };
    let mode = args[0] as u32;
    let radius = args[1];
    let shape = [w as u32, h as u32, 4];
    if mode == 10 {
        static CELLS: ComputeShader = ComputeShader {
            name: "cell-average",
            source: include_str!("shaders/cells.wgsl"),
        };
        let cell = radius as usize;
        let cells = w
            .div_ceil(cell)
            .checked_mul(h.div_ceil(cell))?
            .checked_mul(4)?;
        let means = p.push(&CELLS, original, original, vec![radius, 0.0], cells, shape);
        p.result = p.push(&CELLS, means, original, vec![radius, 1.0], len, shape);
        p.work = w * h * 16;
        return Some(p);
    }
    let gaussian = |p: &mut ComputeProgram, r: f32| {
        if r < 0.5 {
            original
        } else {
            p.rgba_blur(
                original,
                w,
                h,
                (r / 3f32.sqrt()).round().max(1.0) as usize,
                3,
            )
        }
    };
    if mode <= 1 {
        if mode == 1 && args[2] <= 0.0 {
            return None;
        }
        let low = if mode == 1 && radius < 0.5 {
            p.rgba_blur(original, w, h, 1, 1)
        } else {
            gaussian(&mut p, radius)
        };
        p.result = p.push(&COMPOUND, low, original, args.to_vec(), len, shape);
    } else {
        if radius <= 0.0 {
            return None;
        }
        let mut result = original;
        for (i, k) in [0.34, 0.67, 1.0].into_iter().enumerate() {
            let low = gaussian(&mut p, radius * k);
            let mut params = args.to_vec();
            params[2] = i as f32;
            result = p.push(&COMPOUND, low, result, params, len, shape);
        }
        p.result = result;
    }
    p.work = w * h * ((radius.max(1.0) as usize) * 12 + 32) * if mode >= 2 { 3 } else { 1 };
    Some(p)
}
