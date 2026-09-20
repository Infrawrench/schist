//! Whole-image and selection operations: Select ▸ Modify, Color
//! Range, destructive adjustments, Auto Tone/Contrast/Color, canvas
//! rotation, Trim, and colour mode.

use super::*;
use schist_i18n::t;

impl Workspace {
    /// Run a Select ▸ Modify operation as one history entry.
    pub fn apply_select_modify(&mut self, kind: ModifyKind, amount: f32, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        if doc.selection.is_empty() {
            self.status = t("common.select_something_first").into();
            cx.notify();
            return;
        }
        let n = amount.round().max(0.0) as i32;
        let mut edit = doc.begin_edit(kind.title());
        edit.change_selection(|sel, canvas| match kind {
            ModifyKind::Expand => sel.expand(n, canvas),
            ModifyKind::Contract => sel.contract(n, canvas),
            ModifyKind::Border => sel.border(n, canvas),
            ModifyKind::Smooth => sel.smooth(n, canvas),
            ModifyKind::Feather => sel.feather(amount.max(0.0)),
        });
        edit.commit();
        self.status = kind.title().into();
        self.after_change(cx);
    }

    /// Select ▸ Color Range: every pixel within `tolerance` of `target`.
    pub fn apply_color_range(&mut self, tolerance: f32, target: Rgba, cx: &mut Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        if let Some(request) = self.doc.as_ref().and_then(|doc| {
            let tiles = &doc.tree.find(doc.active_layer?)?.as_raster()?.tiles;
            let rect = doc.canvas_rect();
            let name = t("workspace.history.color_range");
            schist_plugin_api::GpuEdit::selection(
                tiles,
                rect,
                schist_core::selection_gpu::ColorMatch::FuzzyRgb {
                    color: [target.r, target.g, target.b],
                    tolerance: tolerance / 255.0,
                },
                None,
                name,
                move |doc, mask| {
                    let w = rect.width() as usize;
                    let mut edit = doc.begin_edit(name);
                    edit.change_selection(|sel, _| {
                        sel.deselect();
                        sel.activate();
                        sel.apply_shape(rect, schist_core::SelectOp::Replace, |x, y| {
                            (mask[(y - rect.top) as usize * w + (x - rect.left) as usize] * 255.0)
                                .round() as u8
                        });
                    });
                    edit.commit();
                },
            )
        }) {
            self.queue_browser_edit(request, cx);
            return;
        }
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(raster) = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .and_then(|l| l.as_raster())
        else {
            self.status = t("workspace.canvas.color_range_needs_pixel_layer").into();
            cx.notify();
            return;
        };
        let canvas = doc.canvas_rect();
        let tol = tolerance / 255.0;
        // Coverage falls off across the tolerance band rather than
        // cutting hard, which is what makes Photoshop's Fuzziness feather
        // the edges of a colour selection.
        let w = canvas.width() as usize;
        let cov = schist_core::selection_gpu::classify(
            &raster.tiles,
            canvas,
            schist_core::selection_gpu::ColorMatch::FuzzyRgb {
                color: [target.r, target.g, target.b],
                tolerance: tol,
            },
            None,
        )
        .unwrap_or_else(|| {
            let mut cov = vec![0u8; (canvas.width() * canvas.height()) as usize];
            for y in canvas.top..canvas.bottom {
                for x in canvas.left..canvas.right {
                    let c = raster.tiles.pixel(x, y);
                    let d = (c.r - target.r)
                        .abs()
                        .max((c.g - target.g).abs())
                        .max((c.b - target.b).abs());
                    let v = if tol <= 0.0 {
                        if d == 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        (1.0 - d / tol).clamp(0.0, 1.0)
                    };
                    cov[(y - canvas.top) as usize * w + (x - canvas.left) as usize] =
                        (v * 255.0).round() as u8;
                }
            }
            cov
        });
        let mut edit = doc.begin_edit(t("workspace.history.color_range"));
        edit.change_selection(|sel, canvas| {
            sel.deselect();
            sel.activate();
            sel.apply_shape(canvas, schist_core::SelectOp::Replace, |x, y| {
                cov[(y - canvas.top) as usize * w + (x - canvas.left) as usize]
            });
        });
        edit.commit();
        self.status = t("workspace.history.color_range").into();
        self.after_change(cx);
    }

