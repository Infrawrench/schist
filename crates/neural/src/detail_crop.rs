//! Compute only the detail decoder's consumed center; keep the full transformer.
//! This rewrite is exclusive to the pinned export and preserves float32 weights.
use anyhow::{bail, ensure, Context, Result};
use std::collections::HashMap;
use tract_onnx::pb::{self, NodeProto, TensorProto};

type Region = [i64; 4]; // top, left, bottom (exclusive), right (exclusive)
const SOURCE: &str = "b6240e8404b30bd94c1e84498a03949b7d2e7e891bed85ed06ac1f8ae1d1dc58";
const IMAGE: &str = "/Concat_output_0";
const FEATURES: &str = "/network/backbone/encoder/layer.11/residual/Add_output_0";

pub(super) fn requested(id: &str) -> bool {
    id == "detail-matting"
        && crate::execution::adaptive_enabled()
        && std::env::var_os("SCHIST_NEURAL_LEGACY_CROP").is_none()
}

pub(super) fn matches(bytes: &[u8]) -> bool {
    crate::sha256_hex(bytes) == SOURCE
}

fn ints(node: &NodeProto, key: &str) -> Result<Vec<i64>> {
    node.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.ints.clone())
        .with_context(|| format!("missing decoder {key}"))
}
fn int(node: &NodeProto, key: &str) -> Result<i64> {
    node.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.i)
        .with_context(|| format!("missing decoder {key}"))
}
fn string<'a>(node: &'a NodeProto, key: &str) -> Option<&'a [u8]> {
    node.attribute
        .iter()
        .find(|a| a.name == key)
        .map(|a| a.s.as_slice())
}

struct Crop {
    nodes: HashMap<String, NodeProto>,
    shapes: HashMap<String, [i64; 2]>,
    regions: HashMap<String, Region>,
    built: HashMap<String, String>,
    output: Vec<NodeProto>,
    constants: Vec<TensorProto>,
}
impl Crop {
    fn clipped(&self, value: &str, r: Region) -> Result<Region> {
        let &[h, w] = self.shapes.get(value).context("unknown decoder shape")?;
        let r = [r[0].max(0), r[1].max(0), r[2].min(h), r[3].min(w)];
        ensure!(r[0] < r[2] && r[1] < r[3], "empty decoder region");
        Ok(r)
    }
    fn conv_bounds(node: &NodeProto, r: Region) -> Result<Region> {
        let s = ints(node, "strides")?;
        let p = ints(node, "pads")?;
        let k = ints(node, "kernel_shape")?;
        ensure!(
            s.len() == 2 && p.len() == 4 && k.len() == 2,
            "invalid decoder convolution"
        );
        Ok([
            r[0] * s[0] - p[0],
            r[1] * s[1] - p[1],
            (r[2] - 1) * s[0] - p[0] + k[0],
            (r[3] - 1) * s[1] - p[1] + k[1],
        ])
    }
    fn dependencies(&self, value: &str, r: Region) -> Result<Vec<(String, Region)>> {
        let Some(node) = self.nodes.get(value) else {
            return Ok(Vec::new());
        };
        let sources = match node.op_type.as_str() {
            "Relu" | "Sigmoid" => vec![(node.input[0].clone(), r)],
            "Concat" => node.input.iter().map(|v| (v.clone(), r)).collect(),
            "Conv" => vec![(
                node.input[0].clone(),
                self.clipped(&node.input[0], Self::conv_bounds(node, r)?)?,
            )],
            "Resize" => {
                let bounds = [
                    (r[0] - 1).div_euclid(2),
                    (r[1] - 1).div_euclid(2),
                    r[2] / 2 + 1,
                    r[3] / 2 + 1,
                ];
                vec![(node.input[0].clone(), self.clipped(&node.input[0], bounds)?)]
            }
            _ => bail!("unsupported decoder operation"),
        };
        Ok(sources)
    }
    fn need(&mut self, value: &str, mut r: Region) -> Result<()> {
        if let Some(old) = self.regions.get(value) {
            r = [
                r[0].min(old[0]),
                r[1].min(old[1]),
                r[2].max(old[2]),
                r[3].max(old[3]),
            ];
            if &r == old {
                return Ok(());
            }
        }
        self.regions.insert(value.into(), r);
        for (source, r) in self.dependencies(value, r)? {
            self.need(&source, r)?;
        }
        Ok(())
    }
    fn slice(&mut self, source: String, r: Region, extent: Region) -> Result<String> {
        ensure!(
            r[0] >= extent[0] && r[1] >= extent[1] && r[2] <= extent[2] && r[3] <= extent[3],
            "decoder slice out of bounds"
        );
        if r == extent {
            return Ok(source);
        }
        let name = format!("schist-center-slice-{}", self.output.len());
        let mut inputs = vec![source];
        for (suffix, data) in [
            ("starts", vec![r[0] - extent[0], r[1] - extent[1]]),
            ("ends", vec![r[2] - extent[0], r[3] - extent[1]]),
            ("axes", vec![2, 3]),
        ] {
            let name = format!("{name}-{suffix}");
            self.constants.push(TensorProto {
                name: name.clone(),
                dims: vec![2],
                data_type: 7,
                int64_data: data,
                ..Default::default()
            });
            inputs.push(name);
        }
        self.output.push(NodeProto {
            name: name.clone(),
            op_type: "Slice".into(),
            input: inputs,
            output: vec![name.clone()],
            ..Default::default()
        });
        Ok(name)
    }
    fn build(&mut self, value: &str, requested: Region) -> Result<String> {
        let r = *self.regions.get(value).context("missing decoder region")?;
        let source = if let Some(source) = self.built.get(value) {
            source.clone()
        } else {
            let source = if let Some(mut node) = self.nodes.get(value).cloned() {
                let dependencies = self.dependencies(value, r)?;
                for (i, (source, required)) in dependencies.iter().enumerate() {
                    node.input[i] = self.build(source, *required)?;
                }
                if node.op_type == "Conv" {
                    let raw = Self::conv_bounds(&node, r)?;
                    let bounded = dependencies[0].1;
                    node.attribute
                        .iter_mut()
                        .find(|a| a.name == "pads")
                        .unwrap()
                        .ints = vec![
                        bounded[0] - raw[0],
                        bounded[1] - raw[1],
                        raw[2] - bounded[2],
                        raw[3] - bounded[3],
                    ];
                }
                node.name = format!("schist-center-{}", node.name);
                let name = format!("schist-center-{value}");
                node.output = vec![name.clone()];
                let resize = node.op_type == "Resize";
                self.output.push(node);
                if resize {
                    let d = dependencies[0].1;
                    self.slice(name, r, [d[0] * 2, d[1] * 2, d[2] * 2, d[3] * 2])?
                } else {
                    name
                }
            } else {
                let [h, w] = self.shapes[value];
                self.slice(value.into(), r, [0, 0, h, w])?
            };
            self.built.insert(value.into(), source.clone());
            source
        };
        self.slice(source, requested, r)
    }
}

