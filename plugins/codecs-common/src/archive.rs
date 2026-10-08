//! Shared pieces of the zip-based layered formats (OpenRaster and Krita).
//!
//! Both are ODF-style packages: a zip whose first entry is a stored
//! `mimetype`, an XML part describing the layer tree, and PNG or tiled
//! pixel payloads beside it. The container itself is the hand-written one
//! in `schist-codec-idml` (see `docs/idml-format.md` for why it is not the
//! `zip` crate); this module adds the limits a pixel-carrying package
//! needs and the bits of PNG and XML handling both formats share.
use anyhow::{ensure, Result};
use schist_codec_idml::container::{self, ContainerError, Package};
use schist_codec_idml::xml::{self, Element};
use schist_color::Depth;
use schist_core::{blit_rgba_f32, IntRect, Layer, RawBlock};
use schist_i18n::t;

use crate::layered::{self, invalid, MAX_BYTES, MAX_LAYERS};

/// Deepest element nesting accepted in a layer tree part. Real stacks are
/// a handful of groups deep; the bound keeps recursion (and the parsed
/// tree's recursive drop) far away from the thread's stack limit.
const MAX_XML_DEPTH: usize = 128;
/// Largest XML part read, before parsing.
const MAX_XML_BYTES: usize = 16 * 1024 * 1024;

/// The payload of a stored `mimetype` first entry, read from the local
/// header alone, as an ODF-style reader is meant to.
pub fn stored_mimetype(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(..4)? != b"PK\x03\x04" {
        return None;
    }
    let u16_at = |at: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
    };
    // Stored, and named `mimetype`.
    if u16_at(8)? != 0 {
        return None;
    }
    let size = u32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?) as usize;
    let (name_len, extra_len) = (u16_at(26)?, u16_at(28)?);
    if bytes.get(30..30 + name_len)? != b"mimetype" || size > 256 {
        return None;
    }
    let start = 30 + name_len + extra_len;
    bytes.get(start..start + size)
}

/// Read a whole package within the decoded-data budget.
pub fn open(bytes: &[u8]) -> Result<Package> {
    container::read_bounded(bytes, MAX_BYTES as u64, MAX_LAYERS * 4 + 256).map_err(|e| match e {
        ContainerError::TooLarge => anyhow::anyhow!("{}", t("codec.layered.too_large")),
        other => {
            log::info!("layered zip: {other}");
            invalid()
        }
    })
}

/// Parse an XML part into a tree, refusing pathological nesting first.
pub fn parse_xml(bytes: &[u8]) -> Result<Element> {
    ensure!(
        bytes.len() <= MAX_XML_BYTES,
        "{}",
        t("codec.layered.too_large")
    );
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(_)) => {
                depth += 1;
                ensure!(depth <= MAX_XML_DEPTH, "{}", t("codec.layered.too_large"));
            }
            Ok(quick_xml::events::Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                log::info!("layered xml: {e}");
                return Err(invalid());
            }
        }
    }
    xml::parse(text).map_err(|e| {
        log::info!("layered xml: {e}");
        invalid()
    })
}

/// Escape text for an XML attribute value.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            // XML 1.0 cannot carry other control characters at all.
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// A decoded PNG (or other raster) payload.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA, 0..=1 for integer sources.
    pub rgba: Vec<f32>,
    /// More than eight bits a channel in the file.
    pub deep: bool,
    pub icc: Option<Vec<u8>>,
}

/// Width and height from a PNG's IHDR, without decoding it.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
        u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
    ))
}

/// Decode a layer payload, charging the decoded size to `remaining`.
pub fn decode(bytes: &[u8], remaining: &mut usize) -> Result<Raster> {
    use image::ImageDecoder as _;
    let format = image::guess_format(bytes).map_err(|_| invalid())?;
    if let Some((w, h)) = png_size(bytes) {
        // Checked before decoding: a 1 kB PNG may claim gigapixels.
        layered::budget(remaining, layered::size(w, h, 16)?)?;
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_BYTES as u64);
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| invalid())?;
    let icc = decoder
        .icc_profile()
        .ok()
        .flatten()
        .filter(|b| !b.is_empty());
    let img = image::DynamicImage::from_decoder(decoder).map_err(|e| {
        log::info!("layered payload: {e}");
        invalid()
    })?;
    let (width, height) = (img.width(), img.height());
    if format != image::ImageFormat::Png {
        layered::budget(remaining, layered::size(width, height, 16)?)?;
    }
    layered::size(width, height, 16)?;
    let deep = !matches!(
        img.color(),
        image::ColorType::L8
            | image::ColorType::La8
            | image::ColorType::Rgb8
            | image::ColorType::Rgba8
    );
    let rgba = if deep {
        img.to_rgba32f().into_raw()
    } else {
        img.to_rgba8()
            .into_raw()
            .into_iter()
            .map(|v| v as f32 / 255.0)
            .collect()
    };
    Ok(Raster {
        width,
        height,
        rgba,
        deep,
        icc,
    })
}

