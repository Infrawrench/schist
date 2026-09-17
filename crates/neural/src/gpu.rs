//! Checked ONNX subset for resident GPU inference. An unsupported node rejects
//! the whole graph at load time; tract remains the reference/fallback runtime.
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};
use std::collections::HashMap;
use tract_onnx::pb::{ModelProto, NodeProto, TensorProto};
static SHADER: ComputeShader = ComputeShader {
    name: "neural-tensor",
    source: include_str!("gpu.wgsl"),
};
#[derive(Clone)]
struct Value {
    source: ComputeSource,
    shape: Vec<usize>,
}
pub(super) struct Network {
    pub program: ComputeProgram,
    pub shape: Vec<usize>,
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
    pub fn compile(proto: &ModelProto, input_shape: Vec<usize>) -> Option<Self> {
        let graph = proto.graph.as_ref()?;
        if graph.output.len() != 1 {
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
        for t in &graph.initializer {
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
            .filter(|v| !constants.contains_key(&v.name))
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
            if !node.domain.is_empty() && node.domain != "ai.onnx" || node.output.len() != 1 {
                return None;
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
            let a = values.get(node.input.first()?)?.clone();
            let mut shape = a.shape.clone();
            let mut auxiliary = a.source;
            let mut work = size(&shape)?.saturating_mul(4);
            let params = match node.op_type.as_str() {
                "Identity" => {
                    values.insert(node.output[0].clone(), a);
                    continue;
                }
                "Conv" => {
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
                    let (oc, kh, kw) = (weights.shape[0], weights.shape[2], weights.shape[3]);
                    let (ic, ih, iw) = (shape[1], shape[2], shape[3]);
                    if !ic.is_multiple_of(groups)
                        || !oc.is_multiple_of(groups)
                        || weights.shape[1] != ic / groups
                    {
                        return None;
                    }
                    // Bound shader coordinates and require a nonnegative window span before division.
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
                    if height < 0 || width < 0 || height > 16_777_216 || width > 16_777_216 {
                        return None;
                    }
                    let oh = height / s[0] + 1;
                    let ow = width / s[1] + 1;
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
                        0.0,
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
                "Relu" => vec![1.0],
                "LeakyRelu" => vec![2.0, float(node, "alpha", 0.01)],
                "Sigmoid" => vec![5.0],
                "Tanh" => vec![6.0],
                "Add" | "Mul" | "Div" => {
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
                            _ => 13.0,
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
                "Resize" => {
                    // Checked nearest-neighbor subset used by the bundled encoder/decoder models.
                    let string = |key: &str, default: &[u8]| {
                        node.attribute
                            .iter()
                            .find(|a| a.name == key)
                            .map(|a| a.s.clone())
                            .unwrap_or_else(|| default.to_vec())
                    };
                    if shape.len() != 4
                        || node.input.len() < 3
                        || node.input.get(3).is_some_and(|n| !n.is_empty())
                        || string("mode", b"nearest") != b"nearest"
                        || string("coordinate_transformation_mode", b"half_pixel") != b"asymmetric"
                        || string("nearest_mode", b"round_prefer_floor") != b"floor"
                        || node
                            .attribute
                            .iter()
                            .any(|a| a.name == "axes" || a.name == "antialias" && a.i != 0)
                    {
                        return None;
                    }
                    let scales = constants.get(&node.input[2])?;
                    if scales.len() != 4
                        || scales[0] != 1.0
                        || scales[1] != 1.0
                        || scales.iter().any(|&v| v <= 0.0 || !v.is_finite())
                    {
                        return None;
                    }
                    let (ih, iw) = (shape[2], shape[3]);
                    let oh = (ih as f32 * scales[2]).floor();
                    let ow = (iw as f32 * scales[3]).floor();
                    if oh < 1.0 || ow < 1.0 || oh > 16_777_216.0 || ow > 16_777_216.0 {
                        return None;
                    }
                    shape[2] = oh as usize;
                    shape[3] = ow as usize;
                    vec![11.0, iw as f32, ih as f32, ow, oh, scales[3], scales[2]]
                }
                "Concat" => {
                    if node.input.len() != 2 {
                        return None;
                    }
                    let b = values.get(&node.input[1])?;
                    let rank = shape.len() as i64;
                    let axis = integer(node, "axis", 0);
                    let axis = usize::try_from(if axis < 0 { axis + rank } else { axis }).ok()?;
                    if axis >= shape.len()
                        || shape.len() != b.shape.len()
                        || shape
                            .iter()
                            .zip(&b.shape)
                            .enumerate()
                            .any(|(i, (a, b))| i != axis && a != b)
                    {
                        return None;
                    }
                    auxiliary = b.source;
                    let inner = size(&shape[axis + 1..])?;
                    let params = vec![12.0, inner as f32, shape[axis] as f32, b.shape[axis] as f32];
                    shape[axis] = shape[axis].checked_add(b.shape[axis])?;
                    params
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
                    if shape.len() != 2 || b.shape.len() != 2 || shape[1] != b.shape[0] {
                        return None;
                    }
                    auxiliary = b.source;
                    let params = vec![8.0, shape[0] as f32, shape[1] as f32, b.shape[1] as f32];
                    work = shape[0]
                        .saturating_mul(shape[1])
                        .saturating_mul(b.shape[1])
                        .saturating_mul(2);
                    shape[1] = b.shape[1];
                    params
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
                shader: &SHADER,
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
        p.result = output.source;
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
        p.valid(size(&input_shape)?).then_some(Self {
            program: p,
            shape: output.shape.clone(),
        })
    }
}
