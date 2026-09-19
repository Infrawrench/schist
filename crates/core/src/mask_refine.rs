//! Image-guided raster mask refinement. Sessions only read the document until
//! `apply`: a preview, or dropping a session, cannot damage source pixels.

use crate::{
    Document, DocumentId, IntRect, Layer, LayerId, LayerMask, MaskTileMap, Selection, TileCoord,
    TILE_SIZE,
};
use schist_color::{ColorMode, Depth, Rgba};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Radius in document pixels used to find foreground/background samples.
    pub radius: f32,
    pub refine: f32,
    pub smooth: f32,
    pub feather: f32,
    /// Positive expands, negative contracts, in document pixels.
    pub shift: f32,
    pub decontaminate: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            radius: 6.0,
            refine: 1.0,
            smooth: 0.0,
            feather: 0.0,
            shift: 0.0,
            decontaminate: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefineError {
    NoRaster,
    Locked,
    NoMask,
    Changed,
    UnsupportedColor,
    TooLarge,
}

/// Cheap copy-on-write snapshot, including the selection used to seed the mask.
#[derive(Clone)]
pub struct Session {
    document: DocumentId,
    revision: u64,
    layer: Layer,
    selection: Option<Selection>,
    bounds: IntRect,
    depth: Depth,
    mode: ColorMode,
}

#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgba>,
    pub mask: Vec<f32>,
    /// Number of document pixels represented by one sampled pixel.
    pub scale: f32,
}

impl Session {
    pub fn capture(doc: &Document) -> Result<Self, RefineError> {
        let layer = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .ok_or(RefineError::NoRaster)?;
        if layer.as_raster().is_none() || layer.render_offset != (0, 0) {
            return Err(RefineError::NoRaster);
        }
        if layer.locked {
            return Err(RefineError::Locked);
        }
        let selection = (!doc.selection.is_empty()).then(|| doc.selection.clone());
        if selection.is_none() && layer.mask.is_none() {
            return Err(RefineError::NoMask);
        }
        Ok(Self {
            document: doc.id,
            revision: doc.revision,
            layer: layer.clone(),
            selection,
            bounds: doc.canvas_rect(),
            depth: doc.depth,
            mode: doc.mode,
        })
    }

    /// A bounded-size preview or full-resolution processing input. Sampling is
    /// deterministic and never modifies the document or its selection.
    pub fn sample(&self, max_side: usize) -> Result<Image, RefineError> {
        self.sample_region(self.bounds, max_side)
    }

    fn sample_region(&self, bounds: IntRect, max_side: usize) -> Result<Image, RefineError> {
        let scale = (bounds.width().max(bounds.height()) as f32 / max_side.max(1) as f32).max(1.0);
        let width = (bounds.width() as f32 / scale).ceil().max(1.0) as usize;
        let height = (bounds.height() as f32 / scale).ceil().max(1.0) as usize;
        // Refine uses several float planes. Refuse an excessive allocation
        // before constructing any of them rather than terminating the app.
        if width.checked_mul(height).is_none_or(|n| n > 32_000_000) {
            return Err(RefineError::TooLarge);
        }
        let mut pixels = Vec::with_capacity(width * height);
        let mut mask = Vec::with_capacity(width * height);
        let raster = self.layer.as_raster().ok_or(RefineError::NoRaster)?;
        for y in 0..height {
            for x in 0..width {
                let sx = (bounds.left + ((x as f32 + 0.5) * scale) as i32).min(bounds.right - 1);
                let sy = (bounds.top + ((y as f32 + 0.5) * scale) as i32).min(bounds.bottom - 1);
                pixels.push(raster.tiles.pixel(sx, sy));
                let value = match &self.selection {
                    Some(selection) => selection.coverage(sx, sy),
                    None => self.layer.mask.as_ref().map_or(255, |m| m.value(sx, sy)),
                };
                mask.push(value as f32 / 255.0);
            }
        }
        Ok(Image {
            width,
            height,
            pixels,
            mask,
            scale,
        })
    }

