//! JPEG XL import and export.
//!
//! Decoding is jxl-oxide, pure Rust on every target: lossy (VarDCT) and
//! lossless (modular) files, 8- to 16-bit integer and float samples,
//! alpha, embedded ICC profiles and HDR, the first frame of an animation.
//!
//! Encoding has two backends:
//!
//! - libjxl, the reference encoder, when the system has it (desktop
//!   only; dlopen'd at runtime like libheif, see `libjxl`). It does
//!   everything: lossy and lossless, effort, 8/16-bit and float
//!   samples, alpha, the document's ICC profile.
//! - zune-jpegxl otherwise, a pure-Rust encoder that is lossless only
//!   and writes 8- or 16-bit samples in sRGB. It is what the browser,
//!   iOS and Android builds always use, and what a desktop without
//!   libjxl falls back to; the export dialog hides the quality and
//!   effort controls when it is the one that will run.
//!
//! No pure-Rust lossy JPEG XL encoder exists under a licence this repo
//! can take (jxl-encoder is AGPL), and building libjxl from source would
//! put a C++ toolchain in every build.

use anyhow::Context as _;
use schist_color::Depth;
use schist_core::Document;
use schist_i18n::{t, tf};
use schist_plugin_api::{CodecPlugin, ExportOptions};

#[cfg(not(any(
    target_arch = "wasm32",
    target_os = "ios",
    target_os = "android",
    schist_library
)))]
mod libjxl;

/// A bare codestream starts with this...
const CODESTREAM: &[u8] = &[0xFF, 0x0A];
/// ...and the ISO-BMFF container with this signature box.
const CONTAINER: &[u8] = b"\0\0\0\x0cJXL \r\n\x87\n";

pub struct JxlCodec;

impl CodecPlugin for JxlCodec {
    fn id(&self) -> &'static str {
        "codec.jxl"
    }
    fn name(&self) -> &'static str {
        t("codec.jxl.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["jxl"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(CODESTREAM) || bytes.starts_with(CONTAINER)
    }
    fn import(&self, bytes: &[u8]) -> anyhow::Result<Document> {
        import(bytes, self.name())
    }
    fn can_export(&self) -> bool {
        true
    }
    fn export(&self, doc: &Document) -> anyhow::Result<Vec<u8>> {
        export(doc, &ExportOptions::default())
    }
    fn export_with(&self, doc: &Document, options: &ExportOptions) -> anyhow::Result<Vec<u8>> {
        export(doc, options)
    }
    // Quality and effort only mean something to libjxl; the pure-Rust
    // fallback is lossless and has no knobs.
    fn supports_quality(&self) -> bool {
        lossy_encoder_available()
    }
    fn supports_effort(&self) -> bool {
        lossy_encoder_available()
    }
    fn bit_depths(&self) -> &'static [u8] {
        if lossy_encoder_available() {
            &[8, 16, 32]
        } else {
            &[8, 16]
        }
    }
}

/// Whether libjxl is on this machine to encode lossy files.
pub fn lossy_encoder_available() -> bool {
    #[cfg(not(any(
        target_arch = "wasm32",
        target_os = "ios",
        target_os = "android",
        schist_library
    )))]
    {
        libjxl::get().is_some()
    }
    #[cfg(any(
        target_arch = "wasm32",
        target_os = "ios",
        target_os = "android",
        schist_library
    ))]
    {
        false
    }
}

