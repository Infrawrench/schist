//! Equivalent static layouts for the pinned background models' Core ML graphs.
//! The source ONNX remains untouched. These rewrites run before caching a
//! derived graph; callers must check the original model's SHA-256 first.
use anyhow::{ensure, Context, Result};
use std::collections::{HashMap, HashSet};
use tract_onnx::pb::{
    AttributeProto as Attr, GraphProto, ModelProto, NodeProto as Node, TensorProto as Tensor,
};

#[cfg(feature = "coreml-export")]
pub(super) fn guide(proto: &mut ModelProto) -> Result<()> {
    let graph = proto.graph.as_mut().context("missing guide graph")?;
    let mut nodes = Vec::new();
    let mut count = 0;
    for n in std::mem::take(&mut graph.node) {
        if n.op_type != "HardSwish" {
            nodes.push(n);
            continue;
        }
        ensure!(
            n.domain.is_empty()
                && n.input.len() == 1
                && n.output.len() == 1
                && n.attribute.is_empty(),
            "unsupported HardSwish layout"
        );
        let intermediate = format!("{}/coreml_hard_sigmoid", n.output[0]);
        let float = |name: &str, value| Attr {
            name: name.into(),
            r#type: 1,
            f: value,
            ..Default::default()
        };
        nodes.push(node(
            "HardSigmoid",
            &intermediate,
            n.input.clone(),
            vec![intermediate.clone()],
            vec![float("alpha", 1.0 / 6.0), float("beta", 0.5)],
        ));
        nodes.push(node(
            "Mul",
            &n.name,
            vec![n.input[0].clone(), intermediate],
            n.output,
            vec![],
        ));
        count += 1;
    }
    ensure!(count == 20, "unexpected guide HardSwish count: {count}");
    graph.node = nodes;
    Ok(())
}

