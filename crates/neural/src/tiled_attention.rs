//! Exact dense attention in bounded query blocks. Never materialize the full
//! batch × query × key score tensor. Keep FP32, scaling, and stable softmax.
use tract_linalg::multithread::par_chunks_mut;
use tract_onnx::tract_core::{
    internal::*,
    ops::{
        binary::TypedBinOp,
        einsum::EinSum,
        math::Mul,
        nn::{Softmax, SoftmaxExp, SoftmaxKind},
    },
};

const QUERY_BLOCK: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TiledAttention {
    shapes: [Vec<usize>; 5],
    queries: usize,
    keys: usize,
    depth: usize,
    channels: usize,
}
impl TiledAttention {
    fn new(shapes: [Vec<usize>; 5]) -> Option<Self> {
        let &[batch, queries, depth] = shapes[0].as_slice() else {
            return None;
        };
        let &[kb, kd, keys] = shapes[1].as_slice() else {
            return None;
        };
        let &[vb, vk, channels] = shapes[2].as_slice() else {
            return None;
        };
        if batch != kb
            || batch != vb
            || kd != depth
            || vk != keys
            || !(1..=16).contains(&depth)
            || !(1..=16).contains(&channels)
            || !(16..=2048).contains(&keys)
            || shapes[3].len() > 3
            || shapes[3].iter().any(|&d| d != 1)
            || shapes[4] != [batch, queries, channels]
        {
            return None;
        }
        for shape in &shapes {
            let size =
                shape
                    .iter()
                    .try_fold(1usize, |n, &d| if d == 0 { None } else { n.checked_mul(d) })?;
            if size > isize::MAX as usize / size_of::<f32>() {
                return None;
            }
        }
        Some(Self {
            shapes,
            queries,
            keys,
            depth,
            channels,
        })
    }

    fn rows(
        &self,
        inputs: [&[f32]; 4],
        first: usize,
        output: &mut [f32],
        native: bool,
    ) -> TractResult<()> {
        let [q, k, v, scale] = inputs;
        let mut scores = vec![0.; QUERY_BLOCK * self.keys];
        let mut shifted = vec![0.; scores.len()];
        // A transposed value batch makes the portable dot products contiguous.
        let mut values = vec![0.; self.keys * self.channels];
        let mut value_batch = usize::MAX;
        let max = (tract_linalg::ops().max_f32)();
        let sum = (tract_linalg::ops().sum_f32)();
        let mul = (tract_linalg::ops().mul_by_scalar_f32)();
        let mut done = 0;
        while done < output.len() / self.channels {
            let row = first + done;
            let batch = row / self.queries;
            let query = row % self.queries;
            let count = QUERY_BLOCK
                .min(self.queries - query)
                .min(output.len() / self.channels - done);
            let q = &q[row * self.depth..(row + count) * self.depth];
            let k = &k[batch * self.depth * self.keys..(batch + 1) * self.depth * self.keys];
            let v = &v[batch * self.keys * self.channels..(batch + 1) * self.keys * self.channels];
            let scores = &mut scores[..count * self.keys];
            let shifted = &mut shifted[..scores.len()];
            let output = &mut output[done * self.channels..(done + count) * self.channels];
            #[cfg(target_os = "macos")]
            if native {
                // SAFETY: these disjoint initialized slices are exactly M×K,
                // K×N, and M×N. Checked dimensions fit i32; BLAS is synchronous.
                unsafe {
                    crate::accelerate_matrix::cblas_sgemm(
                        101,
                        111,
                        111,
                        count as i32,
                        self.keys as i32,
                        self.depth as i32,
                        1.,
                        q.as_ptr(),
                        self.depth as i32,
                        k.as_ptr(),
                        self.keys as i32,
                        0.,
                        scores.as_mut_ptr(),
                        self.keys as i32,
                    );
                }
            } else {
                self.scores(q, k, scores);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = native;
                self.scores(q, k, scores);
            }
            // Scaling remains a separate operation after the dot product, as
            // in the export. Each row uses its own maximum and normalization.
            mul.run_with_params(scores, scale[0])?;
            for (row, shifted) in scores
                .chunks_exact(self.keys)
                .zip(shifted.chunks_exact_mut(self.keys))
            {
                let maximum = max.run(row)?;
                for (out, &value) in shifted.iter_mut().zip(row) {
                    *out = value - maximum;
                }
            }
            #[cfg(target_os = "macos")]
            if native {
                let len = i32::try_from(scores.len())?;
                // SAFETY: disjoint slices of len initialized f32 values; the
                // bounded tile fits i32, and vForce finishes before either moves.
                unsafe {
                    crate::vector_softmax::vvexpf(scores.as_mut_ptr(), shifted.as_ptr(), &len);
                }
            } else {
                exponentials(scores, shifted);
            }
            #[cfg(not(target_os = "macos"))]
            exponentials(scores, shifted);
            for row in scores.chunks_exact_mut(self.keys) {
                mul.run_with_params(row, sum.run(row)?.recip())?;
            }
            #[cfg(target_os = "macos")]
            if native {
                // SAFETY: scores=M×K, values=K×N and output=M×N; all are
                // contiguous, disjoint, initialized, and dimensions fit i32.
                unsafe {
                    crate::accelerate_matrix::cblas_sgemm(
                        101,
                        111,
                        111,
                        count as i32,
                        self.channels as i32,
                        self.keys as i32,
                        1.,
                        scores.as_ptr(),
                        self.keys as i32,
                        v.as_ptr(),
                        self.channels as i32,
                        0.,
                        output.as_mut_ptr(),
                        self.channels as i32,
                    );
                }
                done += count;
                continue;
            }
            if value_batch != batch {
                for (c, column) in values.chunks_exact_mut(self.keys).enumerate() {
                    for (dst, row) in column.iter_mut().zip(v.chunks_exact(self.channels)) {
                        *dst = row[c];
                    }
                }
                value_batch = batch;
            }
            for (probabilities, output) in scores
                .chunks_exact(self.keys)
                .zip(output.chunks_exact_mut(self.channels))
            {
                for (dst, column) in output.iter_mut().zip(values.chunks_exact(self.keys)) {
                    *dst = dot(probabilities, column);
                }
            }
            done += count;
        }
        Ok(())
    }

