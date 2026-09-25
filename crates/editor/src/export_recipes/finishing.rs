//! Output-only finishing. Never mutate a source document or copy opaque metadata.
use super::*;
use schist_colormgmt::{ColorTransform, Intent, Profile};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    TopLeft,
    TopRight,
    Center,
    BottomLeft,
    #[default]
    BottomRight,
}
impl Placement {
    pub fn label(self) -> &'static str {
        t(match self {
            Self::TopLeft => "filter.choice.light.top_left",
            Self::TopRight => "filter.choice.light.top_right",
            Self::Center => "common.center",
            Self::BottomLeft => "filter.choice.light.bottom_left",
            Self::BottomRight => "filter.choice.light.bottom_right",
        })
    }
    fn origin(self, w: usize, h: usize, mw: usize, mh: usize) -> (usize, usize) {
        let margin = (w.min(h) / 40)
            .min(w.saturating_sub(mw))
            .min(h.saturating_sub(mh));
        let right = w.saturating_sub(mw + margin);
        let bottom = h.saturating_sub(mh + margin);
        match self {
            Self::TopLeft => (margin, margin),
            Self::TopRight => (right, margin),
            Self::Center => (w.saturating_sub(mw) / 2, h.saturating_sub(mh) / 2),
            Self::BottomLeft => (margin, bottom),
            Self::BottomRight => (right, bottom),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetProfile {
    #[default]
    Original,
    Srgb,
    DisplayP3,
    Custom,
}
impl TargetProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => t("versions.original"),
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::Custom => t("common.custom"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Finishing {
    /// Empty disables the watermark; text is shaped with the existing font engine.
    pub text: String,
    /// Em size as a percentage of the final image's shorter edge.
    pub text_size: f32,
    pub opacity: f32,
    pub placement: Placement,
    pub white: bool,
    /// Unsharp-mask amount (0..2), at final output resolution.
    pub sharpen: f32,
    pub profile: TargetProfile,
    pub custom_icc: Vec<u8>,
    pub custom_name: String,
    /// Safe allowlist: only copyright survives. GPS and all other metadata are omitted.
    pub retain_copyright: bool,
    /// Optional copyright override, useful for new and browser documents.
    pub copyright: String,
}
impl Default for Finishing {
    fn default() -> Self {
        Self {
            text: String::new(),
            text_size: 4.0,
            opacity: 0.6,
            placement: Placement::default(),
            white: true,
            sharpen: 0.0,
            profile: TargetProfile::default(),
            custom_icc: Vec::new(),
            custom_name: String::new(),
            retain_copyright: false,
            copyright: String::new(),
        }
    }
}
impl Finishing {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.text.len() <= 2048
                && self.copyright.len() <= 8192
                && !self.copyright.contains('\0')
                && self.text_size.is_finite()
                && (0.5..=25.0).contains(&self.text_size)
                && self.opacity.is_finite()
                && (0.0..=1.0).contains(&self.opacity)
                && self.sharpen.is_finite()
                && (0.0..=2.0).contains(&self.sharpen),
            "{}",
            t("metadata.invalid")
        );
        ensure!(
            self.custom_icc.len() <= 4 * 1024 * 1024,
            "{}",
            t("metadata.invalid")
        );
        if self.profile == TargetProfile::Custom {
            let profile =
                Profile::from_bytes(&self.custom_icc).context(t("dialog.profile.convert_title"))?;
            profile
                .validate_mode(schist_color::ColorMode::Rgb)
                .context(t("dialog.profile.convert_title"))?;
        }
        Ok(())
    }
    pub fn apply(&self, doc: &mut Document) -> Result<()> {
        self.validate()?;
        if self.text.is_empty() && self.sharpen == 0.0 && self.profile == TargetProfile::Original {
            return Ok(());
        }
        let (w, h) = (doc.width as usize, doc.height as usize);
        let mut pixels = schist_compositor::composite_region_f32(doc, doc.canvas_rect());
        sharpen(&mut pixels, w, h, self.sharpen);
        if !self.text.trim().is_empty() && self.opacity > 0.0 {
            let mut spec = schist_text_engine::TextSpec {
                text: self.text.clone(),
                size: (w.min(h) as f32 * self.text_size / 100.0).max(1.0),
                wrap_width: Some(w as f32 * 0.9),
                ..Default::default()
            };
            let mut raster = schist_text_engine::rasterize(&spec)
                .ok_or_else(|| anyhow::anyhow!("{}", t("dialog.fonts.missing_intro")))?;
            // Fit wrapped multiline text within the output rather than silently clipping it.
            for _ in 0..8 {
                let ratio = (w as f32 * 0.95 / raster.bounds.width().max(1) as f32)
                    .min(h as f32 * 0.95 / raster.bounds.height().max(1) as f32);
                if ratio >= 1.0 {
                    break;
                }
                spec.size *= ratio * 0.95;
                raster = schist_text_engine::rasterize(&spec)
                    .ok_or_else(|| anyhow::anyhow!("{}", t("dialog.fonts.missing_intro")))?;
            }
            ensure!(!raster.is_empty(), "{}", t("dialog.fonts.missing_intro"));
            let (mw, mh) = (
                raster.bounds.width() as usize,
                raster.bounds.height() as usize,
            );
            let (x, y) = self.placement.origin(w, h, mw, mh);
            overlay(
                &mut pixels,
                w,
                h,
                &raster.coverage,
                mw,
                mh,
                x,
                y,
                self.opacity,
                if self.white { 1.0 } else { 0.0 },
            );
        }
        if self.profile != TargetProfile::Original {
            let source = doc
                .icc_profile
                .as_deref()
                .map(Profile::from_bytes)
                .transpose()
                .context(t("dialog.profile.convert_title"))?
                .unwrap_or_else(Profile::srgb);
            let target = match self.profile {
                TargetProfile::DisplayP3 => Profile::display_p3(),
                TargetProfile::Custom => Profile::from_bytes(&self.custom_icc)?,
                _ => Profile::srgb(),
            };
            ColorTransform::new(&source, &target, Intent::RelativeColorimetric)
                .context(t("dialog.profile.convert_title"))?
                .apply(&mut pixels);
            doc.icc_profile = target.icc_bytes().map(<[u8]>::to_vec);
        }
        let mut layer = Layer::new_raster(t("common.background_layer"));
        schist_core::blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &pixels,
        );
        doc.tree.layers = vec![layer];
        Ok(())
    }
    /// Encoders produce clean containers; rebuild just the permitted copyright field.
    pub fn metadata(&self, codec: &str, bytes: Vec<u8>, source_copyright: &str) -> Result<Vec<u8>> {
        if !self.retain_copyright {
            return Ok(bytes);
        }
        let copyright = if self.copyright.is_empty() {
            source_copyright
        } else {
            &self.copyright
        };
        if copyright.is_empty() {
            return Ok(bytes);
        }
        ensure!(
            copyright.len() <= 8192 && !copyright.contains('\0'),
            "{}",
            t("metadata.invalid")
        );
        embed_copyright(codec, bytes, copyright)
    }
}