fn integer(n: &Node, key: &str, default: i64) -> i64 {
    n.attribute
        .iter()
        .find(|a| a.name == key)
        .map_or(default, |a| a.i)
}
fn ints<'a>(n: &'a Node, key: &str) -> Option<&'a [i64]> {
    n.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.ints.as_slice())
}
fn ai(name: &str, values: &[i64]) -> Attr {
    Attr {
        name: name.into(),
        r#type: 7,
        ints: values.into(),
        ..Default::default()
    }
}
fn av(name: &str, value: i64) -> Attr {
    Attr {
        name: name.into(),
        r#type: 2,
        i: value,
        ..Default::default()
    }
}
fn astr(name: &str, value: &str) -> Attr {
    Attr {
        name: name.into(),
        r#type: 3,
        s: value.as_bytes().into(),
        ..Default::default()
    }
}
fn node(
    kind: &str,
    name: &str,
    input: Vec<String>,
    output: Vec<String>,
    attribute: Vec<Attr>,
) -> Node {
    Node {
        op_type: kind.into(),
        name: name.into(),
        input,
        output,
        attribute,
        ..Default::default()
    }
}
fn count(dims: &[i64]) -> Result<usize> {
    dims.iter().try_fold(1usize, |n, &d| {
        let d = usize::try_from(d)?;
        ensure!(d > 0, "empty native model constant");
        n.checked_mul(d)
            .filter(|&n| n <= 134_217_728)
            .context("native model constant exceeds bound")
    })
}
fn i64s(t: &Tensor) -> Result<Vec<i64>> {
    ensure!(
        t.data_type == 7 && t.external_data.is_empty(),
        "expected embedded int64 constant"
    );
    let values = if t.raw_data.is_empty() {
        t.int64_data.clone()
    } else {
        ensure!(t.raw_data.len().is_multiple_of(8), "invalid int64 bytes");
        t.raw_data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|v| i64::from_le_bytes(*v))
            .collect()
    };
    ensure!(count(&t.dims)? == values.len(), "invalid int64 shape");
    Ok(values)
}
fn float_bytes(t: &Tensor) -> Result<Vec<u8>> {
    ensure!(
        t.data_type == 1 && t.external_data.is_empty(),
        "expected embedded float32 constant"
    );
    let bytes = if t.raw_data.is_empty() {
        t.float_data.iter().flat_map(|v| v.to_le_bytes()).collect()
    } else {
        t.raw_data.clone()
    };
    ensure!(
        count(&t.dims)?.checked_mul(4) == Some(bytes.len()),
        "invalid float32 shape"
    );
    Ok(bytes)
}
fn constants(g: &GraphProto) -> HashMap<String, Tensor> {
    let mut values: HashMap<_, _> = g
        .initializer
        .iter()
        .map(|v| (v.name.clone(), v.clone()))
        .collect();
    for n in &g.node {
        if n.op_type == "Constant" && n.output.len() == 1 {
            if let Some(t) = n
                .attribute
                .iter()
                .find(|a| a.name == "value")
                .and_then(|a| a.t.as_ref())
            {
                values.insert(n.output[0].clone(), t.clone());
            }
        }
    }
    values
}
fn prune(g: &mut GraphProto) {
    let mut live: HashSet<String> = g.output.iter().map(|v| v.name.clone()).collect();
    let mut nodes = Vec::new();
    for n in std::mem::take(&mut g.node).into_iter().rev() {
        if n.output.iter().any(|o| live.contains(o)) {
            live.extend(n.input.iter().cloned());
            nodes.push(n);
        }
    }
    nodes.reverse();
    g.node = nodes;
    g.initializer.retain(|t| live.contains(&t.name));
    g.value_info.clear();
}
struct Builder {
    prefix: String,
    nodes: Vec<Node>,
    constants: Vec<Tensor>,
}
impl Builder {
    fn op(&mut self, kind: &str, name: &str, inputs: Vec<String>, attrs: Vec<Attr>) -> String {
        let out = format!("{}{name}", self.prefix);
        self.nodes
            .push(node(kind, &out, inputs, vec![out.clone()], attrs));
        out
    }
    fn tensor(&mut self, name: &str, mut t: Tensor) -> String {
        t.name = format!("{}{name}", self.prefix);
        let out = t.name.clone();
        self.constants.push(t);
        out
    }
    fn ints(&mut self, name: &str, data: &[i64]) -> String {
        self.tensor(
            name,
            Tensor {
                dims: vec![data.len() as i64],
                data_type: 7,
                int64_data: data.into(),
                ..Default::default()
            },
        )
    }
    fn floats(&mut self, name: &str, data: &[f32]) -> String {
        self.tensor(
            name,
            Tensor {
                dims: vec![data.len() as i64],
                data_type: 1,
                float_data: data.into(),
                ..Default::default()
            },
        )
    }
    fn reshape(&mut self, name: &str, input: String, shape: &[i64]) -> String {
        let shape = self.ints(&format!("{name}/shape"), shape);
        self.op("Reshape", name, vec![input, shape], vec![])
    }
    fn slice(&mut self, name: &str, input: String, axis: i64, index: i64) -> String {
        let start = self.ints(&format!("{name}/start"), &[index]);
        let end = self.ints(&format!("{name}/end"), &[index + 1]);
        let axis = self.ints(&format!("{name}/axis"), &[axis]);
        self.op("Slice", name, vec![input, start, end, axis], vec![])
    }
}