/// A raster layer holding `raster` with its top-left corner at `(x, y)`.
pub fn raster_layer(
    name: impl Into<String>,
    raster: &Raster,
    x: i32,
    y: i32,
    depth: Depth,
    remaining: &mut usize,
) -> Result<Layer> {
    let bounds = layered::rect(x, y, raster.width, raster.height)?;
    layered::budget(remaining, layered::tile_bytes(bounds, depth, false))?;
    let mut layer = Layer::new_raster(name);
    blit_rgba_f32(
        &mut layer.as_raster_mut().unwrap().tiles,
        depth,
        bounds,
        &raster.rgba,
    );
    Ok(layer)
}

/// The visible note layer used when part of a file could not be imported:
/// the file's own flattened render, scaled to the canvas if it is not
/// canvas-sized, so the document still looks the way the author saw it.
pub fn merged_layer(
    name: &str,
    merged: &Raster,
    width: u32,
    height: u32,
    depth: Depth,
    remaining: &mut usize,
) -> Result<Layer> {
    if merged.width == width && merged.height == height {
        return raster_layer(name, merged, 0, 0, depth, remaining);
    }
    let image = image::Rgba32FImage::from_raw(merged.width, merged.height, merged.rgba.clone())
        .ok_or_else(invalid)?;
    let scaled =
        image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle);
    let raster = Raster {
        width,
        height,
        rgba: scaled.into_raw(),
        deep: merged.deep,
        icc: None,
    };
    raster_layer(name, &raster, 0, 0, depth, remaining)
}

/// Encode straight-alpha RGBA as a PNG, 8 or 16 bits a channel.
pub fn encode_png(w: u32, h: u32, rgba: &[f32], deep: bool, icc: Option<&[u8]>) -> Result<Vec<u8>> {
    use image::ImageEncoder as _;
    let mut out = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut out);
    if let Some(icc) = icc {
        let _ = encoder.set_icc_profile(icc.to_vec());
    }
    if deep {
        let raw: Vec<u8> = rgba
            .iter()
            .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_ne_bytes())
            .collect();
        encoder.write_image(&raw, w, h, image::ExtendedColorType::Rgba16)?;
    } else {
        let raw: Vec<u8> = rgba
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
            .collect();
        encoder.write_image(&raw, w, h, image::ExtendedColorType::Rgba8)?;
    }
    Ok(out)
}

/// A layer's straight-alpha pixels over `bounds`.
pub fn layer_pixels(layer: &Layer, bounds: IntRect) -> Vec<f32> {
    let tiles = &layer.as_raster().expect("raster layer").tiles;
    let mut out = Vec::with_capacity(bounds.width() as usize * bounds.height() as usize * 4);
    for y in bounds.top..bounds.bottom {
        for x in bounds.left..bounds.right {
            let p = tiles.pixel(x, y);
            out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
        }
    }
    out
}

/// Attributes another application wrote that Schist does not interpret,
/// kept on the layer so that saving back to the same format re-emits
/// them. The block rides through PSD saves like Schist's other private
/// blocks.
pub fn preserve(layer: &mut Layer, attributes: &[(String, String)]) {
    if attributes.is_empty() {
        return;
    }
    let mut data = Vec::new();
    for (key, value) in attributes {
        data.extend_from_slice(key.as_bytes());
        data.push(0);
        data.extend_from_slice(value.as_bytes());
        data.push(0);
    }
    layer.extras.push(RawBlock {
        key: layered::PRESERVED_ATTRIBUTES,
        data,
    });
}

/// The attributes `preserve` stored, in their original order.
pub fn preserved(layer: &Layer) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in layer
        .extras
        .iter()
        .filter(|b| b.key == layered::PRESERVED_ATTRIBUTES)
    {
        let mut parts = block.data.split(|&b| b == 0);
        while let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            if key.is_empty() {
                break;
            }
            out.push((
                String::from_utf8_lossy(key).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimetype_is_read_from_the_first_local_header() {
        let package = container::write(&[
            ("stack.xml".into(), b"<image/>".to_vec()),
            ("mimetype".into(), b"image/openraster".to_vec()),
        ]);
        assert_eq!(stored_mimetype(&package), Some(&b"image/openraster"[..]));
        assert_eq!(stored_mimetype(&package[..20]), None);
        assert_eq!(stored_mimetype(b"PK\x03\x04"), None);
    }

    #[test]
    fn deep_xml_is_refused_before_it_is_built() {
        let deep = "<a>".repeat(10_000);
        assert!(parse_xml(deep.as_bytes()).is_err());
        assert!(parse_xml(b"<a><b/></a>").is_ok());
        assert!(parse_xml(b"<a><b></a>").is_err());
    }

    #[test]
    fn preserved_attributes_round_trip() {
        let mut layer = Layer::new_raster("x");
        let attrs = vec![
            ("mypaint:x".to_string(), "1 & 2".to_string()),
            (
                "xmlns:mypaint".to_string(),
                "http://mypaint.org/ns/openraster".to_string(),
            ),
        ];
        preserve(&mut layer, &attrs);
        assert_eq!(preserved(&layer), attrs);
        assert_eq!(escape("a&\"<\u{1}"), "a&amp;&quot;&lt;");
    }

    #[test]
    fn a_png_claiming_gigapixels_is_refused_before_decoding() {
        let mut png = encode_png(1, 1, &[0.0; 4], false, None).unwrap();
        png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        let mut remaining = MAX_BYTES;
        assert!(decode(&png, &mut remaining).is_err());
    }
}
