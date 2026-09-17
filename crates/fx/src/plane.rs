//! Single-channel kernels shared by layer styles and selection masks.
use crate::{ComputeProgram, ComputeShader, ComputeSource, ComputeStep};

pub static ALPHA_BOX: ComputeShader =
    ComputeShader::new("alpha-box", include_str!("kernels/alpha_box.wgsl"));
pub static SIGNED_DISTANCE: ComputeShader = ComputeShader::new(
    "signed-distance",
    include_str!("kernels/signed_distance.wgsl"),
);
pub static MASK_MORPH: ComputeShader =
    ComputeShader::new("mask-morph", include_str!("kernels/mask_morph.wgsl"));
pub static ALPHA_OFFSET: ComputeShader =
    ComputeShader::new("alpha-offset", include_str!("kernels/alpha_offset.wgsl"));

/// Keep mixed radii and the selection feather's historic accumulation order.
pub fn alpha_blur_program(w: usize, h: usize, radii: &[usize], selection: bool) -> ComputeProgram {
    let mut steps = Vec::new();
    for &r in radii {
        for vertical in [false, true] {
            let source = if steps.is_empty() {
                ComputeSource::Input(0)
            } else {
                ComputeSource::Step(steps.len() - 1)
            };
            steps.push(ComputeStep {
                shader: ALPHA_BOX,
                source,
                auxiliary: ComputeSource::Input(0),
                params: vec![
                    r as f32,
                    u8::from(vertical) as f32,
                    u8::from(selection) as f32,
                ],
                output_len: w.saturating_mul(h),
                invocations: if vertical { w } else { h },
                shape: [w as u32, h as u32, 1],
            });
        }
    }
    let result = if steps.is_empty() {
        ComputeSource::Input(0)
    } else {
        ComputeSource::Step(steps.len() - 1)
    };
    ComputeProgram {
        buffers: vec![],
        result,
        work: w.saturating_mul(h).saturating_mul(steps.len()),
        steps,
    }
}
