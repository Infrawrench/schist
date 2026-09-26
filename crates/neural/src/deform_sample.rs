//! Fuse the four-corner sampling expansion emitted by deform_conv2d_onnx_exporter.
//! Preserve its coefficients, summation order, modulation and convolution layout,
//! without materializing four channel-expanded gathers and their transposes.
use std::collections::{HashMap, HashSet};
use tract_onnx::{
    pb,
    tract_hir::{infer::*, internal::*},
};

struct Graph<'a> {
    nodes: HashMap<&'a str, &'a pb::NodeProto>,
    constants: HashMap<&'a str, &'a pb::TensorProto>,
}
impl<'a> Graph<'a> {
    fn node(&self, name: &str, kind: &str) -> Option<&'a pb::NodeProto> {
        let n = *self.nodes.get(name)?;
        (n.op_type == kind && (n.domain.is_empty() || n.domain == "ai.onnx")).then_some(n)
    }
    fn shape(&self, name: &str) -> Option<Vec<usize>> {
        let t = self.constants.get(name)?;
        if t.data_type != 7 || t.dims.len() != 1 {
            return None;
        }
        let values = if t.raw_data.is_empty() {
            t.int64_data.clone()
        } else {
            if !t.raw_data.len().is_multiple_of(8) {
                return None;
            }
            t.raw_data
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| i64::from_le_bytes(*c))
                .collect()
        };
        if values.len() != usize::try_from(t.dims[0]).ok()? {
            return None;
        }
        values
            .into_iter()
            .map(|v| usize::try_from(v).ok().filter(|v| *v > 0))
            .collect()
    }
    fn reshape(&self, name: &str) -> Option<(&'a str, Vec<usize>)> {
        let n = self.node(name, "Reshape")?;
        if n.input.len() != 2
            || n.attribute
                .iter()
                .any(|a| a.name == "allowzero" && a.i != 0)
        {
            return None;
        }
        Some((&n.input[0], self.shape(&n.input[1])?))
    }
    fn transpose(&self, name: &str, perm: &[i64]) -> Option<&'a str> {
        let n = self.node(name, "Transpose")?;
        (n.input.len() == 1
            && n.attribute
                .iter()
                .any(|a| a.name == "perm" && a.ints == perm))
        .then(|| n.input[0].as_str())
    }

    fn sampling(&self, output: &str) -> Option<(Sample, Vec<String>)> {
        let (transposed, final_shape) = self.reshape(output)?;
        let packed = self.transpose(transposed, &[0, 1, 4, 2, 5, 3])?;
        let (modulated, packed_shape) = self.reshape(packed)?;
        if packed_shape.len() != 6 {
            return None;
        }
        let modulation = self.node(modulated, "Mul")?;
        if modulation.input.len() != 2 {
            return None;
        }
        let sum = self.node(&modulation.input[0], "Sum")?;
        if sum.input.len() != 4 {
            return None;
        }
        let (_, mask_shape) = self.reshape(&modulation.input[1])?;
        let mut data_name = None;
        let mut sample = None;
        let mut indices = Vec::new();
        let mut weights = Vec::new();
        for term in &sum.input {
            let product = self.node(term, "Mul")?;
            if product.input.len() != 2 {
                return None;
            }
            let (_, weight_shape) = self.reshape(&product.input[0])?;
            let (transposed, sampled_shape) = self.reshape(&product.input[1])?;
            let gathered = self.transpose(transposed, &[0, 1, 3, 2])?;
            let gather = self.node(gathered, "GatherND")?;
            if gather.input.len() != 2
                || !gather
                    .attribute
                    .iter()
                    .any(|a| a.name == "batch_dims" && a.i == 2)
            {
                return None;
            }
            let data = self.transpose(&gather.input[0], &[0, 1, 3, 4, 2])?;
            let (_, data_shape) = self.reshape(data)?;
            let op = Sample::new(
                data_shape,
                sampled_shape,
                [packed_shape[2], packed_shape[3]],
            )?;
            if weight_shape != op.weights()
                || mask_shape != op.weights()
                || packed_shape != op.packed()
                || final_shape != op.output()
                || data_name.is_some_and(|name| name != data)
                || sample.as_ref().is_some_and(|previous| previous != &op)
            {
                return None;
            }
            data_name = Some(data);
            sample = Some(op);
            indices.push(gather.input[1].clone());
            weights.push(product.input[0].clone());
        }
        let mut inputs = vec![data_name?.to_owned()];
        inputs.extend(indices);
        inputs.extend(weights);
        inputs.push(modulation.input[1].clone());
        Some((sample?, inputs))
    }
}

