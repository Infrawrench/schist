//! Media Foundation Source Reader; no subprocess or bundled codec library.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::core::{Interface, GUID, PCWSTR, PROPVARIANT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

struct Runtime {
    uninitialize_com: bool,
}
impl Runtime {
    unsafe fn new() -> Result<Self> {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        // An existing STA is valid too; only balance our own successful init.
        if initialized.0 != 0x80010106u32 as i32 {
            initialized.ok()?;
        }
        if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
            if initialized.is_ok() {
                CoUninitialize();
            }
            return Err(error.into());
        }
        Ok(Self {
            uninitialize_com: initialized.is_ok(),
        })
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            if self.uninitialize_com {
                CoUninitialize();
            }
        }
    }
}

pub fn probe(path: &Path, job: Arc<Job>) -> Result<Info> {
    let decoder = Decoder::open(path, 0.0, None, 0, job)?;
    Ok(Info {
        duration: decoder.duration,
    })
}

pub struct Decoder {
    reader: IMFSourceReader,
    job: Arc<Job>,
    start: f64,
    end: f64,
    edge: u32,
    duration: f64,
    source_rotation: u32,
    // Field order releases the reader before shutting down MF and COM.
    _runtime: Runtime,
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
        unsafe {
            let runtime = Runtime::new().context(t("video.decode_failed"))?;
            let path = local_file(path)?;
            let path = path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            // canonicalize returns a verbatim Windows path (\\?\...). Open
            // it as a file: the URL resolver can mistake it for a network URL.
            let stream = MFCreateFile(
                MF_ACCESSMODE_READ,
                MF_OPENMODE_FAIL_IF_NOT_EXIST,
                MF_FILEFLAGS_NONE,
                PCWSTR(path.as_ptr()),
            )
            .context(t("video.decode_failed"))?;
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes.context(t("video.decode_failed"))?;
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            let reader = MFCreateSourceReaderFromByteStream(&stream, &attributes)
                .context(t("video.decode_failed"))?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader
                .SetStreamSelection(VIDEO, true)
                .context(t("video.no_stream"))?;
            let native_type = reader.GetNativeMediaType(VIDEO, 0)?;
            let source_rotation = native_type.GetUINT32(&MF_MT_VIDEO_ROTATION).unwrap_or(0);
            let media_type = MFCreateMediaType()?;
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            media_type.SetUINT64(&MF_MT_FRAME_SIZE, native_type.GetUINT64(&MF_MT_FRAME_SIZE)?)?;
            media_type.SetUINT64(
                &MF_MT_PIXEL_ASPECT_RATIO,
                native_type
                    .GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO)
                    .unwrap_or((1 << 32) | 1),
            )?;
            reader
                .SetCurrentMediaType(VIDEO, None, &media_type)
                .context(t("video.decode_failed"))?;
            let duration = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)?;
            let duration = u64::try_from(&duration)? as f64 / 10_000_000.0;
            ensure!(
                duration.is_finite() && duration > 0.0,
                "{}",
                t("video.no_duration")
            );
            let start = start.max(0.0);
            if start > 0.0 {
                reader.SetCurrentPosition(
                    &GUID::zeroed(),
                    &PROPVARIANT::from((start * 10_000_000.0).round() as i64),
                )?;
            }
            Ok(Self {
                reader,
                job,
                start,
                end: span.map_or(duration, |s| (start + s).min(duration)),
                edge,
                duration,
                source_rotation,
                _runtime: runtime,
            })
        }
    }
    pub fn next(&mut self) -> Result<Option<Frame>> {
        unsafe {
            loop {
                if self.job.cancelled() {
                    return Ok(None);
                }
                let (mut flags, mut timestamp, mut sample) = (0u32, 0i64, None);
                self.reader
                    .ReadSample(
                        VIDEO,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut timestamp),
                        Some(&mut sample),
                    )
                    .context(t("video.decode_failed"))?;
                ensure!(
                    flags & MF_SOURCE_READERF_ERROR.0 as u32 == 0,
                    "{}",
                    t("video.decode_failed")
                );
                let time = timestamp as f64 / 10_000_000.0;
                // A final sample can carry EOS: return it before ending.
                let Some(sample) = sample else {
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        return Ok(None);
                    }
                    continue;
                };
                if time + 0.000001 < self.start {
                    continue;
                }
                if time > self.end + 0.000001 {
                    return Ok(None);
                }
                let format = self.reader.GetCurrentMediaType(VIDEO)?;
                let size = format.GetUINT64(&MF_MT_FRAME_SIZE)?;
                let (width, height) = ((size >> 32) as u32, size as u32);
                pixel_count(width, height)?;
                let crop = display_area(&format, width, height)?;
                let count = pixel_count(crop[2], crop[3])?;
                let aspect = format
                    .GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO)
                    .unwrap_or((1 << 32) | 1);
                let aspect = (aspect >> 32) as f64 / (aspect as u32).max(1) as f64;
                // MF describes the stored image's counter-clockwise rotation.
                // Undo it for display, preserving the source metadata if RGB
                // conversion omits that attribute.
                let matrix = match format
                    .GetUINT32(&MF_MT_VIDEO_ROTATION)
                    .unwrap_or(self.source_rotation)
                {
                    90 => [0, 1, -1, 0],
                    180 => [-1, 0, 0, -1],
                    270 => [0, -1, 1, 0],
                    _ => [1, 0, 0, 1],
                };
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut rgba = Vec::with_capacity(count * 4);
                if let Ok(buffer) = buffer.cast::<IMF2DBuffer>() {
                    let (mut base, mut stride) = (std::ptr::null_mut(), 0);
                    buffer.Lock2D(&mut base, &mut stride)?;
                    let locked = Locked2D(buffer);
                    copy_rows(base, stride, crop, &mut rgba)?;
                    drop(locked);
                } else {
                    let (mut base, mut length) = (std::ptr::null_mut(), 0);
                    buffer.Lock(&mut base, None, Some(&mut length))?;
                    let locked = LockedBuffer(buffer);
                    let stride = format
                        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                        .map(|s| s as i32)
                        .unwrap_or(width as i32 * 4);
                    let needed = stride.unsigned_abs() as u64 * height.saturating_sub(1) as u64
                        + width as u64 * 4;
                    ensure!(needed <= length as u64, "{}", t("video.decode_failed"));
                    if stride < 0 {
                        base = base.add(stride.unsigned_abs() as usize * (height - 1) as usize);
                    }
                    copy_rows(base, stride, crop, &mut rgba)?;
                    drop(locked);
                }
                return display_frame(
                    Frame {
                        time,
                        width: crop[2],
                        height: crop[3],
                        rgba,
                    },
                    aspect,
                    matrix,
                    self.edge,
                )
                .map(Some);
            }
        }
    }
}