    fn scores(&self, q: &[f32], k: &[f32], scores: &mut [f32]) {
        for (query, row) in q
            .chunks_exact(self.depth)
            .zip(scores.chunks_exact_mut(self.keys))
        {
            row.fill(0.);
            for (&a, keys) in query.iter().zip(k.chunks_exact(self.keys)) {
                for (out, &b) in row.iter_mut().zip(keys) {
                    *out += a * b;
                }
            }
        }
    }
}

fn exponentials(output: &mut [f32], input: &[f32]) {
    for (out, &value) in output.iter_mut().zip(input) {
        *out = value.exp();
    }
}

// Independent accumulators expose SIMD without changing precision or calling
// a scalar strided dot product for every output channel. Reduction rounding
// differs from tract/BLAS; operator and full-model parity tests bound it.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sums = [0.; 8];
    let (a8, at) = a.as_chunks::<8>();
    let (b8, bt) = b.as_chunks::<8>();
    for (a, b) in a8.iter().zip(b8) {
        for i in 0..8 {
            sums[i] += a[i] * b[i];
        }
    }
    let mut value = sums.iter().sum::<f32>();
    for (&a, &b) in at.iter().zip(bt) {
        value += a * b;
    }
    value
}

impl Op for TiledAttention {
    fn name(&self) -> StaticName {
        "TiledAttention".into()
    }
    op_as_typed_op!();
}
impl TypedOp for TiledAttention {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(inputs.len() == 4, "attention input count changed");
        for (fact, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                fact.datum_type == DatumType::F32
                    && fact.shape.as_concrete() == Some(shape.as_slice()),
                "attention input fact changed"
            );
        }
        Ok(tvec!(f32::fact(&self.shapes[4])))
    }
    as_op!();
}
impl EvalOp for TiledAttention {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(inputs.len() == 4, "attention input count changed");
        for (value, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                value.datum_type() == DatumType::F32 && value.shape() == shape,
                "attention input shape or type changed"
            );
        }
        let views = inputs
            .iter()
            .map(|v| v.to_plain_array_view::<f32>())
            .collect::<TractResult<Vec<_>>>()?;
        let data = views
            .iter()
            .map(|v| v.as_slice().context("non-contiguous attention input"))
            .collect::<TractResult<Vec<_>>>()?;
        let mut result = Tensor::zero::<f32>(&self.shapes[4])?;
        {
            let mut output = result.to_plain_array_view_mut::<f32>()?;
            let output = output.as_slice_mut().unwrap();
            par_chunks_mut(output, self.channels, output.len(), |first, chunk| {
                self.rows([data[0], data[1], data[2], data[3]], first, chunk, true)
            })?;
        }
        Ok(tvec!(result.into()))
    }
}

