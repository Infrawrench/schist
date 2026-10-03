//! AVIF import and export, in pure Rust.
//!
//! An AVIF still is one or more AV1 intra frames in a HEIF container
//! (`container`). Decoding uses rav1d, the Rust port of dav1d, built
//! without its assembly so that no assembler joins the toolchain;
//! encoding uses rav1e. rav1d does not compile for wasm32 (it leans on
//! C integer types the `libc` crate has no definition of there), so the
//! browser build can export AVIF but not open it.
//!
//! Import handles 8-, 10- and 12-bit files, every chroma layout, alpha
//! (straight or premultiplied), grids of tiles, the clean-aperture /
//! rotation / mirror transforms, an embedded ICC profile, and the nclx
//! colour description: PQ and HLG are baked to sRGB like HDR PNG and
//! HEIC, and any other non-sRGB space gets an ICC profile built from its
//! code points. Export writes 4:4:4 at 8, 10 or 12 bits with the
//! document's ICC profile, and quality 100 is lossless (RGB coded
//! directly, quantizer 0) at 8 and 10 bits; rav1e is not bit-exact at
//! 12, where it comes back within a couple of code values.

// The browser build has the encoder and none of the decoding path, so
// the reader's colour and layout handling goes unused there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use schist_color::Depth;
use schist_core::Document;
use schist_i18n::{t, tf};
use schist_plugin_api::{CodecPlugin, ExportOptions};

mod container;

pub struct AvifCodec;

impl CodecPlugin for AvifCodec {
    fn id(&self) -> &'static str {
        "codec.avif"
    }
    fn name(&self) -> &'static str {
        t("codec.avif.name")
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["avif"]
    }
    fn probe(&self, bytes: &[u8]) -> bool {
        container::is_avif(bytes)
    }
    fn import(&self, bytes: &[u8]) -> anyhow::Result<Document> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use anyhow::Context as _;
            import(bytes, self.name())
                .with_context(|| tf!("codec.msg.decoding", name = self.name()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = bytes;
            anyhow::bail!("{}", t("codec.avif.msg.no_decoder"))
        }
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
    fn supports_quality(&self) -> bool {
        true
    }
    fn supports_effort(&self) -> bool {
        true
    }
    fn bit_depths(&self) -> &'static [u8] {
        &[8, 10, 12]
    }
}

