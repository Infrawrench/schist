//! OpenEXR import and export through the pure-Rust `exr` crate.
//!
//! EXR holds scene-linear light. Imports become 32-bit documents tagged
//! with a linear profile built from the file's `chromaticities` (linear
//! Rec. 709 when absent), so the numbers are kept exactly and the display
//! transform does the encoding. Every EXR layer — a part, or a channel
//! prefix such as `diffuse.R` — becomes a Schist layer; the unprefixed
//! beauty pass is the visible base and the others are stacked above it,
//! hidden, since render passes are not meant to be composited over one
//! another.
//!
//! Exports write one scanline part: the flattened image as the unprefixed
//! `R`, `G`, `B` (and `A`) channels, plus, when layered, each visible
//! pixel layer under its own name. A `schistLayers` attribute records the
//! layer order so Schist can rebuild the stack on the way back in; other
//! applications see an ordinary multi-layer EXR.

use crate::layered;
use anyhow::{anyhow, ensure, Context as _, Result};
use exr::prelude::{
    f16, AnyChannel, AnyChannels, AttributeValue, Encoding, FlatSamples, Image, ImageAttributes,
    IntegerBounds, Layer as ExrLayer, LayerAttributes, LineOrder, MetaData, ReadChannels as _,
    ReadLayers as _, SmallVec, Text, Vec2, WritableImage as _,
};
use exr::prelude::{Blocks, Compression};
use schist_color::{ColorMode, Depth};
use schist_core::{blit_rgba_f32, Document, IntRect, Layer, LayerKind};
use schist_i18n::{t, tf};
use schist_plugin_api::{
    CodecPlugin, ExportOptions, ExrCompression, ExrExportOptions, PluginManifest, PluginRegistry,
};
use std::io::Cursor;

/// The four magic bytes every OpenEXR file starts with.
const MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];

/// Records the exported layer order; see the module docs.
const LAYER_ORDER_ATTRIBUTE: &str = "schistLayers";

/// Ceiling on decoded sample storage (the file's samples as f32 plus the
/// document tiles they become). Large enough for 8K multi-pass renders,
/// small enough that a forged header cannot take the machine down.
#[cfg(target_pointer_width = "64")]
const MAX_DECODED_BYTES: usize = 6 << 30;
#[cfg(not(target_pointer_width = "64"))]
const MAX_DECODED_BYTES: usize = 1 << 30;

/// Longest side accepted, matching the other layered readers.
const MAX_EDGE: usize = 1_000_000;

pub struct ExrCodec;

impl CodecPlugin for ExrCodec {
    fn id(&self) -> &'static str {
        "codec.exr"
    }
    fn name(&self) -> &'static str {
        t("codec.exr.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["exr"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(&MAGIC)
    }
    fn import(&self, bytes: &[u8]) -> Result<Document> {
        read(bytes, t("codec.exr.name"))
    }
    fn can_export(&self) -> bool {
        true
    }
    /// Half and float. Declared so switching the export dialog to OpenEXR
    /// keeps a 32-bit choice; the dialog offers them in its own rows.
    fn bit_depths(&self) -> &'static [u8] {
        &[16, 32]
    }
    fn export(&self, doc: &Document) -> Result<Vec<u8>> {
        // Half float unless asked otherwise: what compositing packages
        // write by default, and plenty for colour.
        write(doc, 16, &ExrExportOptions::default())
    }
    fn export_with(&self, doc: &Document, options: &ExportOptions) -> Result<Vec<u8>> {
        write(doc, options.bit_depth, &options.exr)
    }
}

/// Registers only the EXR codec, for hosts that want it on its own.
pub struct ExrPlugin;

impl PluginManifest for ExrPlugin {
    fn id(&self) -> &'static str {
        "schist.codec-exr"
    }
    fn register(&self, registry: &mut PluginRegistry) {
        registry.register_codec(Box::new(ExrCodec));
    }
}

fn decode_error(err: exr::error::Error) -> anyhow::Error {
    match err {
        exr::error::Error::NotSupported(what) => {
            anyhow!("{}", tf!("codec.layered.unsupported", feature = what))
        }
        other => anyhow!("{}: {other}", t("codec.layered.invalid")),
    }
}

fn text_to_string(text: &Text) -> String {
    // OpenEXR text is bytes; writers use UTF-8 in practice, older ones
    // Latin-1.
    match std::str::from_utf8(text.bytes()) {
        Ok(s) => s.to_string(),
        Err(_) => text.chars().collect(),
    }
}

fn text(s: &str) -> Text {
    Text::from_slice_unchecked(s.as_bytes())
}

/// Which colour role a channel's base name plays.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    R,
    G,
    B,
    A,
    Y,
    Other,
}

fn role(base: &str) -> Role {
    match base.to_ascii_lowercase().as_str() {
        "r" | "red" => Role::R,
        "g" | "green" => Role::G,
        "b" | "blue" => Role::B,
        "a" | "alpha" => Role::A,
        "y" | "l" | "luminance" => Role::Y,
        _ => Role::Other,
    }
}

/// The channels of one EXR layer, by index into its part's channel list.
#[derive(Default)]
struct Group {
    name: String,
    r: Option<usize>,
    g: Option<usize>,
    b: Option<usize>,
    a: Option<usize>,
    y: Option<usize>,
    other: Vec<usize>,
}