/// Match topology and concrete shapes, never exporter node names. Unrecognized
/// graphs keep their original operators. The on-disk ONNX remains standard.
pub(super) fn optimize(proto: &mut pb::ModelProto) -> usize {
    let Some(graph) = &mut proto.graph else {
        return 0;
    };
    let mut constants: HashMap<_, _> = graph
        .initializer
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    for n in &graph.node {
        if n.op_type == "Constant"
            && n.output.len() == 1
            && (n.domain.is_empty() || n.domain == "ai.onnx")
        {
            if let Some(t) = n
                .attribute
                .iter()
                .find(|a| a.name == "value")
                .and_then(|a| a.t.as_ref())
            {
                constants.insert(n.output[0].as_str(), t);
            }
        }
    }
    let view = Graph {
        nodes: graph
            .node
            .iter()
            .flat_map(|n| n.output.iter().map(move |o| (o.as_str(), n)))
            .collect(),
        constants,
    };
    let replacements: Vec<_> = graph
        .node
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            if n.output.len() != 1 {
                return None;
            }
            let (op, input) = view.sampling(&n.output[0])?;
            let ints = |name: &str, values: &[usize]| pb::AttributeProto {
                name: name.into(),
                r#type: 7,
                ints: values.iter().map(|v| *v as i64).collect(),
                ..Default::default()
            };
            Some((
                i,
                pb::NodeProto {
                    input,
                    output: n.output.clone(),
                    name: format!("{}.sample", n.name),
                    op_type: "SchistDeformSample".into(),
                    attribute: vec![
                        ints("data", &op.data),
                        ints("sample", &op.sample),
                        ints("kernel", &op.kernel),
                    ],
                    ..Default::default()
                },
            ))
        })
        .collect();
    let count = replacements.len();
    for (i, node) in replacements {
        graph.node[i] = node;
    }
    if count > 0 {
        // Drop the now-unreachable expanded gathers before inference/declutter.
        let mut needed: HashSet<String> = graph.output.iter().map(|o| o.name.clone()).collect();
        let mut keep = vec![false; graph.node.len()];
        for (i, node) in graph.node.iter().enumerate().rev() {
            if node.output.iter().any(|o| needed.contains(o)) {
                keep[i] = true;
                needed.extend(node.input.iter().cloned());
            }
        }
        let mut i = 0;
        graph.node.retain(|_| {
            let retain = keep[i];
            i += 1;
            retain
        });
    }
    count
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Sample {
    data: Vec<usize>,   // N, groups, channels/group, padded H, padded W
    sample: Vec<usize>, // N, groups, channels/group, kernel area, output H, output W
    kernel: [usize; 2],
}
impl Sample {
    fn new(data: Vec<usize>, sample: Vec<usize>, kernel: [usize; 2]) -> Option<Self> {
        let volume = |v: &[usize]| {
            v.iter()
                .try_fold(1usize, |a, &b| if b > 0 { a.checked_mul(b) } else { None })
        };
        // Validate every product before shape construction or indexing.
        if data.len() != 5
            || sample.len() != 6
            || data[..3] != sample[..3]
            || volume(&data).is_none()
            || volume(&sample).is_none()
            || kernel[0].checked_mul(kernel[1]) != Some(sample[3])
            || volume(&[data[0], data[1], sample[3], sample[4], sample[5], 2]).is_none()
        {
            return None;
        }
        Some(Self {
            data,
            sample,
            kernel,
        })
    }
    fn weights(&self) -> Vec<usize> {
        let mut s = self.sample.clone();
        s[2] = 1;
        s
    }
    fn indices(&self) -> Vec<usize> {
        vec![
            self.data[0],
            self.data[1],
            self.sample[3] * self.sample[4] * self.sample[5],
            2,
        ]
    }
    fn packed(&self) -> Vec<usize> {
        vec![
            self.data[0],
            self.data[1] * self.data[2],
            self.kernel[0],
            self.kernel[1],
            self.sample[4],
            self.sample[5],
        ]
    }
    fn output(&self) -> Vec<usize> {
        vec![
            self.data[0],
            self.data[1] * self.data[2],
            self.sample[4] * self.kernel[0],
            self.sample[5] * self.kernel[1],
        ]
    }
    fn facts(&self) -> Vec<TypedFact> {
        let mut facts = vec![f32::fact(&self.data)];
        facts.extend((0..4).map(|_| i64::fact(self.indices())));
        facts.extend((0..5).map(|_| f32::fact(self.weights())));
        facts
    }
}

