//! GPU contractions inside tract's graph when a complete resident graph is
//! unavailable. Integer/shape operations retain their original semantics, and
//! large convolutions are split into exact spatial bands, never smaller model
//! tiles (which would change attention and the restoration result).
use schist_fx::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};
use std::sync::Arc;
use tract_onnx::tract_core::internal::*;
use tract_onnx::tract_core::ops::{cnn::Conv, cnn::KernelFormat, einsum::EinSum, nn::DataFormat};

const BAND_FLOATS: usize = 4 * 1024 * 1024;
static CONV: ComputeShader =
    ComputeShader::workgroup("neural-convolution-band", include_str!("gpu_conv.wgsl"));
static EINSUM: ComputeShader =
    ComputeShader::new("neural-contraction", include_str!("gpu_einsum.wgsl"));

pub(super) struct Partitioned {
    pub plan: Arc<TypedSimplePlan>,
    pub operations: usize,
}

impl Partitioned {
    pub fn compile(model: TypedModel) -> Option<Self> {
        // Optional acceleration must not make a CPU-runnable model unloadable.
        Self::try_compile(model).unwrap_or_else(|error| {
            log::debug!("GPU partition compiler declined: {error:#}");
            None
        })
    }

    fn try_compile(mut model: TypedModel) -> TractResult<Option<Self>> {
        model.declutter()?;
        let mut operations = 0;
        for id in model.eval_order()? {
            let node = model.node(id);
            let kind = if let Some(conv) = node.op_as::<Conv>() {
                if conv.q_params.is_some()
                    || conv.kernel_fmt != KernelFormat::OIHW
                    || conv.pool_spec.data_format != DataFormat::NCHW
                    || conv.pool_spec.kernel_shape.len() != 2
                {
                    continue;
                }
                Kernel::Conv(Box::new(conv.clone()))
            } else if let Some(einsum) = node.op_as::<EinSum>() {
                if einsum.q_params.is_some() || node.inputs.len() != 2 {
                    continue;
                }
                Kernel::EinSum(Box::new(einsum.clone()))
            } else {
                continue;
            };
            let facts = node
                .inputs
                .iter()
                .map(|&i| model.outlet_fact(i))
                .collect::<TractResult<Vec<_>>>()?;
            if facts
                .iter()
                .any(|f| f.datum_type != f32::datum_type() || f.shape.as_concrete().is_none())
            {
                continue;
            }
            let outputs = node
                .outputs
                .iter()
                .map(|o| o.fact.clone())
                .collect::<TVec<_>>();
            if outputs.len() != 1 || outputs[0].shape.as_concrete().is_none() {
                continue;
            }
            // Keep an independently optimized CPU implementation for a device
            // limit, failed dispatch, or loss partway through this operation.
            let mut fallback = TypedModel::default();
            let mut runtime = Vec::new();
            let mut wires = tvec!();
            for (i, fact) in facts.iter().enumerate() {
                wires.push(if let Some(value) = &fact.konst {
                    fallback.add_const(format!("constant-{i}"), value.clone())?
                } else {
                    runtime.push(i);
                    fallback.add_source(format!("input-{i}"), fact.without_value())?
                });
            }
            let out = fallback.wire_node("cpu", node.op.clone(), &wires)?;
            fallback.select_output_outlets(&out)?;
            let cpu = fallback.into_optimized()?.into_runnable()?;
            model.node_mut(id).op = Box::new(GpuOp {
                kind,
                cpu,
                runtime,
                outputs,
            });
            operations += 1;
        }
        if operations == 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            plan: model.into_optimized()?.into_runnable()?,
            operations,
        }))
    }
}

#[derive(Clone, Debug)]
enum Kernel {
    Conv(Box<Conv>),
    EinSum(Box<EinSum>),
}