impl Group {
    /// Source channel for each of R, G, B, A. A luminance-only layer
    /// fills all three colour channels; a layer of data channels
    /// (normals, depth, UVs) maps its first three onto RGB in file order.
    fn mapping(&self) -> [Option<usize>; 4] {
        if self.r.is_some() || self.g.is_some() || self.b.is_some() {
            [self.r, self.g, self.b, self.a]
        } else if let (Some(y), true) = (self.y, self.other.is_empty()) {
            [Some(y), Some(y), Some(y), self.a]
        } else {
            // `Y` beside other data channels is a coordinate (`N.X`,
            // `N.Y`, `N.Z`), not luminance.
            let mut other = self.other.clone();
            other.extend(self.y);
            other.sort_unstable();
            match other.as_slice() {
                [only] => [Some(*only), Some(*only), Some(*only), self.a],
                rest => [
                    rest.first().copied(),
                    rest.get(1).copied(),
                    rest.get(2).copied(),
                    self.a,
                ],
            }
        }
    }
}

fn group_channels<'a>(names: impl Iterator<Item = &'a Text>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (index, name) in names.enumerate() {
        let name = text_to_string(name);
        let (prefix, base) = name.rsplit_once('.').unwrap_or(("", &name));
        let pos = match groups.iter().position(|g| g.name == prefix) {
            Some(pos) => pos,
            None => {
                groups.push(Group {
                    name: prefix.to_string(),
                    ..Default::default()
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[pos];
        let slot = match role(base) {
            Role::R => &mut group.r,
            Role::G => &mut group.g,
            Role::B => &mut group.b,
            Role::A => &mut group.a,
            Role::Y => &mut group.y,
            Role::Other => {
                group.other.push(index);
                continue;
            }
        };
        if slot.is_none() {
            *slot = Some(index);
        } else {
            group.other.push(index);
        }
    }
    groups
}

/// Check sizes from the headers before anything is decoded.
fn check_headers(meta: &MetaData) -> Result<()> {
    ensure!(!meta.headers.is_empty(), "{}", t("codec.layered.invalid"));
    ensure!(
        meta.headers.len() <= layered::MAX_LAYERS,
        "{}",
        t("codec.layered.too_large")
    );
    let mut remaining = MAX_DECODED_BYTES;
    for header in &meta.headers {
        if header.deep {
            return Err(layered::unsupported(t("codec.exr.feature.deep")));
        }
        if header
            .channels
            .list
            .iter()
            .any(|c| c.sampling != Vec2(1, 1))
        {
            return Err(layered::unsupported(t("codec.exr.feature.subsampled")));
        }
        let Vec2(w, h) = header.layer_size;
        ensure!(w > 0 && h > 0, "{}", t("codec.msg.zero_sized"));
        ensure!(
            w <= MAX_EDGE && h <= MAX_EDGE,
            "{}",
            t("codec.layered.too_large")
        );
        let pixels = w.checked_mul(h).ok_or_else(layered::invalid)?;
        let channels = header.channels.list.len();
        // Decoded samples (at most four bytes each), then the f32 RGBA
        // tiles each layer becomes.
        let groups = group_channels(header.channels.list.iter().map(|c| &c.name)).len();
        let bytes = pixels
            .checked_mul(channels * 4 + groups * 16)
            .ok_or_else(layered::invalid)?;
        layered::budget(&mut remaining, bytes)?;
    }
    let display = meta.headers[0].shared_attributes.display_window;
    let Vec2(w, h) = display.size;
    ensure!(w > 0 && h > 0, "{}", t("codec.msg.zero_sized"));
    ensure!(
        w <= MAX_EDGE && h <= MAX_EDGE,
        "{}",
        t("codec.layered.too_large")
    );
    Ok(())
}

/// A sample as f32 that the compositor can blend: NaN becomes 0 and
/// infinities the largest half-float magnitude.
fn finite(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-65504.0, 65504.0)
    }
}

fn read(bytes: &[u8], title: &str) -> Result<Document> {
    let meta = MetaData::read_from_buffered(Cursor::new(bytes), false).map_err(decode_error)?;
    check_headers(&meta)?;

    let image = exr::prelude::read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .all_layers()
        .all_attributes()
        .non_parallel()
        .from_buffered(Cursor::new(bytes))
        .map_err(decode_error)?;

    let display = image.attributes.display_window;
    let (w, h) = (display.size.0 as u32, display.size.1 as u32);
    let mut doc = Document::new(title, w, h, Depth::ThirtyTwo);
    let chroma = image
        .attributes
        .chromaticities
        .map(|c| schist_colormgmt::Chromaticities {
            red: [c.red.0, c.red.1],
            green: [c.green.0, c.green.1],
            blue: [c.blue.0, c.blue.1],
            white: [c.white.0, c.white.1],
        })
        .unwrap_or(schist_colormgmt::Chromaticities::REC709);
    doc.icc_profile = Some(
        schist_colormgmt::linear_profile(&chroma)
            .or_else(|err| {
                log::warn!("EXR chromaticities unusable ({err:#}); assuming Rec. 709");
                schist_colormgmt::linear_profile(&schist_colormgmt::Chromaticities::REC709)
            })
            .context("building the linear profile")?,
    );

    // Layers Schist itself wrote, in stacking order.
    let schist_order: Option<Vec<String>> = image
        .attributes
        .other
        .get(&text(LAYER_ORDER_ATTRIBUTE))
        .or_else(|| {
            image
                .layer_data
                .first()
                .and_then(|l| l.attributes.other.get(&text(LAYER_ORDER_ATTRIBUTE)))
        })
        .and_then(|v| match v {
            AttributeValue::TextVector(names) => Some(names.iter().map(text_to_string).collect()),
            _ => None,
        });

    struct Built {
        name: String,
        beauty: bool,
        layer: Layer,
    }
    let mut built: Vec<Built> = Vec::new();
    for part in &image.layer_data {
        if let Some(density) = part.attributes.horizontal_density {
            if density.is_finite() && density > 0.0 {
                doc.resolution_dpi = density;
            }
        }
        let part_name = part
            .attributes
            .layer_name
            .as_ref()
            .map(text_to_string)
            .unwrap_or_default();
        let channels = &part.channel_data.list;
        let Vec2(lw, lh) = part.size;
        let origin = part.attributes.layer_position;
        let rect = IntRect::from_xywh(
            origin.0 - display.position.0,
            origin.1 - display.position.1,
            lw as u32,
            lh as u32,
        );
        for group in group_channels(channels.iter().map(|c| &c.name)) {
            let name = match (part_name.is_empty(), group.name.is_empty()) {
                (true, _) => group.name.clone(),
                (false, true) => part_name.clone(),
                (false, false) => format!("{part_name}.{}", group.name),
            };
            let [r, g, b, a] = group.mapping();
            let mut rgba = vec![0.0f32; lw * lh * 4];
            for (slot, source) in [r, g, b].into_iter().enumerate() {
                if let Some(index) = source {
                    for (px, v) in rgba
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(channels[index].sample_data.values_as_f32())
                    {
                        px[slot] = finite(v);
                    }
                }
            }
            match a {
                Some(index) => {
                    for (px, v) in rgba
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(channels[index].sample_data.values_as_f32())
                    {
                        // EXR colour is premultiplied by alpha; Schist
                        // stores it straight.
                        let alpha = finite(v);
                        if alpha > 0.0 {
                            for c in &mut px[..3] {
                                *c /= alpha;
                            }
                        }
                        px[3] = alpha.clamp(0.0, 1.0);
                    }
                }
                None => rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .for_each(|px| px[3] = 1.0),
            }
            let mut layer = Layer::new_raster(if name.is_empty() {
                t("common.background_layer").to_string()
            } else {
                name.clone()
            });
            blit_rgba_f32(
                &mut layer.as_raster_mut().unwrap().tiles,
                Depth::ThirtyTwo,
                rect,
                &rgba,
            );
            built.push(Built {
                beauty: name.is_empty(),
                name,
                layer,
            });
        }
    }
    ensure!(!built.is_empty(), "{}", t("codec.exr.msg.no_channels"));
    ensure!(
        built.len() <= layered::MAX_LAYERS,
        "{}",
        t("codec.layered.too_large")
    );

    if let Some(order) = schist_order {
        // Schist's own export: the beauty is just the flattened stack, so
        // rebuild the stack instead and drop it.
        let mut rest: Vec<Built> = built.into_iter().filter(|b| !b.beauty).collect();
        for name in &order {
            if let Some(pos) = rest.iter().position(|b| &b.name == name) {
                doc.push_layer(rest.remove(pos).layer);
            }
        }
        // Anything another tool added afterwards stays, hidden.
        for mut extra in rest {
            extra.layer.visible = false;
            doc.push_layer(extra.layer);
        }
        if !doc.tree.layers.is_empty() {
            return Ok(layered::finish(doc));
        }
        anyhow::bail!("{}", t("codec.exr.msg.no_channels"));
    }

    // The beauty (unnamed) layer is the base, else the first one.
    let base = built.iter().position(|b| b.beauty).unwrap_or(0);
    let base = built.remove(base);
    doc.push_layer(base.layer);
    for mut pass in built {
        pass.layer.visible = false;
        doc.push_layer(pass.layer);
    }
    // The base stays the active layer: it is the one being shown.
    let mut doc = layered::finish(doc);
    doc.active_layer = doc.tree.layers.first().map(|l| l.id);
    Ok(doc)
}

fn compression(c: ExrCompression) -> Compression {
    match c {
        ExrCompression::None => Compression::Uncompressed,
        ExrCompression::Rle => Compression::RLE,
        ExrCompression::Zips => Compression::ZIP1,
        ExrCompression::Zip => Compression::ZIP16,
        ExrCompression::Piz => Compression::PIZ,
        ExrCompression::Pxr24 => Compression::PXR24,
        ExrCompression::B44 => Compression::B44,
        ExrCompression::B44a => Compression::B44A,
    }
}

/// Composite `doc` to scene-linear, premultiplied RGBA over its canvas.
fn linear_pixels(doc: &Document) -> Result<Vec<f32>> {
    let mut pixels = schist_compositor::composite_region_f32(doc, doc.canvas_rect());
    // The compositor hands back RGB for every mode. Only an RGB
    // document's profile describes those numbers; the others come out of
    // their native conversion as sRGB.
    let icc = match doc.mode {
        ColorMode::Rgb => doc.icc_profile.as_deref(),
        _ => None,
    };
    schist_colormgmt::to_scene_linear(&mut pixels, icc)?;
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3].clamp(0.0, 1.0);
        for c in &mut px[..3] {
            *c = finite(*c) * a;
        }
        px[3] = a;
    }
    Ok(pixels)
}

