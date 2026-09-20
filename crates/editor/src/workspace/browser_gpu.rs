//! Foreground, asynchronous WebGPU work. No blocking waits or wasm threads.

use super::*;
use schist_compositor_gpu::{plan, BatchOut, GpuContext};
use schist_i18n::tf;
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
    edit_sequence: u64,
    edit_running: bool,
    edit_request: Option<EditRequest>,
    filter_running: bool,
    filter_request: Option<FilterRequest>,
}

struct EditRequest {
    operation: schist_plugin_api::GpuEdit,
    stamp: (schist_core::DocumentId, u64, Option<schist_core::LayerId>),
    sequence: u64,
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

/// Destructive adjustments use the filter queue's cancellation, snapshot and
/// single-history-entry rules, including its captured CPU fallback.
struct AdjustmentOperation {
    params: schist_adjustments::Params,
    name: &'static str,
}

struct AutoOperation {
    mode: schist_adjustments::auto::AutoMode,
    name: &'static str,
}

impl schist_plugin_api::FilterPlugin for AutoOperation {
    fn id(&self) -> &'static str {
        "adjustment.auto"
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn gpu_operation(
        &self,
        _: &schist_plugin_api::FilterValues,
    ) -> Option<schist_fx::FilterOperation> {
        Some(schist_adjustments::auto::operation(self.mode))
    }

    fn apply(&self, pixels: &mut [f32], _: usize, _: usize, _: &schist_plugin_api::FilterValues) {
        schist_adjustments::auto::apply_cpu(pixels, self.mode);
    }
}

impl schist_plugin_api::FilterPlugin for AdjustmentOperation {
    fn id(&self) -> &'static str {
        "adjustment.destructive"
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn gpu_operation(
        &self,
        _: &schist_plugin_api::FilterValues,
    ) -> Option<schist_fx::FilterOperation> {
        schist_adjustments::gpu::operation(&self.params)
    }

    fn apply(&self, pixels: &mut [f32], _: usize, _: usize, _: &schist_plugin_api::FilterValues) {
        self.params.apply_buffer(pixels);
    }
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
    pub(super) fn cancel_browser_edits(&mut self) {
        self.browser_gpu.edit_sequence = self.browser_gpu.edit_sequence.wrapping_add(1);
        self.browser_gpu.edit_request = None;
    }

    pub fn queue_browser_resize(
        &mut self,
        width: u32,
        height: u32,
        filter: schist_core::Filter,
        cx: &mut Context<Self>,
    ) -> bool {
        self.ensure_browser_gpu(cx);
        let Some(context) = self.browser_gpu.context.clone() else {
            return false;
        };
        let Some(doc) = self.doc.as_ref() else {
            return false;
        };
        let Some(plan) = schist_tools_transform::ClassicResize::capture(doc, width, height, filter)
        else {
            return false;
        };
        let stamp = (doc.id, doc.revision);
        self.cancel_browser_edits();
        let sequence = self.browser_gpu.edit_sequence;
        let epoch = self.browser_gpu.epoch;
        self.close_modal(cx);
        cx.spawn(async move |this, cx| {
            let result = plan.run(context.as_ref()).await;
            this.update(cx, |ws, cx| {
                if ws.browser_gpu.epoch != epoch || ws.browser_gpu.edit_sequence != sequence {
                    return;
                }
                let Some(doc) = ws
                    .doc
                    .as_mut()
                    .filter(|doc| (doc.id, doc.revision) == stamp)
                else {
                    return;
                };
                result.apply(doc);
                ws.status = tf!("dialog.size.image_size_status", w = width, h = height).into();
                ws.after_change(cx);
                ws.fit_to_view();
            })
            .ok();
        })
        .detach();
        true
    }

    pub(super) fn queue_browser_edit(
        &mut self,
        request: schist_plugin_api::GpuEdit,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = &self.doc else {
            return;
        };
        let stamp = (doc.id, doc.revision, doc.active_layer);
        self.ensure_browser_gpu(cx);
        self.browser_gpu.edit_sequence = self.browser_gpu.edit_sequence.wrapping_add(1);
        self.browser_gpu.edit_request = Some(EditRequest {
            operation: request,
            stamp,
            sequence: self.browser_gpu.edit_sequence,
        });
        self.start_browser_edit(cx);
    }

