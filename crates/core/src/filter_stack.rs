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
/// Lossless filtered pixels in source coordinates, before raster placement.
pub const CACHE_KEY: [u8; 4] = *b"ScFc";
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
    /// Raster-only placement. Smart objects keep placement on their own payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<FilterPlacement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterPlacement {
    pub matrix: crate::Affine,
    pub filter: crate::Filter,
}

pub fn has_stack(layer: &Layer) -> bool {
    layer
        .extras
        .iter()
        .any(|b| b.key == STACK_KEY || b.key == SOURCE_KEY || b.key == CACHE_KEY)
}

pub fn without_stack(extras: &[RawBlock]) -> Vec<RawBlock> {
    extras
        .iter()
        .filter(|b| b.key != STACK_KEY && b.key != SOURCE_KEY && b.key != CACHE_KEY)
        .cloned()
        .collect()
}

impl FilterStack {
    pub fn new(region: IntRect) -> Self {
        Self {
            version: 1,
            region,
            effects: Vec::new(),
            placement: None,
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
        ensure!(
            matches!(self.version, 1 | 2),
            "Unsupported filter stack version"
        );
        if let Some(placement) = &self.placement {
            ensure!(
                self.version == 2,
                "Placement requires filter stack version 2"
            );
            validate_matrix(&placement.matrix)?;
            validate_bounds(&placement.matrix, self.region)?;
            validate_render_extent(placement.matrix.transform_bounds(self.region))?;
        }
        ensure!(
            !self.region.is_empty()
                && (self.region.right as i64 - self.region.left as i64) as u64
                    * (self.region.bottom as i64 - self.region.top as i64) as u64
                    <= 32_000_000,
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
        if let Some(cache) = layer.extras.iter().find(|b| b.key == CACHE_KEY) {
            extras.push(cache.clone());
        }
        Ok(extras)
    }

    /// Parameter changes replace the derived cache while keeping pristine source bytes.
    pub fn blocks_with_render(
        &self,
        layer: &Layer,
        source: &TileMap,
        filtered: &TileMap,
    ) -> Result<Vec<RawBlock>> {
        let mut extras = self.blocks(layer, source)?;
        extras.retain(|b| b.key != CACHE_KEY);
        if self.placement.is_some() {
            extras.push(RawBlock {
                key: CACHE_KEY,
                data: encode_source(filtered)?,
            });
        }
        Ok(extras)
    }

    pub fn place(&self, filtered: &TileMap, depth: Depth, clip: IntRect) -> TileMap {
        match &self.placement {
            Some(p) => render_placement(filtered, &p.matrix, depth, p.filter, clip),
            None => filtered.clone(),
        }
    }
}

fn validate_matrix(matrix: &crate::Affine) -> Result<()> {
    ensure!(
        [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
            .iter()
            .all(|v| v.is_finite() && v.abs() < (i32::MAX / 4) as f32)
            && matrix.determinant().is_finite()
            && matrix.invert().is_some(),
        "Invalid filter placement"
    );
    Ok(())
}

fn validate_bounds(matrix: &crate::Affine, bounds: IntRect) -> Result<()> {
    if bounds.is_empty() {
        return Ok(());
    }
    for (x, y) in [
        (bounds.left, bounds.top),
        (bounds.right, bounds.top),
        (bounds.left, bounds.bottom),
        (bounds.right, bounds.bottom),
    ] {
        let (x, y) = matrix.apply(x as f32, y as f32);
        ensure!(
            x.is_finite()
                && y.is_finite()
                && x.abs() < (i32::MAX / 4) as f32
                && y.abs() < (i32::MAX / 4) as f32,
            "Filter placement outside safe coordinates"
        );
    }
    Ok(())
}

fn validate_render_extent(bounds: IntRect) -> Result<()> {
    if bounds.is_empty() {
        return Ok(());
    }
    let first = TileCoord::containing(bounds.left, bounds.top);
    let last = TileCoord::containing(bounds.right - 1, bounds.bottom - 1);
    let tiles = (last.tx as i64 - first.tx as i64 + 1) as u64
        * (last.ty as i64 - first.ty as i64 + 1) as u64;
    ensure!(
        bounds.width() as u64 * bounds.height() as u64 <= 32_000_000
            && tiles <= (MAX_BYTES / (TILE_PIXELS * 5 * 4)) as u64,
        "Filter placement would allocate too many pixels"
    );
    Ok(())
}

/// Whether placement can be rendered without interpolation or color conversion.
pub fn is_integer_translation(matrix: &crate::Affine) -> bool {
    matrix.a == 1.0
        && matrix.b == 0.0
        && matrix.c == 0.0
        && matrix.d == 1.0
        && matrix.tx.fract() == 0.0
        && matrix.ty.fract() == 0.0
        && matrix.tx.abs() < (i32::MAX / 2) as f32
        && matrix.ty.abs() < (i32::MAX / 2) as f32
}

/// Preserve native samples and hidden colors exactly for integer translations.
pub fn render_placement(
    source: &TileMap,
    matrix: &crate::Affine,
    depth: Depth,
    filter: crate::Filter,
    clip: IntRect,
) -> TileMap {
    if is_integer_translation(matrix) {
        return source.translated(matrix.tx as i32, matrix.ty as i32, depth);
    }
    crate::resample::transform_tiles(source, matrix, depth, filter, clip)
}

/// Fully prepared immutable transform input. Preparing can fail on malformed
/// metadata, so callers prepare before changing pixels or recording history.
#[derive(Clone)]
pub struct LayerTransform {
    pub source: TileMap,
    pub matrix: crate::Affine,
    pub filter: crate::Filter,
    pub extras: Vec<RawBlock>,
    pub smart: Option<Box<crate::SmartObject>>,
}

impl LayerTransform {
    pub fn prepare(layer: &Layer, matrix: &crate::Affine, filter: crate::Filter) -> Result<Self> {
        validate_matrix(matrix)?;
        let raster = layer.as_raster().context("Transform needs a pixel layer")?;
        if let Some(mut smart) = layer.smart.clone() {
            smart.apply(matrix);
            smart.filter = filter;
            validate_matrix(&smart.transform)?;
            validate_bounds(&smart.transform, smart.source.tile_bounds())?;
            if has_stack(layer) {
                validate_render_extent(smart.transform.transform_bounds(smart.source_bounds))?;
            }
            return Ok(Self {
                source: smart.source.clone(),
                matrix: smart.transform,
                filter,
                extras: layer.extras.clone(),
                smart: Some(smart),
            });
        }
        let Some(mut stack) = FilterStack::read(layer)? else {
            ensure!(!has_stack(layer), "Incomplete filter stack");
            return Ok(Self {
                source: raster.tiles.clone(),
                matrix: *matrix,
                filter,
                extras: layer.extras.clone(),
                smart: None,
            });
        };
        let (source, composed) = if let Some(placement) = &stack.placement {
            let cache = layer
                .extras
                .iter()
                .find(|b| b.key == CACHE_KEY)
                .context("Missing filter stack render cache")?;
            (decode_source(&cache.data)?, matrix.then(&placement.matrix))
        } else {
            (raster.tiles.clone(), *matrix)
        };
        validate_matrix(&composed)?;
        validate_bounds(&composed, source.tile_bounds())?;
        validate_render_extent(composed.transform_bounds(source.content_bounds()))?;
        stack.version = 2;
        stack.placement = Some(FilterPlacement {
            matrix: composed,
            filter,
        });
        let original = read_source(layer)?;
        let extras = if layer.extras.iter().any(|b| b.key == CACHE_KEY) {
            stack.blocks(layer, &original)?
        } else {
            stack.blocks_with_render(layer, &original, &source)?
        };
        Ok(Self {
            source,
            matrix: composed,
            filter,
            extras,
            smart: None,
        })
    }

    /// Reuse a gesture's decoded source and compressed blocks for every preview.
    /// Only the small placement recipe changes while dragging.
    pub fn then(&self, matrix: &crate::Affine, filter: crate::Filter) -> Result<Self> {
        validate_matrix(matrix)?;
        let mut next = self.clone();
        next.matrix = matrix.then(&self.matrix);
        next.filter = filter;
        validate_matrix(&next.matrix)?;
        validate_bounds(&next.matrix, self.source.tile_bounds())?;
        if let Some(smart) = next.smart.as_mut() {
            smart.transform = next.matrix;
            smart.filter = filter;
        } else if let Some(block) = next.extras.iter_mut().find(|b| b.key == STACK_KEY) {
            let mut stack: FilterStack = serde_json::from_slice(&block.data)?;
            stack.version = 2;
            stack.placement = Some(FilterPlacement {
                matrix: next.matrix,
                filter,
            });
            stack.validate()?;
            block.data = serde_json::to_vec(&stack)?;
        }
        if next.extras.iter().any(|b| b.key == STACK_KEY) {
            validate_render_extent(next.matrix.transform_bounds(next.source.content_bounds()))?;
        }
        Ok(next)
    }

    pub fn render(&self, depth: Depth, clip: IntRect) -> TileMap {
        render_placement(&self.source, &self.matrix, depth, self.filter, clip)
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
