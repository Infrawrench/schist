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
            FilterOperation::Program { .. }
            | FilterOperation::Captured { .. }
            | FilterOperation::Sequence(_) => {
                let program = operation.program(width, height)?;
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

impl schist_fx::AsyncCompute for GpuContext {
    async fn compute_async<'a>(&'a self, job: schist_fx::ComputeJob<'a>) -> Option<Vec<f32>> {
        self.run_compute_async(&job).await
    }
}

impl GpuContext {
    /// Flatten a captured document at full precision, retaining native channels.
    /// Batches bound temporary GPU allocations; a declined batch leaves the
    /// caller's source document available for complete CPU fallback.
    pub async fn flatten_async(&self, doc: &schist_core::Document) -> Option<schist_core::TileMap> {
        use crate::BatchOut;
        use schist_core::{TileBuf, TileCoord, TileMap};
        let plan = crate::plan::build(doc).ok()?;
        let coords = TileCoord::covering(&doc.canvas_rect()).collect::<Vec<_>>();
        let mut tiles = TileMap::new_in_mode(doc.mode);
        for batch in coords.chunks(8) {
            match self.composite_batch_async(&plan, batch, false).await? {
                BatchOut::F32(outputs) => {
                    for (&coord, pixels) in batch.iter().zip(outputs) {
                        tiles.insert(
                            coord,
                            std::sync::Arc::new(TileBuf::F32(pixels.into_boxed_slice())),
                        );
                    }
                }
                BatchOut::Native(outputs) => {
                    for (&coord, pixels) in batch.iter().zip(outputs) {
                        let mut tile =
                            TileBuf::new_in_mode(schist_color::Depth::ThirtyTwo, doc.mode);
                        for (i, pixel) in pixels.into_iter().enumerate() {
                            tile.set_native_pixel(i, pixel);
                        }
                        tiles.insert(coord, std::sync::Arc::new(tile));
                    }
                }
                BatchOut::Rgba8(_) => return None,
            }
        }
        Some(tiles)
    }
}
