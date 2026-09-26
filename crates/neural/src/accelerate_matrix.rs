//! Run compatible float32 model contractions with macOS Accelerate BLAS.
//! Shape/stride proofs select direct matrix views; unsupported layouts retain
//! tract. No tensor repacking, quantization or model resolution changes.
use tract_onnx::tract_core::{internal::*, ops::einsum::EinSum};

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    // The stable LP64 CBLAS ABI also supports macOS versions before 13.3.
    fn cblas_sgemm(
        order: i32,
        trans_a: i32,
        trans_b: i32,
        m: i32,
        n: i32,
        k: i32,
        alpha: f32,
        a: *const f32,
        lda: i32,
        b: *const f32,
        ldb: i32,
        beta: f32,
        c: *mut f32,
        ldc: i32,
    );
}

fn volume(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |n, &d| if d == 0 { None } else { n.checked_mul(d) })
}
fn strides(shape: &[usize]) -> Option<Vec<usize>> {
    volume(shape)?;
    let mut result = vec![1; shape.len()];
    for i in (1..shape.len()).rev() {
        result[i - 1] = result[i] * shape[i];
    }
    Some(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Axis {
    dim: usize,
    stride: [usize; 3],
}

// Compound row/column axes can be flattened only when every stride agrees.
fn linear(axes: &[Axis], input: usize) -> Option<(usize, usize)> {
    let step = axes.last().map_or(1, |a| a.stride[input]);
    let mut count = 1usize;
    for a in axes.iter().rev() {
        if a.stride[input] != count.checked_mul(step)? {
            return None;
        }
        count = count.checked_mul(a.dim)?;
    }
    Some((count, step))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Matrix {
    transpose: i32,
    leading: i32,
    span: usize,
}
impl Matrix {
    fn new(rows: usize, cols: usize, rs: usize, cs: usize) -> Option<Self> {
        let (transpose, leading) = if cs == 1 || cols == 1 {
            let leading = if rows == 1 { cols } else { rs };
            if leading < cols {
                return None;
            }
            (111, leading)
        } else if rs == 1 || rows == 1 {
            let leading = if cols == 1 { rows } else { cs };
            if leading < rows {
                return None;
            }
            (112, leading)
        } else {
            return None;
        };
        let span = (rows.checked_sub(1)?)
            .checked_mul(rs)?
            .checked_add((cols.checked_sub(1)?).checked_mul(cs)?)?
            .checked_add(1)?;
        Some(Self {
            transpose,
            leading: i32::try_from(leading).ok()?,
            span,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MatrixProduct {
    shapes: [Vec<usize>; 3],
    m: i32,
    n: i32,
    k: i32,
    a: Matrix,
    b: Matrix,
    c: Matrix,
    batches: Vec<Axis>,
    batch_count: usize,
}
impl MatrixProduct {
    fn new(op: &EinSum, shapes: [Vec<usize>; 3]) -> Option<Self> {
        if op.q_params.is_some()
            || op.operating_dt != DatumType::F32
            || op.axes.input_count() != 2
            || op.axes.output_count() != 1
        {
            return None;
        }
        if op.axes.rank(InOut::In(0)) != shapes[0].len()
            || op.axes.rank(InOut::In(1)) != shapes[1].len()
            || op.axes.rank(InOut::Out(0)) != shapes[2].len()
        {
            return None;
        }
        let lengths = [
            volume(&shapes[0])?,
            volume(&shapes[1])?,
            volume(&shapes[2])?,
        ];
        let layout = [
            strides(&shapes[0])?,
            strides(&shapes[1])?,
            strides(&shapes[2])?,
        ];
        let mut output: Vec<Option<Axis>> = vec![None; shapes[2].len()];
        let mut reduction = Vec::new();
        for axis in op.axes.iter_all_axes() {
            if axis.inputs.iter().any(|a| a.len() > 1) || axis.outputs[0].len() > 1 {
                return None;
            }
            let mut dim = 1;
            let mut stride = [0; 3];
            for (i, slot) in stride.iter_mut().enumerate().take(2) {
                if let Some(&j) = axis.inputs[i].first() {
                    let d = *shapes[i].get(j)?;
                    if d != 1 && dim != 1 && d != dim {
                        return None;
                    }
                    dim = dim.max(d);
                    if d > 1 {
                        *slot = layout[i][j];
                    }
                }
            }
            if let Some(&j) = axis.outputs[0].first() {
                if *shapes[2].get(j)? != dim {
                    return None;
                }
                stride[2] = layout[2][j];
                output[j] = Some(Axis { dim, stride });
            } else if dim > 1 {
                if stride[0] == 0 || stride[1] == 0 {
                    return None;
                }
                reduction.push(Axis { dim, stride });
            }
        }
        if reduction.is_empty() {
            return None;
        }
        // Reshape folding can split an attention token axis into height and
        // width. Flatten it only when both operands use the same linear order.
        reduction.sort_by_key(|a| std::cmp::Reverse(a.stride[0]));
        let (k, ak) = linear(&reduction, 0)?;
        let (bk_count, bk) = linear(&reduction, 1)?;
        if bk_count != k {
            return None;
        }
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        let mut batches = Vec::new();
        for axis in output {
            let axis = axis?;
            if axis.dim == 1 {
                continue;
            }
            match (axis.stride[0] != 0, axis.stride[1] != 0) {
                (true, false) => rows.push(axis),
                (false, true) => columns.push(axis),
                (true, true) => batches.push(axis),
                _ => return None,
            }
        }
        if rows.is_empty() || columns.is_empty() {
            return None;
        }
        let (m, am) = linear(&rows, 0)?;
        let (n, bn) = linear(&columns, 1)?;
        let (_, cm) = linear(&rows, 2)?;
        let (_, cn) = linear(&columns, 2)?;
        let a = Matrix::new(m, k, am, ak)?;
        let b = Matrix::new(k, n, bk, bn)?;
        let c = Matrix::new(m, n, cm, cn)?;
        let batch_count = volume(&batches.iter().map(|a| a.dim).collect::<Vec<_>>())?;
        if m.checked_mul(n)?.checked_mul(batch_count)? != lengths[2] {
            return None;
        }
        // Prove the complete extent of every FFI view, including broadcasts
        // and interleaved batch axes, before a pointer can reach BLAS.
        for (i, span) in [a.span, b.span, c.span].into_iter().enumerate() {
            let maximum_offset = batches.iter().try_fold(0usize, |n, axis| {
                n.checked_add((axis.dim - 1).checked_mul(axis.stride[i])?)
            })?;
            if maximum_offset.checked_add(span)? > lengths[i] {
                return None;
            }
        }
        Some(Self {
            shapes,
            m: m.try_into().ok()?,
            n: n.try_into().ok()?,
            k: k.try_into().ok()?,
            a,
            b,
            c,
            batches,
            batch_count,
        })
    }
}

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<usize> {
    let mut count = 0;
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(op) = node.op_as::<EinSum>() else {
            continue;
        };
        if node.inputs.len() != 2 || node.outputs.len() != 1 {
            continue;
        }
        let facts = [
            model.outlet_fact(node.inputs[0])?,
            model.outlet_fact(node.inputs[1])?,
            &node.outputs[0].fact,
        ];
        if facts
            .iter()
            .any(|f| f.datum_type != DatumType::F32 || f.shape.as_concrete().is_none())
        {
            continue;
        }
        let shapes = facts.map(|f| f.shape.as_concrete().unwrap().to_vec());
        let Some(product) = MatrixProduct::new(op, shapes.clone()) else {
            if std::env::var_os("SCHIST_MATRIX_LAYOUTS").is_some() {
                log::info!(target: "schist_neural::execution", "matrix fallback {}: {shapes:?}", op.axes);
            }
            continue;
        };
        let work = (product.m as usize)
            .saturating_mul(product.n as usize)
            .saturating_mul(product.k as usize);
        if work < 1_000_000 {
            continue;
        }
        model.node_mut(id).op = Box::new(product);
        count += 1;
    }
    Ok(count)
}

impl Op for MatrixProduct {
    fn name(&self) -> StaticName {
        "AccelerateMatMul".into()
    }
    op_as_typed_op!();
}
impl TypedOp for MatrixProduct {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(inputs.len() == 2, "matrix input count changed");
        for (input, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(
                input.datum_type == DatumType::F32
                    && input.shape.as_concrete() == Some(shape.as_slice()),
                "matrix input fact changed"
            );
        }
        Ok(tvec!(f32::fact(&self.shapes[2])))
    }
    as_op!();
}
impl EvalOp for MatrixProduct {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(inputs.len() == 2, "matrix input count changed");
        for (input, shape) in inputs.iter().zip(&self.shapes) {
            ensure!(input.shape() == shape, "matrix input shape changed");
        }
        let a = inputs[0].to_plain_array_view::<f32>()?;
        let a = a.as_slice().context("non-contiguous matrix input")?;
        let b = inputs[1].to_plain_array_view::<f32>()?;
        let b = b.as_slice().context("non-contiguous matrix input")?;
        let mut result = Tensor::zero::<f32>(&self.shapes[2])?;
        {
            let mut view = result.to_plain_array_view_mut::<f32>()?;
            let c = view
                .as_slice_mut()
                .context("non-contiguous matrix output")?;
            for batch in 0..self.batch_count {
                let mut index = batch;
                let mut offsets = [0; 3];
                for axis in self.batches.iter().rev() {
                    let coordinate = index % axis.dim;
                    index /= axis.dim;
                    for (offset, stride) in offsets.iter_mut().zip(axis.stride) {
                        *offset += coordinate * stride;
                    }
                }
                let transpose = |matrix: &Matrix| if matrix.transpose == 111 { 112 } else { 111 };
                // SAFETY: construction checks positive i32 dimensions/leading
                // strides and proves every matrix view lies in its allocation.
                // Runtime inputs have the exact validated shape/type. Inputs are
                // immutable, the separate output is unique, and BLAS is synchronous.
                unsafe {
                    if self.c.transpose == 112 {
                        // (A B)^T = B^T A^T writes transposed output directly.
                        cblas_sgemm(
                            101,
                            transpose(&self.b),
                            transpose(&self.a),
                            self.n,
                            self.m,
                            self.k,
                            1.,
                            b.as_ptr().add(offsets[1]),
                            self.b.leading,
                            a.as_ptr().add(offsets[0]),
                            self.a.leading,
                            0.,
                            c.as_mut_ptr().add(offsets[2]),
                            self.c.leading,
                        );
                    } else {
                        cblas_sgemm(
                            101,
                            self.a.transpose,
                            self.b.transpose,
                            self.m,
                            self.n,
                            self.k,
                            1.,
                            a.as_ptr().add(offsets[0]),
                            self.a.leading,
                            b.as_ptr().add(offsets[1]),
                            self.b.leading,
                            0.,
                            c.as_mut_ptr().add(offsets[2]),
                            self.c.leading,
                        );
                    }
                }
            }
        }
        Ok(tvec!(result.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(shape: &[usize], phase: f32) -> TractResult<TValue> {
        let values = (0..volume(shape).unwrap())
            .map(|i| (i as f32 * 0.37 + phase).sin())
            .collect::<Vec<_>>();
        Ok(Tensor::from_shape(shape, &values)?.into())
    }

    fn parity(equation: &str, shapes: [&[usize]; 3]) -> TractResult<()> {
        let op = EinSum::new(equation.parse()?, DatumType::F32);
        let product = MatrixProduct::new(&op, shapes.map(<[usize]>::to_vec))
            .unwrap_or_else(|| panic!("unsupported fixture {equation} {shapes:?}"));
        let inputs = tvec!(values(shapes[0], 0.)?, values(shapes[1], 0.8)?);
        // Independently evaluate the original general Einstein contraction.
        // This checks indexing/layout as well as BLAS accumulation differences.
        let expected = op.eval(inputs.clone())?;
        let actual = product.eval(inputs)?;
        assert_eq!(actual[0].shape(), shapes[2]);
        let a = actual[0].to_plain_array_view::<f32>()?;
        let b = expected[0].to_plain_array_view::<f32>()?;
        for (&a, &b) in a.iter().zip(b.iter()) {
            assert!(
                a.is_finite() && (a - b).abs() < 2e-5 * (1. + b.abs()),
                "{equation}: {a} != {b}"
            );
        }
        Ok(())
    }

    #[test]
    fn matrix_products_match_einsum_for_input_and_output_transposes() -> TractResult<()> {
        for (eq, a, b, c) in [
            ("mk,kn->mn", [7, 9], [9, 11], [7, 11]),
            ("km,kn->mn", [9, 7], [9, 11], [7, 11]),
            ("mk,nk->mn", [7, 9], [11, 9], [7, 11]),
            ("km,nk->mn", [9, 7], [11, 9], [7, 11]),
            ("mk,kn->nm", [7, 9], [9, 11], [11, 7]),
            ("km,nk->nm", [9, 7], [11, 9], [11, 7]),
        ] {
            parity(eq, [&a, &b, &c])?;
        }
        Ok(())
    }

    #[test]
    fn matrix_products_match_einsum_with_interleaved_heads_and_split_reduction() -> TractResult<()>
    {
        parity(
            "bmhk,bkhn->bmhn",
            [&[2, 7, 3, 9], &[2, 9, 3, 11], &[2, 7, 3, 11]],
        )?;
        parity(
            "bmhk,bnhk->bmhn",
            [&[2, 7, 3, 9], &[2, 11, 3, 9], &[2, 7, 3, 11]],
        )?;
        parity(
            "bmhk,bnhk->bnhm",
            [&[2, 7, 3, 9], &[2, 11, 3, 9], &[2, 11, 3, 7]],
        )?;
        parity(
            "bmhij,bijhn->bmhn",
            [&[2, 7, 3, 4, 5], &[2, 4, 5, 3, 11], &[2, 7, 3, 11]],
        )?;
        Ok(())
    }

    #[test]
    fn matrix_products_fold_contiguous_rows_and_broadcast_weights() -> TractResult<()> {
        parity("abmk,kn->abmn", [&[2, 3, 7, 9], &[9, 11], &[2, 3, 7, 11]])?;
        parity(
            "abmk,abkn->abmn",
            [&[2, 3, 7, 9], &[1, 1, 9, 11], &[2, 3, 7, 11]],
        )?;
        parity("bmk,bkn->bmn", [&[3, 17, 65], &[3, 65, 19], &[3, 17, 19]])?;
        Ok(())
    }

    #[test]
    fn matrix_planner_rejects_invalid_or_non_matrix_views_before_ffi() -> TractResult<()> {
        let op = EinSum::new("mk,kn->mn".parse()?, DatumType::F32);
        for shapes in [
            [vec![2, 3, 1], vec![3, 4], vec![2, 4]], // rank mismatch
            [vec![0, 3], vec![3, 4], vec![0, 4]],
            [vec![2, 3], vec![4, 4], vec![2, 4]],
            [vec![2, 3], vec![3, 4], vec![2, 5]],
            [vec![usize::MAX, 4], vec![4, 3], vec![usize::MAX, 3]],
            [
                vec![2, i32::MAX as usize + 1],
                vec![i32::MAX as usize + 1, 2],
                vec![2, 2],
            ],
        ] {
            assert!(MatrixProduct::new(&op, shapes).is_none());
        }
        let diagonal = EinSum::new("mm,mn->mn".parse()?, DatumType::F32);
        assert!(MatrixProduct::new(&diagonal, [vec![3, 3], vec![3, 4], vec![3, 4]]).is_none());
        let reversed = EinSum::new("mij,jin->mn".parse()?, DatumType::F32);
        assert!(
            MatrixProduct::new(&reversed, [vec![2, 3, 4], vec![4, 3, 5], vec![2, 5]]).is_none()
        );
        let quantized = EinSum::newq("mk,kn->mn".parse()?, DatumType::I32, DatumType::I8);
        assert!(MatrixProduct::new(&quantized, [vec![2, 3], vec![3, 4], vec![2, 4]]).is_none());
        let product = MatrixProduct::new(&op, [vec![2, 3], vec![3, 4], vec![2, 4]]).unwrap();
        assert!(product
            .eval(tvec!(values(&[3, 2], 0.)?, values(&[3, 4], 0.)?))
            .is_err());
        assert!(product
            .eval(tvec!(tensor2(&[[0i32; 3]; 2]).into(), values(&[3, 4], 0.)?))
            .is_err());
        Ok(())
    }

    #[test]
    fn matrix_rewrite_selects_large_float_products_and_preserves_results() -> TractResult<()> {
        for (m, k, n, replaced) in [(128, 129, 130, true), (2, 3, 4, false)] {
            let mut model = TypedModel::default();
            let a = model.add_source("input", f32::fact([m, k]))?;
            let b = values(&[k, n], 0.8)?;
            let b = model.add_const("weights", b.into_tensor())?;
            let out = model.wire_node(
                "product",
                EinSum::new("mk,kn->mn".parse()?, DatumType::F32),
                &[a, b],
            )?;
            model.select_output_outlets(&out)?;
            let original = model.clone().into_optimized()?.into_runnable()?;
            assert_eq!(optimize(&mut model)?, usize::from(replaced));
            assert_eq!(model.node(out[0].node).op_is::<MatrixProduct>(), replaced);
            let optimized = model.into_optimized()?.into_runnable()?;
            let input = tvec!(values(&[m, k], 0.)?);
            let expected = original.run(input.clone())?;
            let actual = optimized.run(input)?;
            let expected = expected[0].to_plain_array_view::<f32>()?;
            let actual = actual[0].to_plain_array_view::<f32>()?;
            for (&a, &b) in actual.iter().zip(expected.iter()) {
                assert!(a.is_finite() && (a - b).abs() < 2e-5 * (1. + b.abs()));
            }
        }
        Ok(())
    }
}