/// Decoded samples of one plane set, before colour conversion.
#[cfg(not(target_arch = "wasm32"))]
struct Raster {
    width: usize,
    height: usize,
    bits: u32,
    /// Three interleaved channels for colour, one for alpha, 0..1.
    channels: usize,
    data: Vec<f32>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Raster {
    fn transform(self, transform: container::Transform) -> Raster {
        use container::Transform;
        let (w, h, c) = (self.width, self.height, self.channels);
        let at = |x: usize, y: usize| &self.data[(y * w + x) * c..(y * w + x + 1) * c];
        let (nw, nh) = match transform {
            Transform::Crop { width, height, .. } => (width as usize, height as usize),
            Transform::Rotate(1 | 3) => (h, w),
            _ => (w, h),
        };
        let mut data = Vec::with_capacity(nw * nh * c);
        for y in 0..nh {
            for x in 0..nw {
                let (sx, sy) = match transform {
                    Transform::Crop { left, top, .. } => (x + left as usize, y + top as usize),
                    // Anti-clockwise quarter turns.
                    Transform::Rotate(1) => (w - 1 - y, x),
                    Transform::Rotate(2) => (w - 1 - x, h - 1 - y),
                    Transform::Rotate(3) => (y, h - 1 - x),
                    Transform::Rotate(_) => (x, y),
                    Transform::Mirror(true) => (w - 1 - x, y),
                    Transform::Mirror(false) => (x, h - 1 - y),
                };
                data.extend_from_slice(at(sx, sy));
            }
        }
        Raster {
            width: nw,
            height: nh,
            data,
            ..self
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn import(bytes: &[u8], title: &str) -> anyhow::Result<Document> {
    let avif = container::read(bytes)?;
    let color = decode_image(&avif.color, false)?;
    let alpha = avif
        .alpha
        .as_ref()
        .map(|alpha| decode_image(alpha, true))
        .transpose()?;
    // The primary item's transforms describe the whole image; the alpha
    // plane is coded at the same orientation.
    let (mut color, mut alpha) = (color, alpha);
    for transform in &avif.color.transforms {
        color = color.transform(*transform);
        alpha = alpha.map(|a| a.transform(*transform));
    }
    let (w, h) = (color.width, color.height);
    anyhow::ensure!(w > 0 && h > 0, "{}", t("codec.msg.zero_sized"));
    if let Some(a) = &alpha {
        anyhow::ensure!(
            (a.width, a.height) == (w, h),
            "AVIF alpha plane is {}x{}, the image {w}x{h}",
            a.width,
            a.height
        );
    }

    let mut rgba = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        let a = alpha.as_ref().map_or(1.0, |a| a.data[i]);
        let scale = if avif.premultiplied && a > 0.0 {
            1.0 / a
        } else {
            1.0
        };
        rgba.extend(color.data[i * 3..i * 3 + 3].iter().map(|c| c * scale));
        rgba.push(a);
    }

    let nclx = avif.color.nclx;
    let mut icc = avif.color.icc.clone();
    if icc.is_none() {
        if let Some(nclx) = nclx {
            if let transfer @ (16 | 18) = nclx.transfer {
                match schist_colormgmt::bake_hdr_to_srgb(
                    &mut rgba,
                    nclx.primaries as u8,
                    transfer as u8,
                ) {
                    Ok(()) => {
                        let bytes: Vec<u8> = rgba
                            .iter()
                            .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                            .collect();
                        return crate::flat_document(title, w as u32, h as u32, &bytes, None);
                    }
                    Err(err) => log::warn!("displaying HDR {title} unmapped: {err:#}"),
                }
            }
            icc = profile_for(nclx);
        }
    }

    if color.bits <= 8 {
        let bytes: Vec<u8> = rgba
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
            .collect();
        crate::flat_document(title, w as u32, h as u32, &bytes, icc)
    } else {
        for v in &mut rgba {
            *v = v.clamp(0.0, 1.0);
        }
        crate::deep_document(title, w as u32, h as u32, &rgba, Depth::Sixteen, icc)
    }
}

/// An ICC profile for an nclx colour space that is not sRGB, so the
/// display transform knows what the numbers mean. sRGB, and the
/// "unspecified" code points every decoder reads as sRGB, need none.
fn profile_for(nclx: container::Nclx) -> Option<Vec<u8>> {
    use moxcms::{
        CicpColorPrimaries, CicpProfile, ColorProfile, MatrixCoefficients, TransferCharacteristics,
    };
    if matches!(nclx.primaries, 1 | 2) && matches!(nclx.transfer, 2 | 13) {
        return None;
    }
    let primaries = match nclx.primaries {
        2 => 1,
        p => p,
    };
    let transfer = match nclx.transfer {
        2 => 13,
        t => t,
    };
    let profile = ColorProfile::new_from_cicp(CicpProfile {
        color_primaries: CicpColorPrimaries::try_from(primaries as u8).ok()?,
        transfer_characteristics: TransferCharacteristics::try_from(transfer as u8).ok()?,
        matrix_coefficients: MatrixCoefficients::Identity,
        full_range: true,
    });
    profile.encode().ok()
}

/// Decode a coded image or a grid of them to RGB (or, for an alpha
/// plane, a single channel), stitching and cropping grids.
#[cfg(not(target_arch = "wasm32"))]
fn decode_image(image: &container::Image, alpha: bool) -> anyhow::Result<Raster> {
    let Some(grid) = &image.grid else {
        return decode_coded(&image.data, image.nclx, alpha);
    };
    let tiles = grid
        .tiles
        .iter()
        .map(|tile| decode_coded(&tile.data, tile.nclx.or(image.nclx), alpha))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let first = tiles
        .first()
        .ok_or_else(|| anyhow::anyhow!("empty AVIF grid"))?;
    let (tw, th, c) = (first.width, first.height, first.channels);
    let (w, h) = (image.width as usize, image.height as usize);
    anyhow::ensure!(
        tw * grid.columns as usize >= w && th * grid.rows as usize >= h,
        "AVIF grid tiles do not cover the image"
    );
    let mut data = vec![0f32; w * h * c];
    for (i, tile) in tiles.iter().enumerate() {
        anyhow::ensure!(
            (tile.width, tile.height) == (tw, th),
            "AVIF grid tiles differ in size"
        );
        let (left, top) = (
            (i % grid.columns as usize) * tw,
            (i / grid.columns as usize) * th,
        );
        for y in 0..th.min(h.saturating_sub(top)) {
            let span = tw.min(w.saturating_sub(left)) * c;
            let src = y * tw * c;
            let dst = ((top + y) * w + left) * c;
            data[dst..dst + span].copy_from_slice(&tile.data[src..src + span]);
        }
    }
    Ok(Raster {
        width: w,
        height: h,
        bits: first.bits,
        channels: c,
        data,
    })
}

/// Decode one AV1 image and convert it out of YUV.
#[cfg(not(target_arch = "wasm32"))]
fn decode_coded(obus: &[u8], nclx: Option<container::Nclx>, alpha: bool) -> anyhow::Result<Raster> {
    let frame = av1::decode(obus)?;
    let (w, h) = (frame.width, frame.height);
    let max = ((1u32 << frame.bits) - 1) as f32;
    // The container's colour box wins over the bitstream's, as the
    // AVIF specification has it.
    let (matrix, full_range) = match nclx {
        Some(n) => (n.matrix, n.full_range),
        None => (frame.matrix, frame.full_range),
    };
    let shift = frame.bits - 8;
    let (luma_offset, luma_range, chroma_range) = if full_range {
        (0.0, max, max)
    } else {
        (
            (16u32 << shift) as f32,
            (219u32 << shift) as f32,
            (224u32 << shift) as f32,
        )
    };
    let luma = |v: u16| (v as f32 - luma_offset) / luma_range;

    if alpha {
        let data = frame.planes[0].iter().map(|&v| luma(v)).collect();
        return Ok(Raster {
            width: w,
            height: h,
            bits: frame.bits,
            channels: 1,
            data,
        });
    }
    let half = (1u32 << (frame.bits - 1)) as f32;
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let yv = frame.planes[0][y * w + x];
            if frame.monochrome {
                let v = luma(yv);
                data.extend([v; 3]);
                continue;
            }
            let (u, v) = frame.chroma(x, y);
            let rgb = match matrix {
                // RGB coded as is: G in Y, B in U, R in V.
                0 => [
                    (v - luma_offset) / luma_range,
                    luma(yv),
                    (u - luma_offset) / luma_range,
                ],
                // YCgCo
                8 => {
                    let (yy, cg, co) = (
                        luma(yv),
                        (u - half) / chroma_range,
                        (v - half) / chroma_range,
                    );
                    let t = yy - cg;
                    [t + co, yy + cg, t - co]
                }
                m => {
                    let (kr, kb) = match m {
                        1 => (0.2126, 0.0722),
                        4 => (0.30, 0.11),
                        7 => (0.212, 0.087),
                        9 | 10 => (0.2627, 0.0593),
                        // BT.601, which is also what decoders assume
                        // for "unspecified".
                        _ => (0.299, 0.114),
                    };
                    let kg = 1.0 - kr - kb;
                    let (yy, cb, cr) = (
                        luma(yv),
                        (u - half) / chroma_range,
                        (v - half) / chroma_range,
                    );
                    let r = yy + 2.0 * (1.0 - kr) * cr;
                    let b = yy + 2.0 * (1.0 - kb) * cb;
                    [r, (yy - kr * r - kb * b) / kg, b]
                }
            };
            data.extend(rgb);
        }
    }
    Ok(Raster {
        width: w,
        height: h,
        bits: frame.bits,
        channels: 3,
        data,
    })
}

/// rav1d, the Rust port of dav1d, through its C-shaped API.
#[cfg(not(target_arch = "wasm32"))]
mod av1 {
    use std::ptr::NonNull;

    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
    use rav1d::include::dav1d::headers::{
        DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
    };
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use rav1d::src::lib::{
        dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings,
        dav1d_get_picture, dav1d_open, dav1d_picture_unref, dav1d_send_data,
    };

