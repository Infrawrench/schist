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
                if crate::fast_host_ops() {
                    transpose_f32(input, output, self.rows, self.columns);
                } else {
                    transpose::transpose(input, output, self.columns, self.rows);
                }
            }
        }
        Ok(tvec!(result.into()))
    }
}

fn transpose_f32(input: &[f32], output: &mut [f32], rows: usize, columns: usize) {
    let len = rows
        .checked_mul(columns)
        .expect("transpose dimensions overflow");
    assert_eq!(input.len(), len);
    assert_eq!(output.len(), len);
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("neon") {
        // SAFETY: NEON was detected. Both slices are disjoint, complete matrices;
        // every vector load/store below stays within a full 4x4 tile.
        unsafe { transpose_neon(input, output, rows, columns) };
        return;
    }
    transpose::transpose(input, output, columns, rows);
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn transpose_neon(input: &[f32], output: &mut [f32], rows: usize, columns: usize) {
    use std::arch::aarch64::*;
    const BLOCK: usize = 32;
    let full_rows = rows / 4 * 4;
    let full_columns = columns / 4 * 4;
    for top in (0..full_rows).step_by(BLOCK) {
        for left in (0..full_columns).step_by(BLOCK) {
            for y in (top..(top + BLOCK).min(full_rows)).step_by(4) {
                for x in (left..(left + BLOCK).min(full_columns)).step_by(4) {
                    // These shuffles only move bits; they do not round floats.
                    unsafe {
                        let a = vld1q_f32(input.as_ptr().add(y * columns + x));
                        let b = vld1q_f32(input.as_ptr().add((y + 1) * columns + x));
                        let c = vld1q_f32(input.as_ptr().add((y + 2) * columns + x));
                        let d = vld1q_f32(input.as_ptr().add((y + 3) * columns + x));
                        let ab0 = vtrn1q_f32(a, b);
                        let ab1 = vtrn2q_f32(a, b);
                        let cd0 = vtrn1q_f32(c, d);
                        let cd1 = vtrn2q_f32(c, d);
                        vst1q_f32(
                            output.as_mut_ptr().add(x * rows + y),
                            vcombine_f32(vget_low_f32(ab0), vget_low_f32(cd0)),
                        );
                        vst1q_f32(
                            output.as_mut_ptr().add((x + 1) * rows + y),
                            vcombine_f32(vget_low_f32(ab1), vget_low_f32(cd1)),
                        );
                        vst1q_f32(
                            output.as_mut_ptr().add((x + 2) * rows + y),
                            vcombine_f32(vget_high_f32(ab0), vget_high_f32(cd0)),
                        );
                        vst1q_f32(
                            output.as_mut_ptr().add((x + 3) * rows + y),
                            vcombine_f32(vget_high_f32(ab1), vget_high_f32(cd1)),
                        );
                    }
                }
            }
        }
    }
    for y in 0..full_rows {
        for x in full_columns..columns {
            output[x * rows + y] = input[y * columns + x];
        }
    }
    for y in full_rows..rows {
        for x in 0..columns {
            output[x * rows + y] = input[y * columns + x];
        }
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

    #[test]
    fn transpose_preserves_bits_across_vector_and_cache_block_tails() {
        for rows in [1, 3, 4, 17, 32, 65, 129] {
            for columns in [1, 3, 4, 19, 32, 67, 131] {
                let input: Vec<f32> = (0..rows * columns)
                    .map(|i| f32::from_bits((i as u32).wrapping_mul(0x7935abcd)))
                    .collect();
                let mut output = vec![0.; input.len()];
                transpose_f32(&input, &mut output, rows, columns);
                for y in 0..rows {
                    for x in 0..columns {
                        assert_eq!(
                            output[x * rows + y].to_bits(),
                            input[y * columns + x].to_bits()
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "transpose dimensions overflow")]
    fn overflowing_transpose_dimensions_are_rejected() {
        transpose_f32(&[], &mut [], usize::MAX, 2);
    }

    #[test]
    #[ignore = "development timing; make profile-neural-tensors"]
    fn profile_host_transposes() {
        for (rows, columns) in [(64, 65536), (576, 65536), (256, 16384)] {
            let input = vec![0.125f32; rows * columns];
            let mut output = vec![0.; input.len()];
            for native in [false, true] {
                let start = std::time::Instant::now();
                for _ in 0..3 {
                    if native {
                        transpose_f32(&input, &mut output, rows, columns);
                    } else {
                        transpose::transpose(&input, &mut output, columns, rows);
                    }
                    std::hint::black_box(&output);
                }
                eprintln!(
                    "transpose {rows}x{columns} native={native}: {:.3}s",
                    start.elapsed().as_secs_f64()
                );
            }
        }
    }
}
