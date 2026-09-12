//! HEIC/HEIF import on iOS and iPadOS, through ImageIO.
//!
//! The system decodes HEVC natively (it is the camera's own format), so
//! there is no library to download and nothing to dlopen: `CGImageSource`
//! reads the container, applies its rotation/mirror/crop items the way
//! libheif does on the desktop, and hands back a `CGImage` which is drawn
//! once into an RGBA bitmap in the image's own colour space, so the
//! pixels come out untouched and the profile rides along as ICC data for
//! the colour-managed pipeline. Alpha comes back premultiplied and is
//! divided out. Import only, like the desktop path.

use std::ffi::c_void;

use anyhow::Context as _;
use core_foundation::{
    base::{CFRelease, CFTypeRef, TCFType},
    data::{CFData, CFDataRef},
    dictionary::{CFDictionaryGetValue, CFDictionaryRef},
    number::{CFNumber, CFNumberRef},
    string::CFStringRef,
};
use core_graphics::{
    base::kCGImageAlphaPremultipliedLast,
    color_space::CGColorSpace,
    context::CGContext,
    geometry::{CGPoint, CGRect, CGSize},
    image::CGImage,
};
use foreign_types::ForeignType;
use schist_core::Document;

/// `kCGBitmapByteOrder32Big`: with premultiplied-last alpha, memory order
/// R, G, B, A.
const BYTE_ORDER_32_BIG: u32 = 4 << 12;
/// `kCGColorSpaceModelRGB`.
const COLOR_SPACE_MODEL_RGB: i32 = 1;

type CGImageSourceRef = *const c_void;

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CFDataRef, options: CFDictionaryRef) -> CGImageSourceRef;
    fn CGImageSourceGetCount(source: CGImageSourceRef) -> usize;
    fn CGImageSourceCreateImageAtIndex(
        source: CGImageSourceRef,
        index: usize,
        options: CFDictionaryRef,
    ) -> *mut c_void;
    fn CGImageSourceCopyPropertiesAtIndex(
        source: CGImageSourceRef,
        index: usize,
        options: CFDictionaryRef,
    ) -> CFDictionaryRef;
    static kCGImagePropertyOrientation: CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceGetModel(space: *mut c_void) -> i32;
    fn CGColorSpaceCopyICCData(space: *mut c_void) -> CFDataRef;
}

/// A CoreFoundation object released when dropped.
struct Released(CFTypeRef);

impl Drop for Released {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

pub(crate) fn import(bytes: &[u8]) -> anyhow::Result<Document> {
    unsafe {
        let data = CFData::from_buffer(bytes);
        let source = CGImageSourceCreateWithData(data.as_concrete_TypeRef(), std::ptr::null());
        anyhow::ensure!(
            !source.is_null(),
            "ImageIO could not read the HEIF container"
        );
        let source = Released(source);
        anyhow::ensure!(
            CGImageSourceGetCount(source.0) > 0,
            "the HEIF container holds no image"
        );

        let image = CGImageSourceCreateImageAtIndex(source.0, 0, std::ptr::null());
        anyhow::ensure!(
            !image.is_null(),
            "ImageIO could not decode the primary image"
        );
        let image = CGImage::from_ptr(image as *mut _);
        let (w, h) = (image.width(), image.height());
        anyhow::ensure!(w > 0 && h > 0, "zero-sized image");

        // Draw in the image's own RGB space so nothing is converted, and
        // keep that space's profile for the document. Anything else
        // (grey, CMYK, indexed) is converted to device RGB and left
        // unmanaged, which is at least the right picture.
        let space = image.color_space();
        let rgb = CGColorSpaceGetModel(space.as_ptr() as *mut c_void) == COLOR_SPACE_MODEL_RGB;
        let icc = if rgb {
            let icc = CGColorSpaceCopyICCData(space.as_ptr() as *mut c_void);
            (!icc.is_null()).then(|| CFData::wrap_under_create_rule(icc).bytes().to_vec())
        } else {
            None
        };
        let target = if rgb {
            space
        } else {
            CGColorSpace::create_device_rgb()
        };

        let mut context = CGContext::create_bitmap_context(
            None,
            w,
            h,
            8,
            w * 4,
            &target,
            kCGImageAlphaPremultipliedLast | BYTE_ORDER_32_BIG,
        );
        context.draw_image(
            CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(w as f64, h as f64)),
            &image,
        );
        let mut rgba = context.data().to_vec();
        anyhow::ensure!(
            rgba.len() == w * h * 4,
            "bitmap context has an unexpected size"
        );
        for px in rgba.as_chunks_mut::<4>().0 {
            if px[3] > 0 && px[3] < 255 {
                for c in 0..3 {
                    px[c] = (px[c] as u32 * 255 / px[3] as u32).min(255) as u8;
                }
            }
        }

        // ImageIO applies the container's own transformations but reports
        // an EXIF orientation as a property; honour that too.
        let orientation = orientation(source.0).unwrap_or(1);
        let (w, h, rgba) = reorient(w as u32, h as u32, rgba, orientation);

        crate::flat_document("HEIF", w, h, &rgba, icc).context("assembling document")
    }
}

unsafe fn orientation(source: CGImageSourceRef) -> Option<u32> {
    unsafe {
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, std::ptr::null());
        if properties.is_null() {
            return None;
        }
        let properties = Released(properties as CFTypeRef);
        let value = CFDictionaryGetValue(
            properties.0 as CFDictionaryRef,
            kCGImagePropertyOrientation as *const c_void,
        );
        if value.is_null() {
            return None;
        }
        let number = CFNumber::wrap_under_get_rule(value as CFNumberRef);
        number.to_i64().map(|n| n as u32)
    }
}

/// Applies an EXIF orientation (1..=8) to an RGBA8 buffer.
fn reorient(w: u32, h: u32, rgba: Vec<u8>, orientation: u32) -> (u32, u32, Vec<u8>) {
    if !(2..=8).contains(&orientation) {
        return (w, h, rgba);
    }
    let (ow, oh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // Where the source pixel (x, y) lands.
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            let from = ((y * w + x) * 4) as usize;
            let to = ((dy * ow + dx) * 4) as usize;
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    (ow, oh, out)
}
