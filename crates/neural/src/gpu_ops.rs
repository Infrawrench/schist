use super::*;

pub(super) fn axis(value: i64, rank: usize) -> Option<usize> {
    usize::try_from(if value < 0 {
        value + rank as i64
    } else {
        value
    })
    .ok()
    .filter(|&i| i < rank)
}
fn string<'a>(node: &'a NodeProto, key: &str, default: &'a [u8]) -> &'a [u8] {
    node.attribute
        .iter()
        .find(|a| a.name == key)
        .map_or(default, |a| a.s.as_slice())
}

pub(super) fn matmul(a: &mut Vec<usize>, b: &[usize]) -> Option<Vec<f32>> {
    if a.is_empty() || b.is_empty() || a.len() > 8 || b.len() > 8 {
        return None;
    }
    let (av, bv) = (a.len() == 1, b.len() == 1);
    let mut aa = a.clone();
    let mut bb = b.to_vec();
    if av {
        aa.insert(0, 1);
    }
    if bv {
        bb.push(1);
    }
    let rank = aa.len().max(bb.len());
    while aa.len() < rank {
        aa.insert(0, 1);
    }
    while bb.len() < rank {
        bb.insert(0, 1);
    }
    let (m, k, n) = (aa[rank - 2], aa[rank - 1], bb[rank - 1]);
    if k != bb[rank - 2] {
        return None;
    }
    let (astride, bstride) = (strides(&aa), strides(&bb));
    let mut params = vec![8.0, m as f32, k as f32, n as f32, (rank - 2) as f32];
    a.clear();
    for i in 0..rank - 2 {
        if aa[i] != bb[i] && aa[i] != 1 && bb[i] != 1 {
            return None;
        }
        a.push(aa[i].max(bb[i]));
        params.extend([
            aa[i].max(bb[i]) as f32,
            if aa[i] == 1 { 0.0 } else { astride[i] as f32 },
            if bb[i] == 1 { 0.0 } else { bstride[i] as f32 },
        ]);
    }
    if !av {
        a.push(m);
    }
    if !bv {
        a.push(n);
    }
    size(a)?;
    Some(params)
}

pub(super) fn resize(
    node: &NodeProto,
    shape: &mut [usize],
    constants: &HashMap<String, Vec<f32>>,
    metadata: &HashMap<String, Vec<i64>>,
    proto: &ModelProto,
) -> Option<Vec<f32>> {
    if shape.len() != 4
        || integer(node, "antialias", 0) != 0
        || integer(node, "exclude_outside", 0) > 1
        || string(node, "keep_aspect_ratio_policy", b"stretch") != b"stretch"
    {
        return None;
    }
    let opset = proto
        .opset_import
        .iter()
        .find(|o| o.domain.is_empty() || o.domain == "ai.onnx")?
        .version;
    let axes = integers(node, "axes", &[0, 1, 2, 3]);
    let axes: Vec<_> = axes
        .into_iter()
        .map(|i| axis(i, 4))
        .collect::<Option<_>>()?;
    let mut seen = [false; 4];
    for &i in &axes {
        if seen[i] {
            return None;
        }
        seen[i] = true;
    }
    let mut scales = [1.0; 4];
    let mut output = shape.to_vec();
    if let Some(name) = node.input.get(3).filter(|n| !n.is_empty()) {
        let sizes = metadata.get(name)?;
        if sizes.len() != axes.len() {
            return None;
        }
        for (&i, &value) in axes.iter().zip(sizes) {
            output[i] = usize::try_from(value)
                .ok()
                .filter(|&v| v > 0 && v <= 16_777_216)?;
            scales[i] = output[i] as f32 / shape[i] as f32;
        }
    } else {
        let values = constants.get(node.input.get(if opset < 11 { 1 } else { 2 })?)?;
        if values.len() != axes.len() {
            return None;
        }
        for (&i, &value) in axes.iter().zip(values) {
            if !value.is_finite() || value <= 0.0 {
                return None;
            }
            scales[i] = value;
            output[i] = (shape[i] as f32 * value).floor() as usize;
        }
    }
    size(&output)?;
    if output[..2] != shape[..2] || scales[..2] != [1.0, 1.0] {
        return None;
    }
    let interpolation = match string(node, "mode", b"nearest") {
        b"nearest" => 0.0,
        b"linear" => 1.0,
        b"cubic" => 2.0,
        _ => return None,
    };
    let coordinate = match string(
        node,
        "coordinate_transformation_mode",
        if opset < 11 {
            b"asymmetric"
        } else {
            b"half_pixel"
        },
    ) {
        b"asymmetric" => 0.0,
        b"half_pixel" => 1.0,
        b"pytorch_half_pixel" => 2.0,
        b"align_corners" => 3.0,
        b"tf_half_pixel_for_nn" => 4.0,
        b"half_pixel_symmetric" => 5.0,
        _ => return None,
    };
    let nearest = match string(
        node,
        "nearest_mode",
        if opset < 11 {
            b"floor"
        } else {
            b"round_prefer_floor"
        },
    ) {
        b"floor" => 0.0,
        b"ceil" => 1.0,
        b"round_prefer_floor" => 2.0,
        b"round_prefer_ceil" => 3.0,
        _ => return None,
    };
    let params = vec![
        11.0,
        shape[3] as f32,
        shape[2] as f32,
        output[3] as f32,
        output[2] as f32,
        scales[3],
        scales[2],
        coordinate,
        interpolation,
        nearest,
        float(node, "cubic_coeff_a", -0.75),
        integer(node, "exclude_outside", 0) as f32,
    ];
    shape.copy_from_slice(&output);
    Some(params)
}

