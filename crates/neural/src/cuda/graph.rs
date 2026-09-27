//! Lower tract's concrete, constant-folded graph before CPU kernel packing.
//! Unsupported operators reject the whole graph, never force hidden readbacks.
use anyhow::{bail, ensure, Context, Result};
use std::collections::HashMap;
use tract_onnx::prelude::*;
use tract_onnx::tract_core::{
    internal::*,
    ops::{
        array,
        binary::TypedBinOp,
        cast::Cast,
        change_axes::AxisOp,
        cnn::{Conv, KernelFormat},
        einsum::EinSum,
        element_wise::ElementWiseOp,
        nn::{DataFormat, Reduce, Reducer, Softmax},
    },
};

#[derive(Debug)]
pub(super) struct Step {
    pub inputs: Vec<usize>,
    pub output: usize,
    pub params: Vec<u32>,
    pub kernel: usize,
    pub blocks: u32,
}
#[derive(Debug)]
pub(super) struct Graph {
    pub constants: HashMap<usize, Vec<u32>>,
    pub lengths: Vec<usize>,
    pub steps: Vec<Step>,
    pub input: usize,
    pub input_shape: Vec<usize>,
    pub outputs: Vec<(usize, Vec<usize>)>,
}
fn count(shape: &[usize]) -> Result<usize> {
    let len = shape
        .iter()
        .try_fold(1usize, |a, &b| a.checked_mul(b))
        .context("CUDA tensor size overflow")?;
    ensure!(
        len > 0 && len <= 268_435_456,
        "CUDA tensor exceeds 1 GiB limit"
    );
    Ok(len)
}
fn strides(shape: &[usize]) -> Vec<usize> {
    let mut out = vec![1; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        out[i] = out[i + 1] * shape[i + 1];
    }
    out
}
fn shape(fact: &TypedFact) -> Result<Vec<usize>> {
    let shape = fact
        .shape
        .as_concrete()
        .context("CUDA requires static dimensions")?
        .to_vec();
    ensure!(shape.len() <= 8, "CUDA tensor rank exceeds 8");
    count(&shape)?;
    Ok(shape)
}
fn constant(t: &Tensor) -> Result<Vec<u32>> {
    // Integer shape subgraphs have already run in tract using exact integers.
    // Only constants consumed by device operations are converted to float.
    if t.datum_type() != DatumType::F32 {
        let ints = t.cast_to::<i64>()?;
        ensure!(
            ints.to_plain_array_view::<i64>()?
                .iter()
                .all(|v| v.unsigned_abs() <= 16_777_216),
            "CUDA integer constant is not exactly representable"
        );
    }
    let tensor = t.cast_to::<f32>()?;
    let values = tensor.to_plain_array_view::<f32>()?;
    ensure!(
        values.iter().all(|v| v.is_finite()),
        "nonfinite CUDA constant"
    );
    Ok(values.iter().map(|v| v.to_bits()).collect())
}
impl Graph {
    pub fn compile(model: &TypedModel) -> Result<Self> {
        ensure!(model.input_outlets()?.len() == 1, "CUDA expects one input");
        let inlet = model.input_outlets()?[0];
        ensure!(
            model.outlet_fact(inlet)?.datum_type == DatumType::F32,
            "CUDA input must be float32"
        );
        let input_shape = shape(model.outlet_fact(inlet)?)?;
        let mut g = Self {
            constants: HashMap::new(),
            lengths: vec![count(&input_shape)?],
            steps: vec![],
            input: 0,
            input_shape,
            outputs: vec![],
        };
        let mut values = HashMap::from([(inlet, 0)]);
        let mut folded = HashMap::<OutletId, Arc<Tensor>>::new();
        for n in model.nodes() {
            for (slot, output) in n.outputs.iter().enumerate() {
                if let Some(t) = &output.fact.konst {
                    folded.insert(OutletId::new(n.id, slot), t.clone());
                }
            }
        }
        // Demand-load constants: metadata such as i64::MAX Slice sentinels
        // must never be coerced through float just because it exists upstream.
        for id in model.eval_order()? {
            let node = model.node(id);
            if node.id == inlet.node {
                continue;
            }
            if node
                .outputs
                .iter()
                .enumerate()
                .all(|(slot, _)| folded.contains_key(&OutletId::new(id, slot)))
            {
                continue;
            }
            if node.op.is_stateless() && node.inputs.iter().all(|i| folded.contains_key(i)) {
                let args = node
                    .inputs
                    .iter()
                    .map(|i| folded[i].clone().into_tvalue())
                    .collect();
                let outputs = node
                    .op
                    .eval_with_session(node.id, &TurnState::default(), args)
                    .with_context(|| {
                        format!("CUDA constant folding {} ({})", node.name, node.op.name())
                    })?;
                for (slot, tensor) in outputs.into_iter().enumerate() {
                    folded.insert(OutletId::new(id, slot), Arc::new(tensor.into_tensor()));
                }
                continue;
            }
            ensure!(
                node.outputs.len() == 1,
                "CUDA multi-output operator {}",
                node.name
            );
            let out_shape = shape(&node.outputs[0].fact)?;
            let mut inputs = Vec::new();
            for &wire in &node.inputs {
                if let Some(&value) = values.get(&wire) {
                    inputs.push(value);
                    continue;
                }
                let value = folded
                    .get(&wire)
                    .with_context(|| format!("CUDA input not lowered: {}", node.name))?;
                // Some operations consume shape arguments exclusively on the
                // host. These will be removed from `inputs` by their lowering.
                let value_id = g.lengths.len();
                g.lengths.push(value.len().max(1));
                // Delay conversion until we know this constant is device-live.
                values.insert(wire, value_id);
                inputs.push(value_id);
            }
            let owned_facts = node
                .inputs
                .iter()
                .map(|i| {
                    let mut f = model.outlet_fact(*i)?.clone();
                    if let Some(t) = folded.get(i) {
                        f.konst = Some(t.clone());
                    }
                    Ok(f)
                })
                .collect::<Result<Vec<_>>>()?;
            let facts = owned_facts.iter().collect::<Vec<_>>();
            let result = g
                .lower(node, &facts, &out_shape, inputs)
                .with_context(|| format!("CUDA {} ({})", node.name, node.op.name()))?;
            values.insert(OutletId::new(id, 0), result);
        }
        for &wire in model.output_outlets()? {
            let value = *values
                .get(&wire)
                .context("CUDA constant output unsupported")?;
            ensure!(
                model.outlet_fact(wire)?.datum_type == DatumType::F32,
                "CUDA output must be float32"
            );
            g.outputs.push((value, shape(model.outlet_fact(wire)?)?));
        }
        let used: std::collections::HashSet<_> = g
            .steps
            .iter()
            .flat_map(|s| s.inputs.iter().copied())
            .chain(g.outputs.iter().map(|o| o.0))
            .collect();
        for (wire, id) in values {
            if used.contains(&id) {
                if let Some(value) = folded.get(&wire) {
                    g.constants.insert(id, constant(value)?);
                }
            }
        }
        ensure!(!g.steps.is_empty(), "empty CUDA graph");
        Ok(g)
    }
    fn emit(
        &mut self,
        inputs: Vec<usize>,
        params: Vec<u32>,
        len: usize,
        kernel: usize,
        blocks: usize,
    ) -> Result<usize> {
        ensure!(
            !inputs.is_empty()
                && inputs.len() < 16
                && len > 0
                && len <= 268_435_456
                && blocks > 0
                && blocks <= i32::MAX as usize,
            "invalid CUDA step"
        );
        let output = self.lengths.len();
        self.lengths.push(len);
        self.steps.push(Step {
            inputs,
            output,
            params,
            kernel,
            blocks: blocks as u32,
        });
        Ok(output)
    }
    fn element(&mut self, inputs: Vec<usize>, params: Vec<u32>, len: usize) -> Result<usize> {
        self.emit(inputs, params, len, 0, len.div_ceil(256))
    }
    fn lower(
        &mut self,
        node: &TypedNode,
        facts: &[&TypedFact],
        out: &[usize],
        mut inputs: Vec<usize>,
    ) -> Result<usize> {
        let len = count(out)?;
        let a = shape(facts.first().context("CUDA operator without inputs")?)?;
        let name = node.op.name();
        if let Some(axis) = node.op_as::<AxisOp>() {
            if let AxisOp::Move(from, to) = axis {
                let mut axes = (0..a.len()).collect::<Vec<_>>();
                let v = axes.remove(*from);
                axes.insert(*to, v);
                let s = strides(&a);
                let mut p = vec![2, out.len() as u32];
                for (&dim, axis) in out.iter().zip(axes) {
                    p.extend([dim as u32, s[axis] as u32, 0, 1]);
                }
                return self.element(vec![inputs[0]], p, len);
            }
            ensure!(len == self.lengths[inputs[0]], "invalid CUDA reshape");
            return Ok(inputs[0]);
        }
        if name == "Identity" {
            return Ok(inputs[0]);
        }
        if let Some(op) = node.op_as::<Conv>() {
            ensure!(
                op.q_params.is_none()
                    && op.kernel_fmt == KernelFormat::OIHW
                    && op.pool_spec.data_format == DataFormat::NCHW
                    && a.len() == 4
                    && out.len() == 4
                    && inputs.len() == 3,
                "unsupported convolution"
            );
            let k = &op.pool_spec.kernel_shape;
            ensure!(
                k.len() == 2 && op.group > 0,
                "unsupported convolution geometry"
            );
            let pads = op.pool_spec.computed_padding(&a[2..]);
            ensure!(
                facts.iter().all(|f| f.datum_type == DatumType::F32),
                "nonfloat convolution"
            );
            // tract represents a missing bias as a scalar zero.
            if self.lengths[inputs[2]] == 1 && out[1] != 1 {
                let bias = facts[2]
                    .konst
                    .as_ref()
                    .context("nonconstant scalar bias")?
                    .cast_to_scalar::<f32>()?;
                let id = self.lengths.len();
                self.lengths.push(out[1]);
                self.constants.insert(id, vec![bias.to_bits(); out[1]]);
                inputs[2] = id;
            }
            ensure!(
                self.lengths[inputs[2]] == out[1],
                "invalid convolution bias"
            );
            let p = vec![
                3,
                a[1] as u32,
                a[2] as u32,
                a[3] as u32,
                out[1] as u32,
                out[2] as u32,
                out[3] as u32,
                k[0] as u32,
                k[1] as u32,
                op.pool_spec.stride(0) as u32,
                op.pool_spec.stride(1) as u32,
                op.pool_spec.dilation(0) as u32,
                op.pool_spec.dilation(1) as u32,
                pads[0].pad_before as u32,
                pads[1].pad_before as u32,
                op.group as u32,
            ];
            // Depthwise/small convolutions avoid a mostly-empty GEMM tile.
            if out[1] / op.group < 8 {
                return self.element(inputs, p, len);
            }
            let blocks =
                a[0] * op.group * (out[1] / op.group).div_ceil(16) * (out[2] * out[3]).div_ceil(16);
            return self.emit(inputs, p, len, 2, blocks);
        }
        if let Some(op) = node.op_as::<TypedBinOp>() {
            ensure!(facts.len() == 2, "invalid binary arity");
            let integer = facts
                .iter()
                .all(|f| matches!(f.datum_type, DatumType::I64 | DatumType::I32));
            ensure!(
                integer || facts.iter().all(|f| f.datum_type == DatumType::F32),
                "nonfloat binary operator"
            );
            let kind = if integer {
                match op.0.name() {
                    "Add" => 8,
                    "Sub" => 9,
                    "Mul" => 10,
                    "Div" => 11,
                    "Min" => 12,
                    "Max" => 13,
                    _ => bail!("unsupported integer operator"),
                }
            } else {
                match op.0.name() {
                    "Add" => 0,
                    "Sub" => 1,
                    "Mul" => 2,
                    "Div" => 3,
                    "Pow" => 4,
                    "Min" => 5,
                    "Max" => 6,
                    _ => bail!("unsupported binary {}", op.0.name()),
                }
            };
            let b = shape(facts[1])?;
            let aa = broadcast_strides(&a, out)?;
            let bb = broadcast_strides(&b, out)?;
            let mut p = vec![1, kind, out.len() as u32];
            for ((&dim, as_), bs) in out.iter().zip(aa).zip(bb) {
                p.extend([dim as u32, as_ as u32, bs as u32]);
            }
            return self.element(inputs, p, len);
        }
        // The ONNX importer wraps Cast in a private ElementWise mini-op;
        // its validated source/destination types determine these same semantics.
        if node.op_as::<Cast>().is_some()
            || (name == "onnx.Cast" && node.op_as::<ElementWiseOp>().is_some())
        {
            let to = node.outputs[0].fact.datum_type;
            if facts[0].datum_type == to
                || (matches!(facts[0].datum_type, DatumType::I64 | DatumType::I32)
                    && to == DatumType::F32)
            {
                return Ok(inputs[0]);
            }
            ensure!(
                facts[0].datum_type == DatumType::F32
                    && matches!(to, DatumType::I64 | DatumType::I32),
                "unsupported CUDA cast"
            );
            return self.element(vec![inputs[0]], vec![10], len);
        }
        if let Some(op) = node.op_as::<ElementWiseOp>() {
            ensure!(
                facts[0].datum_type == DatumType::F32,
                "nonfloat unary operator"
            );
            let kind = match op.0.name().as_str() {
                "Relu" => 1,
                "Sigmoid" => 3,
                "Tanh" => 4,
                "Sqrt" => 5,
                "Exp" => 6,
                "Ln" | "Log" => 7,
                "Abs" => 8,
                "Neg" => 9,
                "Recip" | "Reciprocal" => 10,
                "Erf" => 11,
                "Floor" => 12,
                "Ceil" => 13,
                "HardSwish" => 15,
                "Rsqrt" => 18,
                "Square" => 19,
                _ => bail!("unsupported unary {}", op.0.name()),
            };
            return self.element(vec![inputs[0]], vec![0, kind, 0, 0], len);
        }
        if let Some(op) = node.op_as::<EinSum>() {
            ensure!(
                inputs.len() == 2
                    && op.q_params.is_none()
                    && facts.iter().all(|f| f.datum_type == DatumType::F32),
                "unsupported einsum"
            );
            let shapes = [a, shape(facts[1])?];
            let st = [strides(&shapes[0]), strides(&shapes[1])];
            let mut axes = vec![[0usize; 3]; out.len()];
            let mut red = Vec::new();
            for ax in op.axes.iter_all_axes() {
                ensure!(
                    ax.inputs.iter().all(|v| v.len() <= 1) && ax.outputs[0].len() <= 1,
                    "repeated einsum axis"
                );
                let mut dim = 1;
                let mut s = [0, 0];
                for j in 0..2 {
                    if let Some(&k) = ax.inputs[j].first() {
                        let d = shapes[j][k];
                        ensure!(
                            dim == 1 || d == 1 || dim == d,
                            "incompatible einsum dimensions"
                        );
                        dim = dim.max(d);
                        if d > 1 {
                            s[j] = st[j][k];
                        }
                    }
                }
                let entry = [dim, s[0], s[1]];
                if let Some(&j) = ax.outputs[0].first() {
                    axes[j] = entry;
                } else {
                    red.push(entry);
                }
            }
            ensure!(
                axes.iter().zip(out).all(|(a, &d)| a[0] == d),
                "einsum output mismatch"
            );
            let count = red.iter().map(|a| a[0]).product::<usize>();
            if let Some((p, blocks)) = matrix(&axes, &red, len) {
                return self.emit(inputs, p, len, 1, blocks);
            }
            let mut p = vec![4, axes.len() as u32, red.len() as u32, count as u32];
            for axis in axes.into_iter().chain(red) {
                p.extend(axis.map(|v| v as u32));
            }
            return self.element(inputs, p, len);
        }
        if let Some(op) = node.op_as::<Reduce>() {
            ensure!(facts[0].datum_type == DatumType::F32, "nonfloat reduction");
            let kind = match op.reducer {
                Reducer::Sum => 1,
                Reducer::Max => 2,
                Reducer::Min => 3,
                _ => bail!("unsupported reduction {:?}", op.reducer),
            };
            let st = strides(&a);
            let mut p = vec![5, kind, a.len() as u32, 1];
            let mut n = 1;
            for (k, &d) in a.iter().enumerate() {
                let r = op.axes.contains(&k);
                if r {
                    n *= d;
                }
                p.extend([d as u32, st[k] as u32, u32::from(r)]);
            }
            p[3] = n as u32;
            return self.element(vec![inputs[0]], p, len);
        }
        if let Some(op) = node.op_as::<Softmax>() {
            ensure!(
                name == "Softmax" && op.quant_output_dt.is_none() && op.axes.len() == 1,
                "unsupported softmax"
            );
            let axis = op.axes[0];
            let n = a[axis];
            let inner = count(&a[axis + 1..])?;
            return self.emit(
                vec![inputs[0]],
                vec![6, n as u32, inner as u32],
                len,
                3,
                len / n,
            );
        }
        if let Some(op) = node.op_as::<array::StridedSlice>() {
            ensure!(op.shrink_axis_mask == 0, "shrinking strided slice");
            let data = |i: usize| {
                facts[i]
                    .konst
                    .as_ref()
                    .context("dynamic CUDA slice parameter")
            };
            let axes = if let Some(i) = op.optional_axes_input {
                data(i)?
                    .cast_to::<i64>()?
                    .to_plain_array_view::<i64>()?
                    .iter()
                    .map(|&v| {
                        if v < 0 {
                            (v + a.len() as i64) as usize
                        } else {
                            v as usize
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                (0..a.len()).collect()
            };
            let steps = if let Some(i) = op.optional_steps_input {
                data(i)?
                    .cast_to::<i32>()?
                    .to_plain_array_view::<i32>()?
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
            } else {
                vec![1; a.len()]
            };
            let mut starts = vec![0; a.len()];
            let mut strides_ = vec![1i32; a.len()];
            for (i, &axis) in axes.iter().enumerate() {
                ensure!(axis < a.len(), "invalid slice axis");
                let d = op.prepare_one_dim(i, &a[axis].to_dim(), data(1)?, data(2)?, &steps)?;
                let start = d.begin.to_usize()?;
                let last = start as i64 + (out[axis] as i64 - 1) * d.stride as i64;
                ensure!(
                    start < a[axis] && last >= 0 && last < a[axis] as i64,
                    "CUDA slice exceeds input"
                );
                starts[axis] = start;
                strides_[axis] = d.stride;
            }
            let st = strides(&a);
            let mut p = vec![2, a.len() as u32];
            for i in 0..a.len() {
                p.extend([
                    out[i] as u32,
                    st[i] as u32,
                    starts[i] as u32,
                    strides_[i] as u32,
                ]);
            }
            return self.element(vec![inputs[0]], p, len);
        }
        if let Some(op) = node.op_as::<array::Slice>() {
            let start = op.start.to_usize()?;
            let end = op.end.to_usize()?;
            ensure!(start < end && end <= a[op.axis], "invalid slice");
            let st = strides(&a);
            let mut p = vec![2, a.len() as u32];
            for (i, &d) in out.iter().enumerate() {
                p.extend([
                    d as u32,
                    st[i] as u32,
                    if i == op.axis { start as u32 } else { 0 },
                    1,
                ]);
            }
            return self.element(vec![inputs[0]], p, len);
        }
        if let Some(op) = node.op_as::<array::MultiBroadcastTo>() {
            let _ = op;
            let st = broadcast_strides(&a, out)?;
            let mut p = vec![2, out.len() as u32];
            for (&d, s) in out.iter().zip(st) {
                p.extend([d as u32, s as u32, 0, 1]);
            }
            return self.element(vec![inputs[0]], p, len);
        }
        if let Some(op) = node.op_as::<array::Pad>() {
            let (mode, value) = match &op.mode {
                array::PadMode::Constant(v) => (0, v.cast_to_scalar::<f32>()?.to_bits()),
                array::PadMode::Edge => (1, 0),
                array::PadMode::Reflect => (2, 0),
            };
            let st = strides(&a);
            let mut p = vec![7, a.len() as u32, mode, value];
            for i in 0..a.len() {
                p.extend([
                    out[i] as u32,
                    a[i] as u32,
                    st[i] as u32,
                    op.pads[i].0 as u32,
                ]);
            }
            return self.element(vec![inputs[0]], p, len);
        }
        if let Some(op) = node.op_as::<array::TypedConcat>() {
            let inner = count(&out[op.axis + 1..])?;
            let mut result = inputs[0];
            let mut dim = a[op.axis];
            let outer = count(&a[..op.axis])?;
            for (&input, fact) in inputs.iter().zip(facts).skip(1) {
                let b = shape(fact)?;
                result = self.element(
                    vec![result, input],
                    vec![8, inner as u32, dim as u32, b[op.axis] as u32],
                    outer * (dim + b[op.axis]) * inner,
                )?;
                dim += b[op.axis];
            }
            return Ok(result);
        }
        let resize = if let Some(op) = node.op_as::<tract_onnx_opl::resize::Resize>() {
            use tract_onnx::tract_core::ops::nn::resize;
            use tract_onnx_opl::resize::{AspectRatio, CoordTransform, Nearest};
            ensure!(
                !op.antialias
                    && !op.exclude_outside
                    && op.keep_aspect_ratio_policy == AspectRatio::Stretch
                    && op.optional_roi_input.is_none()
                    && op.axes.as_ref().is_none_or(|a| a == &[0, 1, 2, 3]),
                "unsupported ONNX resize attributes"
            );
            let CoordTransform::Plain(coord) = &op.coord_transformer else {
                bail!("ROI resize unsupported")
            };
            let nearest = match op.nearest {
                Nearest::Floor => resize::Nearest::Floor,
                Nearest::RoundPreferCeil => resize::Nearest::RoundPreferCeil,
                _ if op.interpolator != resize::Interpolator::Nearest => resize::Nearest::Floor,
                _ => bail!("unsupported nearest rounding"),
            };
            Some(resize::Resize {
                coord_transformer: coord.clone(),
                interpolator: op.interpolator.clone(),
                nearest,
                optional_scales_input: op.optional_scales_input,
                optional_sizes_input: op.optional_sizes_input,
            })
        } else {
            node.op_as::<tract_onnx::tract_core::ops::nn::Resize>()
                .cloned()
        };
        if let Some(op) = resize {
            use tract_onnx::tract_core::ops::nn::resize::{
                CoordTransformer as C, Interpolator as I, Nearest as N,
            };
            ensure!(
                a.len() == 4 && out.len() == 4 && a[..2] == out[..2],
                "unsupported resize rank"
            );
            let coord = match op.coord_transformer {
                C::Asymmetric => 0,
                C::HalfPixel => 1,
                C::PytorchHalfPixel => 2,
                C::AlignCorners => 3,
                C::TfHalfPixelForNn => 4,
                _ => bail!("unsupported resize coordinates"),
            };
            let interp = match op.interpolator {
                I::Nearest => 0,
                I::Linear => 1,
                _ => bail!("unsupported resize interpolation"),
            };
            let nearest = match op.nearest {
                N::Floor => 0,
                N::RoundPreferCeil => 3,
            };
            let mut scales = vec![
                1.,
                1.,
                out[2] as f32 / a[2] as f32,
                out[3] as f32 / a[3] as f32,
            ];
            if let Some(i) = op.optional_scales_input {
                if let Some(t) = &facts[i].konst {
                    if t.len() == 4 {
                        scales = t.to_plain_array_view::<f32>()?.iter().copied().collect();
                    }
                }
            }
            return self.element(
                vec![inputs[0]],
                vec![
                    9,
                    a[3] as u32,
                    a[2] as u32,
                    out[3] as u32,
                    out[2] as u32,
                    scales[3].to_bits(),
                    scales[2].to_bits(),
                    coord,
                    interp,
                    nearest,
                ],
                len,
            );
        }
        if let Some(op) = node.op_as::<array::Gather>() {
            let b = shape(facts[1])?;
            let p = vec![
                11,
                count(&a[op.axis + 1..])? as u32,
                a[op.axis] as u32,
                count(&b)? as u32,
            ];
            return self.element(inputs, p, len);
        }
        if let Some(op) = node.op_as::<array::GatherNd>() {
            let b = shape(facts[1])?;
            let tuple = *b.last().context("invalid gather")?;
            let batch = op.batch_dims;
            let mut p = vec![
                12,
                count(&a[batch + tuple..])? as u32,
                count(&b[batch..b.len() - 1])? as u32,
                count(&a[batch..])? as u32,
                tuple as u32,
            ];
            let st = strides(&a);
            for k in batch..batch + tuple {
                p.extend([a[k] as u32, st[k] as u32]);
            }
            return self.element(inputs, p, len);
        }
        if let Some(op) = node.op_as::<crate::deform_sample::FusedSample>() {
            let (data, sample, kernel) = op.geometry();
            let p = vec![
                13,
                data[2] as u32,
                data[3] as u32,
                data[4] as u32,
                sample[4] as u32,
                sample[5] as u32,
                kernel[0] as u32,
                kernel[1] as u32,
            ];
            return self.element(inputs, p, len);
        }
        bail!("unsupported operator {}", node.op.name())
    }
}
fn broadcast_strides(input: &[usize], output: &[usize]) -> Result<Vec<usize>> {
    ensure!(input.len() <= output.len(), "broadcast rank mismatch");
    let mut input = [vec![1; output.len() - input.len()], input.to_vec()].concat();
    let st = strides(&input);
    for (&a, &b) in input.iter().zip(output) {
        ensure!(a == 1 || a == b, "broadcast mismatch");
    }
    for (d, s) in input.iter_mut().zip(st) {
        *d = if *d == 1 { 0 } else { s };
    }
    Ok(input)
}
fn matrix(output: &[[usize; 3]], red: &[[usize; 3]], len: usize) -> Option<(Vec<u32>, usize)> {
    let mut rows = vec![];
    let mut cols = vec![];
    let mut batches = vec![];
    let mut stride = len;
    for &[dim, a, b] in output {
        stride /= dim;
        if dim == 1 {
            continue;
        }
        let v = [dim, a, b, stride];
        match (a != 0, b != 0) {
            (true, false) => rows.push(v),
            (false, true) => cols.push(v),
            (true, true) => batches.push(v),
            _ => return None,
        }
    }
    if rows.is_empty() || cols.is_empty() {
        return None;
    }
    let m = rows.iter().map(|a| a[0]).product::<usize>();
    let n = cols.iter().map(|a| a[0]).product::<usize>();
    let k = red.iter().map(|a| a[0]).product::<usize>();
    let blocks = m.div_ceil(16) * n.div_ceil(16) * batches.iter().map(|a| a[0]).product::<usize>();
    let mut p = vec![
        4,
        m as u32,
        n as u32,
        k as u32,
        rows.len() as u32,
        cols.len() as u32,
        batches.len() as u32,
        red.len() as u32,
    ];
    for v in rows.into_iter().chain(cols).chain(batches) {
        p.extend(v.map(|v| v as u32));
    }
    for v in red {
        p.extend(v.map(|v| v as u32));
    }
    Some((p, blocks))
}
