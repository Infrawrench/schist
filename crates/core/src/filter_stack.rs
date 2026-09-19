//! Schist-owned editable filters. Raster tiles hold the visible result; these
//! private layer blocks retain an immutable, lossless source and ordered recipe.
//! Unknown PSD readers display the raster and preserve/ignore these blocks.
use crate::{
    IntRect, Layer, NativeSamples, NativeTile, RawBlock, TileBuf, TileCoord, TileMap, TILE_PIXELS,
};
use anyhow::{bail, ensure, Context, Result};
use schist_color::{ColorMode, Depth};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

pub const STACK_KEY: [u8; 4] = *b"ScFs";
pub const SOURCE_KEY: [u8; 4] = *b"ScFo";
const MAX_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_EFFECTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterEffect {
    pub id: String,
    pub enabled: bool,
    pub values: BTreeMap<String, f32>,
    pub foreground: [f32; 4],
    pub background: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterStack {
    pub version: u32,
    pub region: IntRect,
    pub effects: Vec<FilterEffect>,
}

pub fn has_stack(layer: &Layer) -> bool {
    layer
        .extras
        .iter()
        .any(|b| b.key == STACK_KEY || b.key == SOURCE_KEY)
}

pub fn without_stack(extras: &[RawBlock]) -> Vec<RawBlock> {
    extras
        .iter()
        .filter(|b| b.key != STACK_KEY && b.key != SOURCE_KEY)
        .cloned()
        .collect()
}

impl FilterStack {
    pub fn new(region: IntRect) -> Self {
        Self {
            version: 1,
            region,
            effects: Vec::new(),
        }
    }

    pub fn read(layer: &Layer) -> Result<Option<Self>> {
        let Some(block) = layer.extras.iter().find(|b| b.key == STACK_KEY) else {
            return Ok(None);
        };
        ensure!(
            block.data.len() <= 1024 * 1024,
            "Filter stack metadata too large"
        );
        let stack: Self = serde_json::from_slice(&block.data)?;
        stack.validate()?;
        Ok(Some(stack))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported filter stack version");
        ensure!(
            !self.region.is_empty()
                && self.region.width() as u64 * self.region.height() as u64 <= 32_000_000,
            "Invalid filter stack region"
        );
        let safe = i32::MAX - crate::TILE_SIZE * 2;
        ensure!(
            [
                self.region.left,
                self.region.top,
                self.region.right,
                self.region.bottom
            ]
            .iter()
            .all(|v| v.abs_diff(0) < safe as u32),
            "Filter region outside safe tile coordinates"
        );
        let first = TileCoord::containing(self.region.left, self.region.top);
        let last = TileCoord::containing(self.region.right - 1, self.region.bottom - 1);
        let covered_tiles = (last.tx as i64 - first.tx as i64 + 1) as u64
            * (last.ty as i64 - first.ty as i64 + 1) as u64;
        ensure!(
            covered_tiles <= (MAX_BYTES / (TILE_PIXELS * 5 * 4)) as u64,
            "Filter region would allocate too many tiles"
        );
        ensure!(self.effects.len() <= MAX_EFFECTS, "Too many filter effects");
        for effect in &self.effects {
            ensure!(
                !effect.id.is_empty() && effect.id.len() <= 256 && effect.values.len() <= 256,
                "Invalid filter effect"
            );
            ensure!(
                effect
                    .values
                    .values()
                    .chain(effect.foreground.iter())
                    .chain(effect.background.iter())
                    .all(|v| v.is_finite()),
                "Invalid filter parameter"
            );
        }
        Ok(())
    }

    /// Reuses the compressed source byte-for-byte during parameter edits.
    pub fn blocks(&self, layer: &Layer, source: &TileMap) -> Result<Vec<RawBlock>> {
        self.validate()?;
        let mut extras = without_stack(&layer.extras);
        extras.push(RawBlock {
            key: STACK_KEY,
            data: serde_json::to_vec(self)?,
        });
        extras.push(match layer.extras.iter().find(|b| b.key == SOURCE_KEY) {
            Some(block) => block.clone(),
            None => RawBlock {
                key: SOURCE_KEY,
                data: encode_source(source)?,
            },
        });
        Ok(extras)
    }
}

fn mode_code(mode: ColorMode) -> u8 {
    match mode {
        ColorMode::Cmyk => 4,
        ColorMode::Lab => 9,
        _ => 3,
    }
}
fn read_mode(code: u8) -> Result<ColorMode> {
    match code {
        3 => Ok(ColorMode::Rgb),
        4 => Ok(ColorMode::Cmyk),
        9 => Ok(ColorMode::Lab),
        _ => bail!("Invalid source color mode"),
    }
}

/// Sparse native tiles, including transparent pixel colors and exact float bits.
/// Compression is bounded both before encoding and while decoding untrusted files.
pub fn encode_source(source: &TileMap) -> Result<Vec<u8>> {
    let size = source
        .iter()
        .try_fold(9usize, |n, (_, tile)| n.checked_add(10 + tile.byte_len()))
        .context("Source size overflow")?;
    ensure!(size <= MAX_BYTES, "Filter stack source too large");
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(b"SFS1");
    bytes.push(mode_code(source.mode()));
    bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
    let mut tiles: Vec<_> = source.iter().collect();
    tiles.sort_by_key(|(coord, _)| **coord);
    for (coord, tile) in tiles {
        bytes.extend_from_slice(&coord.tx.to_le_bytes());
        bytes.extend_from_slice(&coord.ty.to_le_bytes());
        bytes.push(mode_code(tile.mode()));
        bytes.push(tile.depth().bytes_per_channel() as u8);
        match tile.as_ref() {
            TileBuf::U8(v) => bytes.extend_from_slice(v),
            TileBuf::U16(v) => {
                for x in v {
                    bytes.extend_from_slice(&x.to_le_bytes());
                }
            }
            TileBuf::F32(v) => {
                for x in v {
                    bytes.extend_from_slice(&x.to_bits().to_le_bytes());
                }
            }
            TileBuf::Native(v) => match &v.samples {
                NativeSamples::U8(v) => bytes.extend_from_slice(v),
                NativeSamples::U16(v) => {
                    for x in v {
                        bytes.extend_from_slice(&x.to_le_bytes());
                    }
                }
                NativeSamples::F32(v) => {
                    for x in v {
                        bytes.extend_from_slice(&x.to_bits().to_le_bytes());
                    }
                }
            },
        }
    }
    Ok(miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 6))
}