pub(super) fn reduce(
    node: &NodeProto,
    shape: &mut Vec<usize>,
    metadata: &HashMap<String, Vec<i64>>,
) -> Option<Vec<f32>> {
    let global = node.op_type.starts_with("Global");
    let mut axes = if global {
        (2..shape.len() as i64).collect()
    } else if let Some(name) = node.input.get(1).filter(|n| !n.is_empty()) {
        metadata.get(name)?.clone()
    } else {
        integers(node, "axes", &[])
    };
    if axes.is_empty() && integer(node, "noop_with_empty_axes", 0) == 0 {
        axes = (0..shape.len() as i64).collect();
    }
    let mut reduced = vec![false; shape.len()];
    for a in axes {
        let i = axis(a, shape.len())?;
        if reduced[i] {
            return None;
        }
        reduced[i] = true;
    }
    let mode = if !reduced.iter().any(|v| *v) {
        1.0 // Empty axes with noop_with_empty_axes is identity, even for L2.
    } else {
        match node.op_type.as_str() {
            "ReduceMean" | "GlobalAveragePool" => 0.0,
            "ReduceSum" => 1.0,
            "ReduceMax" | "GlobalMaxPool" => 2.0,
            "ReduceMin" => 3.0,
            "ReduceL2" => 4.0,
            _ => 5.0,
        }
    };
    let count = shape
        .iter()
        .zip(&reduced)
        .filter(|(_, r)| **r)
        .try_fold(1usize, |n, (&d, _)| n.checked_mul(d))?;
    let mut params = vec![17.0, mode, shape.len() as f32, count as f32];
    let oldstrides = strides(shape);
    for i in 0..shape.len() {
        params.extend([
            shape[i] as f32,
            oldstrides[i] as f32,
            u8::from(reduced[i]) as f32,
        ]);
    }
    let keep = global || integer(node, "keepdims", 1) != 0;
    *shape = shape
        .iter()
        .zip(reduced)
        .filter_map(|(&d, r)| if r { keep.then_some(1) } else { Some(d) })
        .collect();
    Some(params)
}

