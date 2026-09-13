//! AVFoundation decodes local tracks into CoreVideo pixel buffers. All
//! native objects stay on the worker thread that created the decoder.
use super::*;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, Encode, Encoding, RefEncode};
use objc2_foundation::NSString;
use std::ffi::c_void;
use std::ptr;

#[repr(C)]
#[derive(Clone, Copy)]
struct Time {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}
unsafe impl Encode for Time {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[
            Encoding::LongLong,
            Encoding::Int,
            Encoding::UInt,
            Encoding::LongLong,
        ],
    );
}
#[repr(C)]
#[derive(Clone, Copy)]
struct TimeRange {
    start: Time,
    duration: Time,
}
unsafe impl Encode for TimeRange {
    const ENCODING: Encoding = Encoding::Struct("?", &[Time::ENCODING, Time::ENCODING]);
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}
unsafe impl Encode for Transform {
    const ENCODING: Encoding =
        Encoding::Struct("CGAffineTransform", &[const { Encoding::Double }; 6]);
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVURLAssetPreferPreciseDurationAndTimingKey: &'static NSString;
    static AVMediaTypeVideo: &'static NSString;
}
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMTimeMakeWithSeconds(seconds: f64, timescale: i32) -> Time;
    fn CMTimeGetSeconds(time: Time) -> f64;
    fn CMSampleBufferGetPresentationTimeStamp(sample: *const c_void) -> Time;
    fn CMSampleBufferGetImageBuffer(sample: *const c_void) -> *mut c_void;
    fn CMSampleBufferGetFormatDescription(sample: *const c_void) -> *const c_void;
    fn CMVideoFormatDescriptionGetPresentationDimensions(
        format: *const c_void,
        aspect: bool,
        aperture: bool,
    ) -> Size;
}
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: *const AnyObject;
    fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *const u8;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const c_void);
}
#[repr(C)]
struct SampleBuffer {
    _opaque: [u8; 0],
}
unsafe impl RefEncode for SampleBuffer {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}
struct Sample(*const c_void);
impl Drop for Sample {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
struct LockedPixels(*mut c_void);
impl Drop for LockedPixels {
    fn drop(&mut self) {
        unsafe {
            CVPixelBufferUnlockBaseAddress(self.0, 1);
        }
    }
}

fn class(name: &'static std::ffi::CStr) -> Result<&'static AnyClass> {
    AnyClass::get(name).context(t("video.decode_failed"))
}
unsafe fn native_error(error: *mut AnyObject) -> anyhow::Error {
    if !error.is_null() {
        let text: Option<Retained<NSString>> = msg_send![error, localizedDescription];
        if let Some(text) = text {
            return anyhow::anyhow!("{}: {}", t("video.decode_failed"), text);
        }
    }
    anyhow::anyhow!("{}", t("video.decode_failed"))
}
unsafe fn asset_and_track(path: &Path) -> Result<(Retained<AnyObject>, Retained<AnyObject>)> {
    let path = local_file(path)?;
    let path = NSString::from_str(path.to_str().context(t("video.invalid_file"))?);
    let url: Retained<AnyObject> = msg_send![class(c"NSURL")?, fileURLWithPath: &*path];
    let precise: Retained<AnyObject> = msg_send![class(c"NSNumber")?, numberWithBool: Bool::YES];
    let options: Retained<AnyObject> = msg_send![class(c"NSDictionary")?, dictionaryWithObject: &*precise, forKey: AVURLAssetPreferPreciseDurationAndTimingKey];
    let asset: Retained<AnyObject> =
        msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*url, options: &*options];
    let tracks: Retained<AnyObject> = msg_send![&*asset, tracksWithMediaType: AVMediaTypeVideo];
    let count: usize = msg_send![&*tracks, count];
    ensure!(count > 0, "{}", t("video.no_stream"));
    let track: Retained<AnyObject> = msg_send![&*tracks, objectAtIndex: 0usize];
    Ok((asset, track))
}
unsafe fn duration(asset: &AnyObject) -> Result<f64> {
    let duration: Time = msg_send![asset, duration];
    let duration = CMTimeGetSeconds(duration);
    ensure!(
        duration.is_finite() && duration > 0.0,
        "{}",
        t("video.no_duration")
    );
    Ok(duration)
}

pub fn probe(path: &Path, job: Arc<Job>) -> Result<Info> {
    ensure!(!job.cancelled(), "{}", t("common.cancel"));
    autoreleasepool(|_| unsafe {
        let (asset, _) = asset_and_track(path)?;
        Ok(Info {
            duration: duration(&asset)?,
        })
    })
}

pub struct Decoder {
    reader: Retained<AnyObject>,
    output: Retained<AnyObject>,
    matrix: [i32; 4],
    edge: u32,
    start: f64,
    end: f64,
    job: Arc<Job>,
}
impl Decoder {
    pub fn open(
        path: &Path,
        start: f64,
        span: Option<f64>,
        edge: u32,
        job: Arc<Job>,
    ) -> Result<Self> {
        ensure!(!job.cancelled(), "{}", t("common.cancel"));
        autoreleasepool(|_| unsafe {
            let (asset, track) = asset_and_track(path)?;
            let duration = duration(&asset)?;
            let start = start.max(0.0);
            let end = span.map_or(duration, |span| (start + span).min(duration));
            let transform: Transform = msg_send![&*track, preferredTransform];
            let matrix = [transform.a, transform.b, transform.c, transform.d].map(|v| {
                if v > 0.5 {
                    1
                } else if v < -0.5 {
                    -1
                } else {
                    0
                }
            });
            let mut error: *mut AnyObject = ptr::null_mut();
            let reader: *mut AnyObject = msg_send![class(c"AVAssetReader")?, alloc];
            let reader: *mut AnyObject =
                msg_send![reader, initWithAsset: &*asset, error: &mut error];
            let reader = Retained::from_raw(reader).ok_or_else(|| native_error(error))?;
            let format: Retained<AnyObject> =
                msg_send![class(c"NSNumber")?, numberWithUnsignedInt: u32::from_be_bytes(*b"BGRA")];
            let settings: Retained<AnyObject> = msg_send![class(c"NSDictionary")?, dictionaryWithObject: &*format, forKey: kCVPixelBufferPixelFormatTypeKey];
            let output: *mut AnyObject = msg_send![class(c"AVAssetReaderTrackOutput")?, alloc];
            let output: *mut AnyObject =
                msg_send![output, initWithTrack: &*track, outputSettings: &*settings];
            let output = Retained::from_raw(output).context(t("video.decode_failed"))?;
            let _: () = msg_send![&*output, setAlwaysCopiesSampleData: Bool::NO];
            let can_add: Bool = msg_send![&*reader, canAddOutput: &*output];
            ensure!(can_add.as_bool(), "{}", t("video.decode_failed"));
            let _: () = msg_send![&*reader, addOutput: &*output];
            // A range starting between frames clips the preceding sample and
            // replaces its PTS with the range start. Begin at an exact sample
            // boundary, then filter original timestamps in next(). For formats
            // without precise sample cursors, decoding from zero is the safe fallback.
            let mut decode_start = CMTimeMakeWithSeconds(0.0, 1);
            let precise: Bool = msg_send![&*asset, providesPreciseDurationAndTiming];
            let cursors: Bool = msg_send![&*track, canProvideSampleCursors];
            if start > 0.0 && precise.as_bool() && cursors.as_bool() {
                let cursor: Option<Retained<AnyObject>> = msg_send![&*track, makeSampleCursorWithPresentationTimeStamp: CMTimeMakeWithSeconds(start, 1_000_000_000)];
                if let Some(cursor) = cursor {
                    let at: Time = msg_send![&*cursor, presentationTimeStamp];
                    let seconds = CMTimeGetSeconds(at);
                    if seconds.is_finite() && seconds >= 0.0 && seconds <= start {
                        decode_start = at;
                    }
                }
            }
            let range = TimeRange {
                start: decode_start,
                duration: CMTimeMakeWithSeconds(
                    (end - CMTimeGetSeconds(decode_start)).max(0.000001),
                    1_000_000_000,
                ),
            };
            let _: () = msg_send![&*reader, setTimeRange: range];
            let started: Bool = msg_send![&*reader, startReading];
            if !started.as_bool() {
                let error: *mut AnyObject = msg_send![&*reader, error];
                return Err(native_error(error));
            }
            Ok(Self {
                reader,
                output,
                matrix,
                edge,
                start,
                end,
                job,
            })
        })
    }
    pub fn next(&mut self) -> Result<Option<Frame>> {
        autoreleasepool(|_| unsafe {
            loop {
                if self.job.cancelled() {
                    return Ok(None);
                }
                let raw: *const SampleBuffer = msg_send![&*self.output, copyNextSampleBuffer];
                if raw.is_null() {
                    let status: isize = msg_send![&*self.reader, status];
                    if status == 3 {
                        // AVAssetReaderStatusFailed
                        let error: *mut AnyObject = msg_send![&*self.reader, error];
                        return Err(native_error(error));
                    }
                    return Ok(None);
                }
                let sample = Sample(raw.cast());
                let time = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample.0));
                ensure!(time.is_finite(), "{}", t("video.decode_failed"));
                if time + 0.000001 < self.start {
                    continue;
                }
                if time > self.end + 0.000001 {
                    return Ok(None);
                }
                let buffer = CMSampleBufferGetImageBuffer(sample.0);
                ensure!(!buffer.is_null(), "{}", t("video.decode_failed"));
                let width = u32::try_from(CVPixelBufferGetWidth(buffer))?;
                let height = u32::try_from(CVPixelBufferGetHeight(buffer))?;
                let count = pixel_count(width, height)?;
                let stride = CVPixelBufferGetBytesPerRow(buffer);
                ensure!(stride >= width as usize * 4, "{}", t("video.decode_failed"));
                ensure!(
                    CVPixelBufferLockBaseAddress(buffer, 1) == 0,
                    "{}",
                    t("video.decode_failed")
                );
                let locked = LockedPixels(buffer);
                let base = CVPixelBufferGetBaseAddress(buffer);
                ensure!(!base.is_null(), "{}", t("video.decode_failed"));
                let mut rgba = Vec::with_capacity(count * 4);
                for y in 0..height as usize {
                    let row = std::slice::from_raw_parts(base.add(y * stride), width as usize * 4);
                    for p in row.as_chunks::<4>().0 {
                        rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
                    }
                }
                drop(locked);
                let description = CMSampleBufferGetFormatDescription(sample.0);
                let display =
                    CMVideoFormatDescriptionGetPresentationDimensions(description, true, false);
                let aspect = (display.width / display.height) / (width as f64 / height as f64);
                return display_frame(
                    Frame {
                        time,
                        width,
                        height,
                        rgba,
                    },
                    aspect,
                    self.matrix,
                    self.edge,
                )
                .map(Some);
            }
        })
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![&*self.reader, cancelReading];
        }
    }
}