    pub fn can_decontaminate(&self) -> bool {
        self.mode == ColorMode::Rgb
            && self
                .layer
                .as_raster()
                .is_some_and(|r| r.tiles.mode() == ColorMode::Rgb)
    }

    /// Compute in bounded chunks with enough overlapping context for every
    /// finite-radius operation. Large photos do not require full float planes.
    pub fn start(
        &self,
        settings: Settings,
        duplicate_name: &str,
    ) -> Result<Preparation, RefineError> {
        if settings.decontaminate > 0.0 && !self.can_decontaminate() {
            return Err(RefineError::UnsupportedColor);
        }
        let mask = LayerMask {
            tiles: MaskTileMap::new(),
            enabled: true,
            linked: self.layer.mask.as_ref().is_none_or(|m| m.linked),
            default_value: 0,
            bounds: self.bounds,
        };
        let duplicate = if settings.decontaminate > 0.0 {
            let mut layer = self.layer.clone();
            layer.id = LayerId::next();
            layer.name = duplicate_name.to_owned();
            layer.visible = true;
            // Source-backed caches would overwrite the corrected raster.
            layer.smart = None;
            layer.raw = None;
            layer.shape = None;
            layer.styled = None;
            layer.extras.clear();
            Some(layer)
        } else {
            None
        };
        let margin = (settings.radius.clamp(0.0, 64.0).round()
            + settings.smooth.clamp(0.0, 20.0).round()
            + settings.shift.clamp(-20.0, 20.0).round().abs()
            + settings.feather.clamp(0.0, 40.0).round()) as i32
            + 2;
        let seed_bounds = self
            .selection
            .as_ref()
            .map(|s| s.bounds())
            .or_else(|| {
                self.layer
                    .mask
                    .as_ref()
                    .and_then(|m| (m.default_value == 0).then_some(m.bounds))
            })
            .unwrap_or(self.bounds);
        let affected = if seed_bounds.is_empty() {
            IntRect::EMPTY
        } else {
            seed_bounds.inflated(margin).intersect(&self.bounds)
        };
        // Align block starts to the tile grid so adjacent chunks never
        // overwrite each other's partial tiles. Keep sparse selections cheap
        // even on a very large canvas.
        let work_bounds = IntRect::new(
            affected.left.div_euclid(512) * 512,
            affected.top.div_euclid(512) * 512,
            affected.right,
            affected.bottom,
        );
        Ok(Preparation {
            session: self.clone(),
            settings,
            margin,
            work_bounds,
            left: work_bounds.left,
            top: work_bounds.top,
            prepared: Prepared {
                document: self.document,
                revision: self.revision,
                source: self.layer.id,
                mask,
                duplicate,
            },
        })
    }

    pub fn prepare(
        &self,
        settings: Settings,
        duplicate_name: &str,
    ) -> Result<Prepared, RefineError> {
        let mut job = self.start(settings, duplicate_name)?;
        while job.step()? {}
        job.finish()
    }

    pub fn apply(
        &self,
        doc: &mut Document,
        settings: Settings,
        history_name: &str,
        duplicate_name: &str,
    ) -> Result<LayerId, RefineError> {
        self.prepare(settings, duplicate_name)?
            .apply(doc, history_name)
    }
}

/// A bounded processing job. Each step handles one 512-pixel block;
/// frontends can yield to input and abandon the job between steps.
pub struct Preparation {
    session: Session,
    settings: Settings,
    margin: i32,
    work_bounds: IntRect,
    left: i32,
    top: i32,
    prepared: Prepared,
}