#[derive(Clone, Debug)]
struct GpuOp {
    kind: Kernel,
    cpu: Arc<TypedSimplePlan>,
    runtime: Vec<usize>,
    outputs: TVec<TypedFact>,
}
impl PartialEq for GpuOp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cpu, &other.cpu)
    }
}
impl Eq for GpuOp {}
impl Op for GpuOp {
    fn name(&self) -> StaticName {
        "GpuContraction".into()
    }
    op_as_typed_op!();
}
impl EvalOp for GpuOp {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        if schist_fx::backend().compute_available(usize::MAX) {
            let shape = self.outputs[0].shape.as_concrete().unwrap();
            let result = match &self.kind {
                Kernel::Conv(conv) => convolution(conv, &inputs, shape),
                Kernel::EinSum(op) => einsum(op, &inputs, shape),
            };
            if let Some(result) = result {
                return Ok(tvec!(Tensor::from_shape(shape, &result)?.into()));
            }
        }
        self.cpu
            .run(self.runtime.iter().map(|&i| inputs[i].clone()).collect())
    }
}
impl TypedOp for GpuOp {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(self.outputs.clone())
    }
    as_op!();
}

fn convolution(conv: &Conv, inputs: &[TValue], output: &[usize]) -> Option<Vec<f32>> {
    let input_view = inputs[0].to_plain_array_view::<f32>().ok()?;
    let input = input_view.as_slice()?;
    let kernel_view = inputs[1].to_plain_array_view::<f32>().ok()?;
    let kernel = kernel_view.as_slice()?;
    let bias_view = inputs[2].to_plain_array_view::<f32>().ok()?;
    let bias = bias_view.as_slice()?;
    let &[batches, ic, ih, iw] = inputs[0].shape() else {
        return None;
    };
    let &[_, oc, oh, ow] = output else {
        return None;
    };
    let [kh, kw] = *conv.pool_spec.kernel_shape.as_slice() else {
        return None;
    };
    let (sy, sx) = (conv.pool_spec.stride(0), conv.pool_spec.stride(1));
    let (dy, dx) = (conv.pool_spec.dilation(0), conv.pool_spec.dilation(1));
    let padding = conv.pool_spec.computed_padding(&[ih, iw]);
    let (pt, pl) = (padding[0].pad_before, padding[1].pad_before);
    // Kernel arguments are exact integers encoded in f32 storage. Decline
    // pathological shapes rather than round an address or overflow WGSL i32.
    if [
        batches, ic, ih, iw, oc, oh, ow, kh, kw, sy, sx, dy, dx, conv.group,
    ]
    .iter()
    .any(|&n| n == 0 || n > 16_777_216)
        || [
            pt,
            pl,
            kh.saturating_mul(dy),
            kw.saturating_mul(dx),
            oh.saturating_mul(sy),
            ow.saturating_mul(sx),
        ]
        .iter()
        .any(|&n| n > 16_777_216)
    {
        return None;
    }
    let (ci, co) = (ic / conv.group, oc / conv.group);
    let work = batches
        .saturating_mul(oc)
        .saturating_mul(oh)
        .saturating_mul(ow)
        .saturating_mul(ci * kh * kw)
        .saturating_mul(2);
    if !schist_fx::backend().compute_available(work) {
        return None;
    }
    let halo = (kh - 1) * dy;
    let rows = (BAND_FLOATS / (ci * iw)).saturating_sub(halo) / sy;
    let rows = rows.min(BAND_FLOATS / (co * ow)).min(oh);
    if rows == 0 || ci * co * kh * kw + co > BAND_FLOATS {
        return None;
    }
    let mut result = vec![0.0; batches * oc * oh * ow];
    for batch in 0..batches {
        for group in 0..conv.group {
            let weight_count = ci * co * kh * kw;
            let mut weights = kernel[group * weight_count..(group + 1) * weight_count].to_vec();
            for c in group * co..(group + 1) * co {
                weights.push(bias[if bias.len() == 1 { 0 } else { c }]);
            }
            let mut program = ComputeProgram {
                buffers: vec![weights],
                steps: vec![],
                result: ComputeSource::Step(0),
                // The threshold applies to the whole contraction, including
                // short final bands. Declining the tail would redo all bands
                // on the CPU after successfully computing most of them.
                work,
            };
            for y in (0..oh).step_by(rows) {
                let height = rows.min(oh - y);
                let first = (y * sy).saturating_sub(pt).min(ih);
                let end = ((y + height - 1) * sy + halo + 1)
                    .saturating_sub(pt)
                    .min(ih);
                if end <= first {
                    return None;
                }
                let mut band = Vec::with_capacity(ci * (end - first) * iw);
                for c in group * ci..(group + 1) * ci {
                    let offset = (batch * ic + c) * ih * iw;
                    band.extend_from_slice(&input[offset + first * iw..offset + end * iw]);
                }
                let len = co * height * ow;
                let groups = co.div_ceil(16) * (height * ow).div_ceil(16);
                let direct = ci * co <= 16;
                let mut params = vec![
                    ci as f32,
                    (end - first) as f32,
                    iw as f32,
                    co as f32,
                    height as f32,
                    ow as f32,
                    kh as f32,
                    kw as f32,
                    sy as f32,
                    sx as f32,
                    dy as f32,
                    dx as f32,
                    (pt + first - y * sy) as f32,
                    pl as f32,
                ];
                if direct {
                    params.insert(0, 0.0);
                    params.push(1.0);
                }
                program.steps = vec![ComputeStep {
                    shader: if direct { super::gpu::SHADER } else { CONV },
                    source: ComputeSource::Input(0),
                    auxiliary: ComputeSource::Input(1),
                    params,
                    output_len: len,
                    invocations: if direct { len } else { groups },
                    shape: [co as u32, height as u32, ow as u32],
                }];
                let values = schist_fx::try_compute(&band, &program)?;
                for c in 0..co {
                    let start = ((batch * oc + group * co + c) * oh + y) * ow;
                    result[start..start + height * ow]
                        .copy_from_slice(&values[c * height * ow..(c + 1) * height * ow]);
                }
            }
        }
    }
    Some(result)
}

