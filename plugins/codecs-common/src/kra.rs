//! Krita (`.kra`, `.krz`) import.
//!
//! Written from Krita's published file behaviour (its GPL source and the
//! user manual's format page) and from packages built by these tests; no
//! Krita code is used. A `.kra` is a zip with a stored `mimetype`
//! (`application/x-krita`) first, `maindoc.xml` describing the layer tree
//! top to bottom, `mergedimage.png` (absent in `.krz`), `preview.png`, and
//! per-node pixel data under `<image name>/layers/`.
//!
//! Paint device data is Krita's "VERSION 2" tile stream: a short text
//! header, then per 64×64 tile a `x,y,LZF,size` line and `size` bytes whose
//! first byte says whether the rest is LZF-compressed (1) or raw (0).
//! Compressed tiles are stored planar — every pixel's first byte, then
//! every pixel's second byte — and raw tiles interleaved.
use anyhow::{ensure, Result};
use schist_codec_idml::container::Package;
use schist_codec_idml::xml::Element;
use schist_color::Depth;
use schist_core::{
    blit_rgba_f32, BlendMode, Document, IntRect, Layer, LayerId, LayerKind, LayerMask, TileCoord,
};
use schist_i18n::{t, tf};
use schist_plugin_api::CodecPlugin;

use crate::archive;
use crate::layered::{self, invalid, MAX_BYTES, MAX_LAYERS};

mod lzf;

pub const MIMETYPE: &str = "application/x-krita";
const TILE: usize = 64;

pub struct KraCodec;

impl CodecPlugin for KraCodec {
    fn id(&self) -> &'static str {
        "codec.kra"
    }
    fn name(&self) -> &'static str {
        t("codec.kra.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["kra", "krz"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        archive::stored_mimetype(bytes) == Some(MIMETYPE.as_bytes())
    }
    fn import(&self, bytes: &[u8]) -> Result<Document> {
        read(bytes)
    }
}

/// The flattened render a package carries, for previews: the full-size
/// merged image when there is one, else Krita's small preview.
pub fn merged_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let package = archive::open(bytes).ok()?;
    package
        .get("mergedimage.png")
        .or_else(|| package.get("preview.png"))
        .map(<[u8]>::to_vec)
}

/// A Krita composite op id as a Schist blend mode.
pub fn blend(id: &str) -> Option<BlendMode> {
    use BlendMode::*;
    Some(match id {
        "normal" => Normal,
        "dissolve" => Dissolve,
        "darken" => Darken,
        "multiply" => Multiply,
        "burn" => ColorBurn,
        "linear_burn" => LinearBurn,
        "darker color" => DarkerColor,
        "lighten" => Lighten,
        "screen" => Screen,
        "dodge" => ColorDodge,
        "linear_dodge" | "add" => LinearDodge,
        "lighter color" => LighterColor,
        "overlay" => Overlay,
        "soft_light" | "soft_light_svg" => SoftLight,
        "hard_light" => HardLight,
        "vivid_light" => VividLight,
        "linear light" => LinearLight,
        "pin_light" => PinLight,
        "hard_mix_photoshop" | "hard mix" => HardMix,
        "diff" => Difference,
        "exclusion" => Exclusion,
        "subtract" => Subtract,
        "divide" => Divide,
        "hue" => Hue,
        "saturation" => Saturation,
        "color" => Color,
        "luminize" => Luminosity,
        "pass through" => PassThrough,
        _ => return None,
    })
}

/// The Krita composite op id for a Schist blend mode.
pub fn blend_id(mode: BlendMode) -> Option<&'static str> {
    use BlendMode::*;
    Some(match mode {
        Normal => "normal",
        Dissolve => "dissolve",
        Darken => "darken",
        Multiply => "multiply",
        ColorBurn => "burn",
        LinearBurn => "linear_burn",
        DarkerColor => "darker color",
        Lighten => "lighten",
        Screen => "screen",
        ColorDodge => "dodge",
        LinearDodge => "linear_dodge",
        LighterColor => "lighter color",
        Overlay => "overlay",
        SoftLight => "soft_light",
        HardLight => "hard_light",
        VividLight => "vivid_light",
        LinearLight => "linear light",
        PinLight => "pin_light",
        HardMix => "hard_mix_photoshop",
        Difference => "diff",
        Exclusion => "exclusion",
        Subtract => "subtract",
        Divide => "divide",
        Hue => "hue",
        Saturation => "saturation",
        Color => "color",
        Luminosity => "luminize",
        PassThrough => "pass through",
    })
}