impl Preparation {
    pub fn step(&mut self) -> Result<bool, RefineError> {
        let session = &self.session;
        if self.top >= self.work_bounds.bottom || self.work_bounds.is_empty() {
            return Ok(false);
        }
        let (left, top) = (self.left, self.top);
        let block = IntRect::new(
            left,
            top,
            (left + 512).min(self.work_bounds.right),
            (top + 512).min(self.work_bounds.bottom),
        );
        let region = block.inflated(self.margin).intersect(&session.bounds);
        let mut image = session.sample_region(region, usize::MAX)?;
        image.refine(self.settings);
        for coord in TileCoord::covering(&block) {
            let rect = coord.rect();
            let clip = rect.intersect(&block);
            let mut tile = [0u8; crate::TILE_PIXELS];
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let i = (y - region.top) as usize * image.width + (x - region.left) as usize;
                    tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                        (image.mask[i].clamp(0.0, 1.0) * 255.0).round() as u8;
                    if let Some(layer) = self.prepared.duplicate.as_mut() {
                        let raster = layer.as_raster_mut().ok_or(RefineError::NoRaster)?;
                        // Keep depth and unchanged samples, including hidden RGB.
                        if image.pixels[i] != raster.tiles.pixel(x, y) {
                            raster
                                .tiles
                                .get_mut_or_insert_mode(coord, session.depth, session.mode)
                                .set(
                                    ((y - rect.top) * TILE_SIZE + x - rect.left) as usize,
                                    image.pixels[i],
                                );
                        }
                    }
                }
            }
            if tile.iter().any(|&value| value != 0) {
                // A 256 MiB output ceiling protects unusually large
                // documents; sparse masks consume only nonzero tiles.
                if self.prepared.mask.tiles.iter().count() >= 4096 {
                    return Err(RefineError::TooLarge);
                }
                self.prepared
                    .mask
                    .tiles
                    .insert(coord, std::sync::Arc::new(tile));
            }
        }
        self.left += 512;
        if self.left >= self.work_bounds.right {
            self.left = self.work_bounds.left;
            self.top += 512;
        }
        Ok(true)
    }

    pub fn finish(mut self) -> Result<Prepared, RefineError> {
        if self.top < self.work_bounds.bottom && !self.work_bounds.is_empty() {
            return Err(RefineError::Changed);
        }
        if let Some(layer) = &mut self.prepared.duplicate {
            layer.mask = Some(self.prepared.mask.clone());
        }
        Ok(self.prepared)
    }
}

/// Ready-to-commit result. Dropping this is cancellation, even when expensive
/// preparation completed on a worker after the dialog was dismissed.
pub struct Prepared {
    document: DocumentId,
    revision: u64,
    source: LayerId,
    mask: LayerMask,
    duplicate: Option<Layer>,
}

impl Prepared {
    pub fn apply(self, doc: &mut Document, history_name: &str) -> Result<LayerId, RefineError> {
        if doc.id != self.document
            || doc.revision != self.revision
            || doc.active_layer != Some(self.source)
        {
            return Err(RefineError::Changed);
        }
        let mut path = doc.tree.path_of(self.source).ok_or(RefineError::Changed)?;
        *path.0.last_mut().ok_or(RefineError::Changed)? += 1;
        let mut edit = doc.begin_edit(history_name);
        let output = if let Some(layer) = self.duplicate {
            edit.change_props(self.source, |layer| layer.visible = false);
            edit.insert_layer(path, layer)
        } else {
            edit.set_mask(self.source, Some(self.mask));
            self.source
        };
        edit.commit();
        doc.active_layer = Some(output);
        doc.selected = vec![output];
        Ok(output)
    }
}

impl Image {
    pub fn refine(&mut self, settings: Settings) {
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 || self.mask.len() != w * h || self.pixels.len() != w * h {
            return;
        }
        let radius = (settings.radius.clamp(0.0, 64.0) / self.scale).round() as usize;
        let smooth = (settings.smooth.clamp(0.0, 20.0) / self.scale).round() as usize;
        let feather = (settings.feather.clamp(0.0, 40.0) / self.scale).round() as usize;
        let shift = (settings.shift.clamp(-20.0, 20.0) / self.scale).round() as i32;
        if smooth > 0 {
            self.mask = box_mean(&self.mask, w, h, smooth);
            // Smooth small islands while keeping a narrow, antialiased edge.
            for a in &mut self.mask {
                *a = ((*a - 0.25) * 2.0).clamp(0.0, 1.0);
            }
        }
        if radius > 0 && (settings.refine > 0.0 || settings.decontaminate > 0.0) {
            self.guide(
                radius,
                settings.refine.clamp(0.0, 1.0),
                settings.decontaminate.clamp(0.0, 1.0),
            );
        }
        if shift != 0 {
            self.mask = morphology(&self.mask, w, h, shift.unsigned_abs() as usize, shift > 0);
        }
        if feather > 0 {
            self.mask = box_mean(&self.mask, w, h, feather);
        }
    }