    /// A frame's planes, copied out of the decoder's buffers.
    pub struct Frame {
        pub width: usize,
        pub height: usize,
        pub bits: u32,
        pub monochrome: bool,
        /// Chroma subsampling shifts.
        pub ss: (u32, u32),
        pub planes: [Vec<u16>; 3],
        pub matrix: u16,
        pub full_range: bool,
    }

    impl Frame {
        /// Chroma at luma position (`x`, `y`), interpolated between the
        /// subsampled samples, which sit at the centre of each block.
        pub fn chroma(&self, x: usize, y: usize) -> (f32, f32) {
            let (sx, sy) = self.ss;
            let cw = (self.width + (1 << sx) - 1) >> sx;
            let ch = (self.height + (1 << sy) - 1) >> sy;
            let pos = |p: usize, s: u32, n: usize| -> (usize, usize, f32) {
                if s == 0 {
                    return (p, p, 0.0);
                }
                let f = ((p as f32 + 0.5) / (1 << s) as f32 - 0.5).clamp(0.0, (n - 1) as f32);
                let i = f.floor() as usize;
                (i, (i + 1).min(n - 1), f - i as f32)
            };
            let (x0, x1, fx) = pos(x, sx, cw);
            let (y0, y1, fy) = pos(y, sy, ch);
            let sample = |plane: &[u16]| {
                let at = |x: usize, y: usize| plane[y * cw + x] as f32;
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                top * (1.0 - fy) + bottom * fy
            };
            (sample(&self.planes[1]), sample(&self.planes[2]))
        }
    }