fn import(bytes: &[u8], title: &str) -> anyhow::Result<Document> {
    use jxl_oxide::image::BitDepth;

    let decoding = || tf!("codec.msg.decoding", name = title);
    let mut image = jxl_oxide::JxlImage::builder()
        .read(bytes)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(decoding)?;
    // A lossy file is coded in XYB and has to be converted back into
    // the colour space it was made in; jxl-oxide can do that for the
    // standard spaces on its own, and needs a CMS for an ICC profile.
    image.set_cms(jxl_oxide::Moxcms);

    let gray = image.pixel_format().is_grayscale();
    let metadata = &image.image_header().metadata;
    let depth = match metadata.bit_depth {
        BitDepth::IntegerSample { bits_per_sample } if bits_per_sample <= 8 => Depth::Eight,
        BitDepth::IntegerSample { bits_per_sample } if bits_per_sample <= 16 => Depth::Sixteen,
        _ => Depth::ThirtyTwo,
    };
    let premultiplied = metadata
        .ec_info
        .iter()
        .find(|ec| ec.is_alpha())
        .and_then(|ec| ec.alpha_associated())
        .unwrap_or(false);
    anyhow::ensure!(
        !image.pixel_format().has_black(),
        "{}",
        tf!("codec.msg.decoding", name = title)
    );

    // Render in the file's own colour space and keep its profile, the
    // way the other codecs keep an embedded ICC. Grey files are the
    // exception: documents are RGB, and a grey profile cannot describe
    // them, so those render to sRGB.
    let mut icc = None;
    if gray {
        image.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
            jxl_oxide::RenderingIntent::Relative,
        ));
    } else if let Some(original) = image.original_icc().map(<[u8]>::to_vec) {
        if image.request_icc(&original).is_ok() {
            icc = Some(original);
        }
    }
    let cicp = image.rendered_cicp();
    let hdr = image.hdr_type().is_some();
    if icc.is_none() && !gray && !hdr {
        // An enumerated colour space other than sRGB (Display P3, Rec.
        // 2020, linear, or custom primaries: libjxl turns any profile it
        // can describe exactly into such a description) gets the profile
        // jxl-oxide synthesises for it.
        icc = match cicp {
            Some([1, 13, _, _]) => None,
            _ => Some(image.rendered_icc()),
        };
    }

    let render = image
        .render_frame(0)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(decoding)?;
    let mut stream = render.stream();
    let (w, h, channels) = (stream.width(), stream.height(), stream.channels() as usize);
    anyhow::ensure!(w > 0 && h > 0, "{}", t("codec.msg.zero_sized"));
    let mut samples = vec![0f32; w as usize * h as usize * channels];
    stream.write_to_buffer(&mut samples);

    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for px in samples.chunks_exact(channels) {
        let (rgb, alpha) = match channels {
            1 => ([px[0]; 3], 1.0),
            2 => ([px[0]; 3], px[1]),
            3 => ([px[0], px[1], px[2]], 1.0),
            _ => ([px[0], px[1], px[2]], px[3]),
        };
        let unmultiply = if premultiplied && alpha > 0.0 {
            1.0 / alpha
        } else {
            1.0
        };
        rgba.extend(rgb.map(|c| c * unmultiply));
        rgba.push(alpha);
    }

    // PQ and HLG files are baked down to sRGB, as HDR PNG, HEIC and
    // AVIF are.
    if let (true, Some([primaries, transfer @ (16 | 18), ..])) = (hdr, cicp) {
        match schist_colormgmt::bake_hdr_to_srgb(&mut rgba, primaries, transfer) {
            Ok(()) => {
                let bytes: Vec<u8> = rgba
                    .iter()
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                    .collect();
                return crate::flat_document(title, w, h, &bytes, None);
            }
            Err(err) => log::warn!("displaying HDR {title} unmapped: {err:#}"),
        }
    }

    if depth == Depth::Eight {
        let bytes: Vec<u8> = rgba
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
            .collect();
        return crate::flat_document(title, w, h, &bytes, icc);
    }
    if depth == Depth::Sixteen {
        for v in &mut rgba {
            *v = v.clamp(0.0, 1.0);
        }
    }
    crate::deep_document(title, w, h, &rgba, depth, icc)
}

/// The same image with an XMP packet attached, in an `xml ` box of the
/// ISO-BMFF container; a bare codestream is wrapped in one first.
pub fn with_xmp(bytes: &[u8], xmp: &[u8]) -> anyhow::Result<Vec<u8>> {
    let plain_box = |kind: &[u8; 4], body: &[u8]| {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    };
    let mut out = if bytes.starts_with(CONTAINER) {
        bytes.to_vec()
    } else {
        anyhow::ensure!(bytes.starts_with(CODESTREAM), "not a JPEG XL file");
        let mut out = CONTAINER.to_vec();
        out.extend(plain_box(b"ftyp", b"jxl \0\0\0\0jxl "));
        out.extend(plain_box(b"jxlc", bytes));
        out
    };
    out.extend(plain_box(b"xml ", xmp));
    Ok(out)
}

