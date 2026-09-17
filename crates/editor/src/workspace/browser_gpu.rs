//! Foreground, asynchronous WebGPU work. No blocking waits or wasm threads.

use super::*;
use schist_compositor_gpu::{plan, BatchOut, GpuContext};
use std::rc::Rc;

#[derive(Default)]
pub(super) struct BrowserGpu {
    pub context: Option<Rc<GpuContext>>,
    pub epoch: u64,
    initializing: bool,
    pending: bool,
    pub requested: Option<(schist_core::DocumentId, ViewportKey)>,
    failed: Option<(schist_core::DocumentId, ViewportKey)>,
    filter_sequence: u64,
    filter_running: bool,
    filter_request: Option<FilterRequest>,
}

struct FilterRequest {
    operation: schist_fx::FilterOperation,
    filter: Arc<dyn schist_plugin_api::FilterPlugin>,
    values: schist_plugin_api::FilterValues,
    foreground: schist_color::Rgba,
    background: schist_color::Rgba,
    backdrop: Option<Vec<f32>>,
    path: Option<Vec<(f32, f32)>>,
    map: Option<Arc<schist_plugin_api::FilterImage>>,
    original: Vec<f32>,
    region: IntRect,
    layer: schist_core::LayerId,
    document: schist_core::DocumentId,
    revision: u64,
    sequence: u64,
    record: bool,
    whole_layer: bool,
}

pub(super) struct FilterInput<'a> {
    pub original: &'a [f32],
    pub region: IntRect,
    pub layer: schist_core::LayerId,
    pub whole_layer: bool,
}

impl BrowserGpu {
    pub fn reset(&mut self) {
        *self = Self {
            epoch: self.epoch.wrapping_add(1),
            ..Default::default()
        };
    }
}

impl Workspace {
    pub(super) fn cancel_browser_filter(&mut self) {
        self.browser_gpu.filter_sequence = self.browser_gpu.filter_sequence.wrapping_add(1);
        self.browser_gpu.filter_request = None;
    }

    pub(super) fn queue_browser_filter(
        &mut self,
        filter: Arc<dyn schist_plugin_api::FilterPlugin>,
        values: &schist_plugin_api::FilterValues,
        preview: FilterInput<'_>,
        record: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(context) = self.browser_gpu.context.clone() else {
            return false;
        };
        let Some(doc) = self.doc.as_ref() else {
            return false;
        };
        // Native filters need their original separations and the existing adapter.
        if matches!(doc.mode, ColorMode::Cmyk | ColorMode::Lab) {
            return false;
        }
        // Avoid composing a backdrop or flattening a path twice for CPU-only filters.
        // Current GPU context descriptors use colors; auxiliary-input filters stay synchronous.
        if (filter.wants_backdrop() || filter.wants_path() || filter.wants_map().is_some())
            && filter.gpu_operation(values).is_none()
        {
            return false;
        }
        let (document, revision) = (doc.id, doc.revision);
        let map = self.filter_map();
        let (mut backdrop, mut path) = (None, None);
        let filter_context = self.filter_context(
            filter.as_ref(),
            preview.layer,
            preview.region,
            &mut backdrop,
            &mut path,
            map.as_deref(),
        );
        let (foreground, background) = (filter_context.foreground, filter_context.background);
        let Some(operation) = filter.gpu_operation_with(values, &filter_context) else {
            return false;
        };
        let count = preview.region.width() as usize * preview.region.height() as usize;
        if !operation.worth_offloading(count) {
            return false;
        }
        self.browser_gpu.filter_sequence = self.browser_gpu.filter_sequence.wrapping_add(1);
        self.browser_gpu.filter_request = Some(FilterRequest {
            operation,
            filter: filter.clone(),
            values: values.clone(),
            original: preview.original.to_vec(),
            region: preview.region,
            layer: preview.layer,
            document,
            revision,
            foreground,
            background,
            backdrop,
            path,
            map,
            sequence: self.browser_gpu.filter_sequence,
            record,
            whole_layer: preview.whole_layer,
        });
        if record {
            self.open_modal(
                Modal::Busy {
                    title: filter.name().into(),
                    what: schist_i18n::tf!("workspace.filters.running", name = filter.name()),
                    note: String::new(),
                },
                cx,
            );
        }
        if self.browser_gpu.filter_running {
            return true;
        }
        self.browser_gpu.filter_running = true;
        let epoch = self.browser_gpu.epoch;
        cx.spawn(async move |this, cx| loop {
            let request = this
                .update(cx, |ws, _| {
                    if ws.browser_gpu.epoch != epoch {
                        return None;
                    }
                    let request = ws.browser_gpu.filter_request.take();
                    if request.is_none() {
                        ws.browser_gpu.filter_running = false;
                    }
                    request
                })
                .ok()
                .flatten();
            let Some(request) = request else { break };
            let (width, height) = (
                request.region.width() as usize,
                request.region.height() as usize,
            );
            let result = context
                .filter_async(&request.operation, &request.original, width, height)
                .await;
            this.update(cx, |ws, cx| {
                if ws.browser_gpu.epoch != epoch
                    || ws.browser_gpu.filter_sequence != request.sequence
                {
                    return;
                }
                if request.record {
                    ws.modal = None;
                }
                if !ws.doc.as_ref().is_some_and(|doc| {
                    doc.id == request.document && doc.revision == request.revision
                }) {
                    cx.notify();
                    return;
                }
                let pixels = result.unwrap_or_else(|| {
                    let mut pixels = request.original.clone();
                    let context = schist_plugin_api::FilterContext {
                        foreground: request.foreground,
                        background: request.background,
                        backdrop: request.backdrop.as_deref(),
                        path: request.path.as_deref(),
                        map: request.map.as_deref(),
                    };
                    request.filter.apply_with(
                        &mut pixels,
                        width,
                        height,
                        &request.values,
                        &context,
                    );
                    pixels
                });
                let name = request.filter.name();
                ws.write_region_inner(
                    request.layer,
                    request.region,
                    &request.original,
                    &pixels,
                    name,
                    request.record,
                    !request.whole_layer,
                );
                if request.record {
                    ws.status = name.into();
                }
                ws.after_change(cx);
            })
            .ok();
        })
        .detach();
        true
    }