/// The chromaticities `linear_pixels` produced.
fn output_chromaticities(doc: &Document) -> schist_colormgmt::Chromaticities {
    let icc = match doc.mode {
        ColorMode::Rgb => doc.icc_profile.as_deref(),
        _ => None,
    };
    icc.and_then(schist_colormgmt::profile_chromaticities)
        .map(|(c, _)| c)
        .unwrap_or(schist_colormgmt::Chromaticities::REC709)
}

/// Every layer that would show if composited on its own: pixel-bearing,
/// visible, and with every enclosing group visible. Bottom to top.
fn visible_pixel_layers(layers: &[Layer], out: &mut Vec<Layer>) {
    for layer in layers {
        if !layer.visible {
            continue;
        }
        match &layer.kind {
            LayerKind::Group(group) => visible_pixel_layers(&group.children, out),
            LayerKind::Adjustment(_) => {}
            LayerKind::Raster(_) => out.push(layer.clone()),
        }
    }
}

/// A layer's name as an EXR channel prefix: no empty names, no NUL, not
/// beyond the 255-byte attribute limit once `.R` is appended, unique.
fn channel_prefix(name: &str, taken: &mut Vec<String>) -> String {
    let mut base: String = name.chars().filter(|c| *c != '\0').collect();
    let base_trimmed = base.trim();
    if base_trimmed.is_empty() {
        base = t("common.layer").to_string();
    } else {
        base = base_trimmed.to_string();
    }
    while base.len() > 200 {
        base.pop();
    }
    let mut candidate = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t == &candidate) {
        candidate = format!("{base} {n}");
        n += 1;
    }
    taken.push(candidate.clone());
    candidate
}

