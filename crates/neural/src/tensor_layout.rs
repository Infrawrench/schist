//! Cache-blocked host transposes for tensors too large to keep on the GPU.
//! In particular, deformable convolution gathers hundreds of MB in NHWC order.
//! A naive strided copy of that tensor spends most of its time missing cache.
use tract_onnx::tract_core::internal::*;
use tract_onnx::tract_core::ops::change_axes::AxisOp;

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(&AxisOp::Move(from, to)) = node.op_as::<AxisOp>() else {
            continue;
        };
        let fact = model.outlet_fact(node.inputs[0])?;
        let Some(shape) = fact.shape.as_concrete() else {
            continue;
        };
        if fact.datum_type != f32::datum_type() || shape.iter().product::<usize>() < 1024 {
            continue;
        }
        if let Some((rows, columns)) = matrix_shape(shape, from, to) {
            let output = node.outputs[0].fact.clone();
            model.node_mut(id).op = Box::new(BlockedTranspose {
                rows,
                columns,
                output,
            });
        }
    }
    Ok(())
}

fn matrix_shape(shape: &[usize], from: usize, to: usize) -> Option<(usize, usize)> {
    if from == to
        || from.max(to) >= shape.len()
        || shape.contains(&0)
        || shape[from.max(to) + 1..].iter().product::<usize>() != 1
    {
        return None;
    }
    let dimensions = if from < to {
        (shape[from], shape[from + 1..=to].iter().product())
    } else {
        (shape[to..from].iter().product(), shape[from])
    };
    // Moving a unit dimension only changes metadata in tract. Do not turn
    // that zero-copy reshape into a full tensor allocation and copy.
    (dimensions.0 > 1 && dimensions.1 > 1).then_some(dimensions)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BlockedTranspose {
    rows: usize,
    columns: usize,
    output: TypedFact,
}
impl Op for BlockedTranspose {
    fn name(&self) -> StaticName {
        "BlockedTranspose".into()
    }
    op_as_typed_op!();
}
impl EvalOp for BlockedTranspose {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let mut result = Tensor::zero::<f32>(self.output.shape.as_concrete().unwrap())?;
        let size = self.rows * self.columns;
        {
            let input = inputs[0].to_plain_array_view::<f32>()?;
            let mut output = result.to_plain_array_view_mut::<f32>()?;
            for (input, output) in input
                .as_slice()
                .context("non-contiguous transpose input")?
                .chunks_exact(size)
                .zip(output.as_slice_mut().unwrap().chunks_exact_mut(size))
            {
                transpose::transpose(input, output, self.columns, self.rows);
            }
        }
        Ok(tvec!(result.into()))
    }
}
impl TypedOp for BlockedTranspose {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(self.output.clone()))
    }
    as_op!();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_moves_match_tensor_permutations_exactly() -> TractResult<()> {
        // Odd tile tails, batches, moves in both directions and trailing units.
        for shape in [vec![2, 17, 37], vec![2, 3, 17, 37, 1], vec![1, 23, 5, 19]] {
            let count = shape.iter().product::<usize>();
            let values: Vec<f32> = (0..count)
                .map(|i| f32::from_bits(i as u32 * 13579))
                .collect();
            let input = Tensor::from_shape(&shape, &values)?;
            for from in 0..shape.len() {
                for to in 0..shape.len() {
                    let Some((rows, columns)) = matrix_shape(&shape, from, to) else {
                        continue;
                    };
                    let expected = input.clone().move_axis(from, to)?;
                    let op = BlockedTranspose {
                        rows,
                        columns,
                        output: f32::fact(expected.shape()),
                    };
                    let actual = op.eval(tvec!(input.clone().into()))?;
                    assert_eq!(actual[0].shape(), expected.shape());
                    assert_eq!(
                        actual[0].to_plain_array_view::<f32>()?,
                        expected.to_plain_array_view::<f32>()?
                    );
                }
            }
        }
        assert!(matrix_shape(&[2, 3, 5], 0, 1).is_none());
        Ok(())
    }
}