pub fn read_source(layer: &Layer) -> Result<TileMap> {
    let block = layer
        .extras
        .iter()
        .find(|b| b.key == SOURCE_KEY)
        .context("Missing filter stack source")?;
    decode_source(&block.data)
}

pub fn decode_source(data: &[u8]) -> Result<TileMap> {
    let bytes = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, MAX_BYTES)
        .map_err(|_| anyhow::anyhow!("Invalid compressed filter source"))?;
    let mut input = bytes.as_slice();
    fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
        ensure!(input.len() >= n, "Truncated filter source");
        let (head, tail) = input.split_at(n);
        *input = tail;
        Ok(head)
    }
    ensure!(
        take(&mut input, 4)? == b"SFS1",
        "Unsupported filter source version"
    );
    let mode = read_mode(take(&mut input, 1)?[0])?;
    let count = u32::from_le_bytes(take(&mut input, 4)?.try_into()?) as usize;
    ensure!(
        count <= MAX_BYTES / (TILE_PIXELS * 4),
        "Too many source tiles"
    );
    let mut tiles = TileMap::new_in_mode(mode);
    for _ in 0..count {
        let tx = i32::from_le_bytes(take(&mut input, 4)?.try_into()?);
        let ty = i32::from_le_bytes(take(&mut input, 4)?.try_into()?);
        ensure!(
            tx.abs_diff(0) < (i32::MAX / crate::TILE_SIZE - 1) as u32
                && ty.abs_diff(0) < (i32::MAX / crate::TILE_SIZE - 1) as u32,
            "Invalid source tile position"
        );
        let tile_mode = read_mode(take(&mut input, 1)?[0])?;
        ensure!(tile_mode == mode, "Mixed source tile color modes");
        let depth = match take(&mut input, 1)?[0] {
            1 => Depth::Eight,
            2 => Depth::Sixteen,
            4 => Depth::ThirtyTwo,
            _ => bail!("Invalid source depth"),
        };
        let samples = TILE_PIXELS * (mode.channels() + 1);
        let data = take(&mut input, samples * depth.bytes_per_channel())?;
        let tile = match depth {
            Depth::Eight => TileBuf::U8(data.to_vec().into_boxed_slice()),
            Depth::Sixteen => TileBuf::U16(
                data.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect(),
            ),
            Depth::ThirtyTwo => TileBuf::F32(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_bits(u32::from_le_bytes(*b)))
                    .collect(),
            ),
        };
        let tile = if mode == ColorMode::Rgb {
            tile
        } else {
            let samples = match tile {
                TileBuf::U8(v) => NativeSamples::U8(v),
                TileBuf::U16(v) => NativeSamples::U16(v),
                TileBuf::F32(v) => NativeSamples::F32(v),
                _ => unreachable!(),
            };
            TileBuf::Native(NativeTile { mode, samples })
        };
        ensure!(
            tiles.get(TileCoord { tx, ty }).is_none(),
            "Duplicate source tile"
        );
        tiles.insert(TileCoord { tx, ty }, Arc::new(tile));
    }
    ensure!(input.is_empty(), "Trailing source data");
    Ok(tiles)
}
