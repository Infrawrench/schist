//! Bounded brush import. ABR byte layouts were checked against GIMP's public
//! reader; no Adobe SDK or header is used. See docs/brushes.md for provenance.
use super::{BrushLibrary, MAX_BYTES, MAX_PRESETS};
use schist_plugin_api::{BrushBitmap, BrushDynamics, BrushPreset, BrushTip};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Limit,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "brush import: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    version: u32,
    presets: Vec<BrushPreset>,
}

pub fn encode_pack(library: &BrushLibrary) -> Result<Vec<u8>, Error> {
    if !library.valid() {
        return Err(Error::Limit);
    }
    let bytes = serde_json::to_vec(&Pack {
        version: 1,
        presets: library.presets.clone(),
    })
    .map_err(|_| Error::Invalid)?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

/// Images use alpha if any pixel is translucent; opaque images use inverted
/// luminance (black paints, white is transparent). Imported names are data.
pub fn decode(bytes: &[u8], extension: &str, name: &str) -> Result<Vec<BrushPreset>, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Limit);
    }
    let presets = match extension.to_ascii_lowercase().as_str() {
        "schist-brushes" => {
            let pack: Pack = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
            if pack.version != 1 {
                return Err(Error::Unsupported);
            }
            pack.presets
        }
        "abr" => abr(bytes, name)?,
        "gbr" => vec![gbr(bytes, name)?],
        "png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff" => vec![bitmap_image(bytes, name)?],
        _ => return Err(Error::Unsupported),
    };
    if presets.is_empty() {
        return Err(Error::Invalid);
    }
    let library = BrushLibrary { presets };
    if !library.valid() {
        return Err(Error::Limit);
    }
    Ok(library
        .presets
        .into_iter()
        .map(BrushPreset::sanitized)
        .collect())
}

fn bitmap_image(bytes: &[u8], name: &str) -> Result<BrushPreset, Error> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::Invalid)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(schist_plugin_api::brush::MAX_BITMAP_SIDE);
    limits.max_image_height = Some(schist_plugin_api::brush::MAX_BITMAP_SIDE);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| Error::Invalid)?.to_rgba8();
    let use_alpha = image.pixels().any(|p| p[3] != 255);
    let pixels = image
        .pixels()
        .map(|p| {
            if use_alpha {
                p[3]
            } else {
                255 - ((54 * u32::from(p[0]) + 183 * u32::from(p[1]) + 19 * u32::from(p[2]) + 128)
                    / 256) as u8
            }
        })
        .collect();
    preset(name.into(), image.width(), image.height(), pixels, 0.15)
}

fn preset(
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    spacing: f32,
) -> Result<BrushPreset, Error> {
    let bitmap = BrushBitmap {
        width,
        height,
        pixels,
    };
    if !bitmap.valid() {
        return Err(Error::Limit);
    }
    Ok(BrushPreset {
        name,
        size: width.max(height) as f32,
        hardness: 1.0,
        bitmap: Some(Arc::new(bitmap)),
        dynamics: BrushDynamics {
            tip: BrushTip::Bitmap,
            spacing,
            ..Default::default()
        },
        ..Default::default()
    }
    .sanitized())
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let value = self.0.get(..n).ok_or(Error::Invalid)?;
        self.0 = &self.0[n..];
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn short(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn long(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn block(&mut self) -> Result<Reader<'a>, Error> {
        let length = self.long()? as usize;
        Ok(Reader(self.take(length)?))
    }
}

fn dimensions(width: u32, height: u32) -> Result<usize, Error> {
    let max = schist_plugin_api::brush::MAX_BITMAP_SIDE;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(Error::Limit);
    }
    Ok(width as usize * height as usize)
}

fn gbr(bytes: &[u8], fallback: &str) -> Result<BrushPreset, Error> {
    let mut r = Reader(bytes);
    let header_size = r.long()? as usize;
    if r.long()? != 2 {
        return Err(Error::Unsupported);
    }
    let (width, height, channels) = (r.long()?, r.long()?, r.long()?);
    let n = dimensions(width, height)?;
    if r.take(4)? != b"GIMP" {
        return Err(Error::Invalid);
    }
    let spacing = r.long()? as f32 / 100.0;
    if header_size < 28 || header_size > 4096 {
        return Err(Error::Invalid);
    }
    let name = std::str::from_utf8(r.take(header_size - 28)?)
        .map_err(|_| Error::Invalid)?
        .trim_end_matches('\0');
    let pixels = match channels {
        1 => r.take(n)?.to_vec(),
        4 => r.take(n * 4)?.chunks_exact(4).map(|p| p[3]).collect(),
        _ => return Err(Error::Unsupported),
    };
    if !r.0.is_empty() {
        return Err(Error::Invalid);
    }
    preset(
        if name.is_empty() { fallback } else { name }.into(),
        width,
        height,
        pixels,
        spacing,
    )
}