    /// A ceiling on the frame a file may ask for, well past any camera.
    const MAX_PIXELS: u32 = 1 << 28;

    pub fn decode(obus: &[u8]) -> anyhow::Result<Frame> {
        anyhow::ensure!(!obus.is_empty(), "AVIF image has no data");
        unsafe {
            let mut settings = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
            dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap());
            let mut settings = settings.assume_init();
            settings.n_threads =
                std::thread::available_parallelism().map_or(1, |n| n.get().min(8)) as _;
            settings.max_frame_delay = 1;
            settings.frame_size_limit = MAX_PIXELS;

            let mut context: Option<Dav1dContext> = None;
            let status = dav1d_open(NonNull::new(&mut context), NonNull::new(&mut settings));
            anyhow::ensure!(status.0 == 0, "AV1 decoder failed to start ({})", status.0);
            struct Close(Option<Dav1dContext>);
            impl Drop for Close {
                fn drop(&mut self) {
                    unsafe { dav1d_close(NonNull::new(&mut self.0)) }
                }
            }
            let context = Close(context);
            let ctx = context.0;

            let mut data = Dav1dData::default();
            let buffer = dav1d_data_create(NonNull::new(&mut data), obus.len());
            anyhow::ensure!(!buffer.is_null(), "AV1 decoder out of memory");
            std::ptr::copy_nonoverlapping(obus.as_ptr(), buffer, obus.len());

            let mut picture = Dav1dPicture::default();
            let mut result = Err(anyhow::anyhow!("AV1 data held no picture"));
            // Feed everything, then drain: a still is one temporal
            // unit, so a picture arrives once the data is in.
            for _ in 0..1024 {
                if data.sz > 0 {
                    let status = dav1d_send_data(ctx, NonNull::new(&mut data));
                    if status.0 < 0 && status.0 != -(libc::EAGAIN) {
                        result = Err(anyhow::anyhow!("AV1 data is corrupt ({})", status.0));
                        break;
                    }
                }
                let status = dav1d_get_picture(ctx, NonNull::new(&mut picture));
                if status.0 == 0 {
                    result = copy(&picture);
                    dav1d_picture_unref(NonNull::new(&mut picture));
                    break;
                }
                if status.0 != -(libc::EAGAIN) {
                    result = Err(anyhow::anyhow!("AV1 data is corrupt ({})", status.0));
                    break;
                }
            }
            dav1d_data_unref(NonNull::new(&mut data));
            drop(context);
            result
        }
    }

