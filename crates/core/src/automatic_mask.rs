//! Snapshot/commit boundary for an automatically generated layer mask.
//! Inference runs elsewhere. A stale result must never overwrite a later edit.

use crate::{
    Document, DocumentId, IntRect, Layer, LayerId, LayerMask, MaskTileMap, TileCoord, TILE_PIXELS,
    TILE_SIZE,
};
use schist_color::{ColorMode, Depth, Rgba};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NoRaster,
    Locked,
    UnsupportedColor,
    TooLarge,
    InvalidMatte,
    Changed,
}

#[derive(Clone)]
pub struct Session {
    document: DocumentId,
    revision: u64,
    layer: Layer,
    bounds: IntRect,
    depth: Depth,
}

impl Session {
    pub fn capture(doc: &Document) -> Result<Self, Error> {
        let layer = doc
            .active_layer
            .and_then(|id| doc.tree.find(id))
            .ok_or(Error::NoRaster)?;
        let raster = layer.as_raster().ok_or(Error::NoRaster)?;
        // An in-progress drag has not committed its pixels/mask coordinates yet.
        if layer.render_offset != (0, 0) {
            return Err(Error::Changed);
        }
        if layer.locked {
            return Err(Error::Locked);
        }
        if doc.mode != ColorMode::Rgb || raster.tiles.mode() != ColorMode::Rgb {
            return Err(Error::UnsupportedColor);
        }
        let bounds = raster.tiles.tile_bounds().intersect(&doc.canvas_rect());
        let count = (bounds.width().max(0) as usize).checked_mul(bounds.height().max(0) as usize);
        if bounds.is_empty() {
            return Err(Error::NoRaster);
        }
        if count.is_none_or(|n| n > 16_777_216) {
            return Err(Error::TooLarge);
        }
        Ok(Self {
            document: doc.id,
            revision: doc.revision,
            layer: layer.clone(),
            bounds,
            depth: doc.depth,
        })
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.bounds.width() as usize, self.bounds.height() as usize)
    }

    /// Neutral-composite transparent pixels for the detector; hidden RGB must
    /// not look like another object. Source pixels are never changed.
    pub fn rgb(&self) -> Vec<f32> {
        let (w, h) = self.dimensions();
        let raster = self.layer.as_raster().expect("captured raster");
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in self.bounds.top..self.bounds.bottom {
            for x in self.bounds.left..self.bounds.right {
                let p = raster.tiles.pixel(x, y);
                let a = p.a.clamp(0.0, 1.0);
                rgb.extend([p.r, p.g, p.b].map(|v| v.clamp(0.0, 1.0) * a + 0.5 * (1.0 - a)));
            }
        }
        rgb
    }

    pub fn prepare(&self, alpha: &[f32]) -> Result<Prepared, Error> {
        let (w, h) = self.dimensions();
        if alpha.len() != w * h
            || alpha
                .iter()
                .any(|a| !a.is_finite() || !(0.0..=1.0).contains(a))
        {
            return Err(Error::InvalidMatte);
        }
        let existing = self.layer.mask.as_ref().filter(|mask| mask.enabled);
        let mut mask = LayerMask {
            tiles: MaskTileMap::new(),
            enabled: true,
            linked: self.layer.mask.as_ref().is_none_or(|m| m.linked),
            default_value: 0,
            bounds: self.bounds,
        };
        for coord in TileCoord::covering(&self.bounds) {
            let rect = coord.rect();
            let clip = rect.intersect(&self.bounds);
            let mut tile = [0; TILE_PIXELS];
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let i = (y - self.bounds.top) as usize * w + (x - self.bounds.left) as usize;
                    let prior = existing.map_or(255, |m| m.value(x, y)) as f32 / 255.0;
                    tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                        (alpha[i] * prior * 255.0).round() as u8;
                }
            }
            if tile.iter().any(|&v| v != 0) {
                mask.tiles.insert(coord, Arc::new(tile));
            }
        }
        Ok(Prepared {
            document: self.document,
            revision: self.revision,
            source: self.layer.id,
            mask,
            duplicate: None,
        })
    }

    /// Put corrected edge colors on a duplicate, retaining the untouched source
    /// layer beneath it. The mask still multiplies any enabled prior mask.
    /// Existing source transparency is kept verbatim: inference saw a neutral
    /// composite there, so it cannot supply reliable straight foreground RGB.
    pub fn prepare_with_foreground(
        &self,
        alpha: &[f32],
        foreground: &[f32],
        duplicate_name: &str,
    ) -> Result<Prepared, Error> {
        let mut prepared = self.prepare(alpha)?;
        if foreground.len() != alpha.len() * 3
            || foreground
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(Error::InvalidMatte);
        }
        let (w, _) = self.dimensions();
        let mut duplicate = self.layer.clone();
        let raster = duplicate.as_raster_mut().ok_or(Error::NoRaster)?;
        let mut changed = false;
        for coord in TileCoord::covering(&self.bounds) {
            let rect = coord.rect();
            let clip = rect.intersect(&self.bounds);
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let i = (y - self.bounds.top) as usize * w + (x - self.bounds.left) as usize;
                    let coverage = alpha[i] * 255.0;
                    if coverage <= 0.5 || coverage >= 254.5 || prepared.mask.value(x, y) == 0 {
                        continue;
                    }
                    let old = raster.tiles.pixel(x, y);
                    if old.a != 1.0
                        || [old.r, old.g, old.b]
                            .iter()
                            .any(|v| !(0.0..=1.0).contains(v))
                    {
                        continue;
                    }
                    let new = Rgba::new(
                        foreground[i * 3],
                        foreground[i * 3 + 1],
                        foreground[i * 3 + 2],
                        old.a,
                    );
                    let tolerance = match self.depth {
                        Depth::Eight => 0.5 / 255.0,
                        Depth::Sixteen => 0.5 / 65535.0,
                        Depth::ThirtyTwo => 0.0,
                    };
                    if (old.r - new.r)
                        .abs()
                        .max((old.g - new.g).abs())
                        .max((old.b - new.b).abs())
                        > tolerance
                    {
                        raster
                            .tiles
                            .get_mut_or_insert_mode(coord, self.depth, ColorMode::Rgb)
                            .set(((y - rect.top) * TILE_SIZE + x - rect.left) as usize, new);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            duplicate.id = LayerId::next();
            duplicate.name = duplicate_name.to_owned();
            duplicate.visible = true;
            // Cached source-backed renderers would replace corrected pixels.
            duplicate.smart = None;
            duplicate.raw = None;
            duplicate.shape = None;
            duplicate.styled = None;
            duplicate.extras.clear();
            duplicate.mask = Some(prepared.mask.clone());
            prepared.duplicate = Some(duplicate);
        }
        Ok(prepared)
    }
}

