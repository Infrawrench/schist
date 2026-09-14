//! Native video via the platform media stack. Decoders only read the
//! source; captured frames become separate, unsaved image documents.
use anyhow::{ensure, Context as _, Result};
use schist_i18n::t;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use schist_i18n::tf;
use std::path::{Path, PathBuf};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "video/apple.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "video/windows.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "video/linux.rs"]
mod platform;
#[cfg(target_os = "android")]
#[path = "video/android.rs"]
pub(crate) mod platform;
#[cfg(any(target_os = "android", test))]
mod yuv;
pub use platform::{probe, Decoder};

pub const PREVIEW_EDGE: u32 = 1280;
const MAX_PIXELS: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub duration: f64,
}

#[derive(Debug)]
pub struct Frame {
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Job {
    cancelled: AtomicBool,
}
impl Job {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

fn local_file(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    ensure!(path.is_file(), "{}", t("video.invalid_file"));
    Ok(path)
}

fn pixel_count(width: u32, height: u32) -> Result<usize> {
    let count = (width as usize)
        .checked_mul(height as usize)
        .context(t("video.frame_too_large"))?;
    ensure!(
        count > 0 && count <= MAX_PIXELS,
        "{}",
        t("video.frame_too_large")
    );
    Ok(count)
}

/// Convert display aspect and the track's orthogonal orientation before
/// resizing so preview, sharpness search and full-size captures agree.
fn display_frame(mut frame: Frame, aspect: f64, matrix: [i32; 4], edge: u32) -> Result<Frame> {
    pixel_count(frame.width, frame.height)?;
    let mut pixels = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .context(t("video.decode_failed"))?;
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let width = (frame.width as f64 * aspect).round().max(1.0) as u32;
    pixel_count(width, frame.height)?;
    if width != frame.width {
        pixels = image::imageops::resize(
            &pixels,
            width,
            frame.height,
            image::imageops::FilterType::Triangle,
        );
    }
    let [a, b, c, d] = matrix;
    // The eight valid axis-aligned rotations/reflections. Reject malformed
    // matrices instead of indexing outside an image supplied by a decoder.
    ensure!(
        a.abs() + c.abs() == 1 && b.abs() + d.abs() == 1 && (a * d - b * c).abs() == 1,
        "{}",
        t("video.decode_failed")
    );
    let (w, h) = pixels.dimensions();
    if matrix != [1, 0, 0, 1] {
        let ow = a.unsigned_abs() * w + c.unsigned_abs() * h;
        let oh = b.unsigned_abs() * w + d.unsigned_abs() * h;
        let ox = if a < 0 {
            w - 1
        } else if c < 0 {
            h - 1
        } else {
            0
        };
        let oy = if b < 0 {
            w - 1
        } else if d < 0 {
            h - 1
        } else {
            0
        };
        let mut oriented = image::RgbaImage::new(ow, oh);
        for (x, y, p) in pixels.enumerate_pixels() {
            let dx = a * x as i32 + c * y as i32 + ox as i32;
            let dy = b * x as i32 + d * y as i32 + oy as i32;
            oriented.put_pixel(dx as u32, dy as u32, *p);
        }
        pixels = oriented;
    }
    let (w, h) = pixels.dimensions();
    if edge > 0 && w.max(h) > edge {
        let scale = edge as f64 / w.max(h) as f64;
        pixels = image::imageops::resize(
            &pixels,
            (w as f64 * scale).round().max(1.0) as u32,
            (h as f64 * scale).round().max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        );
    }
    frame.width = pixels.width();
    frame.height = pixels.height();
    frame.rgba = pixels.into_raw();
    Ok(frame)
}

pub fn frame(path: &Path, time: f64, edge: u32, job: Arc<Job>) -> Result<Frame> {
    // Tolerate timebase rounding when recapturing a displayed frame.
    Decoder::open(path, (time - 0.0001).max(0.0), None, edge, job)?
        .next()?
        .context(t("video.decode_failed"))
}

/// Search every decoded frame within half a second of the chosen moment.
/// A tie stays closest to the original timestamp; never promise a blurry
/// clip contains a sharp frame. The original is always a candidate.
pub fn sharper(path: &Path, time: f64, duration: f64, job: Arc<Job>) -> Result<f64> {
    let start = (time - 0.5).max(0.0);
    let end = (time + 0.5).min(duration);
    let mut decoder = Decoder::open(path, start, Some(end - start), 640, job)?;
    let mut best: Option<(f64, f64)> = None;
    while let Some(frame) = decoder.next()? {
        if frame.time > end + 0.0001 {
            break;
        }
        let score = sharpness(&frame);
        let better = best.is_none_or(|(s, at)| {
            score > s || (score == s && (frame.time - time).abs() < (at - time).abs())
        });
        if better {
            best = Some((score, frame.time));
        }
    }
    best.map(|(_, time)| time).context(t("video.decode_failed"))
}

pub fn seek(path: &Path, time: f64, edge: u32, job: Arc<Job>) -> Result<Frame> {
    match frame(path, time, edge, job.clone()) {
        Ok(frame) => Ok(frame),
        Err(_) if !job.cancelled() => previous(path, time, job),
        Err(err) => Err(err),
    }
}

pub fn previous(path: &Path, time: f64, job: Arc<Job>) -> Result<Frame> {
    // Decoding the preceding interval uses real timestamps for VFR video.
    let start = (time - 2.0).max(0.0);
    let mut decoder = Decoder::open(
        path,
        start,
        Some(time - start + 0.1),
        PREVIEW_EDGE,
        job.clone(),
    )?;
    let mut previous = None;
    while let Some(frame) = decoder.next()? {
        if frame.time >= time - 0.0001 {
            break;
        }
        previous = Some(frame);
    }
    drop(decoder);
    if previous.is_none() && start > 0.0 && !job.cancelled() {
        // Very sparse VFR sources can have gaps longer than two seconds.
        let mut decoder = Decoder::open(path, 0.0, Some(time), PREVIEW_EDGE, job)?;
        while let Some(frame) = decoder.next()? {
            if frame.time >= time - 0.0001 {
                break;
            }
            previous = Some(frame);
        }
    }
    previous.context(t("video.first_frame"))
}

/// Variance of the luminance Laplacian, computed at a common preview size.
fn sharpness(frame: &Frame) -> f64 {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w < 3 || h < 3 {
        return 0.0;
    }
    let luma = |x: usize, y: usize| {
        let p = &frame.rgba[(y * w + x) * 4..];
        0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
    };
    let (mut sum, mut squared) = (0.0, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let value = luma(x - 1, y) + luma(x + 1, y) + luma(x, y - 1) + luma(x, y + 1)
                - 4.0 * luma(x, y);
            sum += value;
            squared += value * value;
        }
    }
    let n = ((w - 2) * (h - 2)) as f64;
    (squared / n - (sum / n).powi(2)).max(0.0)
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn open_editor(editor: &Path, path: &Path) -> Result<()> {
    let path = local_file(path)?;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut cmd = Command::new("/usr/bin/open");
        cmd.arg("-a").arg(editor).arg(&path);
        cmd
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = {
        let mut cmd = Command::new(editor);
        cmd.arg(&path);
        cmd
    };
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| tf!("video.editor_failed", error = editor.display()))?;
    #[cfg(target_os = "macos")]
    if !child.wait()?.success() {
        anyhow::bail!("{}", t("video.editor_unavailable"));
    }
    #[cfg(not(target_os = "macos"))]
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sharp_edges_outscore_blurred_edges_and_flat_frames() {
        let mut sharp = Frame {
            time: 0.0,
            width: 32,
            height: 32,
            rgba: vec![255; 32 * 32 * 4],
        };
        for y in 0..32 {
            for x in 0..16 {
                sharp.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 3].fill(0);
            }
        }
        let mut blur = Frame {
            time: 0.0,
            width: 32,
            height: 32,
            rgba: sharp.rgba.clone(),
        };
        for y in 0..32 {
            for x in 0..32 {
                blur.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 3]
                    .fill(((x as f64 - 8.0) * 255.0 / 16.0).clamp(0.0, 255.0) as u8);
            }
        }
        assert!(sharpness(&sharp) > sharpness(&blur) * 10.0);
        blur.rgba.fill(100);
        assert_eq!(sharpness(&blur), 0.0);
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;
    fn clip(name: &str) -> tempfile::TempPath {
        use std::io::Write as _;
        let bytes: &[u8] = match name {
            "sharp.mp4" => include_bytes!("../tests/fixtures/video/sharp.mp4"),
            "variable.mp4" => include_bytes!("../tests/fixtures/video/variable.mp4"),
            "rotated.mp4" => include_bytes!("../tests/fixtures/video/rotated.mp4"),
            _ => unreachable!(),
        };
        let mut file = tempfile::Builder::new().suffix(".mp4").tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file.into_temp_path()
    }
    fn job() -> Arc<Job> {
        Arc::new(Job::default())
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "requires GStreamer with a native H.264 decoder"
    )]
    fn capture_seek_sharpness_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a clip with spaces #50% café.mp4");
        std::fs::copy(clip("sharp.mp4"), &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let info = probe(&path, job()).unwrap();
        assert!((info.duration - 1.0).abs() < 0.01);
        let selected = frame(&path, 0.375, 0, job()).unwrap();
        assert!((selected.time - 0.375).abs() < 0.001);
        assert_eq!((selected.width, selected.height), (64, 48));
        let blurred = frame(&path, 0.25, 0, job()).unwrap();
        assert!(
            sharpness(&selected) > sharpness(&blurred) * 10.0,
            "sharp={}, blurred={} (at {})",
            sharpness(&selected),
            sharpness(&blurred),
            blurred.time
        );
        assert!((frame(&path, 0.376, 0, job()).unwrap().time - 0.5).abs() < 0.001);
        let at = sharper(&path, 0.5, info.duration, job()).unwrap();
        assert!((at - 0.375).abs() < 0.001, "picked {at}");
        assert!((previous(&path, 0.375, job()).unwrap().time - 0.25).abs() < 0.001);
        assert!((seek(&path, 0.999, PREVIEW_EDGE, job()).unwrap().time - 0.875).abs() < 0.001);
        let handle = job();
        let mut decoder = Decoder::open(&path, 0.0, None, 64, handle.clone()).unwrap();
        assert!(decoder.next().unwrap().is_some());
        handle.cancel();
        assert!(decoder.next().unwrap().is_none());
        drop(decoder);
        assert!(Decoder::open(&path, 0.0, None, 64, handle).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(schist_gallery::backing_psd(&path).is_none());
        assert_eq!(
            schist_gallery::scan_folders(&[dir.path().to_path_buf()], &["mp4".into()])[0]
                .entries
                .len(),
            1
        );
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "requires GStreamer with a native H.264 decoder"
    )]
    fn phone_rotation_and_recapturing_last_frame_after_resume() {
        let path = clip("rotated.mp4");
        let mut decoder = Decoder::open(&path, 0.0416667, None, 64, job()).unwrap();
        let mut last = None;
        let mut count = 0;
        while let Some(frame) = decoder.next().unwrap() {
            last = Some(frame);
            count += 1;
        }
        assert!(count >= 118, "decoded {count} frames");
        let last = last.unwrap();
        let recaptured = frame(&path, last.time, 0, job()).unwrap();
        assert!((recaptured.time - last.time).abs() < 0.001);
        assert_eq!(recaptured.rgba, last.rgba);
        let full = frame(&path, 0.0, 0, job()).unwrap();
        let preview = frame(&path, 0.0, 32, job()).unwrap();
        assert_eq!((full.width, full.height), (48, 64));
        assert_eq!((preview.width, preview.height), (24, 32));
        // Clockwise rotation puts the original blue lower-left at upper-left.
        let p = &full.rgba[(8 * 48 + 8) * 4..];
        assert!(
            p[2] > 180 && p[0] < 80 && p[1] < 80,
            "upper-left: {:?}",
            &p[..4]
        );
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "requires GStreamer with a native H.264 decoder"
    )]
    fn variable_frame_timestamps_and_invalid_files() {
        let path = clip("variable.mp4");
        let mut decoder = Decoder::open(&path, 0.0, None, 64, job()).unwrap();
        let mut times = Vec::new();
        while let Some(frame) = decoder.next().unwrap() {
            times.push(frame.time);
        }
        assert_eq!(times, vec![0.0, 0.125, 0.25, 0.375, 0.5, 0.75, 1.0, 1.25]);
        assert!((frame(&path, times[5], 0, job()).unwrap().time - 0.75).abs() < 0.001);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.mp4");
        std::fs::write(&path, b"not a video").unwrap();
        assert!(probe(&path, job()).is_err());
        assert!(frame(&path, 0.0, 0, job()).is_err());
        assert!(probe(dir.path(), job()).is_err());
    }

    #[test]
    fn display_aspect_reflection_and_size_guards() {
        let make = || Frame {
            time: 0.5,
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let mirrored = display_frame(make(), 1.0, [-1, 0, 0, 1], 0).unwrap();
        assert_eq!(&mirrored.rgba[..4], &[0, 0, 255, 255]);
        let stretched = display_frame(make(), 2.0, [0, 1, -1, 0], 0).unwrap();
        assert_eq!((stretched.width, stretched.height), (1, 4));
        assert!(display_frame(make(), 1.0, [1, 1, 1, 1], 0).is_err());
        assert!(display_frame(make(), f64::MAX, [1, 0, 0, 1], 0).is_err());
        assert!(pixel_count(u32::MAX, u32::MAX).is_err());
        assert!(pixel_count(0, 1).is_err());
    }
}