/// The negotiated frame size can include decoder padding. Only the display
/// aperture contains image pixels; apply it before aspect, rotation or resize.
unsafe fn display_area(format: &IMFMediaType, width: u32, height: u32) -> Result<[u32; 4]> {
    for attribute in [
        MF_MT_PAN_SCAN_APERTURE,
        MF_MT_MINIMUM_DISPLAY_APERTURE,
        MF_MT_GEOMETRIC_APERTURE,
    ] {
        if attribute == MF_MT_PAN_SCAN_APERTURE
            && format.GetUINT32(&MF_MT_PAN_SCAN_ENABLED).unwrap_or(0) == 0
        {
            continue;
        }
        let length = match format.GetBlobSize(&attribute) {
            Ok(length) => length as usize,
            Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => continue,
            Err(error) => return Err(error.into()),
        };
        ensure!(
            length == std::mem::size_of::<MFVideoArea>(),
            "{}",
            t("video.decode_failed")
        );
        let mut bytes = [0u8; std::mem::size_of::<MFVideoArea>()];
        format.GetBlob(&attribute, &mut bytes, None)?;
        // MFVideoArea is a C structure containing only integer fields.
        let area = std::ptr::read_unaligned(bytes.as_ptr().cast::<MFVideoArea>());
        let left = u32::try_from(area.OffsetX.value)?;
        let top = u32::try_from(area.OffsetY.value)?;
        let crop_width = u32::try_from(area.Area.cx)?;
        let crop_height = u32::try_from(area.Area.cy)?;
        pixel_count(crop_width, crop_height)?;
        ensure!(
            left as u64 + crop_width as u64 <= width as u64
                && top as u64 + crop_height as u64 <= height as u64,
            "{}",
            t("video.decode_failed")
        );
        return Ok([left, top, crop_width, crop_height]);
    }
    Ok([0, 0, width, height])
}