fn abr(bytes: &[u8], name: &str) -> Result<Vec<BrushPreset>, Error> {
    let mut r = Reader(bytes);
    let (version, count) = (r.short()?, r.short()?);
    let mut presets = Vec::new();
    match version {
        1 | 2 => {
            if count as usize > MAX_PRESETS {
                return Err(Error::Limit);
            }
            for index in 0..count {
                // Reject unsupported computed entries, without partially importing.
                if r.short()? != 2 {
                    return Err(Error::Unsupported);
                }
                let mut entry = r.block()?;
                entry.take(4)?;
                let spacing = entry.short()? as f32 / 100.0;
                let mut title = format!("{name} {}", index + 1);
                if version == 2 {
                    let chars = entry.long()? as usize;
                    if chars > 1024 {
                        return Err(Error::Limit);
                    }
                    let units: Vec<u16> = entry
                        .take(chars * 2)?
                        .chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect();
                    let text = String::from_utf16(&units).map_err(|_| Error::Invalid)?;
                    if !text.trim_end_matches('\0').is_empty() {
                        title = text.trim_end_matches('\0').into();
                    }
                }
                entry.take(9)?; // antialias byte and four legacy 16-bit bounds
                push_sample(&mut presets, abr_sample(&mut entry, title, spacing)?)?;
                if !entry.0.is_empty() {
                    return Err(Error::Invalid);
                }
            }
        }
        6 | 10 if count == 1 || count == 2 => {
            // Modern packs store samples separately from proprietary brush
            // dynamics descriptors. Only the sampled masks are imported.
            while !r.0.is_empty() {
                if r.take(4)? != b"8BIM" {
                    return Err(Error::Invalid);
                }
                let tag = r.take(4)?;
                let mut section = r.block()?;
                if tag != b"samp" {
                    continue;
                }
                while !section.0.is_empty() {
                    if presets.len() == MAX_PRESETS {
                        return Err(Error::Limit);
                    }
                    let len = section.long()? as usize;
                    let mut entry = Reader(section.take(len)?);
                    section.take((4 - len % 4) % 4)?;
                    entry.take(if count == 1 { 47 } else { 301 })?;
                    let title = format!("{name} {}", presets.len() + 1);
                    push_sample(&mut presets, abr_sample(&mut entry, title, 0.25)?)?;
                    if !entry.0.is_empty() {
                        return Err(Error::Invalid);
                    }
                }
            }
        }
        _ => return Err(Error::Unsupported),
    }
    if !r.0.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(presets)
}

fn push_sample(presets: &mut Vec<BrushPreset>, preset: BrushPreset) -> Result<(), Error> {
    let bytes: usize = presets
        .iter()
        .chain(std::iter::once(&preset))
        .filter_map(|p| p.bitmap.as_ref())
        .map(|b| b.pixels.len())
        .sum();
    if bytes > super::MAX_MASK_BYTES {
        return Err(Error::Limit);
    }
    presets.push(preset);
    Ok(())
}