    unsafe fn copy(picture: &Dav1dPicture) -> anyhow::Result<Frame> {
        let p = &picture.p;
        let (width, height) = (p.w as usize, p.h as usize);
        let bits = p.bpc as u32;
        anyhow::ensure!(
            width > 0 && height > 0 && matches!(bits, 8 | 10 | 12),
            "AV1 picture is {width}x{height} at {bits} bits"
        );
        let monochrome = p.layout == DAV1D_PIXEL_LAYOUT_I400;
        let ss = match p.layout {
            DAV1D_PIXEL_LAYOUT_I420 => (1, 1),
            DAV1D_PIXEL_LAYOUT_I422 => (1, 0),
            _ => (0, 0),
        };
        let wide = bits > 8;
        let plane = |index: usize, w: usize, h: usize, stride: isize| -> anyhow::Result<Vec<u16>> {
            let base = picture.data[index]
                .ok_or_else(|| anyhow::anyhow!("AV1 picture lacks plane {index}"))?
                .as_ptr() as *const u8;
            let mut out = Vec::with_capacity(w * h);
            for y in 0..h {
                let row = unsafe { base.offset(y as isize * stride) };
                if wide {
                    let row = unsafe { std::slice::from_raw_parts(row as *const u16, w) };
                    out.extend_from_slice(row);
                } else {
                    let row = unsafe { std::slice::from_raw_parts(row, w) };
                    out.extend(row.iter().map(|&v| v as u16));
                }
            }
            Ok(out)
        };
        let luma = plane(0, width, height, picture.stride[0])?;
        let (cw, ch) = (
            (width + (1 << ss.0) - 1) >> ss.0,
            (height + (1 << ss.1) - 1) >> ss.1,
        );
        let (u, v) = if monochrome {
            (Vec::new(), Vec::new())
        } else {
            (
                plane(1, cw, ch, picture.stride[1])?,
                plane(2, cw, ch, picture.stride[1])?,
            )
        };
        let (matrix, full_range) = picture
            .seq_hdr
            .map(|h| {
                let h = unsafe { h.as_ref() };
                (h.mtrx as u16, h.color_range != 0)
            })
            .unwrap_or((2, true));
        Ok(Frame {
            width,
            height,
            bits,
            monochrome,
            ss,
            planes: [luma, u, v],
            matrix,
            full_range,
        })
    }
}

/// A JPEG-style quality (1..=100) to rav1e's quantizer (255..=0).
fn quantizer_for(quality: u8) -> usize {
    let q = quality.clamp(1, 100) as f32 / 100.0;
    (255.0 * (1.0 - q).powf(0.6)).round() as usize
}