/// Replace the pinned export's expanded bilinear gathers with GridSample and
/// its spatially interleaved convolution with the same filter as a 1×1 product.
/// GridSample uses the original padded image, border extension and float32
/// coordinates. Coordinate normalization changes rounding, not the geometry.
pub(super) fn deformable(proto: &mut ModelProto) -> Result<usize> {
    let g = proto.graph.as_mut().context("missing ONNX graph")?;
    let values = constants(g);
    let producers: HashMap<_, _> = g
        .node
        .iter()
        .flat_map(|n| n.output.iter().map(move |o| (o.clone(), n.clone())))
        .collect();
    let get = |key: &str| values.get(key).context("missing sampling constant");
    let producer = |key: &str, kind: &str| -> Result<&Node> {
        let n = producers.get(key).context("missing sampling producer")?;
        ensure!(
            n.op_type == kind && n.domain.is_empty(),
            "unexpected sampling topology"
        );
        Ok(n)
    };
    let mut result = Vec::new();
    let mut replaced = 0;
    for mut n in std::mem::take(&mut g.node) {
        if n.op_type != "Conv"
            || n.input.len() != 2
            || !n.input[1].contains(".atrous_conv.regular_conv.weight")
        {
            if n.op_type == "Conv"
                && !n
                    .attribute
                    .iter()
                    .any(|a| a.name == "pads" || a.name == "auto_pad")
            {
                n.attribute.push(ai("pads", &[0; 4]));
            }
            result.push(n);
            continue;
        }
        let prefix = n
            .name
            .rsplit_once('/')
            .context("unexpected sampling name")?
            .0;
        let coord = producer(&format!("{prefix}/Add_output_0"), "Add")?;
        ensure!(coord.input.len() == 2, "invalid sample coordinates");
        let reshape = producer(&coord.input[1], "Reshape")?;
        ensure!(reshape.input.len() == 2, "invalid sample reshape");
        let shape = i64s(get(&reshape.input[1])?)?;
        ensure!(
            shape.len() == 6 && shape[..2] == [1, 1] && shape[3] == 2,
            "unsupported offset groups"
        );
        let (k, h, w) = (shape[2], shape[4], shape[5]);
        count(&shape)?;
        let padded = producer(&format!("{prefix}/Reshape_5_output_0"), "Reshape")?;
        ensure!(padded.input.len() == 2, "invalid padded data");
        let ds = i64s(get(&padded.input[1])?)?;
        ensure!(
            ds.len() == 5 && ds[..2] == [1, 1],
            "unsupported sampling data"
        );
        count(&ds)?;
        let (c, ph, pw) = (ds[2], ds[3], ds[4]);
        let weight = get(&n.input[1])?;
        ensure!(
            weight.data_type == 1
                && weight.dims.len() == 4
                && weight.dims[1] == c
                && weight.dims[2] * weight.dims[3] == k,
            "invalid sampling filter"
        );
        ensure!(
            integer(&n, "group", 1) == 1
                && ints(&n, "strides") == Some(&weight.dims[2..])
                && ints(&n, "pads").is_none_or(|p| p == [0; 4]),
            "unsupported sampling convolution"
        );
        ensure!(
            ints(&n, "dilations").is_none_or(|d| d == [1, 1]),
            "unsupported sampling dilation"
        );
        let mask = format!("{prefix}/Mul_output_0");
        producer(&mask, "Mul")?;
        let mut b = Builder {
            prefix: format!("{prefix}/schist-grid/"),
            nodes: vec![],
            constants: vec![],
        };
        let mut base = get(&coord.input[0])?.clone();
        ensure!(base.dims == shape, "sample base shape differs");
        base.dims = vec![1, k * 2, h, w];
        let base = b.tensor("base", base);
        let pos = b.op(
            "Add",
            "position",
            vec![reshape.input[0].clone(), base],
            vec![],
        );
        let pairs = b.reshape("pairs", pos, &[1, k, 2, h * w]);
        let pairs = b.op(
            "Transpose",
            "yx",
            vec![pairs],
            vec![ai("perm", &[0, 1, 3, 2])],
        );
        let x = b.slice("x", pairs.clone(), 3, 1);
        let y = b.slice("y", pairs, 3, 0);
        let xy = b.op("Concat", "xy", vec![x, y], vec![av("axis", 3)]);
        let scale = b.floats(
            "scale",
            &[(2.0 / pw as f64) as f32, (2.0 / ph as f64) as f32],
        );
        let xy = b.op("Mul", "scaled", vec![xy, scale], vec![]);
        let shift = b.floats(
            "shift",
            &[
                (1.0 / pw as f64 - 1.0) as f32,
                (1.0 / ph as f64 - 1.0) as f32,
            ],
        );
        let xy = b.op("Add", "normalized", vec![xy, shift], vec![]);
        let xy = b.reshape("grid", xy, &[1, k * h, w, 2]);
        let sample = b.op(
            "GridSample",
            "sample",
            vec![padded.input[0].clone(), xy],
            vec![
                astr("mode", "bilinear"),
                astr("padding_mode", "border"),
                av("align_corners", 0),
            ],
        );
        let sample = b.reshape("samples", sample, &[1, c, k, h * w]);
        let mask = b.reshape("mask", mask, &[1, 1, k, h * w]);
        let sample = b.op("Mul", "modulated", vec![sample, mask], vec![]);
        let sample = b.reshape("columns", sample, &[1, c * k, h, w]);
        let mut filter = weight.clone();
        filter.dims = vec![weight.dims[0], c * k, 1, 1];
        let filter = b.tensor("weights", filter);
        b.nodes.push(node(
            "Conv",
            &format!("{}conv", b.prefix),
            vec![sample, filter],
            n.output,
            vec![
                ai("kernel_shape", &[1, 1]),
                ai("strides", &[1, 1]),
                ai("pads", &[0; 4]),
            ],
        ));
        result.extend(b.nodes);
        g.initializer.extend(b.constants);
        replaced += 1;
    }
    ensure!(
        replaced == 20,
        "expected 20 pinned deformable convolutions, found {replaced}"
    );
    g.node = result;
    prune(g);
    Ok(replaced)
}