fn matmul(node: &TypedNode) -> bool {
    node.inputs.len() == 2
        && node.outputs.len() == 1
        && node.op_as::<EinSum>().is_some_and(|op| {
            op.operating_dt == DatumType::F32
                && op.q_params.is_none()
                && op.axes == AxesMapping::for_numpy_matmul(3, false, false, false).unwrap()
        })
}
fn private(model: &TypedModel, outlet: OutletId) -> bool {
    outlet.slot == 0
        && model.node(outlet.node).outputs.len() == 1
        && model.node(outlet.node).outputs[0].successors.len() == 1
        && !model
            .output_outlets()
            .is_ok_and(|outputs| outputs.contains(&outlet))
}

/// Before decluttering folds axes/scales into matmul. Prove topology, FP32,
/// exact batch/feature geometry and scalar scaling; never match node names.
pub(super) fn optimize(model: &mut TypedModel) -> TractResult<usize> {
    let mut count = 0;
    for id in model.eval_order()? {
        let node = model.node(id);
        if !matmul(node) {
            continue;
        }
        let softmax = model.node(node.inputs[0].node);
        let Some(op) = softmax.op_as::<Softmax>() else {
            continue;
        };
        if !private(model, node.inputs[0])
            || softmax.inputs.len() != 1
            || op.axes.as_slice() != [2]
            || op.quant_output_dt.is_some()
            || op.kind != SoftmaxKind::Softmax(SoftmaxExp::Libc)
        {
            continue;
        }
        let scale = model.node(softmax.inputs[0].node);
        let Some(op) = scale.op_as::<TypedBinOp>() else {
            continue;
        };
        if !private(model, softmax.inputs[0])
            || scale.inputs.len() != 2
            || !op.0.is::<Mul>()
            || op.1.is_some()
        {
            continue;
        }
        for slot in 0..2 {
            let product = model.node(scale.inputs[slot].node);
            if !private(model, scale.inputs[slot]) || !matmul(product) {
                continue;
            }
            let outlets = [
                product.inputs[0],
                product.inputs[1],
                node.inputs[1],
                scale.inputs[1 - slot],
                OutletId::new(id, 0),
            ];
            let facts = outlets
                .iter()
                .map(|&o| model.outlet_fact(o))
                .collect::<TractResult<Vec<_>>>()?;
            if facts
                .iter()
                .any(|f| f.datum_type != DatumType::F32 || f.shape.as_concrete().is_none())
            {
                continue;
            }
            let shapes: [Vec<usize>; 5] =
                std::array::from_fn(|i| facts[i].shape.as_concrete().unwrap().to_vec());
            let Some(plan) = TiledAttention::new(shapes) else {
                continue;
            };
            let expected = [plan.shapes[0][0], plan.queries, plan.keys];
            if [product.id, scale.id, softmax.id].iter().any(|&id| {
                let fact = &model.node(id).outputs[0].fact;
                fact.datum_type != DatumType::F32
                    || fact.shape.as_concrete() != Some(expected.as_slice())
            }) {
                continue;
            }
            let mut patch = TypedModelPatch::default();
            let inputs = outlets[..4]
                .iter()
                .map(|&o| patch.tap_model(model, o))
                .collect::<TractResult<Vec<_>>>()?;
            let replacement = patch.wire_node(format!("{}.tiled", node.name), plan, &inputs)?;
            patch.shunt_outside(model, outlets[4], replacement[0])?;
            patch.apply(model)?;
            count += 1;
            break;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tract_onnx::tract_core::ops::math;

    fn data(shape: &[usize], phase: f32) -> TractResult<TValue> {
        Ok(Tensor::from_shape(
            shape,
            &(0..shape.iter().product())
                .map(|i| (i as f32 * 0.173 + phase).sin() * 0.3)
                .collect::<Vec<_>>(),
        )?
        .into())
    }
    fn graph(
        b: usize,
        m: usize,
        n: usize,
        d: usize,
        c: usize,
        scale: f32,
    ) -> TractResult<TypedModel> {
        let mut model = TypedModel::default();
        let q = model.add_source("q", f32::fact([b, m, d]))?;
        let k = model.add_source("k", f32::fact([b, d, n]))?;
        let v = model.add_source("v", f32::fact([b, n, c]))?;
        let scores = model.wire_node(
            "scores",
            EinSum::new("amk,akn->amn".parse()?, DatumType::F32),
            &[q, k],
        )?[0];
        let fac = model.add_const("scale", Tensor::from_shape(&[1, 1, 1], &[scale])?)?;
        let scaled = model.wire_node("scaled", math::mul(), &[scores, fac])?[0];
        let probs = model.wire_node(
            "probabilities",
            Softmax {
                axes: tvec!(2),
                quant_output_dt: None,
                kind: SoftmaxKind::default(),
            },
            &[scaled],
        )?[0];
        let out = model.wire_node(
            "output",
            EinSum::new("amk,akn->amn".parse()?, DatumType::F32),
            &[probs, v],
        )?;
        model.select_output_outlets(&out)?;
        Ok(model)
    }
    fn close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (&a, &b) in a.iter().zip(b) {
            assert!(
                a.is_finite() && b.is_finite() && (a - b).abs() < 2e-5 * (1. + b.abs()),
                "{a} != {b}"
            );
        }
    }
    #[test]
    fn tiled_attention_matches_tract_for_batches_block_tails_and_scaling() -> TractResult<()> {
        for (b, m, n, d, c) in [
            (2, 1, 16, 1, 1),
            (2, 31, 33, 3, 3),
            (3, 65, 63, 7, 5),
            (1, 35, 512, 3, 3),
            (1, 3, 2048, 16, 16),
        ] {
            for scale in [0., 1.7, -3., 100.] {
                let mut model = graph(b, m, n, d, c, scale)?;
                let original = model.clone().into_optimized()?.into_runnable()?;
                assert_eq!(optimize(&mut model)?, 1);
                let op = model
                    .nodes()
                    .iter()
                    .find_map(|n| n.op_as::<TiledAttention>())
                    .unwrap()
                    .clone();
                let optimized = model.into_optimized()?.into_runnable()?;
                let inputs = tvec!(
                    data(&[b, m, d], 0.)?,
                    data(&[b, d, n], 0.3)?,
                    data(&[b, n, c], 0.7)?
                );
                let reference = original.run(inputs.clone())?;
                let actual = optimized.run(inputs.clone())?;
                let reference = reference[0].to_plain_array_view::<f32>()?;
                close(
                    actual[0].to_plain_array_view::<f32>()?.as_slice().unwrap(),
                    reference.as_slice().unwrap(),
                );
                let q = inputs[0].to_plain_array_view::<f32>()?;
                let k = inputs[1].to_plain_array_view::<f32>()?;
                let v = inputs[2].to_plain_array_view::<f32>()?;
                let mut portable = vec![0.; b * m * c];
                op.rows(
                    [
                        q.as_slice().unwrap(),
                        k.as_slice().unwrap(),
                        v.as_slice().unwrap(),
                        &[scale],
                    ],
                    0,
                    &mut portable,
                    false,
                )?;
                close(&portable, reference.as_slice().unwrap());
            }
        }
        Ok(())
    }
    #[test]
    fn attention_rewrite_rejects_changed_semantics_or_observable_intermediates() -> TractResult<()>
    {
        for change in 0..8 {
            let mut model = graph(2, 35, 33, 3, 3, 2.)?;
            let prob = model.node_by_name("probabilities")?.id;
            let score = model.node_by_name("scores")?.id;
            let scaled = model.node_by_name("scaled")?.id;
            let output = model.node_by_name("output")?.id;
            match change {
                0 => {
                    model.node_mut(prob).op = Box::new(Softmax {
                        axes: tvec!(1),
                        quant_output_dt: None,
                        kind: SoftmaxKind::default(),
                    })
                }
                1 => {
                    model.node_mut(prob).op = Box::new(Softmax {
                        axes: tvec!(2),
                        quant_output_dt: None,
                        kind: SoftmaxKind::LogSoftmax,
                    })
                }
                2 => model.node_mut(scaled).op = Box::new(math::add()),
                3 => model
                    .select_output_outlets(&[OutletId::new(output, 0), OutletId::new(score, 0)])?,
                4 => model
                    .select_output_outlets(&[OutletId::new(output, 0), OutletId::new(scaled, 0)])?,
                5 => model
                    .select_output_outlets(&[OutletId::new(output, 0), OutletId::new(prob, 0)])?,
                6 => {
                    model.node_mut(score).op =
                        Box::new(EinSum::new("amk,akn->anm".parse()?, DatumType::F32))
                }
                _ => {
                    model.node_mut(output).op =
                        Box::new(EinSum::new("amk,akn->anm".parse()?, DatumType::F32))
                }
            }
            assert_eq!(optimize(&mut model)?, 0, "change {change}");
        }
        for (b, m, n, d, c) in [(1, 3, 8, 3, 3), (1, 3, 33, 17, 3), (1, 3, 33, 3, 17)] {
            assert_eq!(optimize(&mut graph(b, m, n, d, c, 2.)?)?, 0);
        }
        Ok(())
    }
    #[test]
    fn tiled_attention_rejects_invalid_geometry_and_runtime_inputs() -> TractResult<()> {
        let shapes = [
            vec![2, 35, 3],
            vec![2, 3, 33],
            vec![2, 33, 3],
            vec![1, 1, 1],
            vec![2, 35, 3],
        ];
        let op = TiledAttention::new(shapes.clone()).unwrap();
        for i in 0..5 {
            let mut invalid = shapes.clone();
            invalid[i][0] = 0;
            assert!(TiledAttention::new(invalid).is_none());
        }
        let mut invalid = shapes.clone();
        invalid[0][1] = usize::MAX;
        invalid[4][1] = usize::MAX;
        assert!(TiledAttention::new(invalid).is_none());
        for invalid in [
            Tensor::zero::<i32>(&shapes[0])?.into(),
            data(&[2, 3, 35], 0.)?,
        ] {
            assert!(op
                .eval(tvec!(
                    invalid,
                    data(&shapes[1], 0.)?,
                    data(&shapes[2], 0.)?,
                    data(&shapes[3], 0.)?
                ))
                .is_err());
        }
        assert!(op.eval(tvec!()).is_err());
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn attention_parallel_rows_match_serial_across_batch_and_tile_boundaries() -> TractResult<()> {
        use tract_linalg::multithread::{multithread_tract_scope, Executor};
        let shapes = [
            vec![3, 713, 3],
            vec![3, 3, 65],
            vec![3, 65, 16],
            vec![1],
            vec![3, 713, 16],
        ];
        let op = TiledAttention::new(shapes.clone()).unwrap();
        let inputs = tvec!(
            data(&shapes[0], 0.)?,
            data(&shapes[1], 0.3)?,
            data(&shapes[2], 0.7)?,
            Tensor::from_shape(&[1], &[2.0f32])?.into()
        );
        let a = multithread_tract_scope(Executor::SingleThread, || op.eval(inputs.clone()))?;
        let b = multithread_tract_scope(Executor::multithread(3), || op.eval(inputs))?;
        close(
            a[0].to_plain_array_view::<f32>()?.as_slice().unwrap(),
            b[0].to_plain_array_view::<f32>()?.as_slice().unwrap(),
        );
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn run_full_plan(
        plan: &std::sync::Arc<TypedSimplePlan>,
        data: TValue,
        previous: bool,
    ) -> TractResult<TVec<TValue>> {
        // The previous Mac production path was single-threaded. Other native
        // platforms already used the bounded pool, which both plans retain.
        #[cfg(target_os = "macos")]
        if previous {
            return tract_linalg::multithread::multithread_tract_scope(
                tract_linalg::multithread::Executor::SingleThread,
                || plan.run(tvec!(data)),
            );
        }
        let _ = previous;
        crate::cpu_threads::run("anti-smudge", || plan.run(tvec!(data)))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "full shipped graph and paired timing; make profile-anti-smudge-attention"]
    fn anti_smudge_tiled_attention_matches_previous_cpu() -> TractResult<()> {
        use std::time::Instant;
        use tract_onnx::prelude::*;
        let bytes = crate::decode_model_bytes(crate::ANTI_SMUDGE_ONNX_XZ)?;
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(bytes.as_ref()))?
            .with_input_fact(0, f32::fact([1, 3, 2048, 2048]).into())?
            .into_typed()?;
        let mut check = model.clone();
        assert_eq!(
            optimize(&mut check)?,
            14,
            "all restoration axial attention blocks must fuse"
        );
        drop(check);
        let before = crate::prepare_cpu_plan_with_options(
            "anti-smudge",
            model.clone(),
            false,
            crate::CpuOptimizations {
                restoration_kernels: true,
                tiled_attention: false,
                image_rows: false,
            },
        )?;
        let after = crate::prepare_cpu_plan_with_options(
            "anti-smudge",
            model,
            false,
            crate::CpuOptimizations {
                image_rows: false,
                ..crate::CpuOptimizations::ALL
            },
        )?;
        let data: TValue = Tensor::from_shape(
            &[1, 3, 2048, 2048],
            &(0..3 * 2048 * 2048)
                .map(|i| (i * 17 % 101) as f32 / 100.)
                .collect::<Vec<_>>(),
        )?
        .into();
        let mut expected: Option<TVec<TValue>> = None;
        let mut times = [Vec::new(), Vec::new()];
        let mut max_difference = 0.0f32;
        for round in 0..4 {
            for slot in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                let output = run_full_plan(
                    if slot == 0 { &before } else { &after },
                    data.clone(),
                    slot == 0,
                )?;
                let seconds = start.elapsed().as_secs_f64();
                eprintln!("attention round={round} tiled={}: {seconds:.3}s", slot == 1);
                if round != 0 {
                    times[slot].push(seconds);
                }
                if let Some(reference) = &expected {
                    let reference = reference[0].to_plain_array_view::<f32>()?;
                    let actual = output[0].to_plain_array_view::<f32>()?;
                    assert_eq!(actual.shape(), reference.shape());
                    for (&a, &b) in actual.iter().zip(reference.iter()) {
                        let difference = (a - b).abs();
                        assert!(
                            a.is_finite() && b.is_finite() && difference < 5e-5,
                            "restoration output {a} != {b}"
                        );
                        max_difference = max_difference.max(difference);
                    }
                } else {
                    expected = Some(output);
                }
            }
        }
        let scene: TValue = Tensor::from_shape(
            &[1, 3, 2048, 2048],
            &(0..3 * 2048 * 2048)
                .map(|i| {
                    let c = i / (2048 * 2048);
                    let y = i / 2048 % 2048;
                    let x = i % 2048;
                    if x % 227 < 8 && y % 193 < 8 {
                        return 1.;
                    }
                    let dx = x as f32 - 641.;
                    let dy = y as f32 - 719.;
                    (0.02
                        + 0.07 * c as f32
                        + x as f32 / 8192.
                        + y as f32 / 16384.
                        + 18. / (dx.hypot(dy) + 50.)
                        + (x % 13) as f32 / 1000.)
                        .min(1.)
                })
                .collect::<Vec<_>>(),
        )?
        .into();
        let reference = run_full_plan(&before, scene.clone(), true)?;
        let actual = run_full_plan(&after, scene, false)?;
        let reference = reference[0].to_plain_array_view::<f32>()?;
        let actual = actual[0].to_plain_array_view::<f32>()?;
        for (&a, &b) in actual.iter().zip(reference.iter()) {
            let difference = (a - b).abs();
            assert!(
                a.is_finite() && b.is_finite() && difference < 5e-5,
                "spatial scene output {a} != {b}"
            );
            max_difference = max_difference.max(difference);
        }
        for values in &mut times {
            values.sort_by(f64::total_cmp);
        }
        eprintln!("Attention paired medians (3 warm runs each): {:.3}s -> {:.3}s; max absolute difference {max_difference}",times[0][1],times[1][1]);
        Ok(())
    }
}
