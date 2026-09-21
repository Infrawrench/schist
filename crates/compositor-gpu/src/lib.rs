//! GPU compositor: the wgpu implementation of the `Compositor` seam.
//!
//! GPUI does not expose its render device, so this is a second wgpu
//! instance doing compute only — layer tiles go up as storage buffers,
//! the layer tree (flattened to an op program by [`plan`]) runs as one
//! dispatch per batch, and the composited tiles come back over the same
//! bus. Batched workloads — zoom-outs, exports, multi-tile damage on
//! big documents — are where that trade wins; the semantics are the CPU
//! compositor's, enforced by parity tests, and anything the shader can't
//! express (nesting past the fixed stack) falls back to
//! the CPU reference per call.
//!
//! Install with `schist_compositor::set_backend(Arc::new(GpuCompositor::new()?))`.
//! Browser callers await `GpuContext::new_async` and its asynchronous kernels.

mod exec;
#[cfg(not(target_arch = "wasm32"))]
mod fx;
mod operation;
pub mod plan;

pub use exec::{BatchOut, ComputeCacheStats, GpuContext, TileUploadStats, WarpSource};
#[cfg(not(target_arch = "wasm32"))]
pub use fx::GpuFx;

#[cfg(not(target_arch = "wasm32"))]
use schist_color::NativePixel;
#[cfg(not(target_arch = "wasm32"))]
use schist_compositor::viewport::ViewportParams;
#[cfg(not(target_arch = "wasm32"))]
use schist_compositor::{
    composite_region_f32_cpu, composite_region_rgba8_cpu, composite_tile_cpu, Compositor,
    CpuCompositor,
};
#[cfg(not(target_arch = "wasm32"))]
use schist_core::{Document, IntRect, TileCoord, TILE_SIZE};
use std::sync::Arc;

