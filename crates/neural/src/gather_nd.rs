//! Correct shape inference for ONNX GatherND with batch dimensions.
//!
//! tract 0.23.5's typed kernel handles this operator, but its inference rule
//! reads the indices shape where it needs the data shape and omits batch_dims.
//! BiRefNet's deformable convolutions expose that mismatch. Keep the upstream
//! typed kernel and infer the output using the ONNX definition.

use tract_onnx::tract_hir::{infer::*, internal::*};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct GatherNd {
    batch_dims: usize,
}

impl Expansion for GatherNd {
    fn name(&self) -> StaticName {
        "GatherNdShape".into()
    }

    fn rules<'r, 'p: 'r, 's: 'r>(
        &'s self,
        s: &mut Solver<'r>,
        inputs: &'p [TensorProxy],
        outputs: &'p [TensorProxy],
    ) -> InferenceResult {
        check_input_arity(inputs, 2)?;
        check_output_arity(outputs, 1)?;
        s.equals(&outputs[0].datum_type, &inputs[0].datum_type)?;
        s.given_2(
            &inputs[0].shape,
            &inputs[1].shape,
            move |s, data, indices| {
                ensure!(
                    !data.is_empty() && !indices.is_empty(),
                    "GatherND needs ranked inputs"
                );
                let b = self.batch_dims;
                ensure!(
                    b < data.len() && b < indices.len(),
                    "invalid GatherND batch_dims"
                );
                for axis in 0..b {
                    s.equals(&inputs[0].shape[axis], &inputs[1].shape[axis])?;
                }
                let n = indices[indices.len() - 1].to_usize()?;
                ensure!(n > 0 && n <= data.len() - b, "invalid GatherND index tuple");
                let mut shape: TVec<TDim> = indices[..indices.len() - 1].into();
                shape.extend(data[b + n..].iter().cloned());
                s.equals(&outputs[0].shape, shape)?;
                Ok(())
            },
        )?;
        Ok(())
    }

    fn wire(
        &self,
        prefix: &str,
        model: &mut TypedModel,
        inputs: &[OutletId],
    ) -> TractResult<TVec<OutletId>> {
        model.wire_node(
            prefix,
            tract_onnx::tract_core::ops::array::GatherNd::new(self.batch_dims),
            inputs,
        )
    }
}

pub fn register(onnx: &mut tract_onnx::Onnx) {
    onnx.op_register.insert("GatherND", |_, node| {
        let batch = node
            .attribute
            .iter()
            .find(|a| a.name == "batch_dims")
            .map_or(0, |a| a.i);
        ensure!(batch >= 0, "negative GatherND batch_dims");
        Ok((
            expand(GatherNd {
                batch_dims: batch as usize,
            }),
            vec![],
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_gather_infers_data_tail_and_runs() -> TractResult<()> {
        let mut model = InferenceModel::default();
        let data = model.add_source("data", f32::fact([1, 1, 2, 2, 3]).into())?;
        let indices = model.add_source("indices", i64::fact([1, 1, 2, 2]).into())?;
        let out = model.wire_node(
            "gather",
            expand(GatherNd { batch_dims: 2 }),
            &[data, indices],
        )?;
        model.select_output_outlets(&out)?;
        let plan = model.into_optimized()?.into_runnable()?;
        let data = Tensor::from_shape(
            &[1, 1, 2, 2, 3],
            &(0..12).map(|v| v as f32).collect::<Vec<_>>(),
        )?;
        let indices = Tensor::from_shape(&[1, 1, 2, 2], &[0i64, 1, 1, 0])?;
        let actual = plan.run(tvec!(data.into(), indices.into()))?;
        assert_eq!(actual[0].shape(), &[1, 1, 2, 3]);
        assert_eq!(
            actual[0].to_plain_array_view::<f32>()?.as_slice().unwrap(),
            &[3., 4., 5., 6., 7., 8.]
        );
        Ok(())
    }
}
