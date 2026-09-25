//! Assembling the visible tiles into one image for the canvas element.

use super::*;

impl Workspace {
    pub(super) fn has_browser_gpu(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            self.browser_gpu.context.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Which document pixels can land on screen at the current zoom, pan
    /// and rotation, clipped to the canvas. `width`/`height` are the
    /// canvas element's size in device pixels.
    pub(super) fn visible_doc_rect(
        &self,
        width: usize,
        height: usize,
        scale_factor: f32,
        canvas_rect: IntRect,
    ) -> IntRect {
        let sf = scale_factor.max(0.01);
        let inv_zoom = 1.0 / self.zoom;
        let origin = (f32::from(self.offset.x) * sf, f32::from(self.offset.y) * sf);
        // Rotation is about the middle of the viewport, which is what
        // makes spinning the view feel like turning a sheet of paper.
        let centre = (width as f32 / 2.0, height as f32 / 2.0);
        let (rs, rc) = (-self.rotation).sin_cos();
        let doc_at = |dx: f32, dy: f32| -> (f32, f32) {
            let (ox, oy) = (dx - centre.0, dy - centre.1);
            let (rx, ry) = (ox * rc - oy * rs + centre.0, ox * rs + oy * rc + centre.1);
            (
                (rx - origin.0) * inv_zoom / sf,
                (ry - origin.1) * inv_zoom / sf,
            )
        };
        // With rotation, the visible region is the union of all four
        // corners rather than the span between two of them.
        let mut span = IntRect::EMPTY;
        for (cx, cy) in [
            (0.0, 0.0),
            (width as f32, 0.0),
            (width as f32, height as f32),
            (0.0, height as f32),
        ] {
            let (x, y) = doc_at(cx, cy);
            span = span.union(&IntRect::new(
                x.floor() as i32 - 1,
                y.floor() as i32 - 1,
                x.ceil() as i32 + 1,
                y.ceil() as i32 + 1,
            ));
        }
        span.intersect(&canvas_rect)
    }

    fn viewport_key(&self, bounds: Bounds<Pixels>, scale_factor: f32) -> Option<ViewportKey> {
        let sf = scale_factor.max(0.01);
        let width = (f32::from(bounds.size.width) * sf).round().max(1.0) as usize;
        let height = (f32::from(bounds.size.height) * sf).round().max(1.0) as usize;
        // A sanity cap: a hostile window size shouldn't allocate gigabytes.
        if width * height > 64 << 20 {
            return None;
        }
        let doc = self.doc.as_ref()?;
        Some(ViewportKey {
            revision: doc.revision,
            zoom: self.zoom.to_bits(),
            offset: (
                (f32::from(self.offset.x) * sf).to_bits(),
                (f32::from(self.offset.y) * sf).to_bits(),
            ),
            scale_factor: sf.to_bits(),
            size: (width as u32, height as u32),
            color_epoch: self.color_epoch,
            rotation: self.rotation.to_bits(),
            surround: crate::ui::palette().canvas_bg,
            seamless: self.editor.seamless_painting,
        })
    }

    /// Assemble the visible tiles into one resampled, checkered BGRA image.
    /// While a browser GPU frame is pending, return the cached image positioned
    /// at the current zoom and pan, including after the gesture has ended.
    pub(super) fn assemble_viewport(
        &mut self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        _cx: &mut Context<Self>,
    ) -> Option<(Bounds<Pixels>, Arc<RenderImage>)> {
        let key = self.viewport_key(bounds, scale_factor)?;
        let sf = f32::from_bits(key.scale_factor);
        let (width, height) = (key.size.0 as usize, key.size.1 as usize);
        let doc = self.doc.as_ref()?;
        let canvas_rect = doc.canvas_rect();
        // Even a cache hit or an empty view supersedes an in-flight frame.
        #[cfg(target_arch = "wasm32")]
        {
            self.browser_gpu.requested = Some((doc.id, key));
        }
        if let Some((cached_key, image)) = &self.viewport_image {
            if *cached_key == key {
                return Some((bounds, image.clone()));
            }
        }

        // Which document pixels can land on screen?
        let zoom = self.zoom;
        let origin = (f32::from_bits(key.offset.0), f32::from_bits(key.offset.1));
        let regions = if key.seamless {
            let span = self.visible_doc_rect(
                width,
                height,
                sf,
                IntRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
            );
            schist_compositor::viewport::periodic_regions(span, canvas_rect)
        } else {
            vec![self.visible_doc_rect(width, height, sf, canvas_rect)]
        };
        let visible = regions.iter().fold(IntRect::EMPTY, |a, b| a.union(b));
        if visible.is_empty() {
            // Nothing but background: a 1x1 image keeps the paint path simple.
            let s = (key.surround & 0xFF) as u8;
            let buffer = image::RgbaImage::from_raw(1, 1, vec![s, s, s, 255])?;
            let img = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
            if let Some((_, old)) = self.viewport_image.replace((key, img.clone())) {
                self.retired_images.push(old);
            }
            return Some((bounds, img));
        }

        // Composite the visible tiles, then index them by grid position so
        // sampling is an array lookup rather than a hash per pixel.
        let (tx0, ty0) = (
            visible.left.div_euclid(TILE_SIZE),
            visible.top.div_euclid(TILE_SIZE),
        );
        let cols = ((visible.right - 1).div_euclid(TILE_SIZE) - tx0 + 1).max(1) as usize;
        let rows = ((visible.bottom - 1).div_euclid(TILE_SIZE) - ty0 + 1).max(1) as usize;
        // A repeated view can touch opposite corners of a huge document.
        // Bound the sparse slot table before allocating it.
        let slots = cols.checked_mul(rows)?;
        if slots > 4 * 1024 * 1024 {
            return None;
        }
        let mut coords: Vec<TileCoord> = regions.iter().flat_map(TileCoord::covering).collect();
        coords.sort_unstable_by_key(|c| (c.ty, c.tx));
        coords.dedup();
        #[cfg(target_arch = "wasm32")]
        if !key.seamless && self.queue_browser_viewport(key, visible, &coords, sf, _cx) {
            return self.cached_viewport_quad(bounds, key);
        }
        let mut grid: Vec<Option<Arc<Vec<u8>>>> = vec![None; slots];
        if let Some(doc) = self.doc.as_ref() {
            self.cache.prewarm(doc, &coords);
        }
        for coord in coords {
            let ix = (coord.ty - ty0) as usize * cols + (coord.tx - tx0) as usize;
            if let Some(slot) = grid.get_mut(ix) {
                *slot = self.display_tile(coord);
            }
        }

        // The rest of the document renders during idle time, nearest tiles
        // first, so scrolling lands on warm caches instead of popping in.
        self.rebuild_prefetch_queue(canvas_rect, visible, false);

        // Resample on the active backend (GPU when installed and the grid
        // fits its buffers), with the CPU reference as the always-correct
        // fallback. Both implement the same contract — see
        // `schist_compositor::viewport`.
        let params = schist_compositor::viewport::ViewportParams {
            width,
            height,
            origin,
            zoom,
            scale_factor: sf,
            rotation: self.rotation,
            canvas: canvas_rect,
            grid_origin: (tx0, ty0),
            grid_cols: cols,
            grid_rows: rows,
            surround: key.surround,
        };
        let bgra = if key.seamless {
            schist_compositor::viewport::render_viewport_periodic_cpu(&params, &grid)
        } else {
            schist_compositor::backend()
                .viewport(&params, &grid)
                .unwrap_or_else(|| schist_compositor::viewport::render_viewport_cpu(&params, &grid))
        };

        let buffer = image::RgbaImage::from_raw(width as u32, height as u32, bgra)?;
        let img = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        // Release the previous frame's atlas slot.
        if let Some((_, old)) = self.viewport_image.replace((key, img.clone())) {
            self.retired_images.push(old);
        }
        Some((bounds, img))
    }

    /// Mid-gesture stand-in for `assemble_viewport`: the previous frame's
    /// image, positioned so GPUI stretches it to the current zoom and pan
    /// on its GPU. Resampling the document and uploading a fresh
    /// full-viewport texture on every wheel tick is what made zooming lag;
    /// a slightly soft frame is invisible while the view is in motion, and
    /// the settle timer requests a crisp replacement when the hand stops.
    pub(super) fn gesture_viewport_quad(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
    ) -> Option<(Bounds<Pixels>, Arc<RenderImage>)> {
        if !self.view_gesture_active {
            return None;
        }
        let requested = self.viewport_key(bounds, scale_factor)?;
        // Edits still need a new frame even during a gesture.
        if self.viewport_image.as_ref()?.0.revision != requested.revision {
            return None;
        }
        self.cached_viewport_quad(bounds, requested)
    }

    fn cached_viewport_quad(
        &self,
        bounds: Bounds<Pixels>,
        requested: ViewportKey,
    ) -> Option<(Bounds<Pixels>, Arc<RenderImage>)> {
        let (key, image) = self.viewport_image.as_ref()?;
        let projection = key.reprojection_for(requested)?;
        // Textures cover the logical bounds even when their device-pixel
        // dimensions have been rounded. Use that same mapping for translation.
        let t = (
            projection.translation.0 * f32::from(bounds.size.width) / key.size.0 as f32,
            projection.translation.1 * f32::from(bounds.size.height) / key.size.1 as f32,
        );
        Some((
            Bounds {
                origin: point(bounds.origin.x + px(t.0), bounds.origin.y + px(t.1)),
                size: size(
                    px(f32::from(bounds.size.width) * projection.scale),
                    px(f32::from(bounds.size.height) * projection.scale),
                ),
            },
            image.clone(),
        ))
    }

    /// Can the real frame be rebuilt mid-gesture without stalling? True
    /// when the gesture is a pure pan — zoom and rotation match the last
    /// full frame — and every visible tile is already composited and
    /// colour-managed, so `assemble_viewport` is just a resample. Panning
    /// then renders crisp on every tick and fills ground the stale image
    /// never covered, instead of flashing surround until the hand stops;
    /// the stale-quad path remains for zooming (where every tick
    /// invalidates the whole frame) and for scrolls that outrun the
    /// prefetch into cold tiles.
    pub(super) fn warm_pan_frame_ready(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        canvas_rect: IntRect,
    ) -> bool {
        let Some((key, _)) = &self.viewport_image else {
            return false;
        };
        if key.zoom != self.zoom.to_bits() || key.rotation != self.rotation.to_bits() {
            return false;
        }
        let sf = scale_factor.max(0.01);
        let width = (f32::from(bounds.size.width) * sf).round().max(1.0) as usize;
        let height = (f32::from(bounds.size.height) * sf).round().max(1.0) as usize;
        let visible = self.visible_doc_rect(width, height, sf, canvas_rect);
        TileCoord::covering(&visible)
            .all(|c| self.cache.contains(c) && self.display_tiles.contains_key(&c))
    }

    pub(super) fn refresh_preview(&mut self) -> Option<Arc<RenderImage>> {
        let doc = self.doc.as_ref()?;
        let (w, h) = (
            (doc.width >> PREVIEW_SHIFT).max(1),
            (doc.height >> PREVIEW_SHIFT).max(1),
        );
        let full = IntRect::from_size(doc.width, doc.height);
        if !self.preview.valid || self.preview.w != w || self.preview.h != h {
            self.preview.buf = vec![0u8; (w * h * 4) as usize];
            self.preview.w = w;
            self.preview.h = h;
            self.preview.dirty = vec![full];
            self.preview.valid = true;
        }
        let dirty = std::mem::take(&mut self.preview.dirty);
        if dirty.is_empty() {
            if let Some(img) = &self.preview.image {
                return Some(img.clone());
            }
        }
        let step = 1i32 << PREVIEW_SHIFT;
        for rect in dirty {
            let rect = rect.intersect(&full);
            if rect.is_empty() {
                continue;
            }
            let mut rgba = schist_compositor::composite_region_rgba8(doc, rect);
            if self.color_managed() {
                let mut managed: Vec<f32> = rgba.iter().map(|&v| v as f32 / 255.0).collect();
                self.to_display(&mut managed);
                for (out, value) in rgba.iter_mut().zip(managed) {
                    *out = schist_color::f32_to_u8(value);
                }
            }
            schist_core::ink::preview_rgba8(&doc.ink_channels, doc.ink_preview, rect, &mut rgba);
            let rw = rect.width() as usize;
            // Point-sample the full-res composite into the preview buffer.
            let px0 = rect.left.div_euclid(step).max(0);
            let py0 = rect.top.div_euclid(step).max(0);
            let px1 = ((rect.right - 1).div_euclid(step) + 1).min(w as i32);
            let py1 = ((rect.bottom - 1).div_euclid(step) + 1).min(h as i32);
            for py in py0..py1 {
                let sy = (py * step + step / 2).clamp(rect.top, rect.bottom - 1);
                for pxx in px0..px1 {
                    let sx = (pxx * step + step / 2).clamp(rect.left, rect.right - 1);
                    let s = (((sy - rect.top) as usize * rw) + (sx - rect.left) as usize) * 4;
                    let (r, g, b, a) = (
                        rgba[s] as u32,
                        rgba[s + 1] as u32,
                        rgba[s + 2] as u32,
                        rgba[s + 3] as u32,
                    );
                    let bg = if ((sx >> 3) + (sy >> 3)) & 1 == 0 {
                        0xFFu32
                    } else {
                        0xCCu32
                    };
                    let inv = 255 - a;
                    let d = ((py as u32 * w + pxx as u32) * 4) as usize;
                    self.preview.buf[d] = ((b * a + bg * inv) / 255) as u8;
                    self.preview.buf[d + 1] = ((g * a + bg * inv) / 255) as u8;
                    self.preview.buf[d + 2] = ((r * a + bg * inv) / 255) as u8;
                    self.preview.buf[d + 3] = 255;
                }
            }
        }
        let buffer = image::RgbaImage::from_raw(w, h, self.preview.buf.clone())?;
        let img = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        if let Some(old) = self.preview.image.replace(img.clone()) {
            self.retired_images.push(old);
        }
        Some(img)
    }
}