/// The RGB pixel layouts Schist reads natively.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    /// 8-bit, stored B, G, R, A.
    U8,
    /// 16-bit little-endian, stored B, G, R, A.
    U16,
    /// Half float, stored R, G, B, A.
    F16,
    /// Float, stored R, G, B, A.
    F32,
}

impl Format {
    fn from_id(id: &str) -> Option<Self> {
        // Older files spell the float spaces `RgbAF16`/`RgbAF32`.
        Some(match id.to_ascii_uppercase().as_str() {
            "RGBA" | "RGBAU8" => Format::U8,
            "RGBA16" | "RGBAU16" => Format::U16,
            "RGBAF16" => Format::F16,
            "RGBAF32" => Format::F32,
            _ => return None,
        })
    }
    fn pixel_size(self) -> usize {
        match self {
            Format::U8 => 4,
            Format::U16 | Format::F16 => 8,
            Format::F32 => 16,
        }
    }
    fn depth(self) -> Depth {
        match self {
            Format::U8 => Depth::Eight,
            Format::U16 => Depth::Sixteen,
            Format::F16 | Format::F32 => Depth::ThirtyTwo,
        }
    }
    /// One interleaved pixel as straight-alpha RGBA.
    fn pixel(self, b: &[u8]) -> [f32; 4] {
        let u16_at = |i: usize| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]) as f32 / 65535.0;
        let px = match self {
            Format::U8 => [b[2], b[1], b[0], b[3]].map(|v| v as f32 / 255.0),
            Format::U16 => [u16_at(2), u16_at(1), u16_at(0), u16_at(3)],
            Format::F16 => std::array::from_fn(|i| {
                half::f16::from_bits(u16::from_le_bytes([b[i * 2], b[i * 2 + 1]])).to_f32()
            }),
            Format::F32 => {
                std::array::from_fn(|i| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()))
            }
        };
        let finite = px.map(|v| if v.is_finite() { v } else { 0.0 });
        [finite[0], finite[1], finite[2], finite[3].clamp(0.0, 1.0)]
    }
}

/// One decoded tile: its top-left in device coordinates, interleaved.
struct Tile {
    x: i32,
    y: i32,
    data: Vec<u8>,
}

fn line<'a>(bytes: &'a [u8], pos: &mut usize) -> Result<&'a str> {
    let rest = bytes.get(*pos..).ok_or_else(invalid)?;
    let end = rest
        .iter()
        .take(128)
        .position(|&b| b == b'\n')
        .ok_or_else(invalid)?;
    *pos += end + 1;
    std::str::from_utf8(&rest[..end])
        .map(str::trim)
        .map_err(|_| invalid())
}

/// Parse a "VERSION 2" tile stream into tiles of `pixel_size` bytes a pixel.
///
/// `Ok(None)` is a stream in a layout this reader does not know (the
/// pre-2.x untagged form, or a future version); damage is an error.
fn tiles(bytes: &[u8], pixel_size: usize, remaining: &mut usize) -> Result<Option<Vec<Tile>>> {
    let mut pos = 0;
    let first = line(bytes, &mut pos)?;
    if first != "VERSION 2" {
        log::warn!("krita: unsupported tile stream {first:?}");
        return Ok(None);
    }
    let mut count = None;
    for _ in 0..8 {
        let header = line(bytes, &mut pos)?;
        let (key, value) = header.split_once(' ').ok_or_else(invalid)?;
        let value: usize = value.trim().parse().map_err(|_| invalid())?;
        match key {
            "TILEWIDTH" | "TILEHEIGHT" => ensure!(value == TILE, "{}", t("codec.layered.invalid")),
            "PIXELSIZE" => ensure!(value == pixel_size, "{}", t("codec.layered.invalid")),
            "DATA" => {
                count = Some(value);
                break;
            }
            _ => return Err(invalid()),
        }
    }
    let count = count.ok_or_else(invalid)?;
    let tile_bytes = TILE * TILE * pixel_size;
    // Each tile needs at least a header line and a flag byte.
    ensure!(
        count <= bytes.len() / 8 + 1,
        "{}",
        t("codec.layered.invalid")
    );
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let header = line(bytes, &mut pos)?;
        let fields: Vec<&str> = header.split(',').collect();
        ensure!(fields.len() == 4, "{}", t("codec.layered.invalid"));
        let x: i32 = fields[0].trim().parse().map_err(|_| invalid())?;
        let y: i32 = fields[1].trim().parse().map_err(|_| invalid())?;
        ensure!(fields[2].trim() == "LZF", "{}", t("codec.layered.invalid"));
        let size: usize = fields[3].trim().parse().map_err(|_| invalid())?;
        ensure!(
            size >= 1 && x.abs() <= 1_000_000 && y.abs() <= 1_000_000,
            "{}",
            t("codec.layered.invalid")
        );
        let end = pos.checked_add(size).ok_or_else(invalid)?;
        let payload = bytes.get(pos..end).ok_or_else(invalid)?;
        pos = end;
        layered::budget(remaining, tile_bytes)?;
        let data = match payload[0] {
            1 => {
                let planar = lzf::decompress(&payload[1..], tile_bytes).ok_or_else(invalid)?;
                let plane = TILE * TILE;
                let mut data = vec![0; tile_bytes];
                for (i, pixel) in data.chunks_exact_mut(pixel_size).enumerate() {
                    for (c, byte) in pixel.iter_mut().enumerate() {
                        *byte = planar[c * plane + i];
                    }
                }
                data
            }
            0 => payload.get(1..1 + tile_bytes).ok_or_else(invalid)?.to_vec(),
            _ => return Err(invalid()),
        };
        out.push(Tile { x, y, data });
    }
    Ok(Some(out))
}

