//! GIMP XCF raster interchange, implemented from GIMP's public XCF specification.
//! File pointers and samples are big endian; layer lists run top to bottom.
use std::collections::BTreeMap;

use anyhow::{ensure, Result};
use schist_color::{ColorMode, Depth};
use schist_core::{
    blit_rgba_f32, BlendMode, Document, IntRect, Layer, LayerKind, LayerMask, TileCoord,
};
use schist_i18n::t;
use schist_plugin_api::CodecPlugin;

use crate::layered::{self, invalid, unsupported, Reader, MAX_BYTES, MAX_LAYERS};

pub struct XcfCodec;

impl CodecPlugin for XcfCodec {
    fn id(&self) -> &'static str {
        "codec.xcf"
    }
    fn name(&self) -> &'static str {
        t("codec.xcf.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["xcf"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(b"gimp xcf ")
    }
    fn import(&self, bytes: &[u8]) -> Result<Document> {
        read(bytes)
    }
    fn can_export(&self) -> bool {
        true
    }
    fn export(&self, doc: &Document) -> Result<Vec<u8>> {
        write(doc)
    }
}

type Properties<'a> = BTreeMap<u32, &'a [u8]>;

fn properties<'a>(r: &mut Reader<'a>) -> Result<Properties<'a>> {
    let mut props = BTreeMap::new();
    for _ in 0..4096 {
        let kind = r.be32()?;
        let len = r.be32()? as usize;
        if kind == 0 {
            ensure!(len == 0, "{}", t("codec.layered.invalid"));
            return Ok(props);
        }
        props.insert(kind, r.take(len)?);
    }
    Err(invalid())
}

fn word(props: &Properties<'_>, key: u32, default: u32) -> Result<u32> {
    props
        .get(&key)
        .map_or(Ok(default), |b| Reader::new(b).be32())
}

#[derive(Clone, Copy)]
struct Precision {
    bytes: usize,
    float: bool,
    linear: bool,
    depth: Depth,
}

impl Precision {
    fn parse(version: u32, value: u32) -> Result<Self> {
        // Development builds before v012 stored high precision samples in
        // host byte order. There is no reliable way to infer that byte order.
        if version < 12 && value != 150 {
            return Err(unsupported("XCF precision"));
        }
        let (bytes, float, depth) = match value {
            100 | 150 => (1, false, Depth::Eight),
            200 | 250 => (2, false, Depth::Sixteen),
            300 | 350 => (4, false, Depth::ThirtyTwo),
            500 | 550 => (2, true, Depth::ThirtyTwo),
            600 | 650 => (4, true, Depth::ThirtyTwo),
            700 | 750 => (8, true, Depth::ThirtyTwo),
            _ => return Err(unsupported("XCF precision")),
        };
        Ok(Self {
            bytes,
            float,
            linear: value.is_multiple_of(100),
            depth,
        })
    }