fn channels_for(
    prefix: &str,
    pixels: &[f32],
    float: bool,
    alpha: bool,
) -> Vec<AnyChannel<FlatSamples>> {
    let plane = |c: usize| -> FlatSamples {
        let values = pixels.as_chunks::<4>().0.iter().map(|px| px[c]);
        if float {
            FlatSamples::F32(values.collect())
        } else {
            FlatSamples::F16(values.map(f16::from_f32).collect())
        }
    };
    let name = |base: &str| {
        if prefix.is_empty() {
            text(base)
        } else {
            text(&format!("{prefix}.{base}"))
        }
    };
    let mut out = vec![
        AnyChannel::new(name("R"), plane(0)),
        AnyChannel::new(name("G"), plane(1)),
        AnyChannel::new(name("B"), plane(2)),
    ];
    if alpha {
        out.push(AnyChannel::new(name("A"), plane(3)));
    }
    out
}

fn write(doc: &Document, bit_depth: u8, options: &ExrExportOptions) -> Result<Vec<u8>> {
    let canvas = doc.canvas_rect();
    ensure!(!canvas.is_empty(), "{}", t("codec.msg.zero_sized"));
    let float = bit_depth >= 32;
    let chroma = output_chromaticities(doc);

    let mut channels = channels_for("", &linear_pixels(doc)?, float, options.alpha);
    let mut order = Vec::new();
    if options.layered {
        let mut layers = Vec::new();
        visible_pixel_layers(&doc.tree.layers, &mut layers);
        for mut layer in layers {
            let prefix = channel_prefix(&layer.name, &mut order);
            // Each layer as it would look alone: masks, effects and
            // opacity applied; blend mode and clipping are relations to
            // what is below, which this layer no longer has.
            layer.blend = schist_core::BlendMode::Normal;
            layer.clipping = false;
            let mut solo = Document::new(&doc.title, doc.width, doc.height, doc.depth);
            solo.mode = doc.mode;
            solo.icc_profile = doc.icc_profile.clone();
            solo.push_layer(layer);
            let mut damage = Vec::new();
            schist_compositor::restyle_layers(&mut solo.tree.layers, &mut damage);
            channels.extend(channels_for(
                &prefix,
                &linear_pixels(&solo)?,
                float,
                options.alpha,
            ));
        }
    }

    let size = Vec2(canvas.width() as usize, canvas.height() as usize);
    let mut attributes = LayerAttributes::default();
    if doc.resolution_dpi.is_finite() && doc.resolution_dpi > 0.0 {
        attributes.horizontal_density = Some(doc.resolution_dpi);
    }
    attributes.software_name = Some(text("Schist"));
    let layer = ExrLayer::new(
        size,
        attributes,
        Encoding {
            compression: compression(options.compression),
            blocks: Blocks::ScanLines,
            line_order: LineOrder::Increasing,
        },
        AnyChannels::sort(SmallVec::from_vec(channels)),
    );
    let mut image = Image::from_layer(layer);
    image.attributes = ImageAttributes::new(IntegerBounds::from_dimensions(size));
    image.attributes.chromaticities = Some(exr::meta::attribute::Chromaticities {
        red: Vec2(chroma.red[0], chroma.red[1]),
        green: Vec2(chroma.green[0], chroma.green[1]),
        blue: Vec2(chroma.blue[0], chroma.blue[1]),
        white: Vec2(chroma.white[0], chroma.white[1]),
    });
    if options.layered {
        image.attributes.other.insert(
            text(LAYER_ORDER_ATTRIBUTE),
            AttributeValue::TextVector(order.iter().map(|n| text(n)).collect()),
        );
    }

    let mut out = Cursor::new(Vec::new());
    image
        .write()
        .non_parallel()
        .to_buffered(&mut out)
        .map_err(|e| anyhow!("{}", tf!("codec.exr.msg.write_failed", error = e)))?;
    Ok(out.into_inner())
}