struct Reader<'a> {
    package: &'a Package,
    /// `<image name>/layers/`.
    prefix: String,
    width: u32,
    height: u32,
    depth: Depth,
    remaining: usize,
    count: usize,
    /// Content Schist could not import, which the merged image covers.
    unsupported: Vec<String>,
}

fn attr_i32(el: &Element, key: &str) -> Result<i32> {
    match el.attr(key) {
        None => Ok(0),
        Some(v) => {
            let v: f64 = v.trim().parse().map_err(|_| invalid())?;
            ensure!(
                v.is_finite() && v.abs() <= 1e6,
                "{}",
                t("codec.layered.invalid")
            );
            Ok(v.round() as i32)
        }
    }
}

fn flag(el: &Element, key: &str, default: bool) -> bool {
    match el.attr(key).map(str::trim) {
        Some("1" | "true") => true,
        Some("0" | "false") => false,
        _ => default,
    }
}

impl<'a> Reader<'a> {
    fn part(&self, file: &str, suffix: &str) -> Option<&'a [u8]> {
        self.package.get(&format!("{}{file}{suffix}", self.prefix))
    }

    /// A paint device placed at `(dx, dy)`, as a raster layer's tiles.
    fn paint(
        &mut self,
        layer: &mut Layer,
        file: &str,
        format: Format,
        dx: i32,
        dy: i32,
    ) -> Result<bool> {
        let Some(bytes) = self.part(file, "") else {
            return Ok(false);
        };
        let size = format.pixel_size();
        let Some(tiles) = tiles(bytes, size, &mut self.remaining)? else {
            return Ok(false);
        };
        let default = self
            .part(file, ".defaultpixel")
            .filter(|b| b.len() == size)
            .map(|b| format.pixel(b))
            .filter(|p| p[3] > 0.0);
        let raster = &mut layer.as_raster_mut().ok_or_else(invalid)?.tiles;
        let mut covered = std::collections::HashSet::new();
        let mut rgba = vec![0.0; TILE * TILE * 4];
        for tile in &tiles {
            let (x, y) = (tile.x + dx, tile.y + dy);
            covered.insert((tile.x, tile.y));
            let bounds = layered::rect(x, y, TILE as u32, TILE as u32)?;
            layered::budget(
                &mut self.remaining,
                layered::tile_bytes(bounds, self.depth, false),
            )?;
            for (px, out) in tile
                .data
                .chunks_exact(size)
                .zip(rgba.as_chunks_mut::<4>().0)
            {
                *out = format.pixel(px);
            }
            blit_rgba_f32(raster, self.depth, bounds, &rgba);
        }
        // A device's default pixel fills everywhere it has no tile; Krita
        // backgrounds are often nothing but a default pixel. Fill the
        // canvas with it, which is what Krita shows.
        if let Some(px) = default {
            let canvas = IntRect::from_size(self.width, self.height);
            let fill = px.repeat(TILE * TILE);
            let first = |v: i32| (v - v.rem_euclid(TILE as i32)) as i64;
            let (mut ty, bottom) = (first(-dy), (self.height as i64) - dy as i64);
            while ty < bottom {
                let mut tx = first(-dx);
                while tx < (self.width as i64) - dx as i64 {
                    if !covered.contains(&(tx as i32, ty as i32)) {
                        let bounds = IntRect::from_xywh(
                            tx as i32 + dx,
                            ty as i32 + dy,
                            TILE as u32,
                            TILE as u32,
                        );
                        let clip = bounds.intersect(&canvas);
                        if !clip.is_empty() {
                            layered::budget(
                                &mut self.remaining,
                                layered::tile_bytes(clip, self.depth, false),
                            )?;
                            blit_rgba_f32(
                                raster,
                                self.depth,
                                clip,
                                &fill[..clip.width() as usize * clip.height() as usize * 4],
                            );
                        }
                    }
                    tx += TILE as i64;
                }
                ty += TILE as i64;
            }
        }
        Ok(true)
    }

    /// A transparency mask's pixel selection as a Schist layer mask.
    fn mask(&mut self, el: &Element) -> Result<Option<LayerMask>> {
        let file = el.attr("filename").ok_or_else(invalid)?;
        let Some(bytes) = self.part(file, ".pixelselection") else {
            return Ok(None);
        };
        let Some(tiles) = tiles(bytes, 1, &mut self.remaining)? else {
            return Ok(None);
        };
        let (dx, dy) = (attr_i32(el, "x")?, attr_i32(el, "y")?);
        let mut mask = LayerMask::new_revealing();
        mask.default_value = self
            .part(file, ".pixelselection.defaultpixel")
            .and_then(|b| b.first().copied())
            .unwrap_or(0);
        mask.enabled = flag(el, "visible", true);
        let mut bounds = IntRect::EMPTY;
        for tile in &tiles {
            let rect = layered::rect(tile.x + dx, tile.y + dy, TILE as u32, TILE as u32)?;
            layered::budget(
                &mut self.remaining,
                layered::tile_bytes(rect, self.depth, true),
            )?;
            bounds = bounds.union(&rect);
            for coord in TileCoord::covering(&rect) {
                let region = coord.rect().intersect(&rect);
                let target = mask.tiles.get_mut_or_insert(coord);
                for y in region.top..region.bottom {
                    for x in region.left..region.right {
                        let src = (y - rect.top) as usize * TILE + (x - rect.left) as usize;
                        let dst = (y - coord.rect().top) as usize * schist_core::TILE_SIZE as usize
                            + (x - coord.rect().left) as usize;
                        target[dst] = tile.data[src];
                    }
                }
            }
        }
        mask.bounds = bounds;
        Ok(Some(mask))
    }

    fn layers(
        &mut self,
        el: &Element,
        level: usize,
        active: &mut Option<LayerId>,
    ) -> Result<Vec<Layer>> {
        ensure!(level < 64, "{}", t("codec.layered.too_large"));
        let mut out = Vec::new();
        for node in el.children.iter().filter(|c| c.name == "layer") {
            self.count += 1;
            ensure!(self.count <= MAX_LAYERS, "{}", t("codec.layered.too_large"));
            let kind = node.attr("nodetype").unwrap_or_default();
            let name = node.attr("name").unwrap_or_default().to_string();
            let mut layer = match kind {
                "paintlayer" => {
                    let file = node.attr("filename").ok_or_else(invalid)?;
                    let space = node.attr("colorspacename").unwrap_or("RGBA");
                    let mut layer = Layer::new_raster(name);
                    match Format::from_id(space) {
                        Some(format) => {
                            let (dx, dy) = (attr_i32(node, "x")?, attr_i32(node, "y")?);
                            if !self.paint(&mut layer, file, format, dx, dy)? {
                                self.unsupported.push(kind.to_string());
                            }
                        }
                        None => {
                            self.unsupported.push(space.to_string());
                            continue;
                        }
                    }
                    layer
                }
                "grouplayer" => {
                    let mut layer = Layer::new_group(name);
                    let children = match node.child("layers") {
                        Some(list) => self.layers(list, level + 1, active)?,
                        None => Vec::new(),
                    };
                    if let LayerKind::Group(g) = &mut layer.kind {
                        g.children = children;
                        g.open = !flag(node, "collapsed", false);
                    }
                    layer
                }
                // Reference images float above the canvas and are not part
                // of Krita's render, so there is nothing to stand in for.
                "referenceimages" => continue,
                other => {
                    log::warn!("krita: {other:?} layer {name:?} is not supported");
                    self.unsupported.push(other.to_string());
                    continue;
                }
            };
            layer.visible = flag(node, "visible", true);
            layer.locked = flag(node, "locked", false);
            let opacity: f32 = node
                .attr("opacity")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(255.0);
            layer.opacity = if opacity.is_finite() {
                (opacity / 255.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let op = node.attr("compositeop").unwrap_or("normal");
            layer.blend = blend(op).unwrap_or_else(|| {
                log::warn!("krita: composite op {op:?} is not supported; using normal");
                BlendMode::Normal
            });
            if layer.is_group() {
                layer.blend = if flag(node, "passthrough", false) {
                    BlendMode::PassThrough
                } else if layer.blend == BlendMode::PassThrough {
                    BlendMode::Normal
                } else {
                    layer.blend
                };
            }
            if flag(node, "selected", false) {
                *active = Some(layer.id);
            }
            if let Some(masks) = node.child("masks") {
                let mut transparency = 0;
                for mask in masks.children.iter().filter(|m| m.name == "mask") {
                    match mask.attr("nodetype").unwrap_or_default() {
                        // A local selection: an editing aid, not rendered.
                        "selectionmask" => {}
                        "transparencymask" if transparency == 0 && !layer.is_group() => {
                            transparency += 1;
                            match self.mask(mask)? {
                                Some(m) => layer.mask = Some(m),
                                None => self.unsupported.push("transparencymask".into()),
                            }
                        }
                        other => {
                            log::warn!(
                                "krita: {other:?} mask on {:?} is not supported",
                                layer.name
                            );
                            self.unsupported.push(other.to_string());
                        }
                    }
                }
            }
            out.push(layer);
        }
        // maindoc.xml lists the topmost first; Schist stores bottom first.
        out.reverse();
        Ok(out)
    }
}

fn read(bytes: &[u8]) -> Result<Document> {
    let package = archive::open(bytes)?;
    ensure!(
        package.get("mimetype") == Some(MIMETYPE.as_bytes()),
        "{}",
        t("codec.layered.invalid")
    );
    let root = archive::parse_xml(package.get("maindoc.xml").ok_or_else(invalid)?)?;
    ensure!(root.name == "DOC", "{}", t("codec.layered.invalid"));
    let image = root.child("IMAGE").ok_or_else(invalid)?;
    let dim = |key| {
        image
            .attr(key)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .ok_or_else(invalid)
    };
    let (width, height) = (dim("width")?, dim("height")?);
    layered::size(width, height, 16)?;
    let name = image.attr("name").ok_or_else(invalid)?;
    let space = image.attr("colorspacename").unwrap_or("RGBA");
    let mut remaining = MAX_BYTES;
    let merged = package
        .get("mergedimage.png")
        .or_else(|| package.get("preview.png"))
        .map(|png| archive::decode(png, &mut remaining))
        .transpose()
        .unwrap_or_else(|e| {
            log::warn!("krita: merged image did not decode: {e}");
            None
        });
    let icc = package
        .get(&format!("{name}/annotations/icc"))
        .filter(|b| !b.is_empty())
        .map(<[u8]>::to_vec);
    let dpi = image
        .attr("x-res")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0 && *v < 1e6);

    let Some(format) = Format::from_id(space) else {
        // CMYK, Lab, grayscale, XYZ and YCbCr documents: Krita's merged
        // image is RGB(A) or gray, and is what Schist can show.
        log::warn!("krita: colour space {space:?} imported as the merged image");
        let merged = merged.ok_or_else(|| layered::unsupported(space))?;
        let depth = if merged.deep {
            Depth::Sixteen
        } else {
            Depth::Eight
        };
        let mut doc = Document::new(t("codec.kra.name"), width, height, depth);
        doc.resolution_dpi = dpi.unwrap_or(doc.resolution_dpi);
        // The profile describes the original colour space, not the RGB
        // rendering, so it is deliberately not attached.
        doc.icc_profile = merged.icc.clone();
        let label = tf!("codec.kra.merged_color_space", color_space = space);
        let layer = archive::merged_layer(&label, &merged, width, height, depth, &mut remaining)?;
        doc.tree.layers.push(layer);
        return Ok(layered::finish(doc));
    };

    let mut doc = Document::new(t("codec.kra.name"), width, height, format.depth());
    if let Some(dpi) = dpi {
        doc.resolution_dpi = dpi;
    }
    doc.icc_profile = icc;
    let mut reader = Reader {
        package: &package,
        prefix: format!("{name}/layers/"),
        width,
        height,
        depth: format.depth(),
        remaining,
        count: 0,
        unsupported: Vec::new(),
    };
    let mut active = None;
    if let Some(list) = image.child("layers") {
        doc.tree.layers = reader.layers(list, 0, &mut active)?;
    }
    if !reader.unsupported.is_empty() {
        reader.unsupported.dedup();
        log::warn!(
            "krita: not imported: {}; adding the merged image",
            reader.unsupported.join(", ")
        );
        if let Some(merged) = &merged {
            let label = tf!("codec.layered.merged_layer", format = t("codec.kra.name"));
            let layer = archive::merged_layer(
                &label,
                merged,
                width,
                height,
                reader.depth,
                &mut reader.remaining,
            )?;
            doc.tree.layers.push(layer);
        } else {
            return Err(layered::unsupported(&reader.unsupported.join(", ")));
        }
    }
    doc.active_layer = active;
    Ok(layered::finish(doc))
}