fn gather_constant(data: &Tensor, indices: &Tensor, axis: i64, name: &str) -> Result<Tensor> {
    ensure!(
        axis == 0 && data.dims.len() == 2,
        "unsupported constant gather"
    );
    let input = float_bytes(data)?;
    let indices_data = i64s(indices)?;
    let rows = data.dims[0];
    let stride = usize::try_from(data.dims[1])?
        .checked_mul(4)
        .context("gather stride overflow")?;
    let mut bytes = Vec::with_capacity(
        indices_data
            .len()
            .checked_mul(stride)
            .context("gather output overflow")?,
    );
    for i in indices_data {
        let i = if i < 0 { i + rows } else { i };
        ensure!((0..rows).contains(&i), "constant gather index out of range");
        bytes.extend_from_slice(&input[i as usize * stride..(i as usize + 1) * stride]);
    }
    let mut dims = indices.dims.clone();
    dims.push(data.dims[1]);
    Ok(Tensor {
        name: name.into(),
        dims,
        data_type: 1,
        raw_data: bytes,
        ..Default::default()
    })
}
fn transpose_relative(t: &Tensor, name: &str) -> Result<Tensor> {
    ensure!(t.dims.len() == 3, "invalid relative position filter");
    let data = float_bytes(t)?;
    let [h, k, c] = t.dims.as_slice() else {
        unreachable!()
    };
    let (h, k, c) = (*h as usize, *k as usize, *c as usize);
    let mut bytes = vec![0; data.len()];
    for y in 0..h {
        for z in 0..c {
            for x in 0..k {
                let dst = (y * c * k + z * k + x) * 4;
                let src = (y * k * c + x * c + z) * 4;
                bytes[dst..dst + 4].copy_from_slice(&data[src..src + 4]);
            }
        }
    }
    Ok(Tensor {
        name: name.into(),
        dims: vec![h as i64, c as i64, k as i64],
        data_type: 1,
        raw_data: bytes,
        ..Default::default()
    })
}