fn einsum(op: &EinSum, inputs: &[TValue], output: &[usize]) -> Option<Vec<f32>> {
    let a_view = inputs[0].to_plain_array_view::<f32>().ok()?;
    let a = a_view.as_slice()?;
    let b_view = inputs[1].to_plain_array_view::<f32>().ok()?;
    let b = b_view.as_slice()?;
    let len: usize = output.iter().product();
    if a.is_empty() || b.is_empty() || len == 0 || a.len().max(b.len()).max(len) > BAND_FLOATS * 4 {
        return None;
    }
    let strides = |shape: &[usize]| {
        let mut s = vec![1usize; shape.len()];
        for i in (0..shape.len().saturating_sub(1)).rev() {
            s[i] = s[i + 1] * shape[i + 1];
        }
        s
    };
    let astride = strides(inputs[0].shape());
    let bstride = strides(inputs[1].shape());
    let mut out_axes = vec![[0usize; 3]; output.len()];
    let mut reduction = Vec::new();
    for axis in op.axes.iter_all_axes() {
        if axis.inputs.iter().any(|a| a.len() > 1) || axis.outputs[0].len() > 1 {
            return None;
        }
        let mut dim = 1;
        let mut strides = [0, 0];
        for (i, stride) in strides.iter_mut().enumerate() {
            if let Some(&j) = axis.inputs[i].first() {
                let d = inputs[i].shape()[j];
                if d != 1 && dim != 1 && d != dim {
                    return None;
                }
                dim = dim.max(d);
                if d > 1 {
                    *stride = if i == 0 { astride[j] } else { bstride[j] };
                }
            }
        }
        let entry = [dim, strides[0], strides[1]];
        if let Some(&j) = axis.outputs[0].first() {
            out_axes[j] = entry;
        } else {
            reduction.push(entry);
        }
    }
    let count = reduction.iter().map(|a| a[0]).product::<usize>();
    let work = len.saturating_mul(count).saturating_mul(2);
    if !schist_fx::backend().compute_available(work) {
        return None;
    }
    let mut params = vec![output.len() as f32, reduction.len() as f32, count as f32];
    for axis in out_axes.into_iter().chain(reduction) {
        params.extend(axis.map(|v| v as f32));
    }
    let mut program = ComputeProgram::single(&EINSUM, params, len, [len as u32, 1, 1], work);
    program.buffers.push(b.to_vec());
    program.steps[0].auxiliary = ComputeSource::Input(1);
    schist_fx::try_compute(a, &program)
}