/// Build Krita packages for tests: the same structures a Krita save
/// produces, written by hand from the format description above.
#[cfg(test)]
pub(crate) mod build {
    use super::*;

    /// A VERSION 2 tile stream of 64×64 tiles, each given interleaved.
    pub fn stream(pixel_size: usize, tiles: &[(i32, i32, Vec<u8>)], compress: bool) -> Vec<u8> {
        let mut out = format!(
            "VERSION 2\nTILEWIDTH 64\nTILEHEIGHT 64\nPIXELSIZE {pixel_size}\nDATA {}\n",
            tiles.len()
        )
        .into_bytes();
        for (x, y, data) in tiles {
            assert_eq!(data.len(), TILE * TILE * pixel_size);
            let payload = if compress {
                let mut planar = Vec::with_capacity(data.len());
                for c in 0..pixel_size {
                    planar.extend(data.iter().skip(c).step_by(pixel_size));
                }
                let mut p = vec![1];
                p.extend(lzf::compress(&planar));
                p
            } else {
                let mut p = vec![0];
                p.extend_from_slice(data);
                p
            };
            out.extend(format!("{x},{y},LZF,{}\n", payload.len()).into_bytes());
            out.extend(payload);
        }
        out
    }

    pub fn package(maindoc: &str, parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
        let mut all = vec![
            ("mimetype".to_string(), MIMETYPE.as_bytes().to_vec()),
            ("maindoc.xml".to_string(), maindoc.as_bytes().to_vec()),
        ];
        all.extend(parts);
        schist_codec_idml::container::write(&all)
    }