/// Alpha-weighted local blur prevents invisible RGB from creating bright edge halos.
fn sharpen(pixels: &mut [f32], w: usize, h: usize, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let original = pixels.to_vec();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            if original[i + 3] <= 0.0 {
                continue;
            }
            let mut sum = [0.0; 3];
            let mut weight = 0.0;
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = (yy * w + xx) * 4;
                    let a = original[j + 3];
                    weight += a;
                    for c in 0..3 {
                        sum[c] += original[j + c] * a;
                    }
                }
            }
            for c in 0..3 {
                pixels[i + c] = (original[i + c] + amount * (original[i + c] - sum[c] / weight))
                    .clamp(0.0, 1.0);
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn overlay(
    pixels: &mut [f32],
    w: usize,
    h: usize,
    mask: &[u8],
    mw: usize,
    mh: usize,
    x: usize,
    y: usize,
    opacity: f32,
    color: f32,
) {
    for yy in 0..mh.min(h.saturating_sub(y)) {
        for xx in 0..mw.min(w.saturating_sub(x)) {
            let a = mask[yy * mw + xx] as f32 / 255.0 * opacity;
            if a == 0.0 {
                continue;
            }
            let i = ((y + yy) * w + x + xx) * 4;
            let out_a = a + pixels[i + 3] * (1.0 - a);
            for c in 0..3 {
                pixels[i + c] = (color * a + pixels[i + c] * pixels[i + 3] * (1.0 - a)) / out_a;
            }
            pixels[i + 3] = out_a;
        }
    }
}

// Rebuild only UTF-8 dc:rights. XML escaping prevents injected metadata properties.
fn copyright_xmp(text: &str) -> Result<Vec<u8>> {
    ensure!(text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')), "{}", t("metadata.invalid"));
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\r', "&#13;");
    Ok(format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:rights><rdf:Alt><rdf:li xml:lang="x-default">{escaped}</rdf:li></rdf:Alt></dc:rights></rdf:Description></rdf:RDF></x:xmpmeta>"#).into_bytes())
}
fn embed_copyright(codec: &str, mut bytes: Vec<u8>, text: &str) -> Result<Vec<u8>> {
    let xmp = copyright_xmp(text)?;
    match codec {
        "codec.jpeg" => {
            ensure!(
                bytes.starts_with(&[0xff, 0xd8]),
                "{}",
                t("metadata.invalid")
            );
            let mut segment = vec![0xff, 0xe1];
            segment.extend_from_slice(&((xmp.len() + 31) as u16).to_be_bytes());
            segment.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0");
            segment.extend_from_slice(&xmp);
            // Keep JFIF APP0 immediately after SOI when the encoder emits it.
            let at = if bytes.get(2..4) == Some(&[0xff, 0xe0]) && bytes.len() >= 6 {
                4 + u16::from_be_bytes([bytes[4], bytes[5]]) as usize
            } else {
                2
            };
            ensure!(at <= bytes.len(), "{}", t("metadata.invalid"));
            bytes.splice(at..at, segment);
        }
        "codec.png" => {
            ensure!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && bytes.len() >= 45
                    && bytes.get(12..16) == Some(b"IHDR".as_slice()),
                "{}",
                t("metadata.invalid")
            );
            let mut payload = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
            payload.extend_from_slice(&xmp);
            let mut chunk = (payload.len() as u32).to_be_bytes().to_vec();
            chunk.extend_from_slice(b"iTXt");
            chunk.extend_from_slice(&payload);
            let crc = crc32fast::hash(&chunk[4..]);
            chunk.extend_from_slice(&crc.to_be_bytes());
            // Place iTXt after IHDR, outside the consecutive IDAT sequence.
            bytes.splice(33..33, chunk);
        }
        "codec.webp" => {
            ensure!(
                bytes.len() >= 30
                    && bytes.starts_with(b"RIFF")
                    && bytes.get(8..12) == Some(b"WEBP".as_slice()),
                "{}",
                t("metadata.invalid")
            );
            // The image encoder emits VP8X when ICC is present, otherwise create it.
            if bytes.get(12..16) != Some(b"VP8X".as_slice()) {
                // Our WebP codec emits a simple lossless VP8L bitstream here.
                ensure!(
                    bytes.get(12..16) == Some(b"VP8L".as_slice()) && bytes.len() >= 25,
                    "{}",
                    t("metadata.invalid")
                );
                let packed = u32::from_le_bytes(bytes[21..25].try_into().unwrap());
                let width = (packed & 0x3fff) + 1;
                let height = ((packed >> 14) & 0x3fff) + 1;
                let alpha = packed & (1 << 28) != 0;
                let mut header = b"VP8X\x0a\0\0\0".to_vec();
                header.extend_from_slice(&[if alpha { 0x14 } else { 0x04 }, 0, 0, 0]);
                header.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
                header.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
                bytes.splice(12..12, header);
            } else {
                bytes[20] |= 0x04;
            }
            bytes.extend_from_slice(b"XMP ");
            bytes.extend_from_slice(&(xmp.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&xmp);
            if xmp.len() % 2 == 1 {
                bytes.push(0);
            }
            let size = u32::try_from(bytes.len() - 8).context(t("metadata.invalid"))?;
            bytes[4..8].copy_from_slice(&size.to_le_bytes());
        }
        "codec.tiff" => {
            // Append a new IFD retaining encoder tags and absolute pixel/profile offsets.
            ensure!(
                bytes.len() >= 8
                    && (bytes.starts_with(b"II\x2a\0") || bytes.starts_with(b"MM\0\x2a")),
                "{}",
                t("metadata.invalid")
            );
            let little = bytes.get(..2) == Some(b"II".as_slice());
            let u16_at = |p| {
                let a = [bytes[p], bytes[p + 1]];
                if little {
                    u16::from_le_bytes(a)
                } else {
                    u16::from_be_bytes(a)
                }
            };
            let u32_at = |p| {
                let a = [bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]];
                if little {
                    u32::from_le_bytes(a)
                } else {
                    u32::from_be_bytes(a)
                }
            };
            let offset = u32_at(4) as usize;
            ensure!(
                offset <= bytes.len().saturating_sub(6),
                "{}",
                t("metadata.invalid")
            );
            let count = u16_at(offset) as usize;
            ensure!(
                count < u16::MAX as usize && count * 12 <= bytes.len() - offset - 6,
                "{}",
                t("metadata.invalid")
            );
            ensure!(
                bytes
                    .len()
                    .checked_add(xmp.len() + 2 + 2 + (count + 1) * 12 + 4)
                    .is_some_and(|size| size <= u32::MAX as usize),
                "{}",
                t("metadata.invalid")
            );
            let mut entries: Vec<(u16, Vec<u8>)> = (0..count)
                .map(|i| {
                    let at = offset + 2 + i * 12;
                    (u16_at(at), bytes[at..at + 12].to_vec())
                })
                .collect();
            let next = bytes[offset + 2 + count * 12..offset + 6 + count * 12].to_vec();
            let put16 = |v: u16| {
                if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                }
            };
            let put32 = |v: u32| {
                if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                }
            };
            if bytes.len() % 2 == 1 {
                bytes.push(0);
            }
            let value_offset = bytes.len() as u32;
            let mut value = xmp;
            bytes.extend_from_slice(&value);
            if bytes.len() % 2 == 1 {
                bytes.push(0);
            }
            let ifd_offset = bytes.len() as u32;
            let mut entry = put16(700).to_vec();
            entry.extend_from_slice(&put16(1));
            entry.extend_from_slice(&put32(value.len() as u32));
            if value.len() <= 4 {
                value.resize(4, 0);
                entry.extend_from_slice(&value);
            } else {
                entry.extend_from_slice(&put32(value_offset));
            }
            entries.push((700, entry));
            entries.sort_by_key(|(tag, _)| *tag);
            bytes.extend_from_slice(&put16(entries.len() as u16));
            for (_, entry) in entries {
                bytes.extend_from_slice(&entry);
            }
            bytes.extend_from_slice(&next);
            bytes[4..8].copy_from_slice(&put32(ifd_offset));
        }
        _ => anyhow::bail!("{}", t("metadata.invalid")),
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::CodecPlugin;
    fn document() -> Document {
        let mut doc = Document::new("source", 16, 8, schist_color::Depth::Sixteen);
        let mut layer = Layer::new_raster("pixels");
        schist_core::blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            doc.depth,
            doc.canvas_rect(),
            &[0.4, 0.3, 0.2, 1.0].repeat(128),
        );
        doc.push_layer(layer);
        doc
    }
    #[test]
    fn old_recipes_default_to_no_finishing_and_new_fields_roundtrip() {
        let output: Output = serde_json::from_str(
            r#"{"codec":"codec.png","max_edge":100,"quality":90,"template":"{name}"}"#,
        )
        .unwrap();
        assert_eq!(output.finishing, Finishing::default());
        let mut finishing = Finishing {
            text: "© Photographer".into(),
            sharpen: 0.5,
            profile: TargetProfile::Custom,
            retain_copyright: true,
            ..Default::default()
        };
        finishing.custom_icc = Profile::display_p3().icc_bytes().unwrap().to_vec();
        finishing.validate().unwrap();
        let copy: Finishing =
            serde_json::from_str(&serde_json::to_string(&finishing).unwrap()).unwrap();
        assert_eq!(copy, finishing);
        finishing.opacity = f32::NAN;
        assert!(finishing.validate().is_err());
        finishing.opacity = 0.5;
        finishing.custom_icc[16..20].copy_from_slice(b"GRAY");
        assert!(
            finishing.validate().is_err(),
            "non-RGB profile must be rejected"
        );
        finishing.custom_icc = vec![0; 4 * 1024 * 1024 + 1];
        assert!(
            finishing.validate().is_err(),
            "oversized embedded profile must be rejected"
        );
    }
    #[test]
    fn sharpen_preserves_alpha_uniform_pixels_and_ignores_transparent_rgb() {
        let mut uniform = [0.4, 0.3, 0.2, 0.5].repeat(9);
        let before = uniform.clone();
        sharpen(&mut uniform, 3, 3, 1.0);
        for (a, b) in uniform.iter().zip(&before) {
            assert!((a - b).abs() < 0.00001);
        }
        let mut edge = vec![1.0, 1.0, 1.0, 0.0, 0.4, 0.3, 0.2, 1.0, 0.8, 0.7, 0.6, 1.0];
        sharpen(&mut edge, 3, 1, 1.0);
        assert_eq!(edge[3], 0.0);
        assert_eq!(edge[7], 1.0);
        assert!(edge[4] < 0.4);
        assert!(edge[8] > 0.8);
    }
    #[test]
    fn watermark_placement_and_straight_alpha_compositing() {
        let mut pixels = vec![0.0; 4 * 4 * 4];
        let (x, y) = Placement::BottomRight.origin(4, 4, 1, 1);
        assert_eq!((x, y), (3, 3));
        overlay(&mut pixels, 4, 4, &[255], 1, 1, x, y, 0.5, 1.0);
        assert_eq!(&pixels[60..64], &[1.0, 1.0, 1.0, 0.5]);
        assert!(pixels[..60].iter().all(|v| *v == 0.0));
    }
    #[test]
    fn shaped_text_watermark_changes_export_pixels() {
        let mut doc = Document::new("watermark", 200, 100, schist_color::Depth::Sixteen);
        Finishing {
            text: "© Schist".into(),
            text_size: 12.0,
            ..Default::default()
        }
        .apply(&mut doc)
        .unwrap();
        let pixels = schist_compositor::composite_region_f32(&doc, doc.canvas_rect());
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0.0));
        assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] <= 0.601));
    }
    #[test]
    fn target_profile_changes_numbers_embeds_target_and_preserves_original() {
        let mut doc = document();
        doc.icc_profile = Profile::display_p3().icc_bytes().map(<[u8]>::to_vec);
        let before = schist_compositor::composite_region_f32(&doc, doc.canvas_rect());
        let source_tiff = schist_codecs_common::TiffCodec
            .export_with(
                &doc,
                &schist_plugin_api::ExportOptions {
                    bit_depth: 16,
                    dither: false,
                    ..Default::default()
                },
            )
            .unwrap();
        let imported = schist_codecs_common::TiffCodec
            .import(&source_tiff)
            .unwrap();
        assert_eq!(imported.icc_profile, doc.icc_profile);
        let mut flat = render(&imported, imported.canvas_rect(), &Output::default());
        Finishing {
            profile: TargetProfile::Srgb,
            ..Default::default()
        }
        .apply(&mut flat)
        .unwrap();
        let after = schist_compositor::composite_region_f32(&flat, flat.canvas_rect());
        assert!((before[0] - after[0]).abs() > 0.001);
        let mut expected = before.clone();
        ColorTransform::new(
            &Profile::display_p3(),
            &Profile::srgb(),
            Intent::RelativeColorimetric,
        )
        .unwrap()
        .apply(&mut expected);
        assert!(after
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.0001));
        assert_eq!(
            Profile::from_bytes(flat.icc_profile.as_deref().unwrap())
                .unwrap()
                .color_mode(),
            Some(schist_color::ColorMode::Rgb)
        );
        assert_eq!(
            schist_compositor::composite_region_f32(&doc, doc.canvas_rect()),
            before
        );
        for codec in [
            Box::new(schist_codecs_common::PngCodec) as Box<dyn CodecPlugin>,
            Box::new(schist_codecs_common::JpegCodec),
            Box::new(schist_codecs_common::WebPCodec),
            Box::new(schist_codecs_common::TiffCodec),
        ] {
            let bytes = codec.export(&flat).unwrap();
            let imported = codec.import(&bytes).unwrap();
            assert_eq!(imported.icc_profile, flat.icc_profile, "{}", codec.id());
            if codec.id() == "codec.tiff" {
                // Also inspect the physical ICC tag independently of the importer.
                let little = bytes.starts_with(b"II");
                let read16 = |i| {
                    let a = [bytes[i], bytes[i + 1]];
                    if little {
                        u16::from_le_bytes(a)
                    } else {
                        u16::from_be_bytes(a)
                    }
                };
                let read32 = |i| {
                    let a = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
                    if little {
                        u32::from_le_bytes(a)
                    } else {
                        u32::from_be_bytes(a)
                    }
                };
                let start = read32(4) as usize;
                let entry = (0..read16(start) as usize)
                    .map(|i| start + 2 + i * 12)
                    .find(|i| read16(*i) == 34675)
                    .unwrap();
                let size = read32(entry + 4) as usize;
                let offset = read32(entry + 8) as usize;
                assert_eq!(
                    &bytes[offset..offset + size],
                    flat.icc_profile.as_deref().unwrap()
                );
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn every_real_container_retains_only_copyright_and_strip_mode_adds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        for codec in [
            Box::new(schist_codecs_common::PngCodec) as Box<dyn CodecPlugin>,
            Box::new(schist_codecs_common::JpegCodec),
            Box::new(schist_codecs_common::WebPCodec),
            Box::new(schist_codecs_common::TiffCodec),
        ] {
            for text in ["A", "Photographer 2026", "© 王 <rights> & الشركة"] {
                let clean = codec.export(&document()).unwrap();
                assert_eq!(
                    Finishing::default()
                        .metadata(codec.id(), clean.clone(), text)
                        .unwrap(),
                    clean
                );
                let bytes = Finishing {
                    retain_copyright: true,
                    ..Default::default()
                }
                .metadata(codec.id(), clean, text)
                .unwrap();
                let decoded = image::load_from_memory(&bytes).unwrap();
                assert_eq!((decoded.width(), decoded.height()), (16, 8));
                let path = dir.path().join(format!("test.{}", codec.extensions()[0]));
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    schist_gallery::copyright_of(&path).as_deref(),
                    Some(text),
                    "{}",
                    codec.id()
                );
                assert!(schist_gallery::exif_of(&path)
                    .as_ref()
                    .and_then(schist_gallery::gps_from)
                    .is_none());
                use image::ImageDecoder as _;
                let packet = if codec.id() == "codec.tiff" {
                    image::codecs::tiff::TiffDecoder::new(std::io::Cursor::new(&bytes))
                        .unwrap()
                        .xmp_metadata()
                        .unwrap()
                        .unwrap()
                } else {
                    image::ImageReader::new(std::io::Cursor::new(&bytes))
                        .with_guessed_format()
                        .unwrap()
                        .into_decoder()
                        .unwrap()
                        .xmp_metadata()
                        .unwrap()
                        .unwrap()
                };
                let metadata =
                    schist_gallery::xmp::parse(std::str::from_utf8(&packet).unwrap()).unwrap();
                assert_eq!(metadata.copyright, text);
                assert_eq!(metadata.gps, None);
            }
        }
    }
}
