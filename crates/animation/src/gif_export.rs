//! Animated GIF: 256-colour palettes, one transparent index, centisecond
//! delays.
//!
//! Colours are reduced exactly when a frame (or, with a global palette,
//! the whole animation) uses few enough of them -- pixel art keeps its
//! exact colours -- and with NeuQuant otherwise, optionally with
//! Floyd-Steinberg dithering. A pixel is transparent below half alpha,
//! since GIF has no partial transparency.

use std::collections::HashMap;

use color_quant::NeuQuant;
use schist_core::animation::LoopCount;

use crate::RenderedFrame;

/// Where the colour table lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteMode {
    /// One table per frame: best colours, slightly larger files.
    #[default]
    PerFrame,
    /// One table for the whole animation: no colour shifts between
    /// frames, smaller files.
    Global,
}

/// What a viewer does with a frame before drawing the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposal {
    /// Restore to background when any frame has transparency, so earlier
    /// frames never show through; otherwise leave the frame in place.
    #[default]
    Auto,
    /// Leave the frame in place (GIF "do not dispose").
    Keep,
    /// Clear the frame's area to transparent.
    Background,
    /// Restore what was there before the frame.
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GifOptions {
    pub palette: PaletteMode,
    pub dither: bool,
    pub disposal: Disposal,
}

impl Default for GifOptions {
    fn default() -> Self {
        GifOptions {
            palette: PaletteMode::PerFrame,
            dither: true,
            disposal: Disposal::Auto,
        }
    }
}

/// The NeuQuant sampling factor: 1 looks at every pixel, 30 at one in
/// thirty. 10 is the library's own recommendation for quality and speed.
const SAMPLE_FACTOR: i32 = 10;
/// Pixels fed to a global palette, at most. Sampled evenly across frames.
const GLOBAL_SAMPLE: usize = 1 << 20;

fn transparent(p: &[u8]) -> bool {
    p[3] < 128
}

/// A colour table and a way to map colours into it.
enum Quantizer {
    Exact(HashMap<[u8; 3], u8>),
    Neu(NeuQuant),
}

struct Palette {
    /// RGB triples, the transparent slot (if any) last.
    rgb: Vec<u8>,
    transparent: Option<u8>,
    quantizer: Quantizer,
}

impl Palette {
    /// Build a table for `pixels` (RGBA8). `needs_transparency` reserves
    /// an index for transparent pixels.
    fn build(pixels: &[u8], needs_transparency: bool) -> Palette {
        let slots = if needs_transparency { 255 } else { 256 };
        let mut unique: Vec<[u8; 3]> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for p in pixels.chunks_exact(4) {
            if transparent(p) {
                continue;
            }
            let c = [p[0], p[1], p[2]];
            if seen.insert(c) {
                unique.push(c);
                if unique.len() > slots {
                    break;
                }
            }
        }
        let (mut rgb, quantizer) = if unique.len() <= slots {
            unique.sort_unstable();
            let map = unique
                .iter()
                .enumerate()
                .map(|(i, c)| (*c, i as u8))
                .collect();
            (unique.concat(), Quantizer::Exact(map))
        } else {
            let opaque: Vec<u8> = pixels
                .chunks_exact(4)
                .filter(|p| !transparent(p))
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
            let nq = NeuQuant::new(SAMPLE_FACTOR, slots, &opaque);
            (nq.color_map_rgb(), Quantizer::Neu(nq))
        };
        let transparent = needs_transparency.then(|| {
            let index = (rgb.len() / 3) as u8;
            rgb.extend_from_slice(&[0, 0, 0]);
            index
        });
        Palette {
            rgb,
            transparent,
            quantizer,
        }
    }

    fn index(&self, c: [u8; 3]) -> u8 {
        match &self.quantizer {
            Quantizer::Exact(map) => match map.get(&c) {
                Some(&i) => i,
                // Only reachable with a global table built from a sample
                // or a dithered colour: the nearest entry.
                None => self.nearest(c),
            },
            Quantizer::Neu(nq) => nq.index_of(&[c[0], c[1], c[2], 255]) as u8,
        }
    }

    fn nearest(&self, c: [u8; 3]) -> u8 {
        let entries = self.rgb.len() / 3 - usize::from(self.transparent.is_some());
        (0..entries)
            .min_by_key(|&i| {
                let e = &self.rgb[i * 3..i * 3 + 3];
                (0..3)
                    .map(|k| (e[k] as i32 - c[k] as i32).pow(2))
                    .sum::<i32>()
            })
            .unwrap_or(0) as u8
    }

    fn is_exact(&self) -> bool {
        matches!(self.quantizer, Quantizer::Exact(_))
    }

