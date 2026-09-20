//! Document-wide extra separations. Spot ink is scalar coverage, independent
//! of layer colour and transparency; preserved alpha channels share the storage.
use crate::{IntRect, TileCoord, TILE_PIXELS, TILE_SIZE};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct InkTiles(pub FxHashMap<TileCoord, Arc<Vec<f32>>>);

impl InkTiles {
    pub fn value(&self, x: i32, y: i32) -> f32 {
        self.0.get(&TileCoord::containing(x, y)).map_or(0.0, |t| {
            t[y.rem_euclid(TILE_SIZE) as usize * TILE_SIZE as usize
                + x.rem_euclid(TILE_SIZE) as usize]
        })
    }
    pub fn tile_mut(&mut self, coord: TileCoord) -> &mut Vec<f32> {
        Arc::make_mut(
            self.0
                .entry(coord)
                .or_insert_with(|| Arc::new(vec![0.0; TILE_PIXELS])),
        )
    }
    pub fn set(&mut self, x: i32, y: i32, value: f32) {
        let coord = TileCoord::containing(x, y);
        if value == 0.0 && !self.0.contains_key(&coord) {
            return;
        }
        self.tile_mut(coord)[y.rem_euclid(TILE_SIZE) as usize * TILE_SIZE as usize
            + x.rem_euclid(TILE_SIZE) as usize] = value;
    }
    pub fn remap(
        &self,
        rect: IntRect,
        source: impl Fn(i32, i32) -> (f32, f32),
        interpolate: bool,
    ) -> Self {
        let mut out = Self::default();
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let (sx, sy) = source(x, y);
                let v = if interpolate {
                    let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
                    let (fx, fy) = (sx - sx.floor(), sy - sy.floor());
                    let a = self.value(ix, iy) * (1.0 - fx) + self.value(ix + 1, iy) * fx;
                    let b = self.value(ix, iy + 1) * (1.0 - fx) + self.value(ix + 1, iy + 1) * fx;
                    a * (1.0 - fy) + b * fy
                } else {
                    self.value(sx.round() as i32, sy.round() as i32)
                };
                out.set(x, y, v);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InkChannelInfo {
    pub id: u32,
    pub name: String,
    pub spot: bool,
    /// Unmanaged sRGB display approximation, independent of process ICC data.
    pub color: [f32; 3],
    /// Display opacity (0..1); never changes plate coverage.
    pub solidity: f32,
    pub visible: bool,
    /// Original 13-byte PSD DisplayInfo entry, including unsupported colour
    /// spaces/modes. Cleared only when display colour/solidity is edited.
    pub original_display: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct InkChannel {
    pub info: InkChannelInfo,
    pub pixels: InkTiles,
}

impl InkChannel {
    pub fn spot(name: String, color: [f32; 3]) -> Self {
        let mut bytes = [0; 4];
        getrandom::fill(&mut bytes).expect("operating system entropy unavailable");
        Self {
            info: InkChannelInfo {
                id: u32::from_le_bytes(bytes).max(1),
                name,
                spot: true,
                color: color.map(|v| {
                    if v.is_finite() {
                        v.clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }),
                solidity: 0.0,
                visible: true,
                original_display: None,
            },
            pixels: InkTiles::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InkPreview {
    #[default]
    Process,
    Overprint,
    Separation(u32),
}

/// Apply a display-only ink simulation to already colour-managed RGBA8.
/// Transparent process pixels are shown against white paper. Multiplicative
/// ink absorption at zero solidity blends toward opaque ink at full solidity.
pub fn preview_rgba8(channels: &[InkChannel], preview: InkPreview, rect: IntRect, rgba: &mut [u8]) {
    if preview == InkPreview::Process {
        return;
    }
    for (i, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        let x = rect.left + (i % rect.width() as usize) as i32;
        let y = rect.top + (i / rect.width() as usize) as i32;
        if let InkPreview::Separation(id) = preview {
            let coverage = channels
                .iter()
                .find(|c| c.info.id == id)
                .map_or(0.0, |c| c.pixels.value(x, y));
            let value = ((1.0 - coverage.clamp(0.0, 1.0)) * 255.0).round() as u8;
            pixel.copy_from_slice(&[value, value, value, 255]);
            continue;
        }
        let alpha = pixel[3] as f32 / 255.0;
        let mut rgb = [0.0; 3];
        for c in 0..3 {
            rgb[c] = pixel[c] as f32 / 255.0 * alpha + 1.0 - alpha;
        }
        for channel in channels.iter().filter(|c| c.info.spot && c.info.visible) {
            let a = channel.pixels.value(x, y).clamp(0.0, 1.0);
            let solidity = channel.info.solidity.clamp(0.0, 1.0);
            for (c, value) in rgb.iter_mut().enumerate() {
                let ink = channel.info.color[c].clamp(0.0, 1.0);
                let overprint = *value * ink;
                *value += ((overprint * (1.0 - solidity) + ink * solidity) - *value) * a;
            }
        }
        for c in 0..3 {
            pixel[c] = (rgb[c].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        pixel[3] = 255;
    }
}