/// The channel names and sample types of a written file's first part.
#[cfg(test)]
pub(crate) fn sample_types(bytes: &[u8]) -> Vec<(String, exr::prelude::SampleType)> {
    let meta = MetaData::read_from_buffered(Cursor::new(bytes), false).unwrap();
    meta.headers[0]
        .channels
        .list
        .iter()
        .map(|c| (text_to_string(&c.name), c.sample_type))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use exr::prelude::SampleType;

    fn pixel(doc: &Document, layer: usize, x: i32, y: i32) -> [f32; 4] {
        let p = doc.tree.layers[layer]
            .as_raster()
            .unwrap()
            .tiles
            .pixel(x, y);
        [p.r, p.g, p.b, p.a]
    }

    fn close(a: [f32; 4], b: [f32; 4], tolerance: f32) -> bool {
        a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= tolerance * y.abs().max(1.0))
    }

    /// A 32-bit, linear Rec. 709 document with HDR values and a
    /// half-transparent corner.
    fn hdr_document(w: u32, h: u32) -> Document {
        let mut doc = Document::new("hdr", w, h, Depth::ThirtyTwo);
        doc.icc_profile = Some(
            schist_colormgmt::linear_profile(&schist_colormgmt::Chromaticities::REC709).unwrap(),
        );
        let mut layer = Layer::new_raster("Background");
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if x == 0 && y == 0 { 0.5 } else { 1.0 };
                rgba.extend([x as f32 * 0.75, y as f32 * 0.125, 4.0, a]);
            }
        }
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            IntRect::from_size(w, h),
            &rgba,
        );
        doc.push_layer(layer);
        doc
    }

    macro_rules! encode {
        ($image:expr) => {{
            let image = $image;
            let mut out = Cursor::new(Vec::new());
            image.write().non_parallel().to_buffered(&mut out).unwrap();
            out.into_inner()
        }};
    }

    fn channel(name: &str, values: Vec<f32>) -> AnyChannel<FlatSamples> {
        AnyChannel::new(name, FlatSamples::F32(values))
    }

    #[test]
    fn probe_and_registry() {
        let doc = hdr_document(4, 4);
        let bytes = ExrCodec.export(&doc).unwrap();
        assert!(ExrCodec.probe(&bytes));
        assert!(!ExrCodec.probe(b"\x89PNG"));
        let mut reg = PluginRegistry::new();
        crate::CommonCodecsPlugin.register(&mut reg);
        assert_eq!(reg.codec_for(&bytes, None).unwrap().id(), "codec.exr");
        assert_eq!(reg.codec_for(b"", Some("exr")).unwrap().id(), "codec.exr");
    }

    #[test]
    fn round_trips_every_compression_in_half_and_float() {
        let doc = hdr_document(37, 21);
        for compression in ExrCompression::ALL {
            for bit_depth in [16, 32] {
                let options = ExportOptions {
                    bit_depth,
                    exr: ExrExportOptions {
                        compression,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let bytes = ExrCodec.export_with(&doc, &options).unwrap();
                let want = if bit_depth == 32 {
                    SampleType::F32
                } else {
                    SampleType::F16
                };
                assert!(
                    sample_types(&bytes).iter().all(|(_, t)| *t == want),
                    "{compression:?}/{bit_depth}"
                );
                let back = ExrCodec.import(&bytes).unwrap();
                assert_eq!(
                    (back.width, back.height, back.depth),
                    (37, 21, Depth::ThirtyTwo)
                );
                let lossy = matches!(
                    compression,
                    ExrCompression::B44 | ExrCompression::B44a | ExrCompression::Pxr24
                );
                let tolerance = match (bit_depth, lossy) {
                    (32, false) => 0.0,
                    (16, false) => 1e-3,
                    // B44 leaves float channels alone; PXR24 keeps 15
                    // mantissa bits of a float.
                    (32, true) => 1e-4,
                    _ => 0.03,
                };
                for (x, y) in [(5, 3), (36, 20), (17, 10)] {
                    let got = pixel(&back, 0, x, y);
                    let want = pixel(&doc, 0, x, y);
                    assert!(
                        close(got, want, tolerance),
                        "{compression:?}/{bit_depth} at {x},{y}: {got:?} vs {want:?}"
                    );
                }
                // Straight alpha survives the premultiplied file.
                let corner = pixel(&back, 0, 0, 0);
                assert!(
                    (corner[3] - 0.5).abs() < 2e-3,
                    "{compression:?}: {corner:?}"
                );
                assert!((corner[2] - 4.0).abs() < 0.1, "{compression:?}: {corner:?}");
            }
        }
    }

    #[test]
    fn float_export_is_exact_and_premultiplied_on_disk() {
        let doc = hdr_document(3, 2);
        let options = ExportOptions {
            bit_depth: 32,
            exr: ExrExportOptions {
                compression: ExrCompression::None,
                ..Default::default()
            },
            ..Default::default()
        };
        let bytes = ExrCodec.export_with(&doc, &options).unwrap();
        let image = exr::prelude::read()
            .no_deep_data()
            .largest_resolution_level()
            .all_channels()
            .first_valid_layer()
            .all_attributes()
            .from_buffered(Cursor::new(&bytes))
            .unwrap();
        let b = image
            .layer_data
            .channel_data
            .list
            .iter()
            .find(|c| c.name.eq("B"))
            .unwrap();
        // (0, 0) is 4.0 at alpha 0.5: stored premultiplied.
        assert_eq!(b.sample_data.value_by_flat_index(0).to_f32(), 2.0);
        assert_eq!(b.sample_data.value_by_flat_index(1).to_f32(), 4.0);
        let chroma = image.attributes.chromaticities.unwrap();
        assert_eq!((chroma.red.0, chroma.red.1), (0.64, 0.33));
    }

    #[test]
    fn no_alpha_option_drops_the_channel() {
        let doc = hdr_document(4, 4);
        let options = ExportOptions {
            exr: ExrExportOptions {
                alpha: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let bytes = ExrCodec.export_with(&doc, &options).unwrap();
        let names: Vec<String> = sample_types(&bytes).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["B", "G", "R"]);
        let back = ExrCodec.import(&bytes).unwrap();
        assert_eq!(pixel(&back, 0, 2, 2)[3], 1.0);
    }

    #[test]
    fn eight_bit_srgb_documents_export_as_linear_light() {
        let mut doc = Document::new("srgb", 2, 1, Depth::Eight);
        let mut layer = Layer::new_raster("Background");
        schist_core::blit_rgba8(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::Eight,
            IntRect::from_size(2, 1),
            &[128, 128, 128, 255, 255, 0, 0, 255],
        );
        doc.push_layer(layer);
        let bytes = ExrCodec.export(&doc).unwrap();
        let back = ExrCodec.import(&bytes).unwrap();
        let grey = pixel(&back, 0, 0, 0);
        // sRGB 128/255 is 0.2158 in linear light.
        assert!((grey[0] - 0.2158).abs() < 2e-3, "{grey:?}");
        assert!(close(pixel(&back, 0, 1, 0), [1.0, 0.0, 0.0, 1.0], 1e-3));
    }

    #[test]
    fn chromaticities_become_a_linear_profile_and_go_back_out() {
        let layer = ExrLayer::new(
            Vec2(2, 2),
            LayerAttributes::default(),
            Encoding::FAST_LOSSLESS,
            AnyChannels::sort(SmallVec::from_vec(vec![
                channel("R", vec![0.25; 4]),
                channel("G", vec![0.5; 4]),
                channel("B", vec![2.0; 4]),
            ])),
        );
        let mut image = Image::from_layer(layer);
        let ap1 = schist_colormgmt::Chromaticities::ACES_AP1;
        image.attributes.chromaticities = Some(exr::meta::attribute::Chromaticities {
            red: Vec2(ap1.red[0], ap1.red[1]),
            green: Vec2(ap1.green[0], ap1.green[1]),
            blue: Vec2(ap1.blue[0], ap1.blue[1]),
            white: Vec2(ap1.white[0], ap1.white[1]),
        });
        let doc = ExrCodec.import(&encode!(image)).unwrap();
        let icc = doc.icc_profile.clone().unwrap();
        assert_eq!(
            schist_colormgmt::profile_chromaticities(&icc),
            Some((ap1, true))
        );
        assert_eq!(pixel(&doc, 0, 1, 1), [0.25, 0.5, 2.0, 1.0]);

        // Exported again, the primaries are written back unchanged and
        // the samples are not converted.
        let options = ExportOptions {
            bit_depth: 32,
            ..Default::default()
        };
        let bytes = ExrCodec.export_with(&doc, &options).unwrap();
        let again = ExrCodec.import(&bytes).unwrap();
        assert_eq!(again.icc_profile.as_deref(), Some(icc.as_slice()));
        assert_eq!(pixel(&again, 0, 0, 0), [0.25, 0.5, 2.0, 1.0]);
    }

    #[test]
    fn data_window_is_placed_inside_the_display_window() {
        // Display window 8x6 at (10, 20); data window 3x2 at (12, 21).
        let layer = ExrLayer::new(
            Vec2(3, 2),
            LayerAttributes {
                layer_position: Vec2(12, 21),
                ..Default::default()
            },
            Encoding::FAST_LOSSLESS,
            AnyChannels::sort(SmallVec::from_vec(vec![
                channel("R", (0..6).map(|v| v as f32).collect()),
                channel("G", vec![0.0; 6]),
                channel("B", vec![0.0; 6]),
            ])),
        );
        let mut image = Image::from_layer(layer);
        image.attributes.display_window = IntegerBounds::new(Vec2(10, 20), Vec2(8, 6));
        let doc = ExrCodec.import(&encode!(image)).unwrap();
        assert_eq!((doc.width, doc.height), (8, 6));
        // Data pixel (0, 0) lands at (2, 1); outside it is transparent.
        assert_eq!(pixel(&doc, 0, 2, 1), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(pixel(&doc, 0, 4, 2), [5.0, 0.0, 0.0, 1.0]);
        assert_eq!(pixel(&doc, 0, 0, 0)[3], 0.0);
        assert_eq!(pixel(&doc, 0, 5, 1)[3], 0.0);
    }

    #[test]
    fn data_outside_the_display_window_is_kept_off_canvas() {
        let layer = ExrLayer::new(
            Vec2(4, 1),
            LayerAttributes {
                layer_position: Vec2(-2, 0),
                ..Default::default()
            },
            Encoding::FAST_LOSSLESS,
            AnyChannels::sort(SmallVec::from_vec(vec![channel(
                "Y",
                vec![1.0, 2.0, 3.0, 4.0],
            )])),
        );
        let mut image = Image::from_layer(layer);
        image.attributes.display_window = IntegerBounds::new(Vec2(0, 0), Vec2(2, 1));
        let doc = ExrCodec.import(&encode!(image)).unwrap();
        assert_eq!((doc.width, doc.height), (2, 1));
        assert_eq!(pixel(&doc, 0, -2, 0), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(pixel(&doc, 0, 1, 0), [4.0, 4.0, 4.0, 1.0]);
    }

    #[test]
    fn luminance_alpha_and_uint_channels() {
        let layer = ExrLayer::new(
            Vec2(2, 1),
            LayerAttributes::default(),
            Encoding::FAST_LOSSLESS,
            AnyChannels::sort(SmallVec::from_vec(vec![
                AnyChannel::new(
                    "Y",
                    FlatSamples::F16(vec![f16::from_f32(0.25), f16::from_f32(3.0)]),
                ),
                AnyChannel::new(
                    "A",
                    FlatSamples::F16(vec![f16::from_f32(0.5), f16::from_f32(1.0)]),
                ),
                AnyChannel::new("id.objectId", FlatSamples::U32(vec![7, 9])),
            ])),
        );
        let doc = ExrCodec.import(&encode!(Image::from_layer(layer))).unwrap();
        assert_eq!(doc.tree.layers.len(), 2);
        // Premultiplied 0.25 at alpha 0.5 is straight 0.5.
        assert_eq!(pixel(&doc, 0, 0, 0), [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(pixel(&doc, 0, 1, 0), [3.0, 3.0, 3.0, 1.0]);
        let ids = &doc.tree.layers[1];
        assert_eq!(ids.name, "id");
        assert!(!ids.visible, "passes stack hidden over the beauty");
        assert_eq!(pixel(&doc, 1, 1, 0), [9.0, 9.0, 9.0, 1.0]);
    }

    #[test]
    fn render_passes_become_named_layers_over_the_beauty() {
        let n = 4;
        let layer = ExrLayer::new(
            Vec2(2, 2),
            LayerAttributes::named("render"),
            Encoding::FAST_LOSSLESS,
            AnyChannels::sort(SmallVec::from_vec(vec![
                channel("R", vec![1.0; n]),
                channel("G", vec![1.0; n]),
                channel("B", vec![1.0; n]),
                channel("diffuse.R", vec![0.5; n]),
                channel("diffuse.G", vec![0.25; n]),
                channel("diffuse.B", vec![0.125; n]),
                channel("N.X", vec![-1.0; n]),
                channel("N.Y", vec![0.0; n]),
                channel("N.Z", vec![1.0; n]),
                channel("depth.Z", vec![42.0; n]),
            ])),
        );
        let doc = ExrCodec.import(&encode!(Image::from_layer(layer))).unwrap();
        let names: Vec<&str> = doc.tree.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            ["render", "render.N", "render.depth", "render.diffuse"]
        );
        assert!(doc.tree.layers[0].visible);
        assert!(doc.tree.layers[1..].iter().all(|l| !l.visible));
        assert_eq!(doc.active_layer, Some(doc.tree.layers[0].id));
        assert_eq!(pixel(&doc, 3, 0, 0), [0.5, 0.25, 0.125, 1.0]);
        assert_eq!(pixel(&doc, 1, 0, 0), [-1.0, 0.0, 1.0, 1.0]);
        assert_eq!(pixel(&doc, 2, 1, 1), [42.0, 42.0, 42.0, 1.0]);
    }

    #[test]
    fn multi_part_files_and_tiles() {
        let size = Vec2(40, 33);
        let rgb = |v: f32| {
            AnyChannels::sort(SmallVec::from_vec(vec![
                channel("R", vec![v; 40 * 33]),
                channel("G", vec![v; 40 * 33]),
                channel("B", vec![v; 40 * 33]),
            ]))
        };
        let tiled = Encoding {
            compression: Compression::PIZ,
            blocks: Blocks::Tiles(Vec2(16, 16)),
            line_order: LineOrder::Increasing,
        };
        let image = Image::empty(ImageAttributes::new(IntegerBounds::from_dimensions(size)))
            .with_layer(ExrLayer::new(
                size,
                LayerAttributes::named("beauty"),
                tiled,
                rgb(0.5),
            ))
            .with_layer(ExrLayer::new(
                size,
                LayerAttributes::named("specular"),
                Encoding::SMALL_LOSSLESS,
                rgb(2.0),
            ));
        let doc = ExrCodec.import(&encode!(image)).unwrap();
        let names: Vec<&str> = doc.tree.layers.iter().map(|l| l.name.as_str()).collect();
        // Neither part is unnamed, so the first is the base.
        assert_eq!(names, ["beauty", "specular"]);
        assert_eq!(pixel(&doc, 0, 39, 32), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(pixel(&doc, 1, 20, 20), [2.0, 2.0, 2.0, 1.0]);
        assert!(!doc.tree.layers[1].visible);
    }

    #[test]
    fn layered_export_round_trips_the_stack() {
        let mut doc = hdr_document(6, 4);
        let mut top = Layer::new_raster("Glow.v2");
        blit_rgba_f32(
            &mut top.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            IntRect::from_xywh(1, 1, 2, 2),
            &[8.0, 4.0, 0.0, 1.0].repeat(4),
        );
        doc.push_layer(top);
        let mut hidden = Layer::new_raster("Hidden");
        blit_rgba_f32(
            &mut hidden.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            IntRect::from_size(6, 4),
            &[1.0, 1.0, 1.0, 1.0].repeat(24),
        );
        hidden.visible = false;
        doc.push_layer(hidden);
        // Same name twice: the second gets a suffix.
        let mut dup = Layer::new_raster("Background");
        dup.opacity = 0.5;
        blit_rgba_f32(
            &mut dup.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            IntRect::from_xywh(5, 3, 1, 1),
            &[0.0, 1.0, 0.0, 1.0],
        );
        doc.push_layer(dup);

        let options = ExportOptions {
            bit_depth: 32,
            exr: ExrExportOptions {
                layered: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let bytes = ExrCodec.export_with(&doc, &options).unwrap();
        let names: Vec<String> = sample_types(&bytes).into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"R".to_string()), "beauty for other readers");
        assert!(names.contains(&"Glow.v2.R".to_string()));
        assert!(names.contains(&"Background 2.A".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("Hidden")));

        let back = ExrCodec.import(&bytes).unwrap();
        let names: Vec<&str> = back.tree.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Background", "Glow.v2", "Background 2"]);
        assert!(back.tree.layers.iter().all(|l| l.visible));
        assert_eq!(pixel(&back, 1, 1, 1), [8.0, 4.0, 0.0, 1.0]);
        assert_eq!(
            pixel(&back, 1, 0, 0)[3],
            0.0,
            "transparent around the layer"
        );
        // Opacity is baked into the exported layer.
        let half = pixel(&back, 2, 5, 3);
        assert!(
            (half[3] - 0.5).abs() < 1e-6 && (half[1] - 1.0).abs() < 1e-6,
            "{half:?}"
        );
        // The rebuilt stack composites like the original.
        let a = schist_compositor::composite_region_f32(&doc, doc.canvas_rect());
        let b = schist_compositor::composite_region_f32(&back, back.canvas_rect());
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-4, "{x} vs {y}");
        }
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(ExrCodec.import(b"v/1\x01garbage").is_err());
        let bytes = ExrCodec.export(&hdr_document(8, 8)).unwrap();
        assert!(ExrCodec.import(&bytes[..bytes.len() / 2]).is_err());
    }
}

/// Files written by the reference OpenEXR library (`fixtures/exr`).
#[cfg(test)]
mod fixture_tests {
    use super::*;

    const W: i32 = 40;
    const H: i32 = 24;

    fn fixture(name: &str) -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/exr")
            .join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(ExrCodec.probe(&bytes));
        ExrCodec
            .import(&bytes)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"))
    }

    /// `expected()` in generate.py.
    fn expected(x: i32, y: i32) -> [f32; 4] {
        [
            x as f32 / (W - 1) as f32 * 4.0,
            y as f32 / (H - 1) as f32,
            0.25,
            if x < W / 2 { 1.0 } else { 0.5 },
        ]
    }

    fn pixel(doc: &Document, layer: usize, x: i32, y: i32) -> [f32; 4] {
        let p = doc.tree.layers[layer]
            .as_raster()
            .unwrap()
            .tiles
            .pixel(x, y);
        [p.r, p.g, p.b, p.a]
    }

    /// Worst absolute error against `expected()` over the canvas, offset
    /// by where the data window sits.
    fn worst_error(doc: &Document, layer: usize) -> f32 {
        let mut worst = 0.0f32;
        for y in 0..H {
            for x in 0..W {
                let got = pixel(doc, layer, x, y);
                let want = expected(x, y);
                for c in 0..4 {
                    worst = worst.max((got[c] - want[c]).abs());
                }
            }
        }
        worst
    }

    #[test]
    fn every_compression_written_by_openexr_reads_back() {
        for (name, tolerance) in [
            ("none", 4e-3),
            ("rle", 4e-3),
            ("zips", 4e-3),
            ("zip", 4e-3),
            ("piz", 4e-3),
            ("pxr24", 4e-3),
            ("b44", 0.08),
            ("b44a", 0.08),
            ("dwaa", 0.08),
            ("dwab", 0.08),
        ] {
            let doc = fixture(&format!("half-{name}.exr"));
            assert_eq!((doc.width, doc.height), (W as u32, H as u32));
            assert_eq!(doc.tree.layers.len(), 1);
            let worst = worst_error(&doc, 0);
            assert!(worst <= tolerance, "{name}: worst error {worst}");
        }
    }

    #[test]
    fn tiled_float() {
        let doc = fixture("float-tiled.exr");
        assert_eq!(worst_error(&doc, 0), 0.0);
    }

    #[test]
    fn windows_and_chromaticities() {
        let doc = fixture("windows-acescg.exr");
        assert_eq!((doc.width, doc.height), (W as u32, H as u32));
        // The 30x20 data window starts at (3, 2) inside the display window.
        assert_eq!(pixel(&doc, 0, 2, 2)[3], 0.0);
        assert_eq!(pixel(&doc, 0, 3, 1)[3], 0.0);
        for (x, y) in [(3, 2), (32, 21), (25, 10)] {
            let got = pixel(&doc, 0, x, y);
            let want = expected(x, y);
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() < 4e-3),
                "{x},{y}: {got:?} vs {want:?}"
            );
        }
        assert_eq!(pixel(&doc, 0, 33, 10)[3], 0.0);
        let (chroma, linear) =
            schist_colormgmt::profile_chromaticities(doc.icc_profile.as_deref().unwrap()).unwrap();
        assert!(linear);
        assert_eq!(chroma, schist_colormgmt::Chromaticities::ACES_AP1);
    }

    #[test]
    fn luminance_alpha() {
        let doc = fixture("luminance-alpha.exr");
        for (x, y) in [(0, 0), (39, 23), (10, 12)] {
            let got = pixel(&doc, 0, x, y);
            let want = expected(x, y);
            assert!((got[0] - want[1]).abs() < 4e-3 && got[0] == got[1] && got[1] == got[2]);
            assert_eq!(got[3], want[3]);
        }
    }

    #[test]
    fn render_passes() {
        let doc = fixture("passes.exr");
        let names: Vec<&str> = doc.tree.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names[0], t("common.background_layer"));
        assert_eq!(&names[1..], ["depth", "diffuse", "id"]);
        assert!(worst_error(&doc, 0) < 4e-3);
        assert_eq!(pixel(&doc, 1, 5, 5), [123.5, 123.5, 123.5, 1.0]);
        assert_eq!(pixel(&doc, 2, 5, 5), [0.5, 0.25, 0.125, 1.0]);
        assert_eq!(pixel(&doc, 3, 5, 5), [7.0, 7.0, 7.0, 1.0]);
        assert!(doc.tree.layers[1..].iter().all(|l| !l.visible));
    }

    #[test]
    fn multi_part() {
        let doc = fixture("multipart.exr");
        let names: Vec<&str> = doc.tree.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["beauty", "specular"]);
        assert!(worst_error(&doc, 0) < 4e-3);
        let spec = pixel(&doc, 1, 7, 7);
        assert!((spec[0] - 2.0).abs() < 0.02, "DWAB pass: {spec:?}");
    }

    /// With `SCHIST_EXR_INTEROP_DIR` set, write Schist's exports there for
    /// `fixtures/exr/verify_exports.py` to check with the reference library.
    #[test]
    fn write_interop_exports() {
        let Some(dir) = std::env::var_os("SCHIST_EXR_INTEROP_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The fixture's own pixels, as a two-layer document: the ramp,
        // and a hidden-in-EXR-terms "glow" pass on top.
        let mut doc = fixture("float-tiled.exr");
        let mut glow = Layer::new_raster("glow");
        blit_rgba_f32(
            &mut glow.as_raster_mut().unwrap().tiles,
            Depth::ThirtyTwo,
            IntRect::from_xywh(4, 4, 8, 8),
            &[8.0, 4.0, 2.0, 1.0].repeat(64),
        );
        doc.push_layer(glow);
        for compression in ExrCompression::ALL {
            for bit_depth in [16u8, 32] {
                let options = ExportOptions {
                    bit_depth,
                    exr: ExrExportOptions {
                        compression,
                        layered: true,
                        alpha: true,
                    },
                    ..Default::default()
                };
                let bytes = ExrCodec.export_with(&doc, &options).unwrap();
                let name = format!("schist-{}-{bit_depth}.exr", compression.id());
                std::fs::write(dir.join(name), bytes).unwrap();
            }
        }
    }
}
