//! Copy contiguous rows for fixed float32 constant padding.
//! Preserve every input/value bit; other padding modes retain tract.
use tract_onnx::tract_core::{
    internal::*,
    ops::array::{Pad, PadMode},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContiguousPad {
    input: Vec<usize>,
    output: Vec<usize>,
    prefix: Vec<(usize, usize)>,
    origin: usize,
    chunk: usize,
    value: u32,
}

impl ContiguousPad {
    fn new(op: &Pad, input: &TypedFact) -> Option<Self> {
        if input.datum_type != DatumType::F32 {
            return None;
        }
        let shape = input.shape.as_concrete()?;
        if shape.contains(&0) || shape.len() != op.pads.len() {
            return None;
        }
        let PadMode::Constant(value) = &op.mode else {
            return None;
        };
        let value = value.cast_to_scalar::<f32>().ok()?.to_bits();
        let last = op.pads.iter().rposition(|p| *p != (0, 0))?;
        let output: Vec<usize> = shape
            .iter()
            .zip(&op.pads)
            .map(|(&d, &(a, b))| d.checked_add(a)?.checked_add(b))
            .collect::<Option<_>>()?;
        let mut strides = vec![1usize; shape.len()];
        let mut volume = 1usize;
        for i in (0..shape.len()).rev() {
            strides[i] = volume;
            volume = volume.checked_mul(output[i])?;
        }
        let origin = op
            .pads
            .iter()
            .zip(&strides)
            .try_fold(0usize, |n, (p, s)| n.checked_add(p.0.checked_mul(*s)?))?;
        let chunk = shape[last..]
            .iter()
            .try_fold(1usize, |n, &d| n.checked_mul(d))?;
        let prefix: Vec<_> = shape[..last].iter().copied().zip(strides).collect();
        let end = prefix
            .iter()
            .try_fold(origin, |n, &(d, s)| n.checked_add((d - 1).checked_mul(s)?))?
            .checked_add(chunk)?;
        if end > volume {
            return None;
        }
        Some(Self {
            input: shape.to_vec(),
            output,
            prefix,
            origin,
            chunk,
            value,
        })
    }
}

impl Op for ContiguousPad {
    fn name(&self) -> StaticName {
        "ContiguousPad".into()
    }
    op_as_typed_op!();
}
impl EvalOp for ContiguousPad {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        ensure!(
            inputs.len() == 1 && inputs[0].shape() == self.input,
            "padding input shape changed"
        );
        let input = inputs[0].to_plain_array_view::<f32>()?;
        let input = input.as_slice().context("non-contiguous padding input")?;
        let mut result = Tensor::zero::<f32>(&self.output)?;
        {
            let mut output = result.to_plain_array_view_mut::<f32>()?;
            let output = output.as_slice_mut().unwrap();
            if self.value != 0 {
                output.fill(f32::from_bits(self.value));
            }
            for (mut row, source) in input.chunks_exact(self.chunk).enumerate() {
                let mut offset = self.origin;
                for &(dim, stride) in self.prefix.iter().rev() {
                    offset += (row % dim) * stride;
                    row /= dim;
                }
                output[offset..offset + self.chunk].copy_from_slice(source);
            }
        }
        Ok(tvec!(result.into()))
    }
}
impl TypedOp for ContiguousPad {
    fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        ensure!(
            inputs.len() == 1
                && inputs[0].datum_type == DatumType::F32
                && inputs[0].shape.as_concrete() == Some(self.input.as_slice()),
            "padding input fact changed"
        );
        Ok(tvec!(f32::fact(&self.output)))
    }
    as_op!();
}
pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        if node.inputs.len() != 1 || node.outputs.len() != 1 {
            continue;
        }
        let Some(op) = node.op_as::<Pad>() else {
            continue;
        };
        if let Some(replacement) = ContiguousPad::new(op, model.outlet_fact(node.inputs[0])?) {
            model.node_mut(id).op = Box::new(replacement);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pad(pads: Vec<(usize, usize)>, value: f32) -> Pad {
        Pad {
            pads,
            mode: PadMode::Constant(std::sync::Arc::new(tensor0(value))),
        }
    }
    fn bits(value: &TValue) -> TractResult<Vec<u32>> {
        Ok(value
            .to_plain_array_view::<f32>()?
            .iter()
            .map(|v| v.to_bits())
            .collect())
    }
    #[test]
    fn padding_matches_upstream_bitwise_across_ranks_axes_and_values() -> TractResult<()> {
        for rank in 1..=5 {
            let shape: Vec<usize> = (0..rank).map(|i| 2 + i % 2).collect();
            let values = [
                0.,
                -0.,
                f32::from_bits(0x7fc01234),
                f32::INFINITY,
                f32::NEG_INFINITY,
                0.125,
            ];
            let data: Vec<f32> = (0..shape.iter().product())
                .map(|i| values[i % values.len()])
                .collect();
            let input = Tensor::from_shape(&shape, &data)?;
            for mask in 1..(1 << rank) {
                for value in [0., -0., 1.25, f32::from_bits(0x7fc05678)] {
                    let pads = (0..rank)
                        .map(|i| {
                            if mask & (1 << i) != 0 {
                                (i % 3, 1 + i % 2)
                            } else {
                                (0, 0)
                            }
                        })
                        .collect();
                    let original = pad(pads, value);
                    let fast = ContiguousPad::new(&original, &f32::fact(&shape)).unwrap();
                    let expected = original.eval(tvec!(input.clone().into()))?;
                    let actual = fast.eval(tvec!(input.clone().into()))?;
                    assert_eq!(actual[0].shape(), expected[0].shape());
                    assert_eq!(bits(&actual[0])?, bits(&expected[0])?);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn unsupported_padding_and_changed_runtime_inputs_are_rejected() -> TractResult<()> {
        let op = pad(vec![(1, 2), (2, 3)], 0.);
        for mode in [PadMode::Reflect, PadMode::Edge] {
            assert!(ContiguousPad::new(&Pad { mode, ..op.clone() }, &f32::fact([2, 3])).is_none());
        }
        for fact in [
            f32::fact([0, 3]),
            f32::fact([i64::MAX as usize, 3]),
            f32::fact([2]),
            i32::fact([2, 3]),
        ] {
            assert!(ContiguousPad::new(&op, &fact).is_none());
        }
        assert!(ContiguousPad::new(&pad(vec![(0, 0); 2], 0.), &f32::fact([2, 3])).is_none());
        let fast = ContiguousPad::new(&op, &f32::fact([2, 3])).unwrap();
        assert!(fast
            .eval(tvec!(Tensor::zero::<f32>(&[3, 2])?.into()))
            .is_err());
        assert!(fast
            .eval(tvec!(Tensor::zero::<i32>(&[2, 3])?.into()))
            .is_err());
        Ok(())
    }
    #[test]
    fn optimized_padding_graph_matches_original() -> TractResult<()> {
        let shape = [2, 3, 4, 5];
        let mut model = TypedModel::default();
        let source = model.add_source("input", f32::fact(shape))?;
        let output = model.wire_node(
            "pad",
            pad(vec![(0, 0), (0, 0), (1, 2), (2, 3)], -0.),
            &[source],
        )?;
        model.select_output_outlets(&output)?;
        let mut model = model.into_optimized()?;
        let original = model.clone().into_runnable()?;
        optimize(&mut model)?;
        assert!(model.nodes().iter().any(|n| n.op_is::<ContiguousPad>()));
        let input =
            Tensor::from_shape(&shape, &(0..120).map(|i| i as f32 / 7.).collect::<Vec<_>>())?;
        let expected = original.run(tvec!(input.clone().into()))?;
        let actual = model.into_runnable()?.run(tvec!(input.into()))?;
        assert_eq!(bits(&actual[0])?, bits(&expected[0])?);
        Ok(())
    }
}
