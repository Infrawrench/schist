//! Accurate float32 vector exponentials for macOS attention softmax.
//! Keep stable maximum subtraction, using SIMD summation and normalization.
//! Vector exp and reduction can round differently; no quantization or fast-exp
//! approximation is used.
use tract_onnx::tract_core::{
    internal::*,
    ops::nn::{Softmax, SoftmaxExp, SoftmaxKind},
};

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    // https://developer.apple.com/documentation/accelerate/vvexpf(_:_:_:)
    fn vvexpf(output: *mut f32, input: *const f32, count: *const i32);
}

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(op) = node.op_as::<Softmax>() else {
            continue;
        };
        let fact = model.outlet_fact(node.inputs[0])?;
        let Some(shape) = fact.shape.as_concrete() else {
            continue;
        };
        let mut axes = op.axes.clone();
        axes.sort_unstable();
        if fact.datum_type != DatumType::F32
            || op.quant_output_dt.is_some()
            || op.kind != SoftmaxKind::Softmax(SoftmaxExp::Libc)
            || axes.is_empty()
            || axes.len() > shape.len()
            || !axes
                .iter()
                .enumerate()
                .all(|(i, &a)| a == shape.len() - axes.len() + i)
        {
            continue;
        }
        let Some(len) = shape[shape.len() - axes.len()..]
            .iter()
            .try_fold(1usize, |a, &b| a.checked_mul(b))
        else {
            continue;
        };
        if !(16..=i32::MAX as usize).contains(&len) {
            continue;
        }
        model.node_mut(id).op = Box::new(VectorSoftmax {
            row: len,
            output: fact.clone(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VectorSoftmax {
    row: usize,
    output: TypedFact,
}
impl Op for VectorSoftmax {
    fn name(&self) -> StaticName {
        "VectorSoftmax".into()
    }
    op_as_typed_op!();
}
impl TypedOp for VectorSoftmax {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(self.output.clone()))
    }
    as_op!();
}
impl EvalOp for VectorSoftmax {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let mut result = args_1!(inputs).into_tensor();
        ensure!(
            result.shape() == self.output.shape.as_concrete().unwrap(),
            "softmax input shape changed"
        );
        let mut scratch = vec![0.; self.row];
        let count = i32::try_from(self.row)?;
        {
            let mut view = result.to_plain_array_view_mut::<f32>()?;
            let values = view
                .as_slice_mut()
                .context("non-contiguous softmax input")?;
            let maximum = (tract_linalg::ops().max_f32)();
            let sum = (tract_linalg::ops().sum_f32)();
            let multiply = (tract_linalg::ops().mul_by_scalar_f32)();
            for row in values.chunks_exact_mut(self.row) {
                let max = maximum.run(row)?;
                for (out, &value) in scratch.iter_mut().zip(row.iter()) {
                    *out = value - max;
                }
                // SAFETY: both disjoint float slices contain exactly count
                // elements, count fits i32, and vForce completes synchronously.
                unsafe {
                    vvexpf(row.as_mut_ptr(), scratch.as_ptr(), &count);
                }
                multiply.run_with_params(row, sum.run(row)?.recip())?;
            }
        }
        Ok(tvec!(result.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_softmax_matches_scalar_probabilities_including_extreme_logits() -> TractResult<()> {
        for len in [16, 31, 196, 2304] {
            let scalar = Softmax {
                axes: tvec!(1),
                quant_output_dt: None,
                kind: SoftmaxKind::default(),
            };
            for scale in [0.0, 0.1, 10., 1000.] {
                let input = Tensor::from_shape(
                    &[3, len],
                    &(0..3 * len)
                        .map(|i| ((i as f32 * 0.173).sin() - 0.5) * scale)
                        .collect::<Vec<_>>(),
                )?;
                let expected = scalar.eval(tvec!(input.clone().into()))?;
                let op = VectorSoftmax {
                    row: len,
                    output: f32::fact([3, len]),
                };
                let actual = op.eval(tvec!(input.into()))?;
                let expected = expected[0].to_plain_array_view::<f32>()?;
                let actual = actual[0].to_plain_array_view::<f32>()?;
                for (a, b) in actual.iter().zip(expected.iter()) {
                    assert!((a - b).abs() < 5e-7, "probability error {}", (a - b).abs());
                }
                for row in actual.as_slice().unwrap().chunks_exact(len) {
                    assert!((row.iter().sum::<f32>() - 1.).abs() < 5e-5);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn vector_softmax_only_replaces_contiguous_trailing_float_axes() -> TractResult<()> {
        for (axes, replaced) in [
            (tvec!(2), true),
            (tvec!(1, 2), true),
            (tvec!(0), false),
            (tvec!(0, 2), false),
        ] {
            let mut model = TypedModel::default();
            let input = model.add_source("input", f32::fact([2, 3, 16]))?;
            let output = model.wire_node(
                "softmax",
                Softmax {
                    axes,
                    quant_output_dt: None,
                    kind: SoftmaxKind::default(),
                },
                &[input],
            )?;
            model.select_output_outlets(&output)?;
            optimize(&mut model)?;
            assert_eq!(
                model.node(output[0].node).op_is::<VectorSoftmax>(),
                replaced
            );
        }
        Ok(())
    }
}
