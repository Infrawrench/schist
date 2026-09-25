//! Checked ONNX subset for resident GPU inference. An unsupported node rejects
//! the whole graph at load time; tract remains the reference/fallback runtime.
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};
use std::collections::HashMap;
use tract_onnx::pb::{ModelProto, NodeProto, TensorProto};
use tract_onnx::prelude::*;
use tract_onnx::tract_hir::infer::Factoid;
#[path = "gpu_ops.rs"]
mod ops;
#[path = "gpu_metadata.rs"]
mod shape_metadata;
pub(super) static SHADER: ComputeShader =
    ComputeShader::new("neural-tensor", include_str!("gpu.wgsl"));
#[derive(Clone)]
struct Value {
    source: ComputeSource,
    shape: Vec<usize>,
}
pub(super) struct Network {
    pub program: ComputeProgram,
    pub shapes: Vec<Vec<usize>>,
}
fn size(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .filter(|&n| n > 0 && n <= 16_777_216)
}
fn integers(n: &NodeProto, key: &str, default: &[i64]) -> Vec<i64> {
    n.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.ints.clone())
        .unwrap_or_else(|| default.to_vec())
}
fn integer(n: &NodeProto, key: &str, default: i64) -> i64 {
    n.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.i)
        .unwrap_or(default)
}
fn float(n: &NodeProto, key: &str, default: f32) -> f32 {
    n.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.f)
        .unwrap_or(default)
}
fn tensor(t: &TensorProto) -> Option<Vec<f32>> {
    if t.data_type != 1 || t.data_location.unwrap_or(0) != 0 || !t.external_data.is_empty() {
        return None;
    }
    let shape: Vec<usize> = t
        .dims
        .iter()
        .map(|&d| usize::try_from(d).ok())
        .collect::<Option<_>>()?;
    let n = shape.iter().try_fold(1usize, |n, &d| n.checked_mul(d))?;
    if n > 16_777_216 {
        return None;
    }
    let data = if !t.raw_data.is_empty() {
        if t.raw_data.len() != n * 4 {
            return None;
        }
        t.raw_data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    } else {
        t.float_data.clone()
    };
    (data.len() == n && data.iter().all(|v| v.is_finite())).then_some(data)
}
fn strides(shape: &[usize]) -> Vec<usize> {
    let mut s = vec![1; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        s[i] = s[i + 1] * shape[i + 1];
    }
    s
}
impl Network {
    pub fn compile(
        proto: &ModelProto,
        input_shape: Vec<usize>,
        typed: &InferenceModel,
    ) -> Option<Self> {
        let graph = proto.graph.as_ref()?;
        if graph.output.is_empty() {
            return None;
        }
        let mut p = ComputeProgram {
            buffers: vec![],
            steps: vec![],
            result: ComputeSource::Input(0),
            work: 0,
        };
        let mut values = HashMap::new();
        let mut constants = HashMap::new();
        let mut metadata_shapes = HashMap::<String, Vec<usize>>::new();
        let mut metadata = HashMap::<String, Vec<i64>>::new();
        // Tract already infers and folds the static shape subgraphs for its
        // fallback. Reuse those exact results (including integer Slice sentinels)
        // instead of implementing a second shape interpreter or rounding i64s
        // through float storage on the GPU.
        for node in typed.nodes() {
            for (slot, output) in node.outputs.iter().enumerate() {
                let label = typed
                    .outlet_label(OutletId::new(node.id, slot))
                    .unwrap_or(&node.name);
                let Some(tensor) = output.fact.value.concretize() else {
                    continue;
                };
                if tensor.datum_type() == f32::datum_type() {
                    let data = tensor
                        .to_plain_array_view::<f32>()
                        .ok()?
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();
                    if data.iter().any(|v| !v.is_finite()) {
                        return None;
                    }
                    constants.insert(label.to_owned(), data.clone());
                    p.buffers.push(data);
                    values.insert(
                        label.to_owned(),
                        Value {
                            source: ComputeSource::Input(p.buffers.len()),
                            shape: tensor.shape().to_vec(),
                        },
                    );
                } else if tensor.datum_type() == i64::datum_type()
                    || tensor.datum_type() == i32::datum_type()
                {
                    metadata_shapes.insert(label.to_owned(), tensor.shape().to_vec());
                    let tensor = tensor.cast_to::<i64>().ok()?;
                    metadata.insert(
                        label.to_owned(),
                        tensor
                            .to_plain_array_view::<i64>()
                            .ok()?
                            .iter()
                            .copied()
                            .collect(),
                    );
                }
            }
        }
        for t in &graph.initializer {
            if values.contains_key(&t.name) || metadata.contains_key(&t.name) {
                continue;
            }
            if matches!(t.data_type, 6 | 7) {
                if t.data_location.unwrap_or(0) != 0 || !t.external_data.is_empty() {
                    return None;
                }
                let shape = t
                    .dims
                    .iter()
                    .map(|&d| usize::try_from(d).ok())
                    .collect::<Option<Vec<_>>>()?;
                let count = shape.iter().try_fold(1usize, |n, &d| n.checked_mul(d))?;
                if count > 4096 {
                    return None;
                }
                let data = if !t.raw_data.is_empty() {
                    let width = if t.data_type == 7 { 8 } else { 4 };
                    if t.raw_data.len() != count * width {
                        return None;
                    }
                    if width == 8 {
                        t.raw_data
                            .as_chunks::<8>()
                            .0
                            .iter()
                            .map(|b| i64::from_le_bytes(*b))
                            .collect::<Vec<_>>()
                    } else {
                        t.raw_data
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| i32::from_le_bytes(*b) as i64)
                            .collect::<Vec<_>>()
                    }
                } else if t.data_type == 7 {
                    t.int64_data.clone()
                } else {
                    t.int32_data.iter().map(|&v| v as i64).collect()
                };
                if data.len() != count {
                    return None;
                }
                metadata_shapes.insert(t.name.clone(), shape);
                metadata.insert(t.name.clone(), data);
                continue;
            }
            let data = tensor(t)?;
            let shape = t
                .dims
                .iter()
                .map(|&d| usize::try_from(d).ok())
                .collect::<Option<Vec<_>>>()?;
            constants.insert(t.name.clone(), data.clone());
            p.buffers.push(data);
            values.insert(
                t.name.clone(),
                Value {
                    source: ComputeSource::Input(p.buffers.len()),
                    shape,
                },
            );
        }
        let inputs: Vec<_> = graph
            .input
            .iter()
            .filter(|v| !constants.contains_key(&v.name) && !metadata.contains_key(&v.name))
            .collect();
        if inputs.len() != 1 {
            return None;
        }
        values.insert(
            inputs[0].name.clone(),
            Value {
                source: ComputeSource::Input(0),
                shape: input_shape.clone(),
            },
        );
        for node in &graph.node {
            if node.output.is_empty()
                || !node.domain.is_empty() && node.domain != "ai.onnx"
                || node.output.len() != 1 && node.op_type != "Split"
            {
                return None;
            }
            if values.contains_key(&node.output[0]) || metadata.contains_key(&node.output[0]) {
                continue;
            }
            if shape_metadata::fold(node, &values, &mut metadata, &mut metadata_shapes)? {
                continue;
            }
            if node.op_type == "Cast" && integer(node, "to", 0) == 1 {
                if let Some(data) = metadata.get(node.input.first()?) {
                    if data.iter().any(|v| v.unsigned_abs() > 16_777_216) {
                        return None;
                    }
                    let shape = metadata_shapes.get(&node.input[0])?.clone();
                    let data = data.iter().map(|&v| v as f32).collect::<Vec<_>>();
                    constants.insert(node.output[0].clone(), data.clone());
                    p.buffers.push(data);
                    values.insert(
                        node.output[0].clone(),
                        Value {
                            source: ComputeSource::Input(p.buffers.len()),
                            shape,
                        },
                    );
                    continue;
                }
            }
            if node.op_type == "Cast" && matches!(integer(node, "to", 0), 6 | 7) {
                if let Some(data) = metadata.get(node.input.first()?).cloned() {
                    if integer(node, "to", 0) == 6
                        && data.iter().any(|&v| i32::try_from(v).is_err())
                    {
                        return None;
                    }
                    metadata.insert(node.output[0].clone(), data);
                    continue;
                }
            }
            if node.op_type == "Constant" {
                let t = node
                    .attribute
                    .iter()
                    .find(|a| a.name == "value")?
                    .t
                    .as_ref()?;
                let data = tensor(t)?;
                let shape = t
                    .dims
                    .iter()
                    .map(|&d| usize::try_from(d).ok())
                    .collect::<Option<Vec<_>>>()?;
                constants.insert(node.output[0].clone(), data.clone());
                p.buffers.push(data);
                values.insert(
                    node.output[0].clone(),
                    Value {
                        source: ComputeSource::Input(p.buffers.len()),
                        shape,
                    },
                );
                continue;
            }
            if node.op_type == "Split" {
                let a = values.get(node.input.first()?)?;
                let axis = ops::axis(integer(node, "axis", 0), a.shape.len())?;
                let lengths = if let Some(name) = node.input.get(1).filter(|n| !n.is_empty()) {
                    metadata.get(name)?.clone()
                } else {
                    integers(node, "split", &[])
                };
                let lengths = if lengths.is_empty() {
                    if !a.shape[axis].is_multiple_of(node.output.len()) {
                        return None;
                    }
                    vec![(a.shape[axis] / node.output.len()) as i64; node.output.len()]
                } else {
                    lengths
                };
                if lengths.len() != node.output.len()
                    || lengths.iter().any(|&n| n <= 0)
                    || lengths
                        .iter()
                        .try_fold(0i64, |sum, &n| sum.checked_add(n))?
                        != a.shape[axis] as i64
                {
                    return None;
                }
                let a = a.clone();
                let inner = size(&a.shape[axis + 1..])?;
                let mut start = 0;
                for (name, n) in node.output.iter().zip(lengths) {
                    let mut shape = a.shape.clone();
                    shape[axis] = n as usize;
                    let len = size(&shape)?;
                    let source = p.push(
                        &SHADER,
                        a.source,
                        a.source,
                        vec![
                            24.0,
                            inner as f32,
                            a.shape[axis] as f32,
                            start as f32,
                            n as f32,
                        ],
                        len,
                        [len as u32, 1, 1],
                    );
                    values.insert(name.clone(), Value { source, shape });
                    p.work = p.work.saturating_add(len);
                    start += n;
                }
                continue;
            }
            let a = values.get(node.input.first()?)?.clone();
            let mut shape = a.shape.clone();
            let mut auxiliary = a.source;
            let mut work = size(&shape)?.saturating_mul(4);
            let params = match node.op_type.as_str() {
                "Identity" => {
                    values.insert(node.output[0].clone(), a);
                    continue;
                }
                "Dropout" => {
                    let opset = proto
                        .opset_import
                        .iter()
                        .find(|o| o.domain.is_empty() || o.domain == "ai.onnx")?
                        .version;
                    // Inference Dropout is an identity. Training and mask
                    // outputs keep tract's implementation.
                    if opset < 7 || node.input.get(2).is_some_and(|s| !s.is_empty()) {
                        return None;
                    }
                    values.insert(node.output[0].clone(), a);
                    continue;
                }
                "Conv" | "ConvTranspose" => {
                    let transpose = node.op_type == "ConvTranspose";
                    let weights = values.get(node.input.get(1)?)?;
                    let data = constants.get(node.input.get(1)?)?;
                    if shape.len() != 4 || weights.shape.len() != 4 {
                        return None;
                    }
                    if node
                        .attribute
                        .iter()
                        .any(|a| a.name == "auto_pad" && !a.s.is_empty() && a.s != b"NOTSET")
                    {
                        return None;
                    }
                    let groups = usize::try_from(integer(node, "group", 1)).ok()?;
                    let s = integers(node, "strides", &[1, 1]);
                    let d = integers(node, "dilations", &[1, 1]);
                    let pad = integers(node, "pads", &[0, 0, 0, 0]);
                    if groups == 0
                        || s.len() != 2
                        || d.len() != 2
                        || pad.len() != 4
                        || s.iter().chain(&d).any(|&n| n <= 0)
                        || pad.iter().any(|&n| n < 0)
                        || s.iter().chain(&d).chain(&pad).any(|&n| n > 1_048_576)
                    {
                        return None;
                    }
                    let oc = if transpose {
                        weights.shape[1].checked_mul(groups)?
                    } else {
                        weights.shape[0]
                    };
                    let (kh, kw) = (weights.shape[2], weights.shape[3]);
                    let (ic, ih, iw) = (shape[1], shape[2], shape[3]);
                    if !ic.is_multiple_of(groups)
                        || !oc.is_multiple_of(groups)
                        || if transpose {
                            weights.shape[0] != ic
                        } else {
                            weights.shape[1] != ic / groups
                        }
                    {
                        return None;
                    }
                    let (oh, ow) = if transpose {
                        let extra = integers(node, "output_padding", &[0, 0]);
                        if extra.len() != 2
                            || extra
                                .iter()
                                .enumerate()
                                .any(|(i, &v)| v < 0 || v >= s[i].max(d[i]))
                            || node.attribute.iter().any(|a| a.name == "output_shape")
                        {
                            return None;
                        }
                        let height = (ih as i64 - 1)
                            .checked_mul(s[0])?
                            .checked_add(extra[0])?
                            .checked_add(d[0].checked_mul(kh as i64 - 1)?)?
                            .checked_add(1)?
                            .checked_sub(pad[0])?
                            .checked_sub(pad[2])?;
                        let width = (iw as i64 - 1)
                            .checked_mul(s[1])?
                            .checked_add(extra[1])?
                            .checked_add(d[1].checked_mul(kw as i64 - 1)?)?
                            .checked_add(1)?
                            .checked_sub(pad[1])?
                            .checked_sub(pad[3])?;
                        (height, width)
                    } else {
                        let height = (ih as i64)
                            .checked_add(pad[0])?
                            .checked_add(pad[2])?
                            .checked_sub(d[0].checked_mul(kh as i64 - 1)?)?
                            .checked_sub(1)?;
                        let width = (iw as i64)
                            .checked_add(pad[1])?
                            .checked_add(pad[3])?
                            .checked_sub(d[1].checked_mul(kw as i64 - 1)?)?
                            .checked_sub(1)?;
                        if height < 0 || width < 0 {
                            return None;
                        }
                        (height / s[0] + 1, width / s[1] + 1)
                    };
                    if oh <= 0 || ow <= 0 || oh > 16_777_216 || ow > 16_777_216 {
                        return None;
                    }
                    shape = vec![shape[0], oc, oh as usize, ow as usize];
                    let mut kernel = data.clone();
                    if let Some(name) = node.input.get(2).filter(|n| !n.is_empty()) {
                        let bias = constants.get(name)?;
                        if bias.len() != oc {
                            return None;
                        }
                        kernel.extend(bias);
                    } else {
                        kernel.resize(kernel.len() + oc, 0.0);
                    }
                    p.buffers.push(kernel);
                    auxiliary = ComputeSource::Input(p.buffers.len());
                    work = size(&shape)?
                        .saturating_mul(ic / groups * kh * kw)
                        .saturating_mul(2);
                    vec![
                        if transpose { 16.0 } else { 0.0 },
                        ic as f32,
                        ih as f32,
                        iw as f32,
                        oc as f32,
                        oh as f32,
                        ow as f32,
                        kh as f32,
                        kw as f32,
                        s[0] as f32,
                        s[1] as f32,
                        d[0] as f32,
                        d[1] as f32,
                        pad[0] as f32,
                        pad[1] as f32,
                        groups as f32,
                    ]
                }
                "Reshape" | "Flatten" | "Squeeze" | "Unsqueeze" => {
                    let original_len = size(&shape)?;
                    match node.op_type.as_str() {
                        "Reshape" => {
                            let requested = metadata.get(node.input.get(1)?)?;
                            let mut unknown = None;
                            let mut next = Vec::new();
                            for (i, &dim) in requested.iter().enumerate() {
                                let d = if dim == 0 && integer(node, "allowzero", 0) == 0 {
                                    *shape.get(i)?
                                } else if dim == -1 {
                                    if unknown.replace(i).is_some() {
                                        return None;
                                    }
                                    1
                                } else {
                                    usize::try_from(dim).ok()?
                                };
                                next.push(d);
                            }
                            let known = size(&next)?;
                            if let Some(i) = unknown {
                                if !original_len.is_multiple_of(known) {
                                    return None;
                                }
                                next[i] = original_len / known;
                            }
                            shape = next;
                        }
                        "Flatten" => {
                            let rank = shape.len() as i64;
                            let axis = integer(node, "axis", 1);
                            let axis =
                                usize::try_from(if axis < 0 { axis + rank } else { axis }).ok()?;
                            if axis > shape.len() {
                                return None;
                            }
                            shape = vec![size(&shape[..axis])?, size(&shape[axis..])?];
                        }
                        _ => {
                            let axes =
                                if let Some(name) = node.input.get(1).filter(|n| !n.is_empty()) {
                                    metadata.get(name)?.clone()
                                } else {
                                    integers(node, "axes", &[])
                                };
                            let unsqueeze = node.op_type == "Unsqueeze";
                            let rank = shape.len() + if unsqueeze { axes.len() } else { 0 };
                            let mut selected = vec![false; rank];
                            for axis in axes.iter().copied() {
                                let axis = usize::try_from(if axis < 0 {
                                    axis + rank as i64
                                } else {
                                    axis
                                })
                                .ok()?;
                                if axis >= rank || selected[axis] {
                                    return None;
                                }
                                selected[axis] = true;
                            }
                            if unsqueeze {
                                let mut source = shape.into_iter();
                                shape = selected
                                    .into_iter()
                                    .map(|insert| if insert { Some(1) } else { source.next() })
                                    .collect::<Option<_>>()?;
                            } else {
                                let mut out = Vec::new();
                                for (i, d) in shape.into_iter().enumerate() {
                                    if selected[i] || axes.is_empty() && d == 1 {
                                        if d != 1 {
                                            return None;
                                        }
                                    } else {
                                        out.push(d);
                                    }
                                }
                                shape = out;
                            }
                        }
                    }
                    if size(&shape)? != original_len {
                        return None;
                    }
                    values.insert(
                        node.output[0].clone(),
                        Value {
                            source: a.source,
                            shape,
                        },
                    );
                    continue;
                }
                "Pad" => {
                    if shape.len() > 8 || node.input.get(3).is_some_and(|n| !n.is_empty()) {
                        return None;
                    }
                    let pads = if let Some(name) = node.input.get(1).filter(|n| !n.is_empty()) {
                        metadata.get(name)?.clone()
                    } else {
                        integers(node, "pads", &[])
                    };
                    if pads.len() != 2 * shape.len() {
                        return None;
                    }
                    let mode = node
                        .attribute
                        .iter()
                        .find(|a| a.name == "mode")
                        .map(|a| a.s.as_slice())
                        .unwrap_or(b"constant");
                    let mode = match mode {
                        b"constant" => 0.0,
                        b"edge" => 1.0,
                        b"reflect" => 2.0,
                        _ => return None,
                    };
                    let value = if let Some(name) = node.input.get(2).filter(|n| !n.is_empty()) {
                        *constants.get(name)?.first()?
                    } else {
                        float(node, "value", 0.0)
                    };
                    let rank = shape.len();
                    let old_strides = strides(&shape);
                    let mut params = vec![15.0, rank as f32, mode, value];
                    for k in 0..rank {
                        let dim = (shape[k] as i64)
                            .checked_add(pads[k])?
                            .checked_add(pads[k + rank])?;
                        if !(1..=16_777_216).contains(&dim) || pads[k].unsigned_abs() > 16_777_216 {
                            return None;
                        }
                        params.extend_from_slice(&[
                            dim as f32,
                            shape[k] as f32,
                            old_strides[k] as f32,
                            pads[k] as f32,
                        ]);
                        shape[k] = dim as usize;
                    }
                    params
                }
                "Clip" => {
                    let lo = if let Some(name) = node.input.get(1).filter(|n| !n.is_empty()) {
                        *constants.get(name)?.first()?
                    } else {
                        float(node, "min", f32::MIN)
                    };
                    let hi = if let Some(name) = node.input.get(2).filter(|n| !n.is_empty()) {
                        *constants.get(name)?.first()?
                    } else {
                        float(node, "max", f32::MAX)
                    };
                    if lo > hi {
                        return None;
                    }
                    vec![7.0, lo, hi]
                }
                "Slice" => ops::slice(node, &mut shape, &metadata)?,
                "Gather" => {
                    let axis = ops::axis(integer(node, "axis", 0), shape.len())?;
                    let name = node.input.get(1)?;
                    let indices = metadata.get(name)?;
                    let index_shape = metadata_shapes.get(name)?;
                    let dim = shape[axis] as i64;
                    let mut params = vec![
                        26.0,
                        size(&shape[axis + 1..])? as f32,
                        dim as f32,
                        indices.len() as f32,
                    ];
                    for &index in indices {
                        let index = if index < 0 {
                            index.checked_add(dim)?
                        } else {
                            index
                        };
                        if index < 0 || index >= dim {
                            return None;
                        }
                        params.push(index as f32);
                    }
                    shape.splice(axis..=axis, index_shape.iter().copied());
                    params
                }
                "Expand" => {
                    let requested = metadata.get(node.input.get(1)?)?;
                    let rank = shape.len().max(requested.len());
                    if rank > 8 {
                        return None;
                    }
                    let mut output = vec![1; rank - requested.len()];
                    output.extend(
                        requested
                            .iter()
                            .map(|&n| usize::try_from(n).ok())
                            .collect::<Option<Vec<_>>>()?,
                    );
                    let mut input = vec![1; rank - shape.len()];
                    input.extend(&shape);
                    let stride = strides(&input);
                    let mut params = vec![25.0, rank as f32];
                    for i in 0..rank {
                        if output[i] != input[i] && input[i] != 1 && output[i] != 1 {
                            return None;
                        }
                        output[i] = output[i].max(input[i]);
                        params.extend([
                            output[i] as f32,
                            if input[i] == 1 { 0.0 } else { stride[i] as f32 },
                            0.0,
                            1.0,
                        ]);
                    }
                    shape = output;
                    params
                }
                "InstanceNormalization" => {
                    if shape.len() < 3 || node.input.len() != 3 {
                        return None;
                    }
                    let channels = shape[1];
                    let inner = size(&shape[2..])?;
                    let scale = constants.get(&node.input[1])?;
                    let bias = constants.get(&node.input[2])?;
                    if scale.len() != channels || bias.len() != channels {
                        return None;
                    }
                    let groups = shape[0] * channels;
                    let stats = p.push(
                        &SHADER,
                        a.source,
                        a.source,
                        vec![27.0, inner as f32],
                        groups * 2,
                        [groups as u32, 1, 2],
                    );
                    let mut params = vec![
                        28.0,
                        inner as f32,
                        channels as f32,
                        float(node, "epsilon", 1e-5),
                    ];
                    params.extend(scale);
                    params.extend(bias);
                    let len = size(&shape)?;
                    let source = p.push(&SHADER, a.source, stats, params, len, [len as u32, 1, 1]);
                    p.work = p.work.saturating_add(len.saturating_mul(12));
                    values.insert(node.output[0].clone(), Value { source, shape });
                    continue;
                }
                "Gemm" => {
                    if shape.len() != 2 {
                        return None;
                    }
                    let b = values.get(node.input.get(1)?)?;
                    if b.shape.len() != 2 {
                        return None;
                    }
                    let ta = integer(node, "transA", 0) != 0;
                    let tb = integer(node, "transB", 0) != 0;
                    let (m, k) = if ta {
                        (shape[1], shape[0])
                    } else {
                        (shape[0], shape[1])
                    };
                    let (bk, n) = if tb {
                        (b.shape[1], b.shape[0])
                    } else {
                        (b.shape[0], b.shape[1])
                    };
                    if k != bk {
                        return None;
                    }
                    let mut weights = constants.get(&node.input[1])?.clone();
                    let (rows, cols) =
                        if let Some(name) = node.input.get(2).filter(|n| !n.is_empty()) {
                            let bias = values.get(name)?;
                            if bias.shape.len() > 2 {
                                return None;
                            }
                            let rows = if bias.shape.len() == 2 {
                                bias.shape[0]
                            } else {
                                1
                            };
                            let cols = *bias.shape.last().unwrap_or(&1);
                            if rows != 1 && rows != m || cols != 1 && cols != n {
                                return None;
                            }
                            weights.extend(constants.get(name)?);
                            (rows, cols)
                        } else {
                            weights.push(0.0);
                            (1, 1)
                        };
                    p.buffers.push(weights);
                    auxiliary = ComputeSource::Input(p.buffers.len());
                    shape = vec![m, n];
                    work = m.saturating_mul(n).saturating_mul(k).saturating_mul(2);
                    vec![
                        29.0,
                        m as f32,
                        n as f32,
                        k as f32,
                        u8::from(ta) as f32,
                        u8::from(tb) as f32,
                        float(node, "alpha", 1.0),
                        float(node, "beta", 1.0),
                        rows as f32,
                        cols as f32,
                    ]
                }
                "Relu" => vec![1.0],
                "LeakyRelu" => vec![2.0, float(node, "alpha", 0.01)],
                "Sigmoid" => vec![5.0],
                "Tanh" => vec![6.0],
                "Add" | "Mul" | "Div" | "Sub" | "Pow" | "Min" | "Max" | "PRelu" => {
                    if node.input.len() != 2 {
                        return None;
                    }
                    let b = values.get(node.input.get(1)?)?;
                    auxiliary = b.source;
                    let rank = a.shape.len().max(b.shape.len());
                    if rank > 8 {
                        return None;
                    }
                    let mut aa = vec![1; rank - a.shape.len()];
                    aa.extend(&a.shape);
                    let mut bb = vec![1; rank - b.shape.len()];
                    bb.extend(&b.shape);
                    let ast = strides(&aa);
                    let bst = strides(&bb);
                    shape = Vec::new();
                    let mut params = vec![
                        match node.op_type.as_str() {
                            "Add" => 3.0,
                            "Mul" => 4.0,
                            "Div" => 13.0,
                            "Sub" => 20.0,
                            "Pow" => 21.0,
                            "Min" => 22.0,
                            "PRelu" => 30.0,
                            _ => 23.0,
                        },
                        rank as f32,
                    ];
                    for i in 0..rank {
                        if aa[i] != bb[i] && aa[i] != 1 && bb[i] != 1 {
                            return None;
                        }
                        let dim = aa[i].max(bb[i]);
                        shape.push(dim);
                        params.extend_from_slice(&[
                            dim as f32,
                            if aa[i] == 1 { 0.0 } else { ast[i] as f32 },
                            if bb[i] == 1 { 0.0 } else { bst[i] as f32 },
                        ]);
                    }
                    params
                }
                "Resize" => ops::resize(node, &mut shape, &constants, &metadata, proto)?,
                "Concat" => {
                    let axis = ops::axis(integer(node, "axis", 0), shape.len())?;
                    let inner = size(&shape[axis + 1..])?;
                    let mut result = a.source;
                    for name in &node.input[1..] {
                        let b = values.get(name)?;
                        if shape.len() != b.shape.len()
                            || shape
                                .iter()
                                .zip(&b.shape)
                                .enumerate()
                                .any(|(i, (a, b))| i != axis && a != b)
                        {
                            return None;
                        }
                        let params =
                            vec![12.0, inner as f32, shape[axis] as f32, b.shape[axis] as f32];
                        shape[axis] = shape[axis].checked_add(b.shape[axis])?;
                        let len = size(&shape)?;
                        result = p.push(&SHADER, result, b.source, params, len, [len as u32, 1, 1]);
                        p.work = p.work.saturating_add(len);
                    }
                    values.insert(
                        node.output[0].clone(),
                        Value {
                            source: result,
                            shape,
                        },
                    );
                    continue;
                }
                "Softmax" => {
                    let opset = proto
                        .opset_import
                        .iter()
                        .find(|v| v.domain.is_empty() || v.domain == "ai.onnx")?
                        .version;
                    let rank = shape.len() as i64;
                    let axis = integer(node, "axis", if opset >= 13 { -1 } else { 1 });
                    let axis = usize::try_from(if axis < 0 { axis + rank } else { axis }).ok()?;
                    if axis >= shape.len() || (opset < 13 && axis + 1 != shape.len()) {
                        return None;
                    }
                    work = size(&shape)?.saturating_mul(shape[axis]).saturating_mul(5);
                    vec![14.0, shape[axis] as f32, size(&shape[axis + 1..])? as f32]
                }
                "MatMul" => {
                    let b = values.get(node.input.get(1)?)?;
                    auxiliary = b.source;
                    let params = ops::matmul(&mut shape, &b.shape)?;
                    work = size(&shape)?
                        .saturating_mul(params[2] as usize)
                        .saturating_mul(2);
                    params
                }
                "AveragePool" | "MaxPool" => ops::pool(node, &mut shape)?,
                "ReduceMean" | "ReduceSum" | "ReduceMax" | "ReduceMin" | "ReduceL2"
                | "ReduceSumSquare" | "GlobalAveragePool" | "GlobalMaxPool" => {
                    let params = ops::reduce(node, &mut shape, &metadata)?;
                    work = size(&a.shape)?.saturating_mul(4);
                    params
                }
                "Sqrt" | "Exp" | "Log" | "Abs" | "Neg" | "Reciprocal" | "Erf" | "HardSigmoid"
                | "HardSwish" => {
                    vec![
                        19.0,
                        match node.op_type.as_str() {
                            "Sqrt" => 0.0,
                            "Exp" => 1.0,
                            "Log" => 2.0,
                            "Abs" => 3.0,
                            "Neg" => 4.0,
                            "Reciprocal" => 5.0,
                            "Erf" => 6.0,
                            "HardSigmoid" => 7.0,
                            _ => 8.0,
                        },
                        float(node, "alpha", 0.2),
                        float(node, "beta", 0.5),
                    ]
                }
                "Transpose" => {
                    let rank = shape.len();
                    let default: Vec<i64> = (0..rank as i64).rev().collect();
                    let perm = integers(node, "perm", &default);
                    let mut seen = vec![false; rank];
                    let old = strides(&shape);
                    let mut params = vec![9.0, rank as f32];
                    let mut out = Vec::new();
                    for k in perm {
                        let k = usize::try_from(k).ok()?;
                        if k >= rank || seen[k] {
                            return None;
                        }
                        seen[k] = true;
                        out.push(shape[k]);
                        params.extend_from_slice(&[shape[k] as f32, old[k] as f32]);
                    }
                    if out.len() != rank {
                        return None;
                    }
                    shape = out;
                    params
                }
                "BatchNormalization" => {
                    if integer(node, "training_mode", 0) != 0
                        || shape.len() < 2
                        || node.input.len() != 5
                    {
                        return None;
                    }
                    let c = shape[1];
                    let mut inputs = Vec::new();
                    for name in &node.input[1..] {
                        let v = constants.get(name)?;
                        if v.len() != c {
                            return None;
                        }
                        inputs.push(v);
                    }
                    let epsilon = float(node, "epsilon", 1e-5);
                    let scale: Vec<f32> = (0..c)
                        .map(|i| inputs[0][i] / (inputs[3][i] + epsilon).sqrt())
                        .collect();
                    let mut coefficients = scale.clone();
                    coefficients.extend((0..c).map(|i| inputs[1][i] - inputs[2][i] * scale[i]));
                    p.buffers.push(coefficients);
                    auxiliary = ComputeSource::Input(p.buffers.len());
                    vec![10.0, size(&shape[2..])? as f32, c as f32]
                }
                _ => return None,
            };
            let len = size(&shape)?;
            p.work = p.work.saturating_add(work);
            let source = ComputeSource::Step(p.steps.len());
            p.steps.push(ComputeStep {
                shader: SHADER,
                source: a.source,
                auxiliary,
                params,
                output_len: len,
                invocations: len,
                shape: [len as u32, 1, 1],
            });
            values.insert(node.output[0].clone(), Value { source, shape });
        }
        let output = values.get(&graph.output[0].name)?;
        let mut shapes = vec![output.shape.clone()];
        let mut len = size(&output.shape)?;
        p.result = output.source;
        for output in &graph.output[1..] {
            let value = values.get(&output.name)?;
            let n = size(&value.shape)?;
            let total = len.checked_add(n).filter(|&n| n <= 16_777_216)?;
            p.result = p.push(
                &SHADER,
                p.result,
                value.source,
                vec![12.0, 1.0, len as f32, n as f32],
                total,
                [total as u32, 1, 1],
            );
            len = total;
            shapes.push(value.shape.clone());
        }
        // Remove unused initializers (e.g. weights packed together with bias).
        let mut mapping = HashMap::new();
        let mut buffers = Vec::new();
        let mut remap = |source: &mut ComputeSource| {
            if let ComputeSource::Input(i) = source {
                if *i > 0 {
                    let index = *mapping.entry(*i).or_insert_with(|| {
                        buffers.push(p.buffers[*i - 1].clone());
                        buffers.len()
                    });
                    *i = index;
                }
            }
        };
        for step in &mut p.steps {
            remap(&mut step.source);
            remap(&mut step.auxiliary);
        }
        remap(&mut p.result);
        p.buffers = buffers;
        p.valid(size(&input_shape)?)
            .then_some(Self { program: p, shapes })
    }
}