    pub fn maindoc(space: &str, layers: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE DOC PUBLIC '-//KDE//DTD krita 2.0//EN' 'http://www.calligra.org/DTD/krita-2.0.dtd'>
<DOC xmlns="http://www.calligra.org/DTD/krita" kritaVersion="5.2.2" syntaxVersion="2.0" editor="Krita">
 <IMAGE width="100" height="70" mime="application/x-kra" name="Unnamed" colorspacename="{space}" profile="sRGB-elle-V2-srgbtrc.icc" x-res="300" y-res="300" description="">
  <layers>{layers}</layers>
 </IMAGE>
</DOC>"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::*;

    fn bgra_tile(b: u8, g: u8, r: u8, a: u8) -> Vec<u8> {
        [b, g, r, a].repeat(TILE * TILE)
    }

    #[test]
    fn compressed_tiles_are_planar_and_raw_tiles_interleaved() {
        let mut gradient = Vec::new();
        for i in 0..TILE * TILE {
            gradient.extend_from_slice(&[(i % 256) as u8, 7, (i / 64) as u8, 255]);
        }
        for compress in [true, false] {
            let bytes = stream(4, &[(-64, 128, gradient.clone())], compress);
            let mut remaining = MAX_BYTES;
            let out = tiles(&bytes, 4, &mut remaining).unwrap().unwrap();
            assert_eq!(out.len(), 1);
            assert_eq!((out[0].x, out[0].y), (-64, 128));
            assert_eq!(out[0].data, gradient);
        }
    }

    #[test]
    fn tile_streams_reject_damage_without_panicking() {
        let good = stream(4, &[(0, 0, bgra_tile(1, 2, 3, 4))], true);
        let mut remaining = MAX_BYTES;
        for cut in 0..good.len() {
            let _ = tiles(&good[..cut], 4, &mut remaining);
            remaining = MAX_BYTES;
        }
        for bad in [
            &b"VERSION 2\nTILEWIDTH 32\nDATA 0\n"[..],
            b"VERSION 2\nPIXELSIZE 8\nDATA 0\n",
            b"VERSION 2\nDATA 99999999\n",
            b"VERSION 2\nDATA 1\n0,0,ZIP,2\n\x01\x00",
            b"VERSION 2\nDATA 1\n0,0,LZF,2\n\x07\x00",
        ] {
            assert!(tiles(bad, 4, &mut remaining).is_err(), "{bad:?}");
        }
        assert!(tiles(b"3\n", 4, &mut remaining).unwrap().is_none());
    }

    #[test]
    fn layers_groups_offsets_masks_and_default_pixels() {
        let layers = r#"
   <layer name="Top" filename="layer3" nodetype="paintlayer" colorspacename="RGBA" x="10" y="-5" visible="0" opacity="128" compositeop="multiply" locked="1" selected="true">
    <masks><mask name="Transparency" filename="mask4" nodetype="transparencymask" x="0" y="0" visible="1"/></masks>
   </layer>
   <layer name="Group" filename="layer2" nodetype="grouplayer" passthrough="1" collapsed="1" opacity="255" compositeop="normal" visible="1">
    <layers>
     <layer name="Inner" filename="layer5" nodetype="paintlayer" colorspacename="RGBA" x="0" y="0" opacity="255" compositeop="linear light" visible="1"/>
    </layers>
   </layer>
   <layer name="Background" filename="layer1" nodetype="paintlayer" colorspacename="RGBA" x="0" y="0" opacity="255" compositeop="normal" visible="1"/>"#;
        let p = "Unnamed/layers/";
        let parts = vec![
            (
                format!("{p}layer3"),
                stream(4, &[(0, 0, bgra_tile(255, 0, 0, 255))], true),
            ),
            (format!("{p}layer3.defaultpixel"), vec![0; 4]),
            (
                format!("{p}mask4.pixelselection"),
                stream(1, &[(0, 0, vec![200; TILE * TILE])], true),
            ),
            (format!("{p}mask4.pixelselection.defaultpixel"), vec![0]),
            (
                format!("{p}layer5"),
                stream(4, &[(64, 0, bgra_tile(0, 255, 0, 128))], false),
            ),
            (format!("{p}layer1"), stream(4, &[], true)),
            (format!("{p}layer1.defaultpixel"), vec![255, 255, 255, 255]),
            (
                "Unnamed/annotations/icc".to_string(),
                b"fake profile".to_vec(),
            ),
        ];
        let doc = read(&package(&maindoc("RGBA", layers), parts)).unwrap();
        assert_eq!((doc.width, doc.height, doc.depth), (100, 70, Depth::Eight));
        assert_eq!(doc.resolution_dpi, 300.0);
        assert_eq!(doc.icc_profile.as_deref(), Some(&b"fake profile"[..]));
        let names: Vec<_> = doc.tree.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Background", "Group", "Top"]);

        let background = &doc.tree.layers[0].as_raster().unwrap().tiles;
        assert_eq!(background.pixel(99, 69).r, 1.0);
        assert_eq!(
            background.pixel(100, 69).a,
            0.0,
            "default fill stops at the canvas"
        );

        let group = &doc.tree.layers[1];
        assert_eq!(group.blend, BlendMode::PassThrough);
        assert!(!group.children().is_none());
        let inner = &group.children().unwrap()[0];
        assert_eq!(inner.blend, BlendMode::LinearLight);
        let px = inner.as_raster().unwrap().tiles.pixel(64, 0);
        assert_eq!((px.g, px.a), (1.0, 128.0 / 255.0));

        let top = &doc.tree.layers[2];
        assert!(!top.visible && top.locked);
        assert_eq!(top.blend, BlendMode::Multiply);
        assert_eq!(top.opacity, 128.0 / 255.0);
        assert_eq!(doc.active_layer, Some(top.id));
        let px = top.as_raster().unwrap().tiles.pixel(10, -5);
        assert_eq!((px.r, px.b), (0.0, 1.0), "BGRA order and the layer offset");
        let mask = top.mask.as_ref().unwrap();
        assert_eq!(mask.value(5, 5), 200);
        assert_eq!(mask.value(80, 5), 0);
    }

    #[test]
    fn sixteen_bit_and_float_layers_keep_their_precision() {
        let mut u16_tile = Vec::new();
        for _ in 0..TILE * TILE {
            for v in [1000u16, 2000, 65535, 65535] {
                u16_tile.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut f32_tile = Vec::new();
        for _ in 0..TILE * TILE {
            for v in [2.5f32, 0.25, 0.0, 1.0] {
                f32_tile.extend_from_slice(&v.to_le_bytes());
            }
        }
        for (space, size, tile, check) in [
            (
                "RGBA16",
                8,
                u16_tile,
                (1.0, 1000.0 / 65535.0, Depth::Sixteen),
            ),
            ("RgbAF32", 16, f32_tile, (2.5, 0.0, Depth::ThirtyTwo)),
        ] {
            let layers = format!(
                r#"<layer name="L" filename="layer1" nodetype="paintlayer" colorspacename="{space}" opacity="255" compositeop="normal" visible="1"/>"#
            );
            let parts = vec![(
                "Unnamed/layers/layer1".to_string(),
                stream(size, &[(0, 0, tile)], true),
            )];
            let doc = read(&package(&maindoc(space, &layers), parts)).unwrap();
            assert_eq!(doc.depth, check.2);
            let px = doc.tree.layers[0].as_raster().unwrap().tiles.pixel(3, 3);
            assert!((px.r - check.0).abs() < 1e-3, "{space}: {px:?}");
            assert!((px.b - check.1).abs() < 1e-3, "{space}: {px:?}");
        }
    }

    #[test]
    fn unsupported_layers_and_colour_spaces_use_the_merged_image() {
        let merged =
            archive::encode_png(100, 70, &[0.5, 0.5, 0.5, 1.0].repeat(7000), false, None).unwrap();
        let layers = r#"
   <layer name="Shapes" filename="layer2" nodetype="shapelayer" opacity="255" compositeop="normal" visible="1"/>
   <layer name="Paint" filename="layer1" nodetype="paintlayer" colorspacename="RGBA" opacity="255" compositeop="normal" visible="1"/>"#;
        let parts = vec![
            ("Unnamed/layers/layer1".to_string(), stream(4, &[], true)),
            ("mergedimage.png".to_string(), merged.clone()),
        ];
        let doc = read(&package(&maindoc("RGBA", layers), parts)).unwrap();
        assert_eq!(doc.tree.layers.len(), 2);
        assert_eq!(doc.tree.layers[0].name, "Paint");
        assert_eq!(
            doc.tree.layers[1].name,
            tf!("codec.layered.merged_layer", format = t("codec.kra.name"))
        );

        let doc = read(&package(
            &maindoc("CMYKA", ""),
            vec![("mergedimage.png".to_string(), merged)],
        ))
        .unwrap();
        assert_eq!(doc.tree.layers.len(), 1);
        assert!(
            (doc.tree.layers[0]
                .as_raster()
                .unwrap()
                .tiles
                .pixel(50, 50)
                .r
                - 0.5)
                .abs()
                < 0.01
        );

        // Without a merged image there is nothing honest to show.
        assert!(read(&package(&maindoc("CMYKA", ""), vec![])).is_err());
    }

    #[test]
    fn hostile_packages_fail_cleanly() {
        let layers = r#"<layer name="L" filename="layer1" nodetype="paintlayer" colorspacename="RGBA" opacity="255" compositeop="normal" visible="1"/>"#;
        let good = package(
            &maindoc("RGBA", layers),
            vec![(
                "Unnamed/layers/layer1".to_string(),
                stream(4, &[(0, 0, bgra_tile(1, 2, 3, 4))], true),
            )],
        );
        assert!(read(&good).is_ok());
        for cut in (0..good.len()).step_by(7) {
            let _ = read(&good[..cut]);
        }
        let mut flipped = good.clone();
        for i in (0..flipped.len()).step_by(11) {
            flipped[i] ^= 0x5a;
            let _ = read(&flipped);
        }
        let deep = format!(
            "{}{}",
            r#"<layer nodetype="grouplayer" name="g"><layers>"#.repeat(100),
            r#"</layers></layer>"#.repeat(100)
        );
        assert!(read(&package(&maindoc("RGBA", &deep), vec![])).is_err());
        let huge = maindoc("RGBA", "").replace("width=\"100\"", "width=\"2000000\"");
        assert!(read(&package(&huge, vec![])).is_err());
    }
}