    fn start_browser_edit(&mut self, cx: &mut Context<Self>) {
        if self.browser_gpu.edit_running {
            return;
        }
        let Some(EditRequest {
            operation: request,
            stamp,
            sequence,
        }) = self.browser_gpu.edit_request.take()
        else {
            return;
        };
        self.browser_gpu.edit_running = true;
        let epoch = self.browser_gpu.epoch;
        let context = self.browser_gpu.context.clone();
        cx.spawn(async move |this, cx| {
            let output = if let Some(context) = context {
                context
                    .run_compute_async(&schist_fx::ComputeJob {
                        input: &request.input,
                        program: &request.program,
                    })
                    .await
            } else {
                None
            };
            let output = output.unwrap_or_else(|| (request.fallback)(&request.input));
            this.update(cx, |ws, cx| {
                if ws.browser_gpu.epoch != epoch {
                    return;
                }
                ws.browser_gpu.edit_running = false;
                if ws.browser_gpu.edit_sequence == sequence {
                    if let Some(doc) = ws
                        .doc
                        .as_mut()
                        .filter(|doc| (doc.id, doc.revision, doc.active_layer) == stamp)
                    {
                        (request.apply)(doc, output);
                        ws.status = request.name.into();
                        ws.after_change(cx);
                    }
                }
                ws.start_browser_edit(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn queue_browser_auto(
        &mut self,
        mode: schist_adjustments::auto::AutoMode,
        name: &'static str,
        preview: &FilterPreview,
        cx: &mut Context<Self>,
    ) -> bool {
        self.queue_browser_filter(
            Arc::new(AutoOperation { mode, name }),
            &schist_plugin_api::FilterValues::default(),
            FilterInput {
                original: &preview.original,
                region: preview.region,
                layer: preview.layer,
                whole_layer: preview.whole_layer,
            },
            true,
            cx,
        )
    }

    pub(super) fn queue_browser_adjustment(
        &mut self,
        params: &schist_adjustments::Params,
        name: &'static str,
        preview: &FilterPreview,
        record: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.queue_browser_filter(
            Arc::new(AdjustmentOperation {
                params: params.clone(),
                name,
            }),
            &schist_plugin_api::FilterValues::default(),
            FilterInput {
                original: &preview.original,
                region: preview.region,
                layer: preview.layer,
                whole_layer: preview.whole_layer,
            },
            record,
            cx,
        )
    }

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
        // Captured descriptors include colors, maps, paths and backdrop snapshots.
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
    /// Completions may show intermediate edits so a continuous drag cannot
    /// starve painting. Only tiles from the current revision enter the cache.
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
        let ink_channels = doc.ink_channels.clone();
        let ink_preview = doc.ink_preview;
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
                    let mut tiles = match batch {
                        BatchOut::Rgba8(tiles) => tiles,
                        BatchOut::Native(tiles) => {
                            let transform = schist_colormgmt::NativeColorTransform::new(
                                mode,
                                profile.as_deref(),
                            )
                            .ok();
                            let mut converted = Vec::with_capacity(tiles.len());
                            for tile in tiles {
                                let accelerated = if let Some(program) = transform
                                    .as_ref()
                                    .and_then(|t| t.to_rgb_program(tile.len()))
                                {
                                    let input: Vec<f32> = tile
                                        .iter()
                                        .flat_map(|p| p.color[..mode.channels()].iter().copied())
                                        .collect();
                                    context
                                        .run_compute_async(&schist_fx::ComputeJob {
                                            input: &input,
                                            program: &program,
                                        })
                                        .await
                                } else {
                                    None
                                };
                                let rgba = if let Some(rgb) = accelerated {
                                    rgb.as_chunks::<3>()
                                        .0
                                        .iter()
                                        .zip(&tile)
                                        .flat_map(|(rgb, p)| [rgb[0], rgb[1], rgb[2], p.alpha])
                                        .collect::<Vec<_>>()
                                } else {
                                    schist_colormgmt::native_to_rgba(&tile, transform.as_ref())
                                };
                                converted
                                    .push(rgba.into_iter().map(schist_color::f32_to_u8).collect());
                            }
                            converted
                        }
                        BatchOut::F32(_) => return None,
                    };
                    if (proof.is_some() || display.is_some()) && !tiles.is_empty() {
                        let mut pixels: Vec<f32> =
                            tiles.iter().flatten().map(|&v| v as f32 / 255.0).collect();
                        let transforms = proof.iter().chain(display.iter()).collect::<Vec<_>>();
                        let operations = transforms
                            .iter()
                            .map(|t| t.gpu_operation())
                            .collect::<Option<Vec<_>>>();
                        let accelerated = if let Some(operations) = operations {
                            let operation = schist_fx::FilterOperation::Sequence(operations);
                            context
                                .filter_async(&operation, &pixels, pixels.len() / 4, 1)
                                .await
                        } else {
                            None
                        };
                        if let Some(result) = accelerated {
                            pixels = result;
                        } else {
                            for transform in transforms {
                                transform.apply(&mut pixels);
                            }
                        }
                        let mut start = 0;
                        for tile in &mut tiles {
                            for (out, &value) in tile.iter_mut().zip(&pixels[start..]) {
                                *out = schist_color::f32_to_u8(value);
                            }
                            start += tile.len();
                        }
                    }
                    for (coord, mut tile) in missing.into_iter().zip(tiles) {
                        schist_core::ink::preview_rgba8(
                            &ink_channels,
                            ink_preview,
                            coord.rect(),
                            &mut tile,
                        );
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
                let current_revision = ws
                    .doc
                    .as_ref()
                    .filter(|doc| doc.id == stamp.0 && ws.color_epoch == key.color_epoch)
                    .map(|doc| doc.revision);
                if let Some(revision) = current_revision {
                    if let Some((tiles, image)) = result {
                        // Damage may have invalidated these tiles while the
                        // GPU was working. A displayable intermediate frame
                        // must never repopulate the cache with those pixels.
                        if revision == key.revision {
                            ws.display_tiles.extend(tiles);
                        }
                        let displayed = ws.viewport_image.as_ref().map(|(key, _)| *key);
                        if ws
                            .browser_gpu
                            .requested
                            .is_some_and(|(document, requested)| {
                                document == stamp.0 && key.can_present_for(requested, displayed)
                            })
                        {
                            if let Some((_, old)) = ws.viewport_image.replace((key, image)) {
                                ws.retired_images.push(old);
                            }
                        }
                    } else if revision == key.revision {
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