/// Runs after ONNX Runtime's BASIC static folding, without approximate math.
/// The bounded graph rewrites retain float32 filters and native resolution.
pub(super) fn compact(proto: &mut ModelProto, id: &str) -> Result<()> {
    let (expected_gathers, expected_products, expected_slices, expected_windows) = match id {
        "detail-matting" => (24, 24, 0, 16),
        "foreground-matting" => (0, 0, 72, 53),
        "foreground" => (0, 0, 73, 48),
        _ => anyhow::bail!("unsupported compact graph"),
    };
    let g = proto.graph.as_mut().context("missing ONNX graph")?;
    let mut values = constants(g);
    let mut nodes = Vec::new();
    let mut gathered = 0;
    let mut products = 0;
    let mut slices = 0;
    for n in std::mem::take(&mut g.node) {
        if n.op_type == "Gather" && n.input.len() == 2 {
            let index = values
                .get(&n.input[1])
                .context("nonconstant gather index")?;
            if let Some(data) = values.get(&n.input[0]) {
                let t = gather_constant(data, index, integer(&n, "axis", 0), &n.output[0])?;
                g.initializer.push(t.clone());
                values.insert(n.output[0].clone(), t);
                gathered += 1;
                continue;
            }
            let iv = i64s(index)?;
            ensure!(
                index.dims.is_empty() && iv.len() == 1 && (0..3).contains(&iv[0]),
                "unsupported scalar gather"
            );
            let axis = integer(&n, "axis", 0);
            ensure!((0..5).contains(&axis), "invalid gather axis");
            let mut b = Builder {
                prefix: format!("{}/schist/", n.name),
                nodes: vec![],
                constants: vec![],
            };
            let slice = b.slice("slice", n.input[0].clone(), axis, iv[0]);
            let axes = b.ints("axes", &[axis]);
            b.nodes.push(node(
                "Squeeze",
                &format!("{}squeeze", b.prefix),
                vec![slice, axes],
                n.output,
                vec![],
            ));
            nodes.extend(b.nodes);
            g.initializer.extend(b.constants);
            slices += 1;
            continue;
        }
        if n.op_type == "Einsum" {
            ensure!(
                n.input.len() == 2 && n.output.len() == 1,
                "invalid relative attention"
            );
            let eq = n
                .attribute
                .iter()
                .find(|a| a.name == "equation")
                .context("missing equation")?;
            ensure!(
                eq.s == b"bhwc,hkc->bhwk" || eq.s == b"bhwc,wkc->bhwk",
                "unsupported relative attention"
            );
            let mut b = Builder {
                prefix: format!("{}/schist/", n.name),
                nodes: vec![],
                constants: vec![],
            };
            let weight = transpose_relative(
                values
                    .get(&n.input[1])
                    .context("relative weights not folded")?,
                "weights",
            )?;
            let weight = b.tensor("weights", weight);
            if eq.s == b"bhwc,wkc->bhwk" {
                let a = b.op(
                    "Transpose",
                    "a",
                    vec![n.input[0].clone()],
                    vec![ai("perm", &[0, 2, 1, 3])],
                );
                let product = b.op("MatMul", "product", vec![a, weight], vec![]);
                b.nodes.push(node(
                    "Transpose",
                    &format!("{}out", b.prefix),
                    vec![product],
                    n.output,
                    vec![ai("perm", &[0, 2, 1, 3])],
                ));
            } else {
                b.nodes.push(node(
                    "MatMul",
                    &format!("{}product", b.prefix),
                    vec![n.input[0].clone(), weight],
                    n.output,
                    vec![],
                ));
            }
            nodes.extend(b.nodes);
            g.initializer.extend(b.constants);
            products += 1;
            continue;
        }
        nodes.push(n);
    }
    ensure!(
        (gathered, products, slices) == (expected_gathers, expected_products, expected_slices),
        "pinned attention layout changed: {gathered} constant gathers, {products} products, {slices} scalar gathers"
    );
    let mut consumers: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        for input in &n.input {
            consumers.entry(input.clone()).or_default().push(i);
        }
    }
    let mut changed = 0;
    for i in 0..nodes.len() {
        let n = &nodes[i];
        if n.op_type != "Reshape" || n.input.len() != 2 {
            continue;
        }
        let Some(t) = values.get(&n.input[1]) else {
            continue;
        };
        let dims = i64s(t)?;
        if dims.len() != 6 {
            continue;
        }
        ensure!(dims[0] == 1, "non-singleton six-axis batch");
        let following = consumers.get(&n.output[0]).context("missing transpose")?;
        ensure!(following.len() == 1, "shared six-axis tensor");
        let j = following[0];
        let trans = &nodes[j];
        ensure!(
            trans.op_type == "Transpose",
            "six-axis tensor is not transposed"
        );
        let perm = ints(trans, "perm").context("missing permutation")?;
        ensure!(
            perm.len() == 6 && perm[0] == 0,
            "six-axis transpose moves batch"
        );
        let after = consumers
            .get(&trans.output[0])
            .context("missing output reshape")?;
        ensure!(
            after.len() == 1 && nodes[after[0]].op_type == "Reshape",
            "transpose is shared"
        );
        let perm: Vec<i64> = perm[1..].iter().map(|v| v - 1).collect();
        let name = format!("{}/schist-rank5", n.name);
        g.initializer.push(Tensor {
            name: name.clone(),
            dims: vec![5],
            data_type: 7,
            int64_data: dims[1..].into(),
            ..Default::default()
        });
        nodes[i].input[1] = name;
        let attr = nodes[j]
            .attribute
            .iter_mut()
            .find(|a| a.name == "perm")
            .context("missing permutation")?;
        attr.ints = perm;
        changed += 1;
    }
    ensure!(
        changed == expected_windows,
        "unexpected window layouts: {changed}"
    );
    g.node = nodes;
    prune(g);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_preserves_float_bits_and_negative_multidimensional_indices() {
        let words = [0x8000_0000u32, 0x7fc0_1234, 0x3f80_0000, 0xff80_0000, 7, 9];
        let data = Tensor {
            dims: vec![3, 2],
            data_type: 1,
            raw_data: words.iter().flat_map(|v| v.to_le_bytes()).collect(),
            ..Default::default()
        };
        let indices = Tensor {
            dims: vec![2, 2],
            data_type: 7,
            int64_data: vec![-1, 0, -3, 1],
            ..Default::default()
        };
        let result = gather_constant(&data, &indices, 0, "out").unwrap();
        assert_eq!(result.dims, [2, 2, 2]);
        let expected = [
            7, 9, words[0], words[1], words[0], words[1], words[2], words[3],
        ];
        assert_eq!(
            result.raw_data,
            expected
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
        for index in [-4, 3, i64::MIN, i64::MAX] {
            let bad = Tensor {
                dims: vec![],
                int64_data: vec![index],
                ..indices.clone()
            };
            assert!(gather_constant(&data, &bad, 0, "out").is_err());
        }
    }

    #[test]
    fn relative_weights_transpose_last_axes_without_changing_bits() {
        let data = Tensor {
            dims: vec![2, 3, 4],
            data_type: 1,
            raw_data: (0..24u32).flat_map(|v| v.to_le_bytes()).collect(),
            ..Default::default()
        };
        let result = transpose_relative(&data, "out").unwrap();
        assert_eq!(result.dims, [2, 4, 3]);
        let expected = [
            0u32, 4, 8, 1, 5, 9, 2, 6, 10, 3, 7, 11, 12, 16, 20, 13, 17, 21, 14, 18, 22, 15, 19, 23,
        ];
        assert_eq!(
            result.raw_data,
            expected
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn malformed_constants_and_unrecognized_layouts_are_rejected() {
        for dims in [
            vec![-1],
            vec![0],
            vec![i64::MAX, i64::MAX],
            vec![134_217_729],
        ] {
            assert!(count(&dims).is_err());
        }
        let tensor = Tensor {
            dims: vec![2],
            data_type: 1,
            raw_data: vec![0; 4],
            ..Default::default()
        };
        assert!(float_bytes(&tensor).is_err());
        assert!(i64s(&tensor).is_err());
        let mut graph = ModelProto {
            graph: Some(GraphProto::default()),
            ..Default::default()
        };
        assert!(compact(&mut graph.clone(), "detail-matting").is_err());
        assert!(compact(&mut graph.clone(), "foreground-matting").is_err());
        assert!(deformable(&mut graph).is_err());
    }

    #[test]
    fn pruning_retains_shared_live_inputs_and_removes_dead_weights() {
        let mut graph = GraphProto {
            node: vec![
                node(
                    "Add",
                    "sum",
                    vec!["input".into(), "weight".into()],
                    vec!["sum".into()],
                    vec![],
                ),
                node(
                    "Mul",
                    "dead",
                    vec!["input".into(), "unused".into()],
                    vec!["dead".into()],
                    vec![],
                ),
                node(
                    "Add",
                    "out",
                    vec!["sum".into(), "weight".into()],
                    vec!["out".into()],
                    vec![],
                ),
            ],
            initializer: ["weight", "unused"]
                .into_iter()
                .map(|name| Tensor {
                    name: name.into(),
                    ..Default::default()
                })
                .collect(),
            output: vec![tract_onnx::pb::ValueInfoProto {
                name: "out".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        prune(&mut graph);
        assert_eq!(
            graph
                .node
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["sum", "out"]
        );
        assert_eq!(graph.initializer.len(), 1);
        assert_eq!(graph.initializer[0].name, "weight");
    }
}