fn export(doc: &Document, options: &ExportOptions) -> anyhow::Result<Vec<u8>> {
    let bits: u32 = match options.bit_depth {
        0..=8 => 8,
        9..=10 => 10,
        _ => 12,
    };
    let flat = crate::jxl::flatten(doc, options, bits);
    let lossless = options.quality >= 100;
    let quantizer = quantizer_for(options.quality);
    // Higher effort is slower and smaller, which is rav1e's speed the
    // other way round.
    let speed = (11 - options.effort.clamp(1, 10) as i32).clamp(1, 10) as u8;
    let max = ((1u32 << bits) - 1) as f32;
    let (w, h) = (flat.width as usize, flat.height as usize);

    // Lossless AV1 needs the samples coded as they are: RGB through the
    // identity matrix. Lossy uses BT.601 YCbCr, full range.
    let matrix = if lossless { 0 } else { 6 };
    let mut planes = [vec![0u16; w * h], vec![0u16; w * h], vec![0u16; w * h]];
    let quantise = |v: f32| (v.clamp(0.0, 1.0) * max + 0.5) as u16;
    for (i, px) in flat.pixels.as_chunks::<4>().0.iter().enumerate() {
        let (r, g, b) = (
            px[0].clamp(0.0, 1.0),
            px[1].clamp(0.0, 1.0),
            px[2].clamp(0.0, 1.0),
        );
        let (y, u, v) = if lossless {
            (g, b, r)
        } else {
            let y = 0.299 * r + 0.587 * g + 0.114 * b;
            (
                y,
                (b - y) / (2.0 * (1.0 - 0.114)) + 0.5,
                (r - y) / (2.0 * (1.0 - 0.299)) + 0.5,
            )
        };
        planes[0][i] = quantise(y);
        planes[1][i] = quantise(u);
        planes[2][i] = quantise(v);
    }
    let icc = doc.icc_profile.as_deref();
    let nclx = container::Nclx {
        // With a profile embedded the nclx box only carries the matrix.
        primaries: if icc.is_some() { 2 } else { 1 },
        transfer: if icc.is_some() { 2 } else { 13 },
        matrix,
        full_range: true,
    };
    let (color, color_config) = encode_av1(w, h, bits, &planes, quantizer, speed, Some(nclx))?;
    let alpha = if flat.opaque {
        None
    } else {
        let plane: Vec<u16> = flat
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| quantise(px[3]))
            .collect();
        Some(encode_av1(
            w,
            h,
            bits,
            std::slice::from_ref(&plane),
            quantizer,
            speed,
            None,
        )?)
    };
    let color = container::Coded {
        data: &color,
        config: &color_config,
        depth: bits as u8,
        channels: 3,
    };
    let alpha = alpha.as_ref().map(|(data, config)| container::Coded {
        data,
        config,
        depth: bits as u8,
        channels: 1,
    });
    Ok(container::write(
        flat.width,
        flat.height,
        &color,
        alpha.as_ref(),
        nclx,
        icc,
        None,
    ))
}

/// The same file with an XMP packet attached (export recipes keep the
/// copyright notice this way).
pub fn with_xmp(file: &[u8], xmp: &[u8]) -> anyhow::Result<Vec<u8>> {
    container::with_xmp(file, xmp)
}