pub(super) fn pool(node: &NodeProto, shape: &mut [usize]) -> Option<Vec<f32>> {
    if shape.len() != 4 || integer(node, "storage_order", 0) != 0 {
        return None;
    }
    let kernel = integers(node, "kernel_shape", &[]);
    let strides = integers(node, "strides", &[1, 1]);
    let dilation = integers(node, "dilations", &[1, 1]);
    let mut pads = integers(node, "pads", &[0, 0, 0, 0]);
    if kernel.len() != 2
        || strides.len() != 2
        || dilation.len() != 2
        || pads.len() != 4
        || kernel
            .iter()
            .chain(&strides)
            .chain(&dilation)
            .any(|&v| v <= 0 || v > 16_777_216)
        || pads.iter().any(|&v| !(0..=16_777_216).contains(&v))
    {
        return None;
    }
    let input = [shape[2], shape[3]];
    let mut output = [0usize; 2];
    for i in 0..2 {
        let n = input[i] as i64;
        let effective = (kernel[i] - 1).checked_mul(dilation[i])?.checked_add(1)?;
        let auto = string(node, "auto_pad", b"NOTSET");
        if auto == b"SAME_UPPER" || auto == b"SAME_LOWER" {
            let out = (n + strides[i] - 1) / strides[i];
            let pad = ((out - 1) * strides[i] + effective - n).max(0);
            pads[i] = if auto == b"SAME_LOWER" {
                (pad + 1) / 2
            } else {
                pad / 2
            };
            pads[i + 2] = pad - pads[i];
            output[i] = out as usize;
        } else {
            if auto == b"VALID" {
                pads[i] = 0;
                pads[i + 2] = 0;
            } else if auto != b"NOTSET" {
                return None;
            }
            let num = n + pads[i] + pads[i + 2] - effective;
            let ceil = integer(node, "ceil_mode", 0) != 0;
            let mut out = (num + if ceil { strides[i] - 1 } else { 0 }).div_euclid(strides[i]) + 1;
            if ceil && (out - 1) * strides[i] >= n + pads[i] {
                out -= 1;
            }
            output[i] = usize::try_from(out).ok().filter(|&v| v > 0)?;
        }
    }
    let params = vec![
        18.0,
        u8::from(node.op_type == "MaxPool") as f32,
        input[1] as f32,
        input[0] as f32,
        output[1] as f32,
        output[0] as f32,
        kernel[1] as f32,
        kernel[0] as f32,
        strides[1] as f32,
        strides[0] as f32,
        dilation[1] as f32,
        dilation[0] as f32,
        pads[1] as f32,
        pads[0] as f32,
        integer(node, "count_include_pad", 0) as f32,
    ];
    shape[2..].copy_from_slice(&output);
    size(shape)?;
    Some(params)
}

pub(super) fn slice(
    node: &NodeProto,
    shape: &mut [usize],
    metadata: &HashMap<String, Vec<i64>>,
) -> Option<Vec<f32>> {
    if shape.len() > 8 {
        return None;
    }
    let get = |slot: usize, key: &str, default: &[i64]| -> Option<Vec<i64>> {
        if let Some(name) = node.input.get(slot).filter(|n| !n.is_empty()) {
            Some(metadata.get(name)?.clone())
        } else {
            Some(integers(node, key, default))
        }
    };
    let starts = get(1, "starts", &[])?;
    let ends = get(2, "ends", &[])?;
    let axes = get(3, "axes", &(0..starts.len() as i64).collect::<Vec<_>>())?;
    let steps = get(4, "steps", &vec![1; starts.len()])?;
    if starts.len() != ends.len() || axes.len() != starts.len() || steps.len() != starts.len() {
        return None;
    }
    let mut chosen = vec![false; shape.len()];
    let mut offsets = vec![0i64; shape.len()];
    let mut increments = vec![1i64; shape.len()];
    let stride = strides(shape);
    for i in 0..starts.len() {
        let axis = axis(axes[i], shape.len())?;
        if chosen[axis] || steps[i] == 0 || steps[i].unsigned_abs() > 16_777_216 {
            return None;
        }
        chosen[axis] = true;
        let dim = shape[axis] as i64;
        let positive = steps[i] > 0;
        let normalize = |n: i64| if n < 0 { n.saturating_add(dim) } else { n };
        let start = normalize(starts[i]).clamp(0, if positive { dim } else { dim - 1 });
        let end = normalize(ends[i]).clamp(if positive { 0 } else { -1 }, dim);
        let delta = if positive { end - start } else { start - end };
        let len = (delta.max(0) + steps[i].abs() - 1) / steps[i].abs();
        if len == 0 {
            return None;
        }
        shape[axis] = len as usize;
        offsets[axis] = start;
        increments[axis] = steps[i];
    }
    let mut params = vec![25.0, shape.len() as f32];
    for i in 0..shape.len() {
        params.extend([
            shape[i] as f32,
            stride[i] as f32,
            offsets[i] as f32,
            increments[i] as f32,
        ]);
    }
    Some(params)
}