pub struct Prepared {
    document: DocumentId,
    revision: u64,
    source: LayerId,
    mask: LayerMask,
    duplicate: Option<Layer>,
}

impl Prepared {
    pub fn apply(self, doc: &mut Document, history_name: &str) -> Result<(), Error> {
        if doc.id != self.document
            || doc.revision != self.revision
            || doc.active_layer != Some(self.source)
        {
            return Err(Error::Changed);
        }
        let layer = doc.tree.find(self.source).ok_or(Error::Changed)?;
        if layer.locked || layer.render_offset != (0, 0) {
            return Err(Error::Changed);
        }
        let mut path = doc.tree.path_of(self.source).ok_or(Error::Changed)?;
        *path.0.last_mut().ok_or(Error::Changed)? += 1;
        let mut edit = doc.begin_edit(history_name);
        let output = if let Some(duplicate) = self.duplicate {
            edit.change_props(self.source, |layer| layer.visible = false);
            edit.insert_layer(path, duplicate)
        } else {
            edit.set_mask(self.source, Some(self.mask));
            self.source
        };
        edit.commit();
        if output != self.source {
            doc.active_layer = Some(output);
            doc.selected = vec![output];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayerPath;
    use schist_color::{Depth, Rgba};

    fn document() -> Document {
        let mut doc = Document::new("test", 9, 7, Depth::Eight);
        let layer = Layer::new_raster("source");
        let id = layer.id;
        let mut edit = doc.begin_edit("setup");
        edit.insert_layer(LayerPath(vec![0]), layer);
        let tile = edit.writable_tile(id, TileCoord { tx: 0, ty: 0 }).unwrap();
        tile.set(
            0,
            Rgba {
                r: 0.2,
                g: 0.4,
                b: 0.7,
                a: 0.5,
            },
        );
        edit.commit();
        doc.active_layer = Some(id);
        doc
    }

    #[test]
    fn automatic_mask_preserves_pixels_prior_mask_and_undo() {
        let mut doc = document();
        let id = doc.active_layer.unwrap();
        let mut prior = LayerMask::new_revealing();
        prior.default_value = 128;
        doc.tree.find_mut(id).unwrap().mask = Some(prior);
        let pixel = doc
            .tree
            .find(id)
            .unwrap()
            .as_raster()
            .unwrap()
            .tiles
            .pixel(0, 0);
        let session = Session::capture(&doc).unwrap();
        session
            .prepare(&[0.5; 63])
            .unwrap()
            .apply(&mut doc, "automatic mask")
            .unwrap();
        let layer = doc.tree.find(id).unwrap();
        assert_eq!(layer.mask.as_ref().unwrap().value(0, 0), 64);
        assert_eq!(layer.as_raster().unwrap().tiles.pixel(0, 0), pixel);
        doc.undo();
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .default_value,
            128
        );
        doc.redo();
        assert_eq!(
            doc.tree
                .find(id)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .value(0, 0),
            64
        );
    }

    #[test]
    fn automatic_mask_rejects_stale_and_nonfinite_results() {
        let mut doc = document();
        let session = Session::capture(&doc).unwrap();
        assert!(matches!(
            session.prepare(&[f32::NAN; 63]),
            Err(Error::InvalidMatte)
        ));
        assert!(matches!(
            session.prepare(&[0.5; 62]),
            Err(Error::InvalidMatte)
        ));
        let ready = session.prepare(&[0.5; 63]).unwrap();
        doc.revision += 1;
        assert_eq!(ready.apply(&mut doc, "automatic mask"), Err(Error::Changed));
        assert!(doc
            .tree
            .find(doc.active_layer.unwrap())
            .unwrap()
            .mask
            .is_none());
    }

    #[test]
    fn automatic_mask_checks_locks_and_ignores_hidden_rgb() {
        let mut doc = document();
        let rgb = Session::capture(&doc).unwrap().rgb();
        assert_eq!(&rgb[3..6], &[0.5; 3]);
        doc.tree.find_mut(doc.active_layer.unwrap()).unwrap().locked = true;
        assert!(matches!(Session::capture(&doc), Err(Error::Locked)));
    }

    #[test]
    fn automatic_color_cleanup_keeps_source_transparency_and_undoes_as_one_edit() {
        let mut doc = document();
        let source = doc.active_layer.unwrap();
        let mut edit = doc.begin_edit("opaque edge");
        edit.writable_tile(source, TileCoord { tx: 0, ty: 0 })
            .unwrap()
            .set(1, Rgba::new(0.7, 0.8, 0.9, 1.0));
        edit.commit();
        let original = doc.tree.find(source).unwrap().clone();
        let session = Session::capture(&doc).unwrap();
        for certain in [0.0, 0.0001, 0.9999, 1.0] {
            assert!(session
                .prepare_with_foreground(&[certain; 63], &[0.2; 189], "result")
                .unwrap()
                .duplicate
                .is_none());
        }
        assert!(session
            .prepare_with_foreground(&[0.5; 63], &[f32::NAN; 189], "result")
            .is_err());
        session
            .prepare_with_foreground(&[0.5; 63], &[0.2; 189], "result")
            .unwrap()
            .apply(&mut doc, "remove background")
            .unwrap();
        let output = doc.active_layer.unwrap();
        assert_ne!(output, source);
        assert!(!doc.tree.find(source).unwrap().visible);
        let source_pixels = &doc.tree.find(source).unwrap().as_raster().unwrap().tiles;
        let result = doc.tree.find(output).unwrap();
        let pixels = &result.as_raster().unwrap().tiles;
        assert_eq!(
            pixels.pixel(0, 0),
            original.as_raster().unwrap().tiles.pixel(0, 0)
        );
        assert_eq!(
            source_pixels.pixel(1, 0),
            original.as_raster().unwrap().tiles.pixel(1, 0)
        );
        assert_eq!(pixels.pixel(1, 0), Rgba::new(0.2, 0.2, 0.2, 1.0));
        assert_eq!(result.mask.as_ref().unwrap().value(1, 0), 128);
        doc.undo();
        assert!(doc.tree.find(output).is_none());
        assert!(doc.tree.find(source).unwrap().visible);
        assert!(doc.tree.find(source).unwrap().mask.is_none());
        doc.redo();
        assert!(doc.tree.find(output).is_some());
        assert!(!doc.tree.find(source).unwrap().visible);
    }
}