pub(super) fn register(onnx: &mut tract_onnx::Onnx) {
    onnx.op_register.insert("SchistDeformSample", |_, n| {
        let shape = |name| -> TractResult<Vec<usize>> {
            n.attribute
                .iter()
                .find(|a| a.name == name)
                .context("missing sampling shape")?
                .ints
                .iter()
                .map(|v| Ok(usize::try_from(*v)?))
                .collect()
        };
        let kernel: [usize; 2] = shape("kernel")?
            .try_into()
            .map_err(|_| anyhow!("invalid sampling kernel"))?;
        let op = Sample::new(shape("data")?, shape("sample")?, kernel)
            .context("invalid sampling dimensions")?;
        Ok((expand(op), vec![]))
    });
}
impl Expansion for Sample {
    fn name(&self) -> StaticName {
        "DeformSample".into()
    }
    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(inputs, 10)?;
        check_output_arity(outputs, 1)?;
        for (input, fact) in inputs.iter().zip(self.facts()) {
            // The ONNX importer can represent int64 indices as TDim.
            if fact.datum_type == f32::datum_type() {
                s.equals(&input.datum_type, f32::datum_type())?;
            }
            s.equals(&input.shape, fact.shape.to_tvec())?;
        }
        s.equals(&outputs[0].datum_type, f32::datum_type())?;
        s.equals(
            &outputs[0].shape,
            self.output()
                .into_iter()
                .map(|v| v.to_dim())
                .collect::<TVec<_>>(),
        )?;
        Ok(())
    }
    fn wire(
        &self,
        prefix: &str,
        model: &mut TypedModel,
        inputs: &[OutletId],
    ) -> TractResult<TVec<OutletId>> {
        model.wire_node(prefix, FusedSample(self.clone()), inputs)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct FusedSample(Sample);
impl Op for FusedSample {
    fn name(&self) -> StaticName {
        "DeformSample".into()
    }
    op_as_typed_op!();
}
impl TypedOp for FusedSample {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(inputs.len() == 10, "invalid sampling inputs");
        for (actual, expected) in inputs.iter().zip(self.0.facts()) {
            ensure!(
                actual.shape == expected.shape,
                "sampling input shape changed"
            );
            ensure!(
                actual.datum_type == expected.datum_type
                    || (expected.datum_type == DatumType::I64
                        && matches!(actual.datum_type, DatumType::I32 | DatumType::TDim)),
                "invalid sampling input type"
            );
        }
        Ok(tvec!(f32::fact(self.0.output())))
    }
    as_op!();
}
impl EvalOp for FusedSample {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let op = &self.0;
        ensure!(inputs.len() == 10, "invalid sampling inputs");
        for (input, expected) in inputs.iter().zip(op.facts()) {
            ensure!(
                input.shape() == expected.shape.as_concrete().unwrap(),
                "sampling input shape changed"
            );
        }
        let data = inputs[0].to_plain_array_view::<f32>()?;
        let data = data.as_slice().context("non-contiguous sampling data")?;
        let indices = inputs[1..5]
            .iter()
            .map(|t| t.cast_to::<i64>())
            .collect::<TractResult<Vec<_>>>()?;
        let index_views = indices
            .iter()
            .map(|t| t.to_plain_array_view::<i64>())
            .collect::<TractResult<Vec<_>>>()?;
        let weights = inputs[5..]
            .iter()
            .map(|t| t.to_plain_array_view::<f32>())
            .collect::<TractResult<Vec<_>>>()?;
        let weights = weights
            .iter()
            .map(|v| v.as_slice().context("non-contiguous sampling weights"))
            .collect::<TractResult<Vec<_>>>()?;
        let ih = op.data[3];
        let iw = op.data[4];
        let channels = op.data[2];
        let oh = op.sample[4];
        let ow = op.sample[5];
        let [kh, kw] = op.kernel;
        let tuples = op.sample[3] * oh * ow;
        let indices = index_views
            .iter()
            .map(|v| v.as_slice().context("non-contiguous sampling indices"))
            .collect::<TractResult<Vec<_>>>()?;
        let normalize = |v: i64, size: usize| -> TractResult<usize> {
            let size = i64::try_from(size)?;
            let v = if v < 0 { v + size } else { v };
            ensure!((0..size).contains(&v), "sampling index out of bounds");
            Ok(v as usize)
        };
        struct Pixel {
            offsets: [usize; 4],
            weights: [f32; 5],
        }
        const BLOCK: usize = 1024;
        let mut pixels = Vec::with_capacity(BLOCK);
        let mut output = Tensor::zero::<f32>(&op.output())?;
        {
            let mut view = output.to_plain_array_view_mut::<f32>()?;
            let out = view.as_slice_mut().unwrap();
            for bg in 0..op.data[0] * op.data[1] {
                for first in (0..tuples).step_by(BLOCK) {
                    let end = first.saturating_add(BLOCK).min(tuples);
                    pixels.clear();
                    // Put sampling metadata in final convolution order once,
                    // then reuse this bounded cache-sized block for all channels.
                    for i in first..end {
                        let dy = i / (ow * kw);
                        let dx = i % (ow * kw);
                        let (y, ky) = (dy / kh, dy % kh);
                        let (x, kx) = (dx / kw, dx % kw);
                        let t = bg * tuples + ((ky * kw + kx) * oh + y) * ow + x;
                        let mut offsets = [0; 4];
                        for corner in 0..4 {
                            offsets[corner] = normalize(indices[corner][t * 2], ih)? * iw
                                + normalize(indices[corner][t * 2 + 1], iw)?;
                        }
                        pixels.push(Pixel {
                            offsets,
                            weights: std::array::from_fn(|c| weights[c][t]),
                        });
                    }
                    for c in 0..channels {
                        let source = &data[(bg * channels + c) * ih * iw..][..ih * iw];
                        let dst = &mut out[(bg * channels + c) * tuples + first..][..end - first];
                        for (pixel, out) in pixels.iter().zip(dst) {
                            let w = pixel.weights;
                            let i = pixel.offsets;
                            let value = ((w[0] * source[i[0]] + w[1] * source[i[1]])
                                + w[2] * source[i[2]])
                                + w[3] * source[i[3]];
                            *out = value * w[4];
                        }
                    }
                }
            }
        }
        Ok(tvec!(output.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tract_onnx::tract_core::ops::array::GatherNd;

    fn input(op: &Sample, negative: bool) -> TractResult<TVec<TValue>> {
        let data = (0..op.data.iter().product())
            .map(|i| (i as f32 * 0.37).sin())
            .collect::<Vec<_>>();
        let mut values = tvec!(Tensor::from_shape(&op.data, &data)?.into());
        for corner in 0..4 {
            let indices = (0..op.indices().iter().product::<usize>())
                .map(|i| {
                    let size = op.data[3 + i % 2] as i64;
                    let index = (i as i64 * 13 + corner * 7) % size;
                    if negative {
                        index - size
                    } else {
                        index
                    }
                })
                .collect::<Vec<_>>();
            values.push(Tensor::from_shape(&op.indices(), &indices)?.into());
        }
        for corner in 0..5 {
            let weights = (0..op.weights().iter().product())
                .map(|i| ((i + corner * 19) % 97) as f32 / 97.)
                .collect::<Vec<_>>();
            values.push(Tensor::from_shape(&op.weights(), &weights)?.into());
        }
        Ok(values)
    }

    #[test]
    fn fused_sampling_matches_expanded_gathers_modulation_and_layout() -> TractResult<()> {
        for kernel in [[1, 1], [3, 2], [7, 7]] {
            for (batch, groups, channels) in [(1, 1, 1), (2, 2, 3)] {
                let op = Sample::new(
                    vec![batch, groups, channels, 7, 9],
                    vec![batch, groups, channels, kernel[0] * kernel[1], 5, 6],
                    kernel,
                )
                .unwrap();
                for negative in [false, true] {
                    let inputs = input(&op, negative)?;
                    let positive = input(&op, false)?;
                    let data = inputs[0]
                        .clone()
                        .into_tensor()
                        .permute_axes(&[0, 1, 3, 4, 2])?;
                    let mut sum = vec![0.; op.sample.iter().product()];
                    let tuples = op.sample[3] * op.sample[4] * op.sample[5];
                    for corner in 0..4 {
                        let gathered = GatherNd::new(2)
                            .eval(tvec!(data.clone().into(), positive[1 + corner].clone()))?;
                        let gathered = gathered[0]
                            .clone()
                            .into_tensor()
                            .permute_axes(&[0, 1, 3, 2])?;
                        let view = gathered.to_plain_array_view::<f32>()?;
                        let coeff = inputs[5 + corner].to_plain_array_view::<f32>()?;
                        for (i, (&v, dst)) in view.iter().zip(&mut sum).enumerate() {
                            let weight_index = (i / (channels * tuples)) * tuples + i % tuples;
                            let value = coeff.as_slice().unwrap()[weight_index] * v;
                            if corner == 0 {
                                *dst = value;
                            } else {
                                *dst += value;
                            }
                        }
                    }
                    let mask = inputs[9].to_plain_array_view::<f32>()?;
                    for (i, v) in sum.iter_mut().enumerate() {
                        *v *= mask.as_slice().unwrap()
                            [(i / (channels * tuples)) * tuples + i % tuples];
                    }
                    let expected = Tensor::from_shape(&op.packed(), &sum)?
                        .permute_axes(&[0, 1, 4, 2, 5, 3])?;
                    let actual = FusedSample(op.clone()).eval(inputs)?;
                    assert_eq!(actual[0].as_bytes(), expected.as_bytes());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn fused_sampling_rejects_overflow_wrong_shapes_and_invalid_indices() -> TractResult<()> {
        assert!(
            Sample::new(vec![1, 1, 1, usize::MAX, 2], vec![1, 1, 1, 1, 2, 2], [1, 1]).is_none()
        );
        assert!(Sample::new(vec![1, 1, 1, 2, 2], vec![1, 1, 1, 3, 2, 2], [2, 2]).is_none());
        let op = Sample::new(vec![1, 1, 1, 2, 2], vec![1, 1, 1, 1, 1, 1], [1, 1]).unwrap();
        for index in [i64::MIN, -3, 2, 1 << 32, i64::MAX] {
            let mut inputs = input(&op, false)?;
            inputs[1] = Tensor::from_shape(&op.indices(), &[index, 0])?.into();
            assert!(FusedSample(op.clone()).eval(inputs).is_err());
        }
        let mut inputs = input(&op, false)?;
        inputs[9] = tensor1(&[0f32, 1.]).into();
        assert!(FusedSample(op).eval(inputs).is_err());
        Ok(())
    }

    fn fixture() -> pb::ModelProto {
        let op = Sample::new(vec![1, 2, 3, 7, 9], vec![1, 2, 3, 9, 5, 6], [3, 3]).unwrap();
        let mut graph = pb::GraphProto::default();
        let shape = |graph: &mut pb::GraphProto, name: &str, dims: Vec<usize>| {
            graph.initializer.push(pb::TensorProto {
                name: name.into(),
                data_type: 7,
                dims: vec![dims.len() as i64],
                int64_data: dims.into_iter().map(|v| v as i64).collect(),
                ..Default::default()
            });
        };
        shape(&mut graph, "ds", op.data.clone());
        shape(&mut graph, "ss", op.sample.clone());
        shape(&mut graph, "ws", op.weights());
        shape(&mut graph, "ps", op.packed());
        shape(&mut graph, "os", op.output());
        let mut node =
            |kind: &str, input: Vec<String>, output: String, attr: Vec<pb::AttributeProto>| {
                graph.node.push(pb::NodeProto {
                    op_type: kind.into(),
                    input,
                    output: vec![output],
                    attribute: attr,
                    ..Default::default()
                });
            };
        let perm = |p: &[i64]| {
            vec![pb::AttributeProto {
                name: "perm".into(),
                r#type: 7,
                ints: p.to_vec(),
                ..Default::default()
            }]
        };
        node(
            "Reshape",
            vec!["source".into(), "ds".into()],
            "data".into(),
            vec![],
        );
        node(
            "Transpose",
            vec!["data".into()],
            "nhwc".into(),
            perm(&[0, 1, 3, 4, 2]),
        );
        for i in 0..4 {
            node(
                "GatherND",
                vec!["nhwc".into(), format!("indices{i}")],
                format!("g{i}"),
                vec![pb::AttributeProto {
                    name: "batch_dims".into(),
                    r#type: 2,
                    i: 2,
                    ..Default::default()
                }],
            );
            node(
                "Transpose",
                vec![format!("g{i}")],
                format!("t{i}"),
                perm(&[0, 1, 3, 2]),
            );
            node(
                "Reshape",
                vec![format!("t{i}"), "ss".into()],
                format!("s{i}"),
                vec![],
            );
            node(
                "Reshape",
                vec![format!("weight{i}"), "ws".into()],
                format!("w{i}"),
                vec![],
            );
            node(
                "Mul",
                vec![format!("w{i}"), format!("s{i}")],
                format!("m{i}"),
                vec![],
            );
        }
        node(
            "Sum",
            (0..4).map(|i| format!("m{i}")).collect(),
            "sum".into(),
            vec![],
        );
        node(
            "Reshape",
            vec!["mask".into(), "ws".into()],
            "modulation".into(),
            vec![],
        );
        node(
            "Mul",
            vec!["sum".into(), "modulation".into()],
            "modulated".into(),
            vec![],
        );
        node(
            "Reshape",
            vec!["modulated".into(), "ps".into()],
            "packed".into(),
            vec![],
        );
        node(
            "Transpose",
            vec!["packed".into()],
            "conv_layout".into(),
            perm(&[0, 1, 4, 2, 5, 3]),
        );
        node(
            "Reshape",
            vec!["conv_layout".into(), "os".into()],
            "output".into(),
            vec![],
        );
        graph.output.push(pb::ValueInfoProto {
            name: "output".into(),
            ..Default::default()
        });
        pb::ModelProto {
            graph: Some(graph),
            ..Default::default()
        }
    }

    #[test]
    fn graph_fusion_checks_topology_and_preserves_shared_consumers() {
        let mut proto = fixture();
        proto
            .graph
            .as_mut()
            .unwrap()
            .output
            .push(pb::ValueInfoProto {
                name: "g0".into(),
                ..Default::default()
            });
        assert_eq!(optimize(&mut proto), 1);
        let graph = proto.graph.unwrap();
        assert_eq!(
            graph
                .node
                .iter()
                .filter(|n| n.op_type == "GatherND")
                .count(),
            1
        );
        let fused = graph
            .node
            .iter()
            .find(|n| n.op_type == "SchistDeformSample")
            .unwrap();
        assert_eq!(fused.input.len(), 10);
        for corruption in ["Transpose", "GatherND", "Reshape"] {
            let mut proto = fixture();
            let node = proto
                .graph
                .as_mut()
                .unwrap()
                .node
                .iter_mut()
                .find(|n| n.op_type == corruption)
                .unwrap();
            node.domain = "unrelated.vendor".into();
            assert_eq!(optimize(&mut proto), 0);
        }
    }
}