/// One AV1 still from full-resolution planes: three for 4:4:4 colour,
/// one for monochrome. Returns the bitstream and the `av1C` record.
fn encode_av1(
    width: usize,
    height: usize,
    bits: u32,
    planes: &[Vec<u16>],
    quantizer: usize,
    speed: u8,
    nclx: Option<container::Nclx>,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    use rav1e::prelude::*;

    let mut config = EncoderConfig::with_speed_preset(speed);
    config.width = width;
    config.height = height;
    config.bit_depth = bits as usize;
    config.still_picture = true;
    config.quantizer = quantizer;
    config.min_quantizer = quantizer as u8;
    config.pixel_range = PixelRange::Full;
    config.chroma_sampling = if planes.len() == 1 {
        ChromaSampling::Cs400
    } else {
        ChromaSampling::Cs444
    };
    if let Some(nclx) = nclx {
        config.color_description = Some(ColorDescription {
            color_primaries: if nclx.primaries == 1 {
                ColorPrimaries::BT709
            } else {
                ColorPrimaries::Unspecified
            },
            transfer_characteristics: if nclx.transfer == 13 {
                TransferCharacteristics::SRGB
            } else {
                TransferCharacteristics::Unspecified
            },
            matrix_coefficients: if nclx.matrix == 0 {
                MatrixCoefficients::Identity
            } else {
                MatrixCoefficients::BT601
            },
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        config.tiles = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    }
    let config = Config::new().with_encoder_config(config);

    fn run<T: Pixel>(
        config: &Config,
        planes: &[Vec<u16>],
        width: usize,
        height: usize,
    ) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
        let mut context: Context<T> = config
            .new_context()
            .map_err(|e| anyhow::anyhow!("AV1 encoder: {e}"))?;
        let mut frame = context.new_frame();
        let wide = std::mem::size_of::<T>() == 2;
        for (plane, samples) in frame.planes.iter_mut().zip(planes) {
            let bytes: Vec<u8> = if wide {
                samples.iter().flat_map(|v| v.to_ne_bytes()).collect()
            } else {
                samples.iter().map(|&v| v as u8).collect()
            };
            let width_bytes = if wide { 2 } else { 1 };
            plane.copy_from_raw_u8(&bytes, width * width_bytes, width_bytes);
        }
        let _ = height;
        context
            .send_frame(frame)
            .map_err(|e| anyhow::anyhow!("AV1 encoder: {e}"))?;
        context.flush();
        let mut data = Vec::new();
        loop {
            match context.receive_packet() {
                Ok(packet) => data.extend_from_slice(&packet.data),
                Err(EncoderStatus::Encoded) => {}
                Err(EncoderStatus::LimitReached) => break,
                Err(err) => anyhow::bail!("AV1 encoder: {err}"),
            }
        }
        Ok((data, context.container_sequence_header()))
    }
    if bits > 8 {
        run::<u16>(&config, planes, width, height)
    } else {
        run::<u8>(&config, planes, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_core::{blit_rgba_f32, IntRect, Layer};

    fn document(w: u32, h: u32, depth: Depth, alpha: bool) -> Document {
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if alpha && x >= w / 2 { 0.4 } else { 1.0 };
                pixels.extend([x as f32 / w as f32, y as f32 / h as f32, 0.7, a]);
            }
        }
        let mut doc = Document::new("t", w, h, depth);
        let mut layer = Layer::new_raster("l");
        blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            depth,
            IntRect::from_size(w, h),
            &pixels,
        );
        doc.push_layer(layer);
        doc
    }

    fn max_error(a: &Document, b: &Document) -> f32 {
        let a = schist_compositor::composite_region_f32(a, a.canvas_rect());
        let b = schist_compositor::composite_region_f32(b, b.canvas_rect());
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    fn options(quality: u8, bit_depth: u8) -> ExportOptions {
        ExportOptions {
            quality,
            bit_depth,
            dither: false,
            effort: 1,
        }
    }

    #[test]
    fn lossless_round_trip_is_exact() {
        for bits in [8, 10, 12] {
            let doc = document(17, 9, Depth::Sixteen, true);
            let bytes = AvifCodec.export_with(&doc, &options(100, bits)).unwrap();
            assert!(AvifCodec.probe(&bytes));
            let back = AvifCodec.import(&bytes).unwrap();
            assert_eq!((back.width, back.height), (17, 9));
            let want = if bits == 8 {
                Depth::Eight
            } else {
                Depth::Sixteen
            };
            assert_eq!(back.depth, want, "{bits}-bit");
            let step = 1.0 / ((1u32 << bits) - 1) as f32;
            // rav1e's quantizer 0 is exact at 8 and 10 bits; at 12 it
            // lands within a code value or two, which is still finer
            // than 10-bit lossless.
            let codes = if bits == 12 { 2.5 } else { 0.51 };
            let err = max_error(&doc, &back);
            assert!(err <= step * codes + 1e-4, "{bits}-bit: {err}");
        }
    }

    #[test]
    fn lossy_round_trip_is_close() {
        let doc = document(40, 24, Depth::Eight, true);
        let bytes = AvifCodec.export_with(&doc, &options(90, 8)).unwrap();
        let back = AvifCodec.import(&bytes).unwrap();
        let err = max_error(&doc, &back);
        assert!(err < 0.08, "{err}");
        // Lower quality makes a smaller file.
        let small = AvifCodec.export_with(&doc, &options(30, 8)).unwrap();
        assert!(small.len() < bytes.len());
    }

    #[test]
    fn opaque_images_carry_no_alpha_plane() {
        let doc = document(8, 8, Depth::Eight, false);
        let bytes = AvifCodec.export_with(&doc, &options(80, 8)).unwrap();
        let avif = container::read(&bytes).unwrap();
        assert!(avif.alpha.is_none());
        let doc = document(8, 8, Depth::Eight, true);
        let bytes = AvifCodec.export_with(&doc, &options(80, 8)).unwrap();
        assert!(container::read(&bytes).unwrap().alpha.is_some());
    }

    #[test]
    fn the_profile_survives_a_round_trip() {
        let p3 = schist_colormgmt::Profile::display_p3()
            .icc_bytes()
            .unwrap()
            .to_vec();
        let mut doc = document(8, 8, Depth::Eight, false);
        doc.icc_profile = Some(p3.clone());
        let bytes = AvifCodec.export_with(&doc, &options(100, 8)).unwrap();
        let back = AvifCodec.import(&bytes).unwrap();
        assert_eq!(back.icc_profile.as_deref(), Some(p3.as_slice()));
        assert!(max_error(&doc, &back) < 1e-3);
    }

    /// Files from another encoder (libheif with libaom, through
    /// ImageMagick): left half (200,30,40), right half (10,200,90), the
    /// right half half-transparent in `alpha.avif`.
    fn fixture(name: &str) -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/avif")
            .join(name);
        let bytes = std::fs::read(path).unwrap();
        assert!(AvifCodec.probe(&bytes));
        AvifCodec.import(&bytes).unwrap()
    }

    fn close(got: [u8; 4], want: [u8; 4]) -> bool {
        got.iter()
            .zip(want)
            .all(|(g, w)| (*g as i32 - w as i32).abs() <= 6)
    }

    #[test]
    fn decodes_files_from_libaom() {
        let doc = fixture("alpha.avif");
        assert_eq!((doc.width, doc.height, doc.depth), (16, 8, Depth::Eight));
        let tiles = &doc.tree.layers[0].as_raster().unwrap().tiles;
        let left = tiles.pixel(2, 4).to_u8();
        let right = tiles.pixel(13, 4).to_u8();
        assert!(close(left, [200, 30, 40, 255]), "{left:?}");
        assert!(close(right, [10, 200, 90, 128]), "{right:?}");

        let doc = fixture("ten_bit.avif");
        assert_eq!(doc.depth, Depth::Sixteen);
        let tiles = &doc.tree.layers[0].as_raster().unwrap().tiles;
        assert!(close(tiles.pixel(2, 4).to_u8(), [200, 30, 40, 255]));
        assert!(close(tiles.pixel(13, 4).to_u8(), [10, 200, 90, 255]));
    }

    #[test]
    fn nclx_spaces_other_than_srgb_get_a_profile() {
        let srgb = container::Nclx {
            primaries: 1,
            transfer: 13,
            matrix: 6,
            full_range: true,
        };
        assert!(profile_for(srgb).is_none());
        assert!(profile_for(container::Nclx {
            primaries: 2,
            transfer: 2,
            ..srgb
        })
        .is_none());
        let p3 = profile_for(container::Nclx {
            primaries: 12,
            ..srgb
        })
        .expect("Display P3 needs a profile");
        assert_eq!(&p3[36..40], b"acsp");
    }

    #[test]
    fn transforms_rotate_and_mirror() {
        use container::Transform;
        // 2x1: [a, b]
        let raster = || Raster {
            width: 2,
            height: 1,
            bits: 8,
            channels: 1,
            data: vec![1.0, 2.0],
        };
        let r = raster().transform(Transform::Rotate(1));
        // A quarter turn anti-clockwise puts b on top.
        assert_eq!((r.width, r.height, r.data), (1, 2, vec![2.0, 1.0]));
        let r = raster().transform(Transform::Mirror(true));
        assert_eq!(r.data, vec![2.0, 1.0]);
        let r = raster().transform(Transform::Crop {
            left: 1,
            top: 0,
            width: 1,
            height: 1,
        });
        assert_eq!(r.data, vec![2.0]);
    }
}