    fn sample(self, b: &[u8]) -> f32 {
        match (self.bytes, self.float) {
            (1, _) => b[0] as f32 / 255.0,
            (2, false) => u16::from_be_bytes(b.try_into().unwrap()) as f32 / 65535.0,
            (4, false) => u32::from_be_bytes(b.try_into().unwrap()) as f32 / u32::MAX as f32,
            (2, true) => half::f16::from_bits(u16::from_be_bytes(b.try_into().unwrap())).to_f32(),
            (4, true) => f32::from_be_bytes(b.try_into().unwrap()),
            (8, true) => f64::from_be_bytes(b.try_into().unwrap()) as f32,
            _ => unreachable!(),
        }
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    wide: bool,
    precision: Precision,
    compression: u8,
    palette: Vec<[u8; 3]>,
    remaining: usize,
}

impl Decoder<'_> {
    fn pixels(&mut self, pointer: usize, w: u32, h: u32, components: usize) -> Result<Vec<f32>> {
        let count = layered::size(w, h, components * 4)?;
        layered::budget(&mut self.remaining, count)?;
        let mut r = Reader::at(self.bytes, pointer)?;
        let bpp = components * self.precision.bytes;
        ensure!(
            r.be32()? == w && r.be32()? == h && r.be32()? as usize == bpp,
            "{}",
            t("codec.layered.invalid")
        );
        let level = r.pointer(self.wide)?;
        let mut r = Reader::at(self.bytes, level)?;
        ensure!(
            r.be32()? == w && r.be32()? == h,
            "{}",
            t("codec.layered.invalid")
        );
        let tiles_x = w.div_ceil(64);
        let tiles_y = h.div_ceil(64);
        let count = (tiles_x * tiles_y) as usize;
        let mut pointers = Vec::with_capacity(count);
        for _ in 0..count {
            pointers.push(r.pointer(self.wide)?);
        }
        ensure!(r.pointer(self.wide)? == 0, "{}", t("codec.layered.invalid"));
        let mut out = vec![0.0; w as usize * h as usize * components];
        for (i, &start) in pointers.iter().enumerate() {
            let tx = i as u32 % tiles_x * 64;
            let ty = i as u32 / tiles_x * 64;
            let tw = (w - tx).min(64) as usize;
            let th = (h - ty).min(64) as usize;
            let mut tile = Reader::at(self.bytes, start)?;
            let end = pointers.get(i + 1).copied().unwrap_or(self.bytes.len());
            ensure!(
                end > start && end <= self.bytes.len(),
                "{}",
                t("codec.layered.invalid")
            );
            tile.bytes = &self.bytes[..end];
            let raw = match self.compression {
                0 => tile.take(tw * th * bpp)?.to_vec(),
                1 => decode_rle(&mut tile, tw * th, bpp)?,
                2 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
                    &self.bytes[start..end],
                    tw * th * bpp,
                )
                .map_err(|_| invalid())?,
                _ => return Err(unsupported("XCF compression")),
            };
            ensure!(raw.len() == tw * th * bpp, "{}", t("codec.layered.invalid"));
            for y in 0..th {
                for x in 0..tw {
                    for c in 0..components {
                        let src = ((y * tw + x) * components + c) * self.precision.bytes;
                        let value = self.precision.sample(&raw[src..src + self.precision.bytes]);
                        ensure!(value.is_finite(), "{}", t("codec.layered.invalid"));
                        out[((ty as usize + y) * w as usize + tx as usize + x) * components + c] =
                            value;
                    }
                }
            }
        }
        Ok(out)
    }

    fn layer(
        &mut self,
        pointer: usize,
        version: u32,
        base: u32,
    ) -> Result<(Layer, Vec<usize>, bool)> {
        let mut r = Reader::at(self.bytes, pointer)?;
        let (w, h, kind) = (r.be32()?, r.be32()?, r.be32()?);
        layered::size(w, h, 16)?;
        ensure!(
            kind <= 5 && kind / 2 == base,
            "{}",
            t("codec.layered.invalid")
        );
        let name = r.xcf_string()?;
        let props = properties(&mut r)?;
        let hierarchy = r.pointer(self.wide)?;
        let mask = r.pointer(self.wide)?;
        if version >= 20 && r.pointer(self.wide)? != 0 {
            return Err(unsupported("XCF layer effects"));
        }
        if props.contains_key(&5) {
            return Err(unsupported("XCF floating selection"));
        }
        let mut offset = Reader::new(props.get(&15).copied().unwrap_or(&[0; 8]));
        let (x, y) = (offset.be32()? as i32, offset.be32()? as i32);
        let bounds = layered::rect(x, y, w, h)?;
        let mut layer = if props.contains_key(&29) {
            Layer::new_group(name)
        } else {
            Layer::new_raster(name)
        };
        layer.visible = word(&props, 8, 1)? != 0;
        layer.opacity = if let Some(b) = props.get(&33) {
            f32::from_bits(Reader::new(b).be32()?)
        } else {
            word(&props, 6, 255)? as f32 / 255.0
        };
        ensure!(
            layer.opacity.is_finite() && (0.0..=1.0).contains(&layer.opacity),
            "{}",
            t("codec.layered.invalid")
        );
        layer.locked = word(&props, 28, 0)? != 0;
        layer.blend = blend(word(&props, 7, 0)?)?;
        if let LayerKind::Group(g) = &mut layer.kind {
            g.open = word(&props, 31, 1)? & 1 != 0;
        } else {
            let components = [3, 4, 1, 2, 1, 2][kind as usize];
            layered::budget(
                &mut self.remaining,
                layered::tile_bytes(bounds, self.precision.depth, false),
            )?;
            let samples = self.pixels(hierarchy, w, h, components)?;
            let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
            for pixel in samples.chunks_exact(components) {
                let alpha = if kind % 2 == 1 {
                    pixel[components - 1]
                } else {
                    1.0
                };
                let mut rgb = match kind / 2 {
                    0 => [pixel[0], pixel[1], pixel[2]],
                    1 => [pixel[0]; 3],
                    _ => {
                        let entry = self
                            .palette
                            .get((pixel[0] * 255.0).round() as usize)
                            .ok_or_else(invalid)?;
                        entry.map(|v| v as f32 / 255.0)
                    }
                };
                if self.precision.linear && base != 2 {
                    for v in &mut rgb {
                        *v = if *v <= 0.0031308 {
                            *v * 12.92
                        } else {
                            1.055 * v.powf(1.0 / 2.4) - 0.055
                        };
                    }
                }
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
            blit_rgba_f32(
                &mut layer.as_raster_mut().unwrap().tiles,
                self.precision.depth,
                bounds,
                &rgba,
            );
        }
        if mask != 0 {
            layered::budget(
                &mut self.remaining,
                layered::tile_bytes(bounds, self.precision.depth, true),
            )?;
            let mut r = Reader::at(self.bytes, mask)?;
            ensure!(
                r.be32()? == w && r.be32()? == h,
                "{}",
                t("codec.layered.invalid")
            );
            r.xcf_string()?;
            properties(&mut r)?;
            let samples = self.pixels(r.pointer(self.wide)?, w, h, 1)?;
            let mut m = LayerMask::new_revealing();
            m.bounds = bounds;
            m.enabled = word(&props, 11, 1)? != 0;
            for coord in TileCoord::covering(&bounds) {
                let tile = m.tiles.get_mut_or_insert(coord);
                let region = coord.rect().intersect(&bounds);
                for yy in region.top..region.bottom {
                    for xx in region.left..region.right {
                        let src = ((yy - y) as usize * w as usize) + (xx - x) as usize;
                        let index = (yy - coord.rect().top) as usize
                            * schist_core::TILE_SIZE as usize
                            + (xx - coord.rect().left) as usize;
                        tile[index] = (samples[src].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            }
            layer.mask = Some(m);
        }
        let mut path = Vec::new();
        if let Some(data) = props.get(&30) {
            ensure!(
                data.len().is_multiple_of(4) && data.len() <= 64 * 4,
                "{}",
                t("codec.layered.invalid")
            );
            let mut r = Reader::new(data);
            while r.pos < data.len() {
                path.push(r.be32()? as usize);
            }
        }
        Ok((layer, path, props.contains_key(&2)))
    }
}

fn decode_rle(r: &mut Reader<'_>, pixels: usize, bpp: usize) -> Result<Vec<u8>> {
    let mut out = vec![0; pixels * bpp];
    for channel in 0..bpp {
        let mut pos = 0;
        while pos < pixels {
            let op = r.byte()?;
            let count = match op {
                127 | 128 => u16::from_be_bytes(r.take(2)?.try_into()?) as usize,
                0..=126 => op as usize + 1,
                _ => 256 - op as usize,
            };
            ensure!(
                count > 0 && count <= pixels - pos,
                "{}",
                t("codec.layered.invalid")
            );
            if op <= 127 {
                let value = r.byte()?;
                for p in pos..pos + count {
                    out[p * bpp + channel] = value;
                }
            } else {
                for &value in r.take(count)? {
                    out[pos * bpp + channel] = value;
                    pos += 1;
                }
                continue;
            }
            pos += count;
        }
    }
    Ok(out)
}

fn read(bytes: &[u8]) -> Result<Document> {
    let mut r = Reader::new(bytes);
    ensure!(r.take(9)? == b"gimp xcf ", "{}", t("codec.layered.invalid"));
    let tag = r.take(5)?;
    let version = if tag == b"file\0" {
        0
    } else {
        ensure!(
            tag[0] == b'v' && tag[4] == 0 && tag[1..4].iter().all(u8::is_ascii_digit),
            "{}",
            t("codec.layered.invalid")
        );
        std::str::from_utf8(&tag[1..4])?.parse::<u32>()?
    };
    ensure!(version <= 23, "{}", unsupported("XCF version"));
    let (w, h, base) = (r.be32()?, r.be32()?, r.be32()?);
    layered::size(w, h, 16)?;
    ensure!(base <= 2, "{}", unsupported("XCF color model"));
    let precision = Precision::parse(version, if version >= 4 { r.be32()? } else { 150 })?;
    ensure!(
        base != 2 || precision.bytes == 1,
        "{}",
        t("codec.layered.invalid")
    );
    let props = properties(&mut r)?;
    let mut decoder = Decoder {
        bytes,
        wide: version >= 11,
        precision,
        compression: 0,
        palette: Vec::new(),
        remaining: MAX_BYTES,
    };
    if let Some(b) = props.get(&17) {
        decoder.compression = Reader::new(b).byte()?;
    }
    if let Some(b) = props.get(&1) {
        let mut p = Reader::new(b);
        let n = p.be32()? as usize;
        ensure!(n <= 256, "{}", t("codec.layered.invalid"));
        for rgb in p.take(n * 3)?.as_chunks::<3>().0 {
            decoder.palette.push(*rgb);
        }
    }
    let mut pointers = Vec::new();
    loop {
        let pointer = r.pointer(decoder.wide)?;
        if pointer == 0 {
            break;
        }
        ensure!(
            pointers.len() < MAX_LAYERS,
            "{}",
            t("codec.layered.too_large")
        );
        ensure!(
            !pointers.contains(&pointer),
            "{}",
            t("codec.layered.invalid")
        );
        pointers.push(pointer);
    }
    let mut doc = Document::new(t("codec.xcf.name"), w, h, precision.depth);
    if let Some(b) = props.get(&19) {
        let dpi = f32::from_bits(Reader::new(b).be32()?);
        if dpi.is_finite() && dpi > 0.0 {
            doc.resolution_dpi = dpi;
        }
    }
    if let Some(b) = props.get(&21) {
        let mut p = Reader::new(b);
        while p.pos < b.len() {
            let name = p.xcf_string()?;
            p.be32()?; // parasite flags
            let n = p.be32()? as usize;
            let data = p.take(n)?;
            if name == "icc-profile" {
                doc.icc_profile = Some(data.to_vec());
            }
        }
    }
    for pointer in pointers {
        let (layer, path, active) = decoder.layer(pointer, version, base)?;
        if active {
            doc.active_layer = Some(layer.id);
        }
        let mut siblings = &mut doc.tree.layers;
        if !path.is_empty() {
            for &index in &path[..path.len() - 1] {
                let parent = siblings.get_mut(index).ok_or_else(invalid)?;
                let LayerKind::Group(group) = &mut parent.kind else {
                    return Err(invalid());
                };
                siblings = &mut group.children;
            }
            ensure!(
                path.last() == Some(&siblings.len()),
                "{}",
                t("codec.layered.invalid")
            );
        }
        siblings.push(layer);
    }
    fn reverse(layers: &mut [Layer]) {
        layers.reverse();
        for l in layers {
            if let LayerKind::Group(g) = &mut l.kind {
                reverse(&mut g.children);
            }
        }
    }
    reverse(&mut doc.tree.layers);
    Ok(layered::finish(doc))
}

fn blend(value: u32) -> Result<BlendMode> {
    use BlendMode::*;
    Ok(match value {
        0 | 28 => Normal,
        1 => Dissolve,
        3 | 30 => Multiply,
        4 | 31 => Screen,
        5 | 19 | 45 => SoftLight,
        6 | 32 => Difference,
        7 | 33 => LinearDodge,
        8 | 34 => Subtract,
        9 | 35 => Darken,
        10 | 36 => Lighten,
        11 | 37 => Hue,
        12 | 38 => Saturation,
        13 | 39 => Color,
        14 | 40 | 56 => Luminosity,
        15 | 41 => Divide,
        16 | 42 => ColorDodge,
        17 | 43 => ColorBurn,
        18 | 44 => HardLight,
        23 => Overlay,
        48 => VividLight,
        49 => PinLight,
        50 => LinearLight,
        51 => HardMix,
        52 => Exclusion,
        53 => LinearBurn,
        61 => PassThrough,
        _ => return Err(unsupported("XCF blend mode")),
    })
}

fn blend_id(mode: BlendMode) -> Result<u32> {
    // Use legacy modes where their compositing agrees with Schist's RGB
    // compositor. Modern-only modes explicitly select perceptual RGB below.
    use BlendMode::*;
    Ok(match mode {
        Normal => 0,
        Dissolve => 1,
        Multiply => 3,
        Screen => 4,
        Difference => 6,
        LinearDodge => 7,
        Subtract => 8,
        Darken => 9,
        Lighten => 10,
        Hue => 11,
        Saturation => 12,
        Color => 13,
        Luminosity => 56,
        Divide => 15,
        ColorDodge => 16,
        ColorBurn => 17,
        HardLight => 18,
        SoftLight => 19,
        Overlay => 23,
        VividLight => 48,
        PinLight => 49,
        LinearLight => 50,
        HardMix => 51,
        Exclusion => 52,
        LinearBurn => 53,
        PassThrough => 61,
        _ => return Err(unsupported("XCF blend mode")),
    })
}

struct Writer {
    bytes: Vec<u8>,
    depth: Depth,
    remaining: usize,
}

impl Writer {
    fn word(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }
    fn pointer(&mut self) -> usize {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(&[0; 8]);
        at
    }
    fn patch(&mut self, at: usize) {
        let value = self.bytes.len() as u64;
        self.bytes[at..at + 8].copy_from_slice(&value.to_be_bytes());
    }
    fn string(&mut self, s: &str) {
        self.word(s.len() as u32 + 1);
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
    }
    fn prop(&mut self, key: u32, bytes: &[u8]) {
        self.word(key);
        self.word(bytes.len() as u32);
        self.bytes.extend_from_slice(bytes);
    }
    fn int_prop(&mut self, key: u32, value: u32) {
        self.prop(key, &value.to_be_bytes());
    }
    fn sample(&self, out: &mut Vec<u8>, value: f32) {
        match self.depth {
            Depth::Eight => out.push((value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8),
            Depth::Sixteen => out
                .extend_from_slice(&((value.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_be_bytes()),
            Depth::ThirtyTwo => out.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn hierarchy(&mut self, bounds: IntRect, layer: &Layer, mask: bool) -> Result<()> {
        let w = bounds.width() as u32;
        let h = bounds.height() as u32;
        let components = if mask { 1 } else { 4 };
        let bpp = components
            * match self.depth {
                Depth::Eight => 1,
                Depth::Sixteen => 2,
                Depth::ThirtyTwo => 4,
            };
        layered::budget(&mut self.remaining, layered::size(w, h, bpp)?)?;
        self.word(w);
        self.word(h);
        self.word(bpp as u32);
        let level = self.pointer();
        self.pointer();
        self.patch(level);
        self.word(w);
        self.word(h);
        let count = (w.div_ceil(64) * h.div_ceil(64)) as usize;
        let pointers: Vec<_> = (0..count).map(|_| self.pointer()).collect();
        self.pointer();
        for (i, pointer) in pointers.into_iter().enumerate() {
            self.patch(pointer);
            let tx = i as u32 % w.div_ceil(64) * 64;
            let ty = i as u32 / w.div_ceil(64) * 64;
            let mut raw = Vec::new();
            for y in ty..(ty + 64).min(h) {
                for x in tx..(tx + 64).min(w) {
                    let x = bounds.left + x as i32;
                    let y = bounds.top + y as i32;
                    if mask {
                        self.sample(
                            &mut raw,
                            layer.mask.as_ref().unwrap().value(x, y) as f32 / 255.0,
                        );
                    } else {
                        let px = layer.as_raster().unwrap().tiles.pixel(x, y);
                        for v in [px.r, px.g, px.b, px.a] {
                            self.sample(&mut raw, v);
                        }
                    }
                }
            }
            self.bytes
                .extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
        }
        Ok(())
    }

    fn layer(&mut self, layer: &Layer, path: &[u32], canvas: IntRect, active: bool) -> Result<()> {
        layered::check_layer(layer, true, true)?;
        let bounds = if let Some(r) = layer.as_raster() {
            r.tiles.content_bounds()
        } else {
            layer.content_bounds()
        };
        // A mask can extend beyond today's paint. Keep that coverage so
        // painting into an empty part of the layer after reopening behaves
        // exactly as it did before saving.
        let bounds = layer
            .mask
            .as_ref()
            .map_or(bounds, |mask| bounds.union(&mask.bounds));
        let bounds = if bounds.is_empty() { canvas } else { bounds };
        layered::rect(
            bounds.left,
            bounds.top,
            bounds.width() as u32,
            bounds.height() as u32,
        )?;
        self.word(bounds.width() as u32);
        self.word(bounds.height() as u32);
        self.word(1);
        self.string(&layer.name);
        self.int_prop(6, (layer.opacity * layer.fill_opacity * 255.0 + 0.5) as u32);
        self.int_prop(33, (layer.opacity * layer.fill_opacity).to_bits());
        self.int_prop(8, layer.visible as u32);
        self.int_prop(28, layer.locked as u32);
        self.int_prop(7, blend_id(layer.blend)?);
        self.int_prop(35, 1);
        self.int_prop(36, 2);
        self.int_prop(37, 2);
        let mut offsets = bounds.left.to_be_bytes().to_vec();
        offsets.extend_from_slice(&bounds.top.to_be_bytes());
        self.prop(15, &offsets);
        if active {
            self.prop(2, &[]);
        }
        if path.len() > 1 {
            self.prop(
                30,
                &path
                    .iter()
                    .flat_map(|p| p.to_be_bytes())
                    .collect::<Vec<_>>(),
            );
        }
        if let LayerKind::Group(g) = &layer.kind {
            self.prop(29, &[]);
            self.int_prop(31, g.open as u32);
        }
        if let Some(m) = &layer.mask {
            self.int_prop(11, m.enabled as u32);
        }
        self.prop(0, &[]);
        let hierarchy = self.pointer();
        let mask = self.pointer();
        if !layer.is_group() {
            self.patch(hierarchy);
            self.hierarchy(bounds, layer, false)?;
        }
        if layer.mask.is_some() {
            self.patch(mask);
            self.word(bounds.width() as u32);
            self.word(bounds.height() as u32);
            self.string(&layer.name);
            self.prop(0, &[]);
            let pointer = self.pointer();
            self.patch(pointer);
            self.hierarchy(bounds, layer, true)?;
        }
        Ok(())
    }
}

fn write(doc: &Document) -> Result<Vec<u8>> {
    ensure!(
        doc.mode == ColorMode::Rgb,
        "{}",
        t("codec.layered.export_mode")
    );
    layered::size(doc.width, doc.height, 16)?;
    fn flatten<'a>(
        layers: &'a [Layer],
        path: &mut Vec<u32>,
        out: &mut Vec<(&'a Layer, Vec<u32>)>,
    ) -> Result<()> {
        ensure!(path.len() < 64, "{}", t("codec.layered.too_large"));
        for (i, layer) in layers.iter().rev().enumerate() {
            ensure!(out.len() < MAX_LAYERS, "{}", t("codec.layered.too_large"));
            path.push(i as u32);
            out.push((layer, path.clone()));
            if let Some(children) = layer.children() {
                flatten(children, path, out)?;
            }
            path.pop();
        }
        Ok(())
    }
    let mut layers = Vec::new();
    flatten(&doc.tree.layers, &mut Vec::new(), &mut layers)?;
    let mut w = Writer {
        bytes: b"gimp xcf v012\0".to_vec(),
        depth: doc.depth,
        remaining: MAX_BYTES,
    };
    w.word(doc.width);
    w.word(doc.height);
    w.word(0);
    w.word(match doc.depth {
        Depth::Eight => 150,
        Depth::Sixteen => 250,
        Depth::ThirtyTwo => 650,
    });
    w.prop(17, &[2]);
    let dpi = doc.resolution_dpi.to_bits().to_be_bytes();
    w.prop(19, &[dpi, dpi].concat());
    if let Some(icc) = &doc.icc_profile {
        let mut parasite = Writer {
            bytes: Vec::new(),
            depth: doc.depth,
            remaining: MAX_BYTES,
        };
        parasite.string("icc-profile");
        parasite.word(1);
        parasite.word(icc.len() as u32);
        parasite.bytes.extend_from_slice(icc);
        w.prop(21, &parasite.bytes);
    }
    w.prop(0, &[]);
    let pointers: Vec<_> = (0..layers.len()).map(|_| w.pointer()).collect();
    w.pointer();
    w.pointer();
    for ((layer, path), pointer) in layers.into_iter().zip(pointers) {
        w.patch(pointer);
        w.layer(
            layer,
            &path,
            doc.canvas_rect(),
            doc.active_layer == Some(layer.id),
        )?;
    }
    Ok(w.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_literal_and_repeat_runs_are_planar() {
        let bytes = [253, 1, 2, 3, 2, 8, 128, 0, 3, 4, 5, 6];
        assert_eq!(
            decode_rle(&mut Reader::new(&bytes), 3, 3).unwrap(),
            [1, 8, 4, 2, 8, 5, 3, 8, 6]
        );
        assert_eq!(
            decode_rle(&mut Reader::new(&[127, 0, 3, 9]), 3, 1).unwrap(),
            [9, 9, 9]
        );
    }

    #[test]
    fn rle_cannot_overrun_a_plane_or_make_zero_progress() {
        for bytes in [
            &[3, 1][..],
            &[128, 0, 0][..],
            &[127, 0, 0, 1][..],
            &[253, 1][..],
        ] {
            assert!(decode_rle(&mut Reader::new(bytes), 3, 1).is_err());
        }
    }

    #[test]
    fn raw_tiles_follow_32_bit_offsets() {
        let mut bytes = Vec::new();
        // Padding, hierarchy at 4, level at 24, tile at 40.
        for word in [0u32, 1, 1, 3, 24, 0, 1, 1, 40, 0] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes.extend_from_slice(&[1, 2, 3]);
        let mut decoder = Decoder {
            bytes: &bytes,
            wide: false,
            precision: Precision::parse(0, 150).unwrap(),
            compression: 0,
            palette: Vec::new(),
            remaining: MAX_BYTES,
        };
        assert_eq!(
            decoder.pixels(4, 1, 1, 3).unwrap(),
            vec![1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0]
        );
    }
}