    /// Image ▸ Adjustments: apply an adjustment straight onto the active
    /// layer's pixels, rather than adding a layer for it.
    ///
    /// Opens the same dialog as the adjustment layers do, but previewing
    /// writes pixels; that is what "destructive" means here.
    pub fn apply_adjustment_destructive(
        &mut self,
        kind: schist_core::AdjustmentKind,
        cx: &mut Context<Self>,
    ) {
        self.commit_recording_transform(cx);
        let params = schist_adjustments::Params::default_for(kind);
        if !self.begin_filter_preview() {
            cx.notify();
            return;
        }
        self.open_modal(
            Modal::DestructiveAdjustment {
                kind,
                params: Box::new(params),
                preview: true,
            },
            cx,
        );
    }

    /// Re-run a destructive adjustment's preview from the snapshot.
    pub fn preview_destructive_adjustment(
        &mut self,
        params: Option<&schist_adjustments::Params>,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_arch = "wasm32")]
        self.cancel_browser_filter();
        let Some(preview) = self.filter_preview.clone() else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        if let Some(params) = params {
            if self.queue_browser_adjustment(params, "", &preview, false, cx) {
                return;
            }
        }
        let mut buf = preview.original.clone();
        if let Some(params) = params {
            params.apply_buffer(&mut buf);
        }
        self.write_region(
            preview.layer,
            preview.region,
            &preview.original,
            &buf,
            "",
            false,
        );
        self.after_change(cx);
    }

    /// Commit a destructive adjustment as one history entry.
    pub fn commit_destructive_adjustment(
        &mut self,
        kind: schist_core::AdjustmentKind,
        params: &schist_adjustments::Params,
        cx: &mut Context<Self>,
    ) {
        // Put the previewed pixels back first so the edit records the
        // right "before".
        self.preview_destructive_adjustment(None, cx);
        let Some(preview) = self.filter_preview.take() else {
            return;
        };
        let name = crate::ui::adjustment_name(kind);
        #[cfg(target_arch = "wasm32")]
        if !self.action_recorder.recording
            && self.queue_browser_adjustment(params, name, &preview, true, cx)
        {
            return;
        }
        let mut buf = preview.original.clone();
        params.apply_buffer(&mut buf);
        self.write_region(
            preview.layer,
            preview.region,
            &preview.original,
            &buf,
            name,
            true,
        );
        self.status = name.into();
        self.record_action_step(recorded_actions::Step::PixelAdjustment {
            params: params.clone(),
        });
        self.after_change(cx);
    }

    /// Image ▸ Auto Tone / Auto Contrast / Auto Color.
    ///
    /// All three stretch the histogram to fill the range; they differ in
    /// whether the channels are stretched together (contrast, preserving
    /// the colour cast) or apart (tone and colour, removing it), and
    /// whether the midpoint is re-centred (colour).
    pub fn auto_adjust(&mut self, mode: AutoMode, cx: &mut Context<Self>) {
        if !self.begin_filter_preview() {
            cx.notify();
            return;
        }
        let Some(preview) = self.filter_preview.take() else {
            return;
        };
        let mut buf = preview.original.clone();
        let correction = match mode {
            AutoMode::Tone => schist_adjustments::auto::AutoMode::Tone,
            AutoMode::Contrast => schist_adjustments::auto::AutoMode::Contrast,
            AutoMode::Color => schist_adjustments::auto::AutoMode::Color,
        };
        #[cfg(target_arch = "wasm32")]
        if buf.as_chunks::<4>().0.iter().any(|p| p[3] > 0.0)
            && self.queue_browser_auto(correction, mode.title(), &preview, cx)
        {
            return;
        }
        if !schist_adjustments::auto::apply(&mut buf, correction) {
            self.status = t("workspace.canvas.nothing_to_adjust").into();
            cx.notify();
            return;
        }
        let name = mode.title();
        self.write_region(
            preview.layer,
            preview.region,
            &preview.original,
            &buf,
            name,
            true,
        );
        self.status = name.into();
        self.after_change(cx);
    }

    /// Image ▸ Image Rotation, and the flip entries under Edit ▸ Transform.
    pub fn transform_canvas(&mut self, op: CanvasTransform, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        transform_document(doc, op);
        self.status = op.title().into();
        self.fit_to_view();
        self.after_change(cx);
    }

    /// Image ▸ Trim: crop away uniform borders.
    pub fn trim(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let canvas = doc.canvas_rect();
        // What counts as "border" is the colour of the top-left pixel of
        // the composited image, or transparency where there is none.
        let flat = schist_compositor::composite_region_rgba8(doc, canvas);
        let w = canvas.width() as usize;
        let at = |x: i32, y: i32| -> [u8; 4] {
            let i = (y as usize * w + x as usize) * 4;
            [flat[i], flat[i + 1], flat[i + 2], flat[i + 3]]
        };
        let key = at(0, 0);
        let same = |p: [u8; 4]| p == key || (p[3] == 0 && key[3] == 0);
        let mut keep = IntRect::EMPTY;
        for y in 0..canvas.height() {
            for x in 0..canvas.width() {
                if !same(at(x, y)) {
                    keep = keep.union(&IntRect::new(x, y, x + 1, y + 1));
                }
            }
        }
        if keep.is_empty() || keep == canvas {
            self.status = t("workspace.canvas.nothing_to_trim").into();
            cx.notify();
            return;
        }
        self.resize_canvas_to(keep, cx);
    }

    /// Crop the document to `rect`, moving every layer with it.
    pub fn resize_canvas_to(&mut self, rect: IntRect, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        schist_tools_transform::crop_to(doc, rect);
        self.status = t("workspace.canvas.trimmed").into();
        self.fit_to_view();
        self.after_change(cx);
    }

    /// Image ▸ Mode: switch the document between RGB and Grayscale.
    pub fn set_color_mode(&mut self, mode: schist_color::ColorMode, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        if doc.mode == mode {
            return;
        }
        // Indexed Color needs palette quantisation, which does not exist.
        // It used to fall into the greyscale branch below, desaturating
        // the image and labelling the history entry "Grayscale", so the
        // menu item claimed to do something it had never implemented.
        if mode == schist_color::ColorMode::Indexed {
            self.status = t("workspace.canvas.indexed_not_supported").into();
            cx.notify();
            return;
        }
        let mut edit = doc.begin_edit(color_mode_name(mode));
        edit.set_color_mode(mode);
        edit.commit();
        self.rebuild_color_transforms();
        if let Some(doc) = self.doc.as_mut() {
            doc.damage_all();
        }
        self.status = color_mode_name(mode).into();
        self.after_change(cx);
    }
}