    /// Map a frame to indices, dithering when asked and when the table is
    /// an approximation.
    fn map(&self, rgba: &[u8], width: usize, dither: bool) -> Vec<u8> {
        let n = rgba.len() / 4;
        let mut out = vec![0u8; n];
        if !dither || self.is_exact() {
            for (i, p) in rgba.chunks_exact(4).enumerate() {
                out[i] = match self.transparent.filter(|_| transparent(p)) {
                    Some(t) => t,
                    None => self.index([p[0], p[1], p[2]]),
                };
            }
            return out;
        }
        // Floyd-Steinberg, carrying error on two rows.
        let mut err = vec![[0f32; 3]; width * 2 + 2];
        let height = n / width.max(1);
        for y in 0..height {
            let (cur, next) = err.split_at_mut(width + 1);
            next.iter_mut().for_each(|e| *e = [0.0; 3]);
            for x in 0..width {
                let i = y * width + x;
                let p = &rgba[i * 4..i * 4 + 4];
                if let Some(t) = self.transparent.filter(|_| transparent(p)) {
                    out[i] = t;
                    continue;
                }
                let want = [0, 1, 2].map(|k| (p[k] as f32 + cur[x][k]).clamp(0.0, 255.0));
                let index = self.index(want.map(|v| v.round() as u8));
                out[i] = index;
                let got = &self.rgb[index as usize * 3..index as usize * 3 + 3];
                for k in 0..3 {
                    let e = want[k] - got[k] as f32;
                    cur[x + 1][k] += e * 7.0 / 16.0;
                    if x > 0 {
                        next[x - 1][k] += e * 3.0 / 16.0;
                    }
                    next[x][k] += e * 5.0 / 16.0;
                    next[x + 1][k] += e / 16.0;
                }
            }
            let (cur, next) = err.split_at_mut(width + 1);
            cur.copy_from_slice(&next[..width + 1]);
        }
        out
    }
}

/// GIF delays are centiseconds. Browsers replace anything under two with
/// a tenth of a second, so shorter delays are raised to two rather than
/// written as a value that plays five times slower than asked.
fn centiseconds(ms: u32) -> u16 {
    ((ms + 5) / 10).clamp(2, u16::MAX as u32) as u16
}

/// Encode an animated GIF.
pub fn encode_gif(
    frames: &[RenderedFrame],
    width: u32,
    height: u32,
    loop_count: LoopCount,
    options: &GifOptions,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        width <= u16::MAX as u32 && height <= u16::MAX as u32,
        "GIF is limited to 65535 pixels a side"
    );
    let has_transparency = frames
        .iter()
        .any(|f| f.rgba.chunks_exact(4).any(transparent));
    let dispose = match options.disposal {
        Disposal::Auto if has_transparency => gif::DisposalMethod::Background,
        Disposal::Auto | Disposal::Keep => gif::DisposalMethod::Keep,
        Disposal::Background => gif::DisposalMethod::Background,
        Disposal::Previous => gif::DisposalMethod::Previous,
    };
    let global = (options.palette == PaletteMode::Global).then(|| {
        let total: usize = frames.iter().map(|f| f.rgba.len() / 4).sum();
        let step = total.div_ceil(GLOBAL_SAMPLE).max(1);
        let sample: Vec<u8> = frames
            .iter()
            .flat_map(|f| f.rgba.chunks_exact(4).step_by(step))
            .flatten()
            .copied()
            .collect();
        Palette::build(&sample, has_transparency)
    });
    let mut out = Vec::new();
    {
        let table = global.as_ref().map_or(&[][..], |p| &p.rgb[..]);
        let mut encoder = gif::Encoder::new(&mut out, width as u16, height as u16, table)?;
        // NETSCAPE2.0 counts repeats after the first play; no block at
        // all is how a GIF says "once".
        match loop_count.plays() {
            None => encoder.set_repeat(gif::Repeat::Infinite)?,
            Some(1) => {}
            Some(n) => encoder.set_repeat(gif::Repeat::Finite((n - 1).min(u16::MAX as u32) as u16))?,
        }
        for frame in frames {
            anyhow::ensure!(
                frame.rgba.len() == (width * height * 4) as usize,
                "frame size does not match the animation"
            );
            let local;
            let palette = match &global {
                Some(p) => p,
                None => {
                    local = Palette::build(&frame.rgba, has_transparency);
                    &local
                }
            };
            let indices = palette.map(&frame.rgba, width as usize, options.dither);
            encoder.write_frame(&gif::Frame {
                delay: centiseconds(frame.delay_ms),
                dispose,
                transparent: palette.transparent,
                width: width as u16,
                height: height as u16,
                palette: global.is_none().then(|| palette.rgb.clone()),
                buffer: indices.into(),
                ..gif::Frame::default()
            })?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_round_to_centiseconds_with_the_browser_floor() {
        assert_eq!(centiseconds(100), 10);
        assert_eq!(centiseconds(104), 10);
        assert_eq!(centiseconds(105), 11);
        assert_eq!(centiseconds(0), 2);
        assert_eq!(centiseconds(u32::MAX / 2), u16::MAX);
    }

    #[test]
    fn exact_palettes_keep_every_colour() {
        let rgba = [10, 20, 30, 255, 200, 100, 0, 255, 0, 0, 0, 0];
        let p = Palette::build(&rgba, true);
        assert!(p.is_exact());
        assert_eq!(p.transparent, Some(2));
        let mapped = p.map(&rgba, 3, true);
        assert_eq!(mapped[2], 2);
        for i in 0..2 {
            let e = &p.rgb[mapped[i] as usize * 3..][..3];
            assert_eq!(e, &rgba[i * 4..i * 4 + 3]);
        }
    }

    #[test]
    fn many_colours_quantize_and_dither_within_the_table() {
        let (w, h) = (64usize, 64usize);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 256) as u8, (i / 16) as u8, (i * 7 % 256) as u8, 255])
            .collect();
        let p = Palette::build(&rgba, false);
        assert!(!p.is_exact());
        assert_eq!(p.rgb.len(), 256 * 3);
        assert!(p.transparent.is_none());
        let mapped = p.map(&rgba, w, true);
        assert_eq!(mapped.len(), w * h);
    }
}