/// cjxl's mapping from a JPEG-style quality to a butteraugli distance:
/// 90 is distance 1 ("visually lossless"), 100 is lossless.
// Only libjxl takes a distance; builds without it keep this for the tests.
#[cfg_attr(
    any(
        target_arch = "wasm32",
        target_os = "ios",
        target_os = "android",
        schist_library
    ),
    allow(dead_code)
)]
fn distance_for(quality: u8) -> f32 {
    let q = quality.clamp(1, 100) as f32;
    if q >= 100.0 {
        0.0
    } else if q >= 30.0 {
        0.1 + (100.0 - q) * 0.09
    } else {
        53.0 / 3000.0 * q * q - 23.0 / 20.0 * q + 25.0
    }
}

/// The flattened document, ready for an encoder.
pub(crate) struct Flat {
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA, 0..1 (above 1 only in a float document).
    pub pixels: Vec<f32>,
    pub opaque: bool,
}

pub(crate) fn flatten(doc: &Document, options: &ExportOptions, bits: u32) -> Flat {
    let mut pixels = schist_compositor::composite_region_f32(doc, doc.canvas_rect());
    if options.dither && bits <= 8 {
        schist_colormgmt::dither_to_depth(&mut pixels, doc.width as usize, 1 << bits);
    }
    let opaque = pixels.as_chunks::<4>().0.iter().all(|px| px[3] >= 1.0);
    Flat {
        width: doc.width,
        height: doc.height,
        pixels,
        opaque,
    }
}

fn export(doc: &Document, options: &ExportOptions) -> anyhow::Result<Vec<u8>> {
    let bits = match options.bit_depth {
        0..=8 => 8,
        9..=16 => 16,
        _ => 32,
    };
    #[cfg(not(any(
        target_arch = "wasm32",
        target_os = "ios",
        target_os = "android",
        schist_library
    )))]
    if let Some(lib) = libjxl::get() {
        let flat = flatten(doc, options, bits);
        return libjxl::encode(
            lib,
            &flat,
            bits,
            distance_for(options.quality),
            options.effort,
            doc.icc_profile.as_deref(),
        );
    }
    export_lossless(doc, options, bits.min(16))
}

/// The pure-Rust path: lossless, sRGB, 8- or 16-bit.
fn export_lossless(doc: &Document, options: &ExportOptions, bits: u32) -> anyhow::Result<Vec<u8>> {
    let mut flat = flatten(doc, options, bits);
    // This encoder writes sRGB and nothing else, so a document in
    // another space is converted rather than written with numbers that
    // would be read in the wrong one.
    if let Some(icc) = &doc.icc_profile {
        let profile = schist_colormgmt::Profile::from_bytes(icc)?;
        schist_colormgmt::convert_pixels(
            &mut flat.pixels,
            &profile,
            &schist_colormgmt::Profile::srgb(),
            schist_colormgmt::Intent::RelativeColorimetric,
        )?;
    }
    encode_lossless(&flat, bits)
}

/// Lossless, sRGB, 8- or 16-bit: what zune-jpegxl can write.
pub(crate) fn encode_lossless(flat: &Flat, bits: u32) -> anyhow::Result<Vec<u8>> {
    use zune_core::bit_depth::BitDepth;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::EncoderOptions;

    let channels = if flat.opaque { 3 } else { 4 };
    let samples = flat
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| px[..channels].iter().map(|v| v.clamp(0.0, 1.0)));
    let (raw, depth): (Vec<u8>, _) = if bits <= 8 {
        (
            samples.map(|v| (v * 255.0 + 0.5) as u8).collect(),
            BitDepth::Eight,
        )
    } else {
        // Native-endian pairs, which is what the encoder reads.
        (
            samples
                .flat_map(|v| ((v * 65535.0 + 0.5) as u16).to_ne_bytes())
                .collect(),
            BitDepth::Sixteen,
        )
    };
    let colorspace = if channels == 3 {
        ColorSpace::RGB
    } else {
        ColorSpace::RGBA
    };
    let mut options =
        EncoderOptions::new(flat.width as usize, flat.height as usize, colorspace, depth);
    if cfg!(target_arch = "wasm32") {
        options = options.set_num_threads(1);
    }
    let mut out = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(&raw, options)
        .encode(&mut out)
        .map_err(|err| anyhow::anyhow!("{err:?}"))?;
    if channels == 4 && depth == BitDepth::Sixteen {
        out = widen_alpha(out, flat.width, flat.height)?;
    }
    Ok(out)
}