fn abr_sample(r: &mut Reader<'_>, name: String, spacing: f32) -> Result<BrushPreset, Error> {
    let (top, left, bottom, right) = (
        r.long()? as i32,
        r.long()? as i32,
        r.long()? as i32,
        r.long()? as i32,
    );
    let width = right
        .checked_sub(left)
        .filter(|n| *n > 0)
        .ok_or(Error::Invalid)? as u32;
    let height = bottom
        .checked_sub(top)
        .filter(|n| *n > 0)
        .ok_or(Error::Invalid)? as u32;
    let n = dimensions(width, height)?;
    if r.short()? != 8 {
        return Err(Error::Unsupported);
    }
    let pixels = match r.byte()? {
        0 => r.take(n)?.to_vec(),
        1 => {
            let lengths: Vec<usize> = (0..height)
                .map(|_| r.short().map(usize::from))
                .collect::<Result<_, _>>()?;
            let mut pixels = Vec::with_capacity(n);
            for length in lengths {
                let mut row = Reader(r.take(length)?);
                let end = pixels.len() + width as usize;
                while !row.0.is_empty() {
                    let code = row.byte()? as i8;
                    match code {
                        0..=127 => {
                            let n = code as usize + 1;
                            if pixels.len() + n > end {
                                return Err(Error::Invalid);
                            }
                            pixels.extend_from_slice(row.take(n)?);
                        }
                        -127..=-1 => {
                            let n = (1 - i16::from(code)) as usize;
                            if pixels.len() + n > end {
                                return Err(Error::Invalid);
                            }
                            let value = row.byte()?;
                            pixels.resize(pixels.len() + n, value);
                        }
                        -128 => {}
                    }
                }
                if pixels.len() != end {
                    return Err(Error::Invalid);
                }
            }
            pixels
        }
        _ => return Err(Error::Unsupported),
    };
    preset(name, width, height, pixels, spacing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_bytes(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_raw(width, height, pixels.to_vec()).unwrap();
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    #[test]
    fn png_alpha_and_opaque_luminance_have_distinct_mask_semantics() {
        let rgba = image_bytes(&[255, 0, 0, 64, 0, 0, 0, 255], 2, 1);
        let presets = decode(&rgba, "PNG", "α tip").unwrap();
        assert_eq!(presets[0].name, "α tip");
        assert_eq!(presets[0].bitmap.as_ref().unwrap().pixels, [64, 255]);
        let opaque = image_bytes(&[0, 0, 0, 255, 255, 255, 255, 255], 2, 1);
        let presets = decode(&opaque, "png", "mask").unwrap();
        assert_eq!(presets[0].bitmap.as_ref().unwrap().pixels, [255, 0]);
    }

    fn sample(compressed: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        for bound in [0_i32, 0, 2, 3] {
            bytes.extend_from_slice(&bound.to_be_bytes());
        }
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.push(u8::from(compressed));
        if compressed {
            bytes.extend_from_slice(&2_u16.to_be_bytes());
            bytes.extend_from_slice(&4_u16.to_be_bytes());
            bytes.extend_from_slice(&[254, 255, 2, 0, 128, 255]);
        } else {
            bytes.extend_from_slice(&[255, 255, 255, 0, 128, 255]);
        }
        bytes
    }

    fn legacy(version: u16, compressed: bool) -> Vec<u8> {
        let mut entry = vec![0; 4];
        entry.extend_from_slice(&30_u16.to_be_bytes());
        if version == 2 {
            entry.extend_from_slice(&3_u32.to_be_bytes());
            for code in [0x7b46_u16, 0x2605, 0] {
                entry.extend_from_slice(&code.to_be_bytes());
            }
        }
        entry.extend_from_slice(&[0; 9]);
        entry.extend(sample(compressed));
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&(entry.len() as u32).to_be_bytes());
        bytes.extend(entry);
        bytes
    }

    fn modern(version: u16, subversion: u16, compressed: bool) -> Vec<u8> {
        let mut entry = vec![0; if subversion == 1 { 47 } else { 301 }];
        entry.extend(sample(compressed));
        let mut section = (entry.len() as u32).to_be_bytes().to_vec();
        section.extend(&entry);
        section.resize(section.len() + (4 - entry.len() % 4) % 4, 0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&subversion.to_be_bytes());
        bytes.extend_from_slice(b"8BIMdesc\0\0\0\x04test8BIMsamp");
        bytes.extend_from_slice(&(section.len() as u32).to_be_bytes());
        bytes.extend(section);
        bytes
    }

    #[test]
    fn abr_legacy_and_modern_samples_decode_raw_and_packbits() {
        for compressed in [false, true] {
            for (version, subversion) in [(1, 0), (2, 0), (6, 1), (6, 2), (10, 1), (10, 2)] {
                let bytes = if version <= 2 {
                    legacy(version, compressed)
                } else {
                    modern(version, subversion, compressed)
                };
                let presets = decode(&bytes, "abr", "pack").unwrap();
                let preset = &presets[0];
                assert_eq!(
                    preset.bitmap.as_ref().unwrap().pixels,
                    [255, 255, 255, 0, 128, 255]
                );
                assert_eq!(preset.name, if version == 2 { "筆★" } else { "pack 1" });
                assert_eq!(
                    preset.dynamics.spacing,
                    if version <= 2 { 0.3 } else { 0.25 }
                );
                for end in 0..bytes.len() {
                    assert!(
                        decode(&bytes[..end], "abr", "pack").is_err(),
                        "accepted truncated v{version} at {end}"
                    );
                }
            }
        }
    }

    #[test]
    fn abr_rejects_computed_brushes_unknown_versions_bad_bounds_and_rle_overruns() {
        let mut bytes = legacy(1, false);
        bytes[5] = 1;
        assert_eq!(decode(&bytes, "abr", "pack"), Err(Error::Unsupported));
        bytes[0..2].copy_from_slice(&99_u16.to_be_bytes());
        assert_eq!(decode(&bytes, "abr", "pack"), Err(Error::Unsupported));
        let mut bytes = legacy(1, false);
        // Header 10, misc + spacing + short bounds 15, bottom bound at 33.
        bytes[33..37].copy_from_slice(&i32::MAX.to_be_bytes());
        assert_eq!(decode(&bytes, "abr", "pack"), Err(Error::Limit));
        let mut bytes = legacy(1, true);
        let repeat = bytes.len() - 6;
        bytes[repeat] = 253; // attempts four output bytes into a three-pixel row
        assert_eq!(decode(&bytes, "abr", "pack"), Err(Error::Invalid));
    }

    #[test]
    fn gbr_mask_and_spacing_are_preserved() {
        let mut bytes = Vec::new();
        for value in [32_u32, 2, 3, 1, 1, u32::from_be_bytes(*b"GIMP"), 40] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(b"tip\0\0\x80\xff");
        let preset = decode(&bytes, "gbr", "fallback").unwrap().remove(0);
        assert_eq!(preset.name, "tip");
        assert_eq!(preset.dynamics.spacing, 0.4);
        assert_eq!(preset.bitmap.unwrap().pixels, [0, 128, 255]);
    }

    #[test]
    fn portable_pack_roundtrips_and_merge_never_overwrites_existing_names() {
        let mut preset = decode(&legacy(2, true), "abr", "pack").unwrap().remove(0);
        preset.dynamics.pressure_opacity = true;
        preset.dynamics.rotation = 37.0;
        preset.dynamics.tilt_rotation = true;
        let library = BrushLibrary {
            presets: vec![preset.clone()],
        };
        let bytes = encode_pack(&library).unwrap();
        let restored = decode(&bytes, "schist-brushes", "ignored").unwrap();
        assert_eq!(restored, library.presets);
        let mut merged = library.clone();
        assert_eq!(merged.import_presets(restored), Some(1));
        assert_eq!(merged.presets[0], preset);
        assert_eq!(merged.presets[1].name, "筆★ (2)");
        let bytes = serde_json::to_string(&merged).unwrap();
        assert_eq!(
            BrushLibrary::from_json(&bytes).unwrap().presets,
            merged.presets
        );
        let mut editor = schist_plugin_api::EditorState::default();
        merged.presets[0].apply(&mut editor);
        assert_eq!(BrushPreset::capture(preset.name.clone(), &editor), preset);
    }

    #[test]
    fn invalid_masks_versions_and_budget_overflows_are_atomic() {
        let invalid = br#"{"version":1,"presets":[{"name":"bad","dynamics":{"tip":"Bitmap"},"bitmap":{"width":100,"height":100,"pixels":[255]}}]}"#;
        assert_eq!(decode(invalid, "schist-brushes", "pack"), Err(Error::Limit));
        let unknown = br#"{"version":2,"presets":[]}"#;
        assert_eq!(
            decode(unknown, "schist-brushes", "pack"),
            Err(Error::Unsupported)
        );
        let missing = br#"{"version":1,"presets":[{"name":"bad","dynamics":{"tip":"Bitmap"}}]}"#;
        assert_eq!(decode(missing, "schist-brushes", "pack"), Err(Error::Limit));
        let oversized = preset("big".into(), 1024, 1024, vec![255; 1024 * 1024], 0.15).unwrap();
        let mut library = BrushLibrary::default();
        library.import_presets(vec![oversized.clone(); 4]).unwrap();
        let before = library.presets.clone();
        assert!(library.import_presets(vec![oversized.clone()]).is_none());
        assert_eq!(library.presets, before);
        assert!(!library.save_preset(BrushPreset {
            name: "extra".into(),
            ..oversized
        }));
        assert_eq!(library.presets, before);
        assert!(encode_pack(&library).unwrap().len() <= MAX_BYTES);
        assert!(decode(&vec![0; MAX_BYTES + 1], "png", "large").is_err());
    }
}
