//! Effect-owned shaders: adding a kernel does not require editing the GPU backend.

/// A WGSL function `effect(pos: vec2<i32>) -> vec4<f32>`, appended to
/// [`SHADER_PRELUDE`]. Input/output use straight-alpha RGBA; the helpers
/// explicitly opt into premultiplied sampling when the CPU body does.
/// Keep this beside the CPU effect and use `include_str!` for its source.
pub struct ShaderSpec {
    pub name: &'static str,
    pub source: &'static str,
}

impl ShaderSpec {
    /// Complete source, also usable for offline shader validation.
    pub fn wgsl(&self) -> String {
        format!("{SHADER_PRELUDE}\n{}", self.source)
    }
}

pub const SHADER_PRELUDE: &str = include_str!("shader.wgsl");

/// One same-size RGBA operation. Parameters are a tightly packed float
/// array (`args` in WGSL), with no uniform-buffer alignment to manage.
pub struct ShaderJob<'a> {
    pub shader: &'static ShaderSpec,
    pub px: &'a [f32],
    pub width: usize,
    pub height: usize,
    pub params: &'a [f32],
    /// Maximum vertical sampling distance in rows, including interpolation.
    /// `Some(0)` is pointwise/horizontal; `None` needs the entire source.
    /// The backend can split local effects into overlapping bands.
    pub halo: Option<usize>,
    /// Approximate source samples or equivalent arithmetic per pixel.
    /// Used only to decide whether the transfer is worth doing.
    pub work_per_pixel: usize,
}

impl ShaderJob<'_> {
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self
                .width
                .checked_mul(self.height)
                .and_then(|n| n.checked_mul(4))
                == Some(self.px.len())
            && self.params.iter().all(|p| p.is_finite())
    }
}

/// Attempt acceleration before the CPU body has mutated the input.
/// Returns true only after a complete, correctly sized result is copied.
/// On any decline/failure the buffer is untouched, so the caller continues
/// its existing CPU implementation. Empty images are successful no-ops.
pub fn try_shader_rgba(
    px: &mut [f32],
    width: usize,
    height: usize,
    shader: &'static ShaderSpec,
    params: &[f32],
    halo: Option<usize>,
    work_per_pixel: usize,
) -> bool {
    if width == 0 || height == 0 {
        return true;
    }
    let job = ShaderJob {
        shader,
        px,
        width,
        height,
        params,
        halo,
        work_per_pixel,
    };
    if !job.valid() {
        return false;
    }
    if let Some(out) = super::backend().shader(&job) {
        if out.len() == px.len() {
            px.copy_from_slice(&out);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend, set_backend, FxBackend};
    use std::sync::Arc;

    struct ResultBackend(Option<Vec<f32>>);
    impl FxBackend for ResultBackend {
        fn name(&self) -> &'static str {
            "test"
        }
        fn shader(&self, _: &ShaderJob<'_>) -> Option<Vec<f32>> {
            self.0.clone()
        }
    }
    struct Restore(Arc<dyn FxBackend>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_backend(self.0.clone());
        }
    }

    #[test]
    fn declined_or_malformed_results_leave_the_cpu_input_intact() {
        static SHADER: ShaderSpec = ShaderSpec {
            name: "test",
            source: "unused",
        };
        let _restore = Restore(backend());
        let original = [0.1, 0.2, 0.3, 0.4];
        for result in [None, Some(vec![0.0; 3]), Some(vec![0.0; 8])] {
            set_backend(Arc::new(ResultBackend(result)));
            let mut px = original;
            assert!(!try_shader_rgba(&mut px, 1, 1, &SHADER, &[], Some(0), 1));
            assert_eq!(px, original);
        }
        set_backend(Arc::new(ResultBackend(Some(vec![1.0; 4]))));
        let mut px = original;
        assert!(!try_shader_rgba(&mut px, 2, 1, &SHADER, &[], Some(0), 1));
        assert_eq!(px, original);
        assert!(try_shader_rgba(&mut px, 1, 1, &SHADER, &[], Some(0), 1));
        assert_eq!(px, [1.0; 4]);
        assert!(try_shader_rgba(&mut [], 0, 0, &SHADER, &[], Some(0), 1));
    }
}