/// A JPEG XL bit writer: least significant bit first.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, n: u32, value: u64) {
        self.acc |= value << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn pad(&mut self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.acc = 0;
        self.n = 0;
    }
}

/// zune-jpegxl writes the image header of an RGBA file with the alpha
/// channel's description left at its default, which is 8 bits a sample
/// whatever the colour channels are. Its 16-bit alpha samples then
/// decode as 8-bit ones and saturate, so every partly transparent pixel
/// comes back opaque. The header ends on a byte boundary, so it is
/// rewritten here with the alpha channel declared at 16 bits and the
/// encoder's frame data left as it is.
fn widen_alpha(encoded: Vec<u8>, width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
    let header = |alpha_bits: Option<u32>| {
        let mut bits = Bits::default();
        bits.put(16, 0x0AFF);
        bits.put(1, 0);
        for (size, ratio) in [(height, true), (width, false)] {
            let v = size as u64 - 1;
            match v {
                _ if v < 1 << 9 => {
                    bits.put(2, 0);
                    bits.put(9, v)
                }
                _ if v < 1 << 13 => {
                    bits.put(2, 1);
                    bits.put(13, v)
                }
                _ if v < 1 << 18 => {
                    bits.put(2, 2);
                    bits.put(18, v)
                }
                _ => {
                    bits.put(2, 3);
                    bits.put(30, v)
                }
            }
            if ratio {
                bits.put(3, 0);
            }
        }
        bits.put(1, 0); // metadata not all default
        bits.put(1, 0); // no extra fields
        bits.put(1, 0); // integer samples
        bits.put(2, 3); // bits_per_sample = 1 + u(6)
        bits.put(6, 15);
        bits.put(1, 0); // 16-bit samples do not fit 16-bit buffers
        bits.put(2, 1); // one extra channel
        match alpha_bits {
            None => bits.put(1, 1), // the default: 8-bit alpha
            Some(n) => {
                bits.put(1, 0); // not all default
                bits.put(2, 0); // type: alpha
                bits.put(1, 0); // integer samples
                bits.put(2, 3); // bits_per_sample = 1 + u(6)
                bits.put(6, n as u64 - 1);
                bits.put(2, 0); // dim_shift 0
                bits.put(2, 0); // no name
                bits.put(1, 0); // straight alpha
            }
        }
        bits.put(1, 0); // not XYB
        bits.put(1, 1); // colour encoding all default: sRGB
        bits.put(2, 0); // no extensions
        bits.put(1, 1); // default transform data
        bits.pad();
        bits.out
    };
    let theirs = header(None);
    anyhow::ensure!(
        encoded.starts_with(&theirs),
        "unexpected JPEG XL header from the encoder"
    );
    let mut out = header(Some(16));
    out.extend_from_slice(&encoded[theirs.len()..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_core::{blit_rgba_f32, IntRect, Layer};

    fn doc_with(w: u32, h: u32, depth: Depth, pixels: &[f32]) -> Document {
        let mut doc = Document::new("t", w, h, depth);
        let mut layer = Layer::new_raster("l");
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            depth,
            IntRect::from_size(w, h),
            pixels,
        );
        doc.push_layer(layer);
        doc
    }

    fn pixels_of(doc: &Document) -> Vec<f32> {
        schist_compositor::composite_region_f32(doc, doc.canvas_rect())
    }

    fn gradient(w: u32, h: u32, alpha: bool) -> Vec<f32> {
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if alpha {
                    (x + y) as f32 / (w + h) as f32
                } else {
                    1.0
                };
                out.extend([x as f32 / w as f32, y as f32 / h as f32, 0.3, a]);
            }
        }
        out
    }

    #[test]
    fn probe_accepts_both_wrappings() {
        assert!(JxlCodec.probe(&[0xFF, 0x0A, 0, 0]));
        assert!(JxlCodec.probe(b"\0\0\0\x0cJXL \r\n\x87\n\0\0\0\x14ftypjxl "));
        assert!(!JxlCodec.probe(b"\xFF\xD8\xFF\xE0"));
    }

    #[test]
    fn lossless_round_trip_is_exact_at_eight_bits() {
        let source = gradient(13, 7, true);
        let doc = doc_with(13, 7, Depth::Eight, &source);
        let flat = flatten(&doc, &ExportOptions::default(), 8);
        let bytes = encode_lossless(&flat, 8).unwrap();
        assert!(JxlCodec.probe(&bytes));
        let back = JxlCodec.import(&bytes).unwrap();
        assert_eq!(back.depth, Depth::Eight);
        assert_eq!((back.width, back.height), (13, 7));
        let want = doc.tree.layers[0].as_raster().unwrap();
        let got = back.tree.layers[0].as_raster().unwrap();
        for (x, y) in [(0, 0), (5, 3), (12, 6)] {
            let w = want.tiles.pixel(x, y).to_u8();
            let g = got.tiles.pixel(x, y).to_u8();
            // Fully transparent pixels need not keep their colour.
            if w[3] > 0 {
                assert_eq!(w, g, "pixel {x},{y}");
            }
        }
    }

    /// zune-jpegxl declared a 16-bit file's alpha as 8-bit, so every
    /// partly transparent pixel came back opaque.
    #[test]
    fn sixteen_bit_alpha_survives() {
        let source = gradient(9, 5, true);
        let doc = doc_with(9, 5, Depth::Sixteen, &source);
        let flat = flatten(
            &doc,
            &ExportOptions {
                dither: false,
                ..Default::default()
            },
            16,
        );
        let bytes = encode_lossless(&flat, 16).unwrap();
        let back = JxlCodec.import(&bytes).unwrap();
        assert_eq!(back.depth, Depth::Sixteen);
        let (want, got) = (pixels_of(&doc), pixels_of(&back));
        for (w, g) in want.iter().zip(&got) {
            assert!((w - g).abs() < 2.0 / 65535.0, "{w} came back as {g}");
        }
    }

    /// Without libjxl the export is lossless sRGB, so a profiled
    /// document's colours are converted into sRGB, not reinterpreted.
    #[test]
    fn the_fallback_converts_a_profiled_document_to_srgb() {
        let p3 = schist_colormgmt::Profile::display_p3()
            .icc_bytes()
            .unwrap()
            .to_vec();
        let mut doc = doc_with(2, 2, Depth::Eight, &[1.0, 0.0, 0.0, 1.0].repeat(4));
        doc.icc_profile = Some(p3);
        let bytes = export_lossless(&doc, &ExportOptions::default(), 8).unwrap();
        let back = JxlCodec.import(&bytes).unwrap();
        assert!(back.icc_profile.is_none());
        // P3 red lies outside sRGB: it clips to full red.
        let px = back.tree.layers[0]
            .as_raster()
            .unwrap()
            .tiles
            .pixel(0, 0)
            .to_u8();
        assert!(px[0] > 250 && px[1] < 10 && px[2] < 10, "{px:?}");
    }

    #[test]
    fn export_round_trips_through_whichever_encoder_is_present() {
        let source = gradient(16, 12, false);
        let doc = doc_with(16, 12, Depth::Eight, &source);
        let bytes = JxlCodec
            .export_with(
                &doc,
                &ExportOptions {
                    quality: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        let back = JxlCodec.import(&bytes).unwrap();
        assert_eq!((back.width, back.height), (16, 12));
        let (want, got) = (pixels_of(&doc), pixels_of(&back));
        for (w, g) in want.iter().zip(&got) {
            assert!((w - g).abs() < 1.5 / 255.0, "{w} came back as {g}");
        }
    }

    #[test]
    fn xmp_wraps_the_codestream_in_a_container() {
        let doc = doc_with(4, 4, Depth::Eight, &gradient(4, 4, false));
        let flat = flatten(&doc, &ExportOptions::default(), 8);
        let bare = encode_lossless(&flat, 8).unwrap();
        let tagged = with_xmp(&bare, b"<x:xmpmeta/>").unwrap();
        assert!(JxlCodec.probe(&tagged));
        assert!(tagged.ends_with(b"xml <x:xmpmeta/>"));
        let back = JxlCodec.import(&tagged).unwrap();
        assert_eq!((back.width, back.height), (4, 4));
    }

    #[test]
    fn distance_follows_cjxl() {
        assert_eq!(distance_for(100), 0.0);
        assert!((distance_for(90) - 1.0).abs() < 1e-6);
        assert!(distance_for(10) > distance_for(50));
    }
}
