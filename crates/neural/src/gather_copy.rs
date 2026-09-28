//! Contiguous slice copies for concrete float GatherND tensors. BiRefNet's
//! deformable convolutions otherwise build an ndarray view per index tuple.
//! Semantics: https://onnx.ai/onnx/operators/onnx__GatherND.html
use tract_onnx::tract_core::{internal::*, ops::array::GatherNd};

pub(super) fn optimize(model: &mut TypedModel) -> TractResult<()> {
    for id in model.eval_order()? {
        let node = model.node(id);
        let Some(gather) = node.op_as::<GatherNd>() else {
            continue;
        };
        if let Some(op) = GatherCopy::new(
            model.outlet_fact(node.inputs[0])?,
            model.outlet_fact(node.inputs[1])?,
            gather.batch_dims,
        ) {
            model.node_mut(id).op = Box::new(op);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GatherCopy {
    data_shape: Vec<usize>,
    indices_shape: Vec<usize>,
    // Indexed axis size and stride in the contiguous data tensor.
    axes: Vec<(usize, usize)>,
    batch_stride: usize,
    tuples_per_batch: usize,
    slice_len: usize,
    output: TypedFact,
}

impl GatherCopy {
    fn new(data: &TypedFact, indices: &TypedFact, batch: usize) -> Option<Self> {
        let ds = data.shape.as_concrete()?;
        let ix = indices.shape.as_concrete()?;
        if data.datum_type != f32::datum_type()
            || !matches!(
                indices.datum_type,
                DatumType::I32 | DatumType::I64 | DatumType::TDim
            )
            || batch >= ds.len()
            || batch >= ix.len()
            || ds.contains(&0)
            || ix.contains(&0)
            || ds[..batch] != ix[..batch]
        {
            return None;
        }
        let k = *ix.last()?;
        if k == 0 || k > ds.len() - batch {
            return None;
        }
        let volume = |s: &[usize]| s.iter().try_fold(1usize, |a, &b| a.checked_mul(b));
        let axes = (batch..batch + k)
            .map(|i| Some((ds[i], volume(&ds[i + 1..])?)))
            .collect::<Option<Vec<_>>>()?;
        let mut shape = ix[..ix.len() - 1].to_vec();
        shape.extend_from_slice(&ds[batch + k..]);
        volume(&shape)?;
        Some(Self {
            data_shape: ds.to_vec(),
            indices_shape: ix.to_vec(),
            axes,
            batch_stride: volume(&ds[batch..])?,
            tuples_per_batch: volume(&ix[batch..ix.len() - 1])?,
            slice_len: volume(&ds[batch + k..])?,
            output: f32::fact(shape),
        })
    }
}

impl Op for GatherCopy {
    fn name(&self) -> StaticName {
        "GatherCopy".into()
    }
    op_as_typed_op!();
}
impl EvalOp for GatherCopy {
    fn is_stateless(&self) -> bool {
        true
    }
    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
        let (data, indices) = args_2!(inputs);
        ensure!(
            data.shape() == self.data_shape && indices.shape() == self.indices_shape,
            "GatherCopy input shape changed"
        );
        let data = data.to_plain_array_view::<f32>()?;
        let data = data.as_slice().context("non-contiguous GatherCopy data")?;
        // Preserve 64-bit bounds checks rather than truncating large indices.
        let indices = indices.cast_to::<i64>()?;
        let indices = indices.to_plain_array_view::<i64>()?;
        let indices = indices
            .as_slice()
            .context("non-contiguous GatherCopy indices")?;
        let mut result = Tensor::zero::<f32>(self.output.shape.as_concrete().unwrap())?;
        {
            let mut output = result.to_plain_array_view_mut::<f32>()?;
            let output = output.as_slice_mut().unwrap();
            multithread::par_chunks_mut(output, self.slice_len, output.len(), |first, chunk| {
                for (row, out) in chunk.chunks_exact_mut(self.slice_len).enumerate() {
                    let tuple = first + row;
                    let mut offset = tuple / self.tuples_per_batch * self.batch_stride;
                    let start = tuple * self.axes.len();
                    for (&index, &(size, stride)) in indices[start..start + self.axes.len()]
                        .iter()
                        .zip(&self.axes)
                    {
                        let size = i64::try_from(size)?;
                        let index = if index < 0 { index + size } else { index };
                        ensure!((0..size).contains(&index), "GatherND index out of bounds");
                        offset += index as usize * stride;
                    }
                    out.copy_from_slice(&data[offset..offset + self.slice_len]);
                }
                Ok(())
            })?;
        }
        Ok(tvec!(result.into()))
    }
}
impl TypedOp for GatherCopy {
    fn output_facts(&self, _: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(self.output.clone()))
    }
    as_op!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_gathers_match_upstream_across_batches_and_slice_ranks() -> TractResult<()> {
        for ds in [vec![7, 5, 3], vec![2, 3, 5, 7, 11]] {
            for batch in 0..ds.len() {
                for k in 1..=ds.len() - batch {
                    let mut ix = ds[..batch].to_vec();
                    ix.extend([13, k]);
                    let count = ix.iter().product::<usize>();
                    let values: Vec<i64> = (0..count)
                        .map(|i| ((i * 7 + i / k) % ds[batch + i % k]) as i64)
                        .collect();
                    let input = Tensor::from_shape(
                        &ds,
                        &(0..ds.iter().product())
                            .map(|i| i as f32)
                            .collect::<Vec<_>>(),
                    )?;
                    let indices = Tensor::from_shape(&ix, &values)?;
                    let op = GatherCopy::new(&f32::fact(&ds), &i64::fact(&ix), batch).unwrap();
                    let expected = GatherNd::new(batch)
                        .eval(tvec!(input.clone().into(), indices.clone().into()))?;
                    let actual = op.eval(tvec!(input.clone().into(), indices.into()))?;
                    assert_eq!(
                        actual[0].to_plain_array_view::<f32>()?,
                        expected[0].to_plain_array_view::<f32>()?
                    );
                    // Equivalent negative coordinates, including the -size bound.
                    let negative: Vec<i64> = values
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| v - ds[batch + i % k] as i64)
                        .collect();
                    let actual = op.eval(tvec!(
                        input.into(),
                        Tensor::from_shape(&ix, &negative)?.into()
                    ))?;
                    assert_eq!(
                        actual[0].to_plain_array_view::<f32>()?,
                        expected[0].to_plain_array_view::<f32>()?
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn invalid_coordinates_return_errors_without_truncation() -> TractResult<()> {
        let op = GatherCopy::new(&f32::fact([2, 3]), &i64::fact([1, 1]), 0).unwrap();
        for invalid in [2, -3, i64::MAX, i64::MIN, 1i64 << 32] {
            assert!(op
                .eval(tvec!(
                    Tensor::zero::<f32>(&[2, 3])?.into(),
                    Tensor::from_shape(&[1, 1], &[invalid])?.into()
                ))
                .is_err());
        }
        Ok(())
    }

    #[test]
    fn dimension_indices_used_by_optimized_birefnet_are_supported() -> TractResult<()> {
        let ds = [1, 1, 2, 3, 5];
        let ix = [1, 1, 3, 2];
        let op = GatherCopy::new(&f32::fact(ds), &TDim::fact(ix), 2).unwrap();
        let data = Tensor::from_shape(&ds, &(0..30).map(|i| i as f32).collect::<Vec<_>>())?;
        let indices = Tensor::from_shape(&ix, &[0i64, 2, 1, 0, 1, 1])?
            .cast_to::<TDim>()?
            .into_owned();
        let expected = GatherNd::new(2).eval(tvec!(data.clone().into(), indices.clone().into()))?;
        let actual = op.eval(tvec!(data.into(), indices.into()))?;
        assert_eq!(
            actual[0].to_plain_array_view::<f32>()?,
            expected[0].to_plain_array_view::<f32>()?
        );
        Ok(())
    }

    #[test]
    fn gather_copies_match_upstream_with_i32_indices() -> TractResult<()> {
        let ds = [2, 16, 17, 64];
        let ix = [2, 257, 2];
        let input = Tensor::from_shape(
            &ds,
            &(0..ds.iter().product())
                .map(|i| i as f32)
                .collect::<Vec<_>>(),
        )?;
        let values: Vec<i32> = (0..ix.iter().product())
            .map(|i| ((i * 7 + i / 2) % ds[1 + i % 2]) as i32)
            .collect();
        let indices = Tensor::from_shape(&ix, &values)?;
        let op = GatherCopy::new(&f32::fact(ds), &i32::fact(ix), 1).unwrap();
        let expected =
            GatherNd::new(1).eval(tvec!(input.clone().into(), indices.clone().into()))?;
        let actual = op.eval(tvec!(input.into(), indices.into()))?;
        assert_eq!(
            actual[0].to_plain_array_view::<f32>()?,
            expected[0].to_plain_array_view::<f32>()?
        );
        Ok(())
    }
}