    pub(super) fn ensure_browser_gpu(&mut self, cx: &mut Context<Self>) {
        if self.browser_gpu.initializing
            || !self.view.gpu_compositing
            || !crate::feature_enabled("gpu-compositing")
        {
            return;
        }
        self.browser_gpu.initializing = true;
        let epoch = self.browser_gpu.epoch;
        cx.spawn(async move |this, cx| {
            let context = GpuContext::new_async().await;
            this.update(cx, |ws, cx| {
                if ws.browser_gpu.epoch != epoch || !ws.view.gpu_compositing {
                    return;
                }
                match context {
                    Ok(context) => {
                        log::info!("Browser GPU compute on ({})", context.adapter_info().name);
                        ws.browser_gpu.context = Some(Rc::new(context));
                        ws.cache.invalidate_all();
                        ws.display_tiles.clear();
                        ws.invalidate_viewport_image();
                        cx.notify();
                    }
                    Err(error) => log::warn!("Browser GPU compute unavailable: {error}"),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Keep one submission in flight; newer paints replace the requested stamp.
    /// A completion can populate unchanged tiles, but only the newest view is shown.
    pub(super) fn queue_browser_viewport(
        &mut self,
        key: ViewportKey,
        visible: IntRect,
        coords: &[TileCoord],
        scale_factor: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(context) = self.browser_gpu.context.clone() else {
            return false;
        };
        let Some(doc) = self.doc.as_ref() else {
            return false;
        };
        let stamp = (doc.id, key);
        self.browser_gpu.requested = Some(stamp);
        if self.browser_gpu.failed == Some(stamp) {
            return false;
        }
        if self.browser_gpu.pending {
            return true;
        }
        let missing: Vec<_> = coords
            .iter()
            .copied()
            .filter(|coord| !self.display_tiles.contains_key(coord))
            .collect();
        let snapshot = if missing.is_empty() {
            None
        } else {
            let Ok(plan) = plan::build(doc) else {
                return false;
            };
            Some(plan.snapshot())
        };
        let (tx0, ty0) = (
            visible.left.div_euclid(TILE_SIZE),
            visible.top.div_euclid(TILE_SIZE),
        );
        let cols = ((visible.right - 1).div_euclid(TILE_SIZE) - tx0 + 1) as usize;
        let rows = ((visible.bottom - 1).div_euclid(TILE_SIZE) - ty0 + 1) as usize;
        let mut grid = vec![None; cols * rows];
        for coord in coords {
            grid[(coord.ty - ty0) as usize * cols + (coord.tx - tx0) as usize] =
                self.display_tiles.get(coord).cloned();
        }
        let params = schist_compositor::viewport::ViewportParams {
            width: key.size.0 as usize,
            height: key.size.1 as usize,
            origin: (
                f32::from(self.offset.x) * scale_factor,
                f32::from(self.offset.y) * scale_factor,
            ),
            zoom: self.zoom,
            scale_factor,
            rotation: self.rotation,
            canvas: doc.canvas_rect(),
            grid_origin: (tx0, ty0),
            grid_cols: cols,
            grid_rows: rows,
            surround: key.surround,
        };
        let profile = doc.icc_profile.clone();
        let mode = doc.mode;
        let display = self.display_transform.clone();
        let proof = self.proof_transform.clone();
        let epoch = self.browser_gpu.epoch;
        self.browser_gpu.pending = true;
        cx.spawn(async move |this, cx| {
            let result = async {
                let mut completed = Vec::new();
                if let Some(snapshot) = snapshot {
                    let batch = context
                        .composite_batch_async(&snapshot.plan(), &missing, true)
                        .await?;
                    let tiles = match batch {
                        BatchOut::Rgba8(tiles) => tiles,
                        BatchOut::Native(tiles) => {
                            let transform = schist_colormgmt::NativeColorTransform::new(
                                mode,
                                profile.as_deref(),
                            )
                            .ok();
                            tiles
                                .iter()
                                .map(|tile| {
                                    schist_colormgmt::native_to_rgba(tile, transform.as_ref())
                                        .into_iter()
                                        .map(schist_color::f32_to_u8)
                                        .collect()
                                })
                                .collect()
                        }
                        BatchOut::F32(_) => return None,
                    };
                    for (coord, mut tile) in missing.into_iter().zip(tiles) {
                        if proof.is_some() || display.is_some() {
                            let mut pixels: Vec<_> =
                                tile.iter().map(|&v| v as f32 / 255.0).collect();
                            if let Some(proof) = &proof {
                                proof.apply(&mut pixels);
                            }
                            if let Some(display) = &display {
                                display.apply(&mut pixels);
                            }
                            tile = pixels.into_iter().map(schist_color::f32_to_u8).collect();
                        }
                        let tile = Arc::new(tile);
                        grid[(coord.ty - ty0) as usize * cols + (coord.tx - tx0) as usize] =
                            Some(tile.clone());
                        completed.push((coord, tile));
                    }
                }
                let pixels = context
                    .render_viewport_async(&params, &grid)
                    .await
                    .unwrap_or_else(|| {
                        schist_compositor::viewport::render_viewport_cpu(&params, &grid)
                    });
                let buffer = image::RgbaImage::from_raw(key.size.0, key.size.1, pixels)?;
                let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
                Some((completed, image))
            }
            .await;
            this.update(cx, |ws, cx| {
                if ws.browser_gpu.epoch != epoch {
                    return;
                }
                ws.browser_gpu.pending = false;
                if context.is_lost() {
                    ws.browser_gpu.context = None;
                }
                let same_document = ws
                    .doc
                    .as_ref()
                    .is_some_and(|doc| doc.id == stamp.0 && doc.revision == key.revision)
                    && ws.color_epoch == key.color_epoch;
                if same_document {
                    if let Some((tiles, image)) = result {
                        ws.display_tiles.extend(tiles);
                        if ws.browser_gpu.requested == Some(stamp) {
                            if let Some((_, old)) = ws.viewport_image.replace((key, image)) {
                                ws.retired_images.push(old);
                            }
                        }
                    } else {
                        ws.browser_gpu.failed = Some(stamp);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }
}