/// The name of a colour mode as the chrome shows it; the kernel's
/// `display_name` is the English the file format knows.
fn color_mode_name(mode: schist_color::ColorMode) -> &'static str {
    use schist_color::ColorMode::*;
    t(match mode {
        Rgb => "common.rgb",
        Grayscale => "common.grayscale",
        Cmyk => "common.cmyk",
        Lab => "common.lab",
        Indexed => "common.indexed",
    })
}

/// Turn or flip a whole document, every raster layer with it, as one
/// history entry. The gallery's batch run uses this on documents that
/// never reach the editor.
pub(super) fn transform_document(doc: &mut Document, op: CanvasTransform) {
    let (w, h) = (doc.width, doc.height);
    let swaps = matches!(op, CanvasTransform::Cw90 | CanvasTransform::Ccw90);
    let (nw, nh) = if swaps { (h, w) } else { (w, h) };
    // Read every layer's pixels first: the mapping reads from the old
    // geometry while writing the new one.
    let ids: Vec<schist_core::LayerId> = doc.tree.iter().map(|l| l.id).collect();
    let sources: Vec<(schist_core::LayerId, schist_core::TileMap)> = ids
        .iter()
        .filter_map(|id| {
            doc.tree
                .find(*id)
                .and_then(|l| l.as_raster())
                .map(|r| (*id, r.tiles.clone()))
        })
        .collect();
    let mut edit = doc.begin_edit(op.title());
    edit.set_canvas_size(nw, nh);
    for (id, src) in &sources {
        for coord in TileCoord::covering(&IntRect::from_size(nw, nh)) {
            let trect = coord.rect();
            let Some(tile) = edit.writable_tile(*id, coord) else {
                break;
            };
            for y in trect.top..trect.bottom {
                for x in trect.left..trect.right {
                    // Where this destination pixel came from.
                    let (sx, sy) = match op {
                        CanvasTransform::Cw90 => (y, nw as i32 - 1 - x),
                        CanvasTransform::Ccw90 => (nh as i32 - 1 - y, x),
                        CanvasTransform::Rotate180 => (w as i32 - 1 - x, h as i32 - 1 - y),
                        CanvasTransform::FlipH => (w as i32 - 1 - x, y),
                        CanvasTransform::FlipV => (x, h as i32 - 1 - y),
                    };
                    let ix = ((y - trect.top) * TILE_SIZE + (x - trect.left)) as usize;
                    tile.set_native_pixel(ix, src.native_pixel(sx, sy));
                }
            }
        }
    }
    edit.commit();
}
