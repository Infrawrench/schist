//! Editable mask gradients multiply a retained painted mask. Raster samples
//! remain the compositor contract; this recipe makes the controls persistent.
use crate::{IntRect, Layer, LayerMask, RawBlock, TileCoord, TILE_PIXELS, TILE_SIZE};
use serde::{Deserialize, Serialize};
pub const BLOCK: [u8; 4] = *b"scMk";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaskSnapshot {
    pub bounds: IntRect,
    pub default_value: u8,
    pub enabled: bool,
    pub linked: bool,
    // Compressed individually to retain sparse masks and bounded allocation.
    tiles: Vec<(i32, i32, Vec<u8>)>,
}
impl MaskSnapshot {
    pub fn capture(mask: &LayerMask) -> Self {
        Self {
            bounds: mask.bounds,
            default_value: mask.default_value,
            enabled: mask.enabled,
            linked: mask.linked,
            tiles: mask
                .tiles
                .iter()
                .map(|(c, t)| {
                    (
                        c.tx,
                        c.ty,
                        miniz_oxide::deflate::compress_to_vec_zlib(t.as_ref(), 1),
                    )
                })
                .collect(),
        }
    }
    pub fn restore(&self) -> Option<LayerMask> {
        if self.tiles.len() > 4096 {
            return None;
        }
        let mut mask = LayerMask::new_revealing();
        mask.bounds = self.bounds;
        mask.default_value = self.default_value;
        mask.enabled = self.enabled;
        mask.linked = self.linked;
        for (x, y, data) in &self.tiles {
            if x.abs_diff(0) >= (i32::MAX / TILE_SIZE - 1) as u32
                || y.abs_diff(0) >= (i32::MAX / TILE_SIZE - 1) as u32
            {
                return None;
            }
            let bytes =
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, TILE_PIXELS).ok()?;
            if bytes.len() != TILE_PIXELS {
                return None;
            }
            mask.tiles
                .get_mut_or_insert(TileCoord { tx: *x, ty: *y })
                .copy_from_slice(&bytes);
        }
        Some(mask)
    }
    pub fn translate(&mut self, dx: i32, dy: i32) {
        if let Some(mut mask) = self.restore() {
            mask.tiles = mask.tiles.translated(dx, dy);
            mask.bounds = mask.bounds.translated(dx, dy);
            *self = Self::capture(&mask);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskGradient {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub radial: bool,
    pub reverse: bool,
    pub opacity: f32,
}
impl MaskGradient {
    pub fn value(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let length = dx.hypot(dy).max(1e-6);
        let (x, y) = (x - self.from.0, y - self.from.1);
        let t = if self.radial {
            x.hypot(y) / length
        } else {
            (x * dx + y * dy) / (length * length)
        }
        .clamp(0.0, 1.0);
        let t = if self.reverse { 1.0 - t } else { t };
        1.0 - self.opacity + self.opacity * t
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveMask {
    pub base: MaskSnapshot,
    pub gradients: Vec<MaskGradient>,
}
impl LiveMask {
    pub fn from_layer(layer: &Layer) -> Option<Self> {
        let b = layer.extras.iter().find(|b| b.key == BLOCK)?;
        let mask: Self = serde_json::from_slice(&b.data).ok()?;
        if mask.gradients.len() > 32
            || !mask.gradients.iter().all(|g| {
                [g.from.0, g.from.1, g.to.0, g.to.1, g.opacity]
                    .iter()
                    .all(|v| v.is_finite())
                    && (0.0..=1.0).contains(&g.opacity)
            })
        {
            return None;
        }
        Some(mask)
    }
    pub fn blocks(&self, layer: &Layer) -> Vec<RawBlock> {
        let mut blocks = layer.extras.clone();
        blocks.retain(|b| b.key != BLOCK);
        if !self.gradients.is_empty() {
            blocks.push(RawBlock {
                key: BLOCK,
                data: serde_json::to_vec(self).expect("finite mask controls"),
            });
        }
        blocks
    }
    pub fn render(&self, canvas: IntRect) -> Option<LayerMask> {
        let base = self.base.restore()?;
        Some(self.render_with_base(&base, canvas))
    }
    /// Reuse the in-memory painted mask during a gesture; compress once when
    /// committing, rather than compressing and inflating every pointer move.
    pub fn render_with_base(&self, base: &LayerMask, canvas: IntRect) -> LayerMask {
        if self.gradients.is_empty() {
            return base.clone();
        }
        let mut mask = base.clone();
        mask.bounds = canvas;
        for c in TileCoord::covering(&canvas) {
            let rect = c.rect();
            let clip = rect.intersect(&canvas);
            let tile = mask.tiles.get_mut_or_insert(c);
            for y in clip.top..clip.bottom {
                for x in clip.left..clip.right {
                    let value = self.gradients.iter().fold(base.value(x, y) as f32, |v, g| {
                        v * g.value(x as f32 + 0.5, y as f32 + 0.5)
                    });
                    tile[((y - rect.top) * TILE_SIZE + x - rect.left) as usize] =
                        value.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        mask
    }
    pub fn translate(&mut self, dx: i32, dy: i32) {
        self.base.translate(dx, dy);
        for g in &mut self.gradients {
            g.from.0 += dx as f32;
            g.from.1 += dy as f32;
            g.to.0 += dx as f32;
            g.to.1 += dy as f32;
        }
    }
}