    fn guide(&mut self, radius: usize, strength: f32, decontaminate: f32) {
        let (w, h) = (self.width, self.height);
        let count = w * h;
        let fg: Vec<f32> = self
            .mask
            .iter()
            .zip(&self.pixels)
            .map(|(&a, p)| if a >= 0.95 && p.a > 0.0 { p.a } else { 0.0 })
            .collect();
        let bg: Vec<f32> = self
            .mask
            .iter()
            .zip(&self.pixels)
            .map(|(&a, p)| if a <= 0.05 && p.a > 0.0 { p.a } else { 0.0 })
            .collect();
        let fg_count = box_mean(&fg, w, h, radius);
        let bg_count = box_mean(&bg, w, h, radius);
        let mut foreground = vec![[0.0; 3]; count];
        let mut background = vec![[0.0; 3]; count];
        for channel in 0..3 {
            let component = |p: &Rgba| match channel {
                0 => p.r,
                1 => p.g,
                _ => p.b,
            };
            let f: Vec<f32> = self
                .pixels
                .iter()
                .zip(&fg)
                .map(|(p, &weight)| component(p) * weight)
                .collect();
            let b: Vec<f32> = self
                .pixels
                .iter()
                .zip(&bg)
                .map(|(p, &weight)| component(p) * weight)
                .collect();
            let f = box_mean(&f, w, h, radius);
            let b = box_mean(&b, w, h, radius);
            for i in 0..count {
                foreground[i][channel] = f[i] / fg_count[i].max(1e-8);
                background[i][channel] = b[i] / bg_count[i].max(1e-8);
            }
        }
        for i in 0..count {
            // Only the local boundary band has both known colors. Solid
            // interiors and distant background are kept exactly unchanged.
            if fg_count[i] < 1e-6 || bg_count[i] < 1e-6 {
                continue;
            }
            let f = foreground[i];
            let b = background[i];
            let p = self.pixels[i];
            let c = [p.r, p.g, p.b];
            let mut dot = 0.0;
            let mut contrast = 0.0;
            for k in 0..3 {
                dot += (c[k] - b[k]) * (f[k] - b[k]);
                contrast += (f[k] - b[k]).powi(2);
            }
            // Ambiguous same-color foreground/background retain the seed.
            if contrast > 0.0025 && p.a > 0.0 {
                let alpha = (dot / contrast).clamp(0.0, 1.0);
                let confidence = ((contrast - 0.0025) / 0.04).clamp(0.0, 1.0);
                self.mask[i] += (alpha - self.mask[i]) * strength * confidence;
            }
            // Replace spill colors with local, confidently foreground color.
            // Source alpha never changes; the separate mask carries coverage.
            let amount = decontaminate * (1.0 - self.mask[i]);
            if amount > 0.0 && self.mask[i] > 0.0 && p.a > 0.0 {
                self.pixels[i] = Rgba::new(
                    p.r + (f[0] - p.r) * amount,
                    p.g + (f[1] - p.g) * amount,
                    p.b + (f[2] - p.b) * amount,
                    p.a,
                );
            }
        }
    }
}