pub(crate) fn cast_f32s(values: &[f32]) -> &[u8] {
    // The byte view has weaker alignment and exactly the source slice's size/lifetime.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

pub struct GpuCompositor {
    ctx: Arc<GpuContext>,
}

impl GpuCompositor {
    /// Set up the GPU backend. Fails cleanly (with a reason for the log)
    /// when no adapter exists — headless CI, missing drivers.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Result<GpuCompositor, String> {
        let ctx = Arc::new(GpuContext::new()?);
        Ok(GpuCompositor { ctx })
    }

    /// "vulkan · NVIDIA RTX 4070" — for logs and the About dialog.
    pub fn describe(&self) -> String {
        let info = self.ctx.adapter_info();
        format!("{:?} · {}", info.backend, info.name).to_lowercase()
    }

    pub fn context(&self) -> &Arc<GpuContext> {
        &self.ctx
    }

    /// The filter and warp backend sharing this device — install it with
    /// `schist_fx::set_backend` so blurs and mesh warps run here too.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn fx(&self) -> GpuFx {
        GpuFx::new(self.ctx.clone())
    }

    /// Composite a batch on the GPU; `None` falls back to the CPU.
    #[cfg(not(target_arch = "wasm32"))]
    fn batch(&self, doc: &Document, coords: &[TileCoord], rgba8: bool) -> Option<BatchOut> {
        let plan = match plan::build(doc) {
            Ok(plan) => plan,
            Err(why) => {
                log::debug!("gpu compositor fallback: {why:?}");
                return None;
            }
        };
        match self.ctx.composite_batch(&plan, coords, rgba8)? {
            BatchOut::Native(tiles) => {
                // The GPU retains native channels through the entire layer
                // tree. Apply the same ICC transform as the CPU reference
                // once, at the final display boundary, with separate alpha.
                let transform = schist_colormgmt::NativeColorTransform::new(
                    doc.mode,
                    doc.icc_profile.as_deref(),
                )
                .ok();
                let tiles = tiles
                    .iter()
                    .map(|tile| schist_colormgmt::native_to_rgba(tile, transform.as_ref()));
                Some(if rgba8 {
                    BatchOut::Rgba8(
                        tiles
                            .map(|tile| tile.into_iter().map(schist_color::f32_to_u8).collect())
                            .collect(),
                    )
                } else {
                    BatchOut::F32(tiles.collect())
                })
            }
            out => Some(out),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Compositor for GpuCompositor {
    fn name(&self) -> &'static str {
        "gpu"
    }

    fn tile(&self, doc: &Document, coord: TileCoord) -> Vec<f32> {
        match self.batch(doc, &[coord], false) {
            Some(BatchOut::F32(mut tiles)) => tiles.pop().unwrap(),
            _ => composite_tile_cpu(doc, coord),
        }
    }

    fn native_tile(&self, doc: &Document, coord: TileCoord) -> Vec<NativePixel> {
        self.native_tiles(doc, &[coord]).pop().unwrap()
    }

    fn native_tiles(&self, doc: &Document, coords: &[TileCoord]) -> Vec<Vec<NativePixel>> {
        if let Ok(plan) = plan::build(doc) {
            if plan.is_native() {
                if let Some(BatchOut::Native(tiles)) =
                    self.ctx.composite_batch(&plan, coords, false)
                {
                    return tiles;
                }
            }
        }
        CpuCompositor.native_tiles(doc, coords)
    }

    fn tiles_rgba8(&self, doc: &Document, coords: &[TileCoord]) -> Vec<Vec<u8>> {
        match self.batch(doc, coords, true) {
            Some(BatchOut::Rgba8(tiles)) => tiles,
            _ => CpuCompositor.tiles_rgba8(doc, coords),
        }
    }

    fn region_f32(&self, doc: &Document, region: IntRect) -> Vec<f32> {
        let coords: Vec<TileCoord> = TileCoord::covering(&region).collect();
        match self.batch(doc, &coords, false) {
            Some(BatchOut::F32(tiles)) => {
                let mut out = vec![0.0f32; region.width() as usize * region.height() as usize * 4];
                for (coord, tile) in coords.iter().zip(&tiles) {
                    crop_into(&region, *coord, &mut out, tile);
                }
                out
            }
            _ => composite_region_f32_cpu(doc, region),
        }
    }

    fn region_rgba8(&self, doc: &Document, region: IntRect) -> Vec<u8> {
        let coords: Vec<TileCoord> = TileCoord::covering(&region).collect();
        match self.batch(doc, &coords, true) {
            Some(BatchOut::Rgba8(tiles)) => {
                let mut out = vec![0u8; region.width() as usize * region.height() as usize * 4];
                for (coord, tile) in coords.iter().zip(&tiles) {
                    crop_into(&region, *coord, &mut out, tile);
                }
                out
            }
            _ => composite_region_rgba8_cpu(doc, region),
        }
    }

    fn viewport(&self, params: &ViewportParams, grid: &[Option<Arc<Vec<u8>>>]) -> Option<Vec<u8>> {
        self.ctx.render_viewport(params, grid)
    }
}

/// Copy the intersection of a composited tile into a tightly packed
/// region buffer (works for any 4-element pixel type).
#[cfg(not(target_arch = "wasm32"))]
fn crop_into<T: Copy>(region: &IntRect, coord: TileCoord, out: &mut [T], tile: &[T]) {
    let w = region.width() as usize;
    let trect = coord.rect();
    let clip = trect.intersect(region);
    for y in clip.top..clip.bottom {
        let ly = (y - trect.top) as usize;
        let oy = (y - region.top) as usize;
        for x in clip.left..clip.right {
            let lx = (x - trect.left) as usize;
            let ox = (x - region.left) as usize;
            let s = (ly * TILE_SIZE as usize + lx) * 4;
            let d = (oy * w + ox) * 4;
            out[d..d + 4].copy_from_slice(&tile[s..s + 4]);
        }
    }
}