struct Locked2D(IMF2DBuffer);
impl Drop for Locked2D {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Unlock2D();
        }
    }
}
struct LockedBuffer(IMFMediaBuffer);
impl Drop for LockedBuffer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Unlock();
        }
    }
}
unsafe fn copy_rows(
    base: *const u8,
    stride: i32,
    [left, top, width, height]: [u32; 4],
    rgba: &mut Vec<u8>,
) -> Result<()> {
    ensure!(
        !base.is_null() && stride.unsigned_abs() as u64 >= (left as u64 + width as u64) * 4,
        "{}",
        t("video.decode_failed")
    );
    for y in 0..height as isize {
        let row = std::slice::from_raw_parts(
            base.offset((top as isize + y) * stride as isize)
                .add(left as usize * 4),
            width as usize * 4,
        );
        for p in row.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn set_area(format: &IMFMediaType, key: &GUID, rect: [i32; 4]) {
        let [x, y, width, height] = rect;
        let area = MFVideoArea {
            OffsetX: MFOffset {
                value: x as i16,
                fract: 0,
            },
            OffsetY: MFOffset {
                value: y as i16,
                fract: 0,
            },
            Area: windows::Win32::Foundation::SIZE {
                cx: width,
                cy: height,
            },
        };
        let bytes = std::slice::from_raw_parts(
            std::ptr::from_ref(&area).cast::<u8>(),
            std::mem::size_of::<MFVideoArea>(),
        );
        format.SetBlob(key, bytes).unwrap();
    }

    #[test]
    fn display_apertures_exclude_padding_and_validate_bounds() {
        unsafe {
            let _runtime = Runtime::new().unwrap();
            let format = MFCreateMediaType().unwrap();
            assert_eq!(display_area(&format, 192, 96).unwrap(), [0, 0, 192, 96]);
            set_area(&format, &MF_MT_GEOMETRIC_APERTURE, [0, 0, 80, 60]);
            assert_eq!(display_area(&format, 192, 96).unwrap(), [0, 0, 80, 60]);
            set_area(&format, &MF_MT_MINIMUM_DISPLAY_APERTURE, [0, 0, 64, 48]);
            assert_eq!(display_area(&format, 192, 96).unwrap(), [0, 0, 64, 48]);
            set_area(&format, &MF_MT_PAN_SCAN_APERTURE, [4, 2, 40, 30]);
            assert_eq!(display_area(&format, 192, 96).unwrap(), [0, 0, 64, 48]);
            format.SetUINT32(&MF_MT_PAN_SCAN_ENABLED, 1).unwrap();
            assert_eq!(display_area(&format, 192, 96).unwrap(), [4, 2, 40, 30]);
            for rect in [
                [-1, 0, 64, 48],
                [160, 0, 64, 48],
                [0, 80, 64, 48],
                [0, 0, 0, 48],
            ] {
                set_area(&format, &MF_MT_PAN_SCAN_APERTURE, rect);
                assert!(display_area(&format, 192, 96).is_err(), "{rect:?}");
            }
            format.SetBlob(&MF_MT_PAN_SCAN_APERTURE, &[0u8; 4]).unwrap();
            assert!(display_area(&format, 192, 96).is_err());
        }
    }

    #[test]
    fn cropped_rows_support_padding_and_negative_stride() {
        let mut pixels = [0xee; 80];
        pixels[24..32].copy_from_slice(&[0, 0, 255, 66, 0, 255, 0, 99]);
        pixels[44..52].copy_from_slice(&[255, 0, 0, 17, 255, 255, 255, 0]);
        let expected = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let mut rgba = Vec::new();
        unsafe { copy_rows(pixels.as_ptr(), 20, [1, 1, 2, 2], &mut rgba).unwrap() };
        assert_eq!(rgba, expected);
        let reversed: Vec<_> = pixels
            .as_chunks::<20>()
            .0
            .iter()
            .rev()
            .flatten()
            .copied()
            .collect();
        rgba.clear();
        unsafe {
            copy_rows(reversed.as_ptr().add(60), -20, [1, 1, 2, 2], &mut rgba).unwrap();
            assert!(copy_rows(pixels.as_ptr(), 8, [1, 1, 2, 2], &mut Vec::new()).is_err());
            assert!(copy_rows(std::ptr::null(), 20, [1, 1, 2, 2], &mut Vec::new()).is_err());
        }
        assert_eq!(rgba, expected);
    }
}
