use crate::GpuContext;
use schist_fx::{BlurJob, FilterOperation, ShaderJob};

impl GpuContext {
    /// Submit a complete filter, declining invalid or small jobs before upload.
    pub async fn filter_async(
        &self,
        operation: &FilterOperation,
        pixels: &[f32],
        width: usize,
        height: usize,
    ) -> Option<Vec<f32>> {
        let count = width.checked_mul(height)?;
        if count == 0 || count.checked_mul(4)? != pixels.len() || !operation.worth_offloading(count)
        {
            return None;
        }
        match operation {
            FilterOperation::Program { build, params, .. } => {
                let program = build(width, height, params)?;
                if program.result_len(pixels.len()) != Some(pixels.len()) {
                    return None;
                }
                self.run_compute_async(&schist_fx::ComputeJob {
                    input: pixels,
                    program: &program,
                })
                .await
            }
            FilterOperation::Blur { radius, passes } => {
                self.run_blur_async(&BlurJob {
                    px: pixels,
                    width,
                    height,
                    radius: *radius,
                    passes: *passes,
                })
                .await
            }
            FilterOperation::Shader {
                shader,
                params,
                halo,
                work_per_pixel,
            } => {
                self.run_shader_async(&ShaderJob {
                    px: pixels,
                    width,
                    height,
                    shader,
                    params,
                    halo: *halo,
                    work_per_pixel: *work_per_pixel,
                })
                .await
            }
        }
    }
}
