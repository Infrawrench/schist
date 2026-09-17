//! Blocking entry points for native callers. Browser callers await the same kernels.
use super::*;

#[cfg(not(target_arch = "wasm32"))]
impl GpuContext {
    pub fn run_compute(&self, job: &schist_fx::ComputeJob<'_>) -> Option<Vec<f32>> {
        pollster::block_on(self.run_compute_async(job))
    }
    pub fn new() -> Result<Self, String> {
        pollster::block_on(Self::new_async())
    }
    pub fn render_viewport(
        &self,
        p: &schist_compositor::viewport::ViewportParams,
        grid: &[Option<std::sync::Arc<Vec<u8>>>],
    ) -> Option<Vec<u8>> {
        pollster::block_on(self.render_viewport_async(p, grid))
    }
    pub fn run_carve(&self, job: &schist_fx::CarveJob<'_>) -> Option<schist_fx::Carved> {
        pollster::block_on(self.run_carve_async(job))
    }
    pub fn run_blur(&self, job: &schist_fx::BlurJob<'_>) -> Option<Vec<f32>> {
        pollster::block_on(self.run_blur_async(job))
    }
    pub fn run_lens_blur(&self, job: &schist_fx::LensJob<'_>) -> Option<Vec<f32>> {
        pollster::block_on(self.run_lens_blur_async(job))
    }
    pub fn upload_warp_source(&self, src: &[f32]) -> Option<WarpSource> {
        pollster::block_on(self.upload_warp_source_async(src))
    }
    pub fn run_warp(&self, job: &schist_fx::WarpParams<'_>, src: &WarpSource) -> Option<Vec<f32>> {
        pollster::block_on(self.run_warp_async(job, src))
    }
    pub fn run_warp_banded(
        &self,
        job: &schist_fx::WarpParams<'_>,
        src: &WarpSource,
        budget: usize,
    ) -> Option<Vec<f32>> {
        pollster::block_on(self.run_warp_banded_async(job, src, budget))
    }
    pub fn composite_batch(
        &self,
        plan: &Plan<'_>,
        coords: &[TileCoord],
        rgba8: bool,
    ) -> Option<BatchOut> {
        pollster::block_on(self.composite_batch_async(plan, coords, rgba8))
    }
    pub fn run_shader(&self, job: &schist_fx::ShaderJob<'_>) -> Option<Vec<f32>> {
        pollster::block_on(self.run_shader_async(job))
    }
    pub fn run_carve_paged(&self, job: &schist_fx::CarveJob<'_>) -> Option<schist_fx::Carved> {
        pollster::block_on(self.run_carve_paged_async(job))
    }
    pub fn run_carve_paged_with_edge(
        &self,
        job: &schist_fx::CarveJob<'_>,
        edge: u32,
    ) -> Option<schist_fx::Carved> {
        pollster::block_on(self.run_carve_paged_with_edge_async(job, edge))
    }
}