/// Separable clipped box average, linear in pixel count even at large radii.
fn box_mean(input: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return input.to_vec();
    }
    let mut temp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        let mut sum: f32 = input[y * w..y * w + (radius + 1).min(w)].iter().sum();
        for x in 0..w {
            temp[y * w + x] = sum / ((x + radius + 1).min(w) - x.saturating_sub(radius)) as f32;
            if x >= radius {
                sum -= input[y * w + x - radius];
            }
            if x + radius + 1 < w {
                sum += input[y * w + x + radius + 1];
            }
        }
    }
    for x in 0..w {
        let mut sum: f32 = (0..(radius + 1).min(h)).map(|y| temp[y * w + x]).sum();
        for y in 0..h {
            out[y * w + x] = sum / ((y + radius + 1).min(h) - y.saturating_sub(radius)) as f32;
            if y >= radius {
                sum -= temp[(y - radius) * w + x];
            }
            if y + radius + 1 < h {
                sum += temp[(y + radius + 1) * w + x];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{blit_rgba_f32, SelectOp};
    use schist_color::Depth;

    fn neutral() -> Settings {
        Settings {
            radius: 0.0,
            refine: 0.0,
            ..Settings::default()
        }
    }

    fn fixture() -> Document {
        let mut doc = Document::new("test", 9, 3, Depth::ThirtyTwo);
        let mut layer = Layer::new_raster("source");
        let pixels: Vec<f32> = (0..27)
            .flat_map(|i| {
                let x = i % 9;
                let c = if x < 4 {
                    0.0
                } else if x == 4 {
                    0.5
                } else {
                    1.0
                };
                [c, c, c, 1.0]
            })
            .collect();
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            doc.canvas_rect(),
            &pixels,
        );
        doc.push_layer(layer);
        doc.selection
            .apply_shape(doc.canvas_rect(), SelectOp::Replace, |x, _| {
                if x < 4 {
                    255
                } else {
                    0
                }
            });
        doc.mark_saved();
        doc
    }

    #[test]
    fn image_guidance_recovers_partial_edge_and_preserves_interiors() {
        let doc = fixture();
        let session = Session::capture(&doc).unwrap();
        let mut image = session.sample(100).unwrap();
        image.refine(Settings {
            radius: 3.0,
            ..Settings::default()
        });
        assert_eq!(image.mask[0], 1.0);
        assert!(
            image.mask[4] > 0.35 && image.mask[4] < 0.7,
            "{}",
            image.mask[4]
        );
        assert_eq!(image.mask[8], 0.0);
        assert_eq!(image.pixels[4].r, 0.5);
    }

    #[test]
    fn low_contrast_and_all_hidden_masks_keep_the_seed() {
        let doc = fixture();
        let mut image = Session::capture(&doc).unwrap().sample(100).unwrap();
        image.pixels.fill(Rgba::new(0.5, 0.5, 0.5, 1.0));
        let seed = image.mask.clone();
        image.refine(Settings::default());
        assert_eq!(image.mask, seed);
        image.mask.fill(0.0);
        image.refine(Settings::default());
        assert!(image.mask.iter().all(|&a| a == 0.0));
    }

    #[test]
    fn hidden_rgb_does_not_change_refinement_or_halo_removal() {
        let doc = fixture();
        let mut a = Session::capture(&doc).unwrap().sample(100).unwrap();
        for (i, p) in a.pixels.iter_mut().enumerate() {
            if i % 9 >= 6 {
                *p = Rgba::new(0.0, 1.0, 0.0, 0.0);
            }
        }
        let mut b = a.clone();
        for (i, p) in b.pixels.iter_mut().enumerate() {
            if i % 9 >= 6 {
                *p = Rgba::new(20.0, -3.0, 8.0, 0.0);
            }
        }
        let settings = Settings {
            decontaminate: 1.0,
            ..Settings::default()
        };
        a.refine(settings);
        b.refine(settings);
        assert_eq!(a.mask, b.mask);
        for i in 0..27 {
            if i % 9 < 6 {
                assert_eq!(a.pixels[i], b.pixels[i]);
            } else {
                assert_eq!(a.pixels[i].a, 0.0);
                assert_eq!(b.pixels[i].r, 20.0);
            }
        }
    }

    #[test]
    fn chunked_processing_matches_full_image_across_chunk_boundaries() {
        let mut doc = Document::new("seams", 1030, 7, Depth::ThirtyTwo);
        let mut layer = Layer::new_raster("source");
        let pixels: Vec<f32> = (0..1030 * 7)
            .flat_map(|i| {
                let x = i % 1030;
                let c = ((x as f32 - 506.0) / 12.0).clamp(0.0, 1.0);
                [c, c, c, 1.0]
            })
            .collect();
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            doc.canvas_rect(),
            &pixels,
        );
        doc.push_layer(layer);
        doc.selection
            .apply_shape(doc.canvas_rect(), SelectOp::Replace, |x, _| {
                if x < 512 {
                    255
                } else {
                    0
                }
            });
        let session = Session::capture(&doc).unwrap();
        let settings = Settings {
            radius: 12.0,
            smooth: 2.0,
            shift: -1.0,
            feather: 3.0,
            decontaminate: 0.8,
            ..Settings::default()
        };
        let mut full = session.sample(2000).unwrap();
        full.refine(settings);
        let prepared = session.prepare(settings, "copy").unwrap();
        for y in 0..7 {
            for x in 0..1030 {
                let expected = (full.mask[y * 1030 + x].clamp(0.0, 1.0) * 255.0).round() as u8;
                assert!(
                    (prepared.mask.value(x as i32, y as i32) as i16 - expected as i16).abs() <= 1,
                    "mask seam at {x},{y}"
                );
                let p = prepared
                    .duplicate
                    .as_ref()
                    .unwrap()
                    .as_raster()
                    .unwrap()
                    .tiles
                    .pixel(x as i32, y as i32);
                assert!(
                    (p.r - full.pixels[y * 1030 + x].r).abs() < 0.0001,
                    "color seam at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn discarding_prepared_output_cancels_without_document_changes() {
        let doc = fixture();
        let revision = doc.revision;
        let session = Session::capture(&doc).unwrap();
        drop(
            session
                .prepare(
                    Settings {
                        decontaminate: 1.0,
                        ..Settings::default()
                    },
                    "copy",
                )
                .unwrap(),
        );
        assert_eq!(doc.revision, revision);
        assert_eq!(doc.tree.len(), 1);
        assert!(doc.tree.find(doc.active_layer.unwrap()).unwrap().visible);
        assert!(!doc.dirty);
    }

    #[test]
    fn sparse_selection_on_large_canvas_only_retains_nonzero_tiles() {
        let mut doc = Document::new("large", 100_000, 100_000, Depth::Eight);
        doc.push_layer(Layer::new_raster("source"));
        doc.selection.apply_shape(
            IntRect::new(997, 998, 999, 1000),
            SelectOp::Replace,
            |_, _| 255,
        );
        let session = Session::capture(&doc).unwrap();
        let prepared = session.prepare(neutral(), "copy").unwrap();
        assert_eq!(prepared.mask.tiles.iter().count(), 1);
        assert_eq!(prepared.mask.value(998, 999), 255);
        assert_eq!(prepared.mask.value(999, 999), 0);
        assert_eq!(prepared.mask.value(99_999, 99_999), 0);
    }

    #[test]
    fn incomplete_jobs_cannot_be_committed() {
        let doc = fixture();
        let session = Session::capture(&doc).unwrap();
        let job = session.start(Settings::default(), "copy").unwrap();
        assert!(matches!(job.finish(), Err(RefineError::Changed)));
        assert!(!doc.dirty);
    }

    #[test]
    fn fractional_coverage_shift_and_feather_work_at_image_boundaries() {
        let input = vec![1.0, 1.0, 0.5, 0.0, 0.0];
        assert_eq!(
            morphology(&input, 5, 1, 1, true),
            vec![1.0, 1.0, 1.0, 0.5, 0.0]
        );
        assert_eq!(
            morphology(&input, 5, 1, 1, false),
            vec![1.0, 0.5, 0.0, 0.0, 0.0]
        );
        assert_eq!(box_mean(&[1.0], 1, 1, 100), vec![1.0]);
        let blurred = box_mean(&input, 5, 1, 1);
        assert_eq!(blurred[0], 1.0);
        assert!((blurred[3] - 1.0 / 6.0).abs() < 1e-6);
        assert_eq!(blurred[4], 0.0);
    }

    #[test]
    fn preview_and_cancel_leave_pixels_mask_selection_and_history_untouched() {
        let doc = fixture();
        let revision = doc.revision;
        let selection = doc.selection.generation();
        let history = doc.history.entries().len();
        {
            let session = Session::capture(&doc).unwrap();
            let mut preview = session.sample(4).unwrap();
            preview.refine(Settings {
                decontaminate: 1.0,
                ..Settings::default()
            });
        } // Cancel drops the read-only session.
        assert_eq!(doc.revision, revision);
        assert_eq!(doc.selection.generation(), selection);
        assert_eq!(doc.history.entries().len(), history);
        let layer = doc.tree.find(doc.active_layer.unwrap()).unwrap();
        assert!(layer.mask.is_none());
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(4, 1).r, 0.5);
        assert!(!doc.dirty);
    }

    #[test]
    fn mask_output_is_undoable_and_retains_source_and_selection() {
        let mut doc = fixture();
        let id = doc.active_layer.unwrap();
        let selection = doc.selection.generation();
        Session::capture(&doc)
            .unwrap()
            .apply(&mut doc, neutral(), "refine", "copy")
            .unwrap();
        let layer = doc.tree.find(id).unwrap();
        let mask = layer.mask.as_ref().unwrap();
        assert_eq!(mask.value(0, 0), 255);
        assert_eq!(mask.value(8, 2), 0);
        assert_eq!(mask.value(-1, 0), 0);
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(4, 1).r, 0.5);
        assert_eq!(doc.selection.generation(), selection);
        doc.undo().unwrap();
        assert!(doc.tree.find(id).unwrap().mask.is_none());
        doc.redo().unwrap();
        assert!(doc.tree.find(id).unwrap().mask.is_some());
    }

    #[test]
    fn decontamination_duplicates_and_undo_restores_original_visibility() {
        let mut doc = fixture();
        let source = doc.active_layer.unwrap();
        let session = Session::capture(&doc).unwrap();
        let output = session
            .apply(
                &mut doc,
                Settings {
                    radius: 3.0,
                    decontaminate: 1.0,
                    ..Settings::default()
                },
                "refine",
                "copy",
            )
            .unwrap();
        assert_ne!(source, output);
        assert_eq!(
            doc.tree
                .find(source)
                .unwrap()
                .as_raster()
                .unwrap()
                .tiles
                .pixel(4, 1)
                .r,
            0.5
        );
        assert!(!doc.tree.find(source).unwrap().visible);
        let corrected = doc.tree.find(output).unwrap();
        assert!(corrected.as_raster().unwrap().tiles.pixel(4, 1).r < 0.5);
        assert_eq!(corrected.as_raster().unwrap().tiles.pixel(4, 1).a, 1.0);
        assert!(corrected.mask.is_some());
        doc.undo().unwrap();
        assert!(doc.tree.find(source).unwrap().visible);
        assert!(doc.tree.find(output).is_none());
        doc.redo().unwrap();
        assert!(doc.tree.find(output).is_some());
    }

    #[test]
    fn existing_disabled_mask_is_seeded_and_stale_sessions_refuse_changes() {
        let mut doc = fixture();
        let id = doc.active_layer.unwrap();
        Session::capture(&doc)
            .unwrap()
            .apply(&mut doc, neutral(), "refine", "copy")
            .unwrap();
        doc.selection.deselect();
        doc.tree
            .find_mut(id)
            .unwrap()
            .mask
            .as_mut()
            .unwrap()
            .enabled = false;
        let session = Session::capture(&doc).unwrap();
        assert_eq!(session.sample(100).unwrap().mask[0], 1.0);
        assert_eq!(session.sample(100).unwrap().mask[8], 0.0);
        doc.damage_all();
        assert_eq!(
            session.apply(&mut doc, neutral(), "refine", "copy"),
            Err(RefineError::Changed)
        );
    }

    #[test]
    fn missing_mask_and_locked_layer_have_no_side_effects() {
        let mut doc = fixture();
        doc.selection.deselect();
        assert!(matches!(Session::capture(&doc), Err(RefineError::NoMask)));
        doc.tree.find_mut(doc.active_layer.unwrap()).unwrap().locked = true;
        assert!(matches!(Session::capture(&doc), Err(RefineError::Locked)));
        assert!(!doc.dirty);
    }

    #[test]
    fn native_color_channels_survive_mask_output_at_every_depth() {
        for mode in [ColorMode::Cmyk, ColorMode::Lab] {
            for depth in [Depth::Eight, Depth::Sixteen, Depth::ThirtyTwo] {
                let mut doc = Document::new("native", 2, 1, depth);
                doc.mode = mode;
                let id = doc.push_layer(Layer::new_raster("source"));
                let mut edit = doc.begin_edit("seed");
                edit.writable_tile(id, TileCoord::containing(0, 0))
                    .unwrap()
                    .set_native_pixel(
                        0,
                        schist_color::NativePixel {
                            mode,
                            color: [0.2, 0.3, 0.4, 0.7],
                            alpha: 0.6,
                        },
                    );
                edit.commit();
                doc.selection
                    .apply_shape(doc.canvas_rect(), SelectOp::Replace, |_, _| 128);
                let before = doc
                    .tree
                    .find(id)
                    .unwrap()
                    .as_raster()
                    .unwrap()
                    .tiles
                    .native_pixel(0, 0);
                let session = Session::capture(&doc).unwrap();
                assert!(!session.can_decontaminate());
                assert!(matches!(
                    session.prepare(
                        Settings {
                            decontaminate: 1.0,
                            ..Settings::default()
                        },
                        "copy"
                    ),
                    Err(RefineError::UnsupportedColor)
                ));
                session.apply(&mut doc, neutral(), "mask", "copy").unwrap();
                let layer = doc.tree.find(id).unwrap();
                assert_eq!(layer.as_raster().unwrap().tiles.native_pixel(0, 0), before);
                assert_eq!(layer.mask.as_ref().unwrap().value(0, 0), 128);
            }
        }
    }
}

fn morphology(input: &[f32], w: usize, h: usize, radius: usize, expand: bool) -> Vec<f32> {
    use std::collections::VecDeque;
    let mut temp = vec![0.0; w * h];
    let mut out = vec![0.0; w * h];
    for vertical in [false, true] {
        let (lines, length) = if vertical { (w, h) } else { (h, w) };
        for line in 0..lines {
            let index = |p: usize| if vertical { p * w + line } else { line * w + p };
            let source = if vertical { &temp } else { input };
            let mut queue: VecDeque<usize> = VecDeque::new();
            let mut next = 0;
            let mut values = Vec::with_capacity(length);
            for p in 0..length {
                while next < (p + radius + 1).min(length) {
                    while queue.back().is_some_and(|&back| {
                        if expand {
                            source[index(back)] <= source[index(next)]
                        } else {
                            source[index(back)] >= source[index(next)]
                        }
                    }) {
                        queue.pop_back();
                    }
                    queue.push_back(next);
                    next += 1;
                }
                while queue
                    .front()
                    .is_some_and(|&front| front < p.saturating_sub(radius))
                {
                    queue.pop_front();
                }
                values.push(source[index(*queue.front().expect("nonempty window"))]);
            }
            for (p, value) in values.into_iter().enumerate() {
                if vertical {
                    out[index(p)] = value;
                } else {
                    temp[index(p)] = value;
                }
            }
        }
    }
    out
}