/// Call only after checking the original source hash. Validate the expected
/// local topology as well, failing closed if an exporter changes its sampling.
pub(super) fn optimize(proto: &mut pb::ModelProto) -> Result<()> {
    let graph = proto.graph.as_mut().context("missing detail graph")?;
    ensure!(
        graph.output.len() == 1 && graph.output[0].name == "alpha",
        "unexpected detail output"
    );
    let mut crop = Crop {
        nodes: HashMap::new(),
        shapes: HashMap::from([(IMAGE.into(), [768, 768]), (FEATURES.into(), [48, 48])]),
        regions: HashMap::new(),
        built: HashMap::new(),
        output: Vec::new(),
        constants: Vec::new(),
    };
    for node in &graph.node {
        if !node.name.starts_with("/network/decoder/") || node.op_type == "Constant" {
            continue;
        }
        ensure!(
            node.domain.is_empty() && node.output.len() == 1 && !node.input.is_empty(),
            "unexpected detail node"
        );
        let input = *crop
            .shapes
            .get(&node.input[0])
            .context("decoder not topologically ordered")?;
        let shape = match node.op_type.as_str() {
            "Relu" | "Sigmoid" => {
                ensure!(node.input.len() == 1, "invalid decoder activation");
                input
            }
            "Concat" => {
                ensure!(
                    int(node, "axis")? == 1
                        && node
                            .input
                            .iter()
                            .all(|v| crop.shapes.get(v) == Some(&input)),
                    "invalid decoder concatenation"
                );
                input
            }
            "Conv" => {
                ensure!(
                    node.input.len() == 3
                        && int(node, "group")? == 1
                        && ints(node, "dilations")? == [1, 1],
                    "invalid decoder convolution"
                );
                let s = ints(node, "strides")?;
                let k = ints(node, "kernel_shape")?;
                let p = ints(node, "pads")?;
                ensure!(
                    (s == [1, 1] || s == [2, 2])
                        && ((k == [3, 3] && p == [1, 1, 1, 1])
                            || (k == [1, 1] && p == [0, 0, 0, 0])),
                    "unsupported decoder kernel"
                );
                [
                    (input[0] + p[0] + p[2] - k[0]) / s[0] + 1,
                    (input[1] + p[1] + p[3] - k[1]) / s[1] + 1,
                ]
            }
            "Resize" => {
                ensure!(
                    node.input.len() == 3
                        && node.input[1].is_empty()
                        && string(node, "mode") == Some(b"linear")
                        && string(node, "coordinate_transformation_mode") == Some(b"half_pixel"),
                    "unsupported decoder sampling"
                );
                let constant = graph
                    .node
                    .iter()
                    .find(|n| n.output == [node.input[2].clone()] && n.op_type == "Constant")
                    .context("missing resize scales")?;
                let scales = constant
                    .attribute
                    .iter()
                    .find_map(|a| a.t.as_ref())
                    .context("missing scales tensor")?;
                ensure!(
                    scales.data_type == 1
                        && scales.dims == [4]
                        && scales.raw_data
                            == [1f32, 1., 2., 2.]
                                .iter()
                                .flat_map(|v| v.to_le_bytes())
                                .collect::<Vec<_>>(),
                    "unsupported decoder scales"
                );
                [input[0] * 2, input[1] * 2]
            }
            _ => bail!("unsupported decoder operation {}", node.op_type),
        };
        crop.shapes.insert(node.output[0].clone(), shape);
        crop.nodes.insert(node.output[0].clone(), node.clone());
    }
    ensure!(
        crop.nodes.len() == 26 && crop.shapes.get("alpha") == Some(&[768, 768]),
        "pinned decoder topology changed"
    );
    crop.need("alpha", [128, 128, 640, 640])?;
    ensure!(crop.regions.len() == 28, "pinned decoder branches changed");
    let output = crop.build("alpha", [128, 128, 640, 640])?;
    // Nothing outside the decoder may consume one of its replaced values.
    ensure!(
        graph
            .node
            .iter()
            .filter(|n| !n.output.iter().any(|v| crop.nodes.contains_key(v)))
            .all(|n| !n.input.iter().any(|v| crop.nodes.contains_key(v))),
        "shared decoder output"
    );
    graph
        .node
        .retain(|n| !n.output.iter().any(|v| crop.nodes.contains_key(v)));
    graph.node.extend(crop.output);
    graph.initializer.extend(crop.constants);
    graph.node.push(NodeProto {
        name: "schist-center-alpha".into(),
        op_type: "Identity".into(),
        input: vec![output],
        output: vec!["alpha".into()],
        ..Default::default()
    });
    graph.value_info.clear();
    let tensor = graph.output[0]
        .r#type
        .as_mut()
        .and_then(|t| t.value.as_mut())
        .context("missing output type")?;
    let pb::type_proto::Value::TensorType(tensor) = tensor;
    let shape = tensor.shape.as_mut().context("missing output shape")?;
    ensure!(shape.dim.len() == 4, "unexpected output rank");
    for dim in &mut shape.dim[2..] {
        dim.value = Some(pb::tensor_shape_proto::dimension::Value::DimValue(512));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Instant};
    use tract_linalg::multithread::{multithread_tract_scope, Executor};
    use tract_onnx::prelude::*;

    fn source() -> pb::ModelProto {
        let raw = crate::decode_model_bytes(crate::DETAIL_MATTING_ONNX_XZ).unwrap();
        assert!(matches(&raw));
        tract_onnx::onnx()
            .proto_model_for_read(&mut std::io::Cursor::new(raw))
            .unwrap()
    }
    fn decoder(mut proto: pb::ModelProto) -> pb::ModelProto {
        let graph = proto.graph.as_mut().unwrap();
        graph.node.retain(|n| {
            n.name.starts_with("/network/decoder/") || n.name.starts_with("schist-center-")
        });
        graph.input = [(IMAGE, [1, 4, 768, 768]), (FEATURES, [1, 384, 48, 48])]
            .into_iter()
            .map(|(name, dims)| pb::ValueInfoProto {
                name: name.into(),
                r#type: Some(pb::TypeProto {
                    value: Some(pb::type_proto::Value::TensorType(pb::type_proto::Tensor {
                        elem_type: 1,
                        shape: Some(pb::TensorShapeProto {
                            dim: dims
                                .into_iter()
                                .map(|n| pb::tensor_shape_proto::Dimension {
                                    value: Some(
                                        pb::tensor_shape_proto::dimension::Value::DimValue(n),
                                    ),
                                    ..Default::default()
                                })
                                .collect(),
                        }),
                    })),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect();
        let used: std::collections::HashSet<_> = graph
            .node
            .iter()
            .flat_map(|n| n.input.iter())
            .cloned()
            .collect();
        graph.initializer.retain(|v| used.contains(&v.name));
        graph.value_info.clear();
        proto
    }
    fn plan(proto: &pb::ModelProto) -> Arc<TypedSimplePlan> {
        let mut model = tract_onnx::onnx()
            .model_for_proto_model(proto)
            .unwrap()
            .into_optimized()
            .unwrap();
        crate::tensor_layout::optimize(&mut model).unwrap();
        crate::pad_copy::optimize(&mut model).unwrap();
        crate::resize_copy::optimize(&mut model).unwrap();
        model.into_runnable().unwrap()
    }
    fn inputs(case: usize) -> TVec<TValue> {
        [vec![1, 4, 768, 768], vec![1, 384, 48, 48]]
            .into_iter()
            .map(|shape| {
                let data: Vec<f32> = (0..shape.iter().product())
                    .map(|i| match case {
                        0 => {
                            (((i as u64 * 1664525 + 1013904223) % 65536) as f32 / 65535. - 0.5) * 2.
                        }
                        1 => (i % shape[3]) as f32 / shape[3] as f32,
                        _ => ((i % 7) as f32 - 3.) * 0.25,
                    })
                    .collect();
                Tensor::from_shape(&shape, &data).unwrap().into()
            })
            .collect()
    }

    #[test]
    fn crop_preserves_backbone_and_all_weight_bytes() {
        let mut proto = source();
        let graph = proto.graph.as_ref().unwrap();
        let backbone: Vec<_> = graph
            .node
            .iter()
            .filter(|n| !n.name.starts_with("/network/decoder/"))
            .cloned()
            .collect();
        let weights = graph.initializer.clone();
        optimize(&mut proto).unwrap();
        let graph = proto.graph.as_ref().unwrap();
        assert_eq!(&graph.node[..backbone.len()], backbone.as_slice());
        assert_eq!(&graph.initializer[..weights.len()], weights.as_slice());
        assert_eq!(graph.output[0].name, "alpha");
        assert!(optimize(&mut proto).is_err());
    }

    #[test]
    fn crop_rejects_unfamiliar_sampling_without_modifying_source() {
        let mut proto = source();
        let node = proto
            .graph
            .as_mut()
            .unwrap()
            .node
            .iter_mut()
            .find(|n| n.name.starts_with("/network/decoder/") && n.op_type == "Resize")
            .unwrap();
        node.attribute
            .iter_mut()
            .find(|a| a.name == "coordinate_transformation_mode")
            .unwrap()
            .s = b"align_corners".to_vec();
        let original = proto.clone();
        assert!(optimize(&mut proto).is_err());
        assert_eq!(proto, original);
    }

    #[test]
    #[ignore = "production decoder numerical parity and CPU timing; make profile-background-portable"]
    fn profile_portable_detail_decoder() {
        let original = source();
        let mut cropped = original.clone();
        optimize(&mut cropped).unwrap();
        let full = plan(&decoder(original));
        let cropped = plan(&decoder(cropped));
        let workers: Vec<_> = [1, 2, 4]
            .into_iter()
            .map(|n| {
                (
                    n,
                    if n == 1 {
                        Executor::SingleThread
                    } else {
                        Executor::MultiThread(Arc::new(
                            rayon::ThreadPoolBuilder::new()
                                .num_threads(n)
                                .build()
                                .unwrap(),
                        ))
                    },
                )
            })
            .collect();
        for case in 0..3 {
            let input = inputs(case);
            let mut reference = Vec::new();
            let mut previous = None;
            for (threads, executor) in &workers {
                for (label, plan) in [("full", &full), ("center", &cropped)] {
                    let start = Instant::now();
                    let output =
                        multithread_tract_scope(executor.clone(), || plan.run(input.clone()))
                            .unwrap();
                    let seconds = start.elapsed().as_secs_f64();
                    let view = output[0].to_plain_array_view::<f32>().unwrap();
                    let values = view.as_slice().unwrap();
                    let center: Vec<_> = if label == "full" {
                        (128..640)
                            .flat_map(|y| values[y * 768 + 128..y * 768 + 640].iter().copied())
                            .collect()
                    } else {
                        values.to_vec()
                    };
                    assert_eq!(center.len(), 512 * 512);
                    if reference.is_empty() {
                        reference = center.clone();
                    }
                    let max = reference
                        .iter()
                        .zip(&center)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0f32, f32::max);
                    let mean = reference
                        .iter()
                        .zip(&center)
                        .map(|(a, b)| (a - b).abs() as f64)
                        .sum::<f64>()
                        / center.len() as f64;
                    assert!(
                        center.iter().all(|v| v.is_finite()) && max < 0.0005,
                        "decoder mismatch: {max}"
                    );
                    if label == "center" {
                        if let Some(previous) = &previous {
                            assert_eq!(&center, previous, "thread count changed arithmetic");
                        }
                        previous = Some(center);
                    }
                    eprintln!("portable decoder case={case} threads={threads} {label}: {seconds:.3}s max={max:.9} mean={mean:.9}");
                }
            }
        }
    }
}
