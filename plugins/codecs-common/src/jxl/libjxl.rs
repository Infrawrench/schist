//! libjxl, the JPEG XL reference encoder, loaded at runtime.
//!
//! Only the encoder is used: decoding is jxl-oxide's on every target.
//! The library is looked for where the system keeps it (every current
//! Linux distribution packages it, Homebrew's `jpeg-xl` on macOS) and
//! never downloaded: without it, exports fall back to zune-jpegxl's
//! lossless encoder and the export dialog hides the lossy controls.
//!
//! The ABI used is the one libjxl has kept since 0.7 (the first release
//! with `JxlEncoderFrameSettings`): the structs below are laid out as
//! its public `codestream_header.h` and `types.h` declare them, and
//! `JxlBasicInfo` carries 100 bytes of padding so it can grow without
//! changing size. Only our own pixels are handed to it, never a file
//! from elsewhere, so a decoder vulnerability in an old copy is no
//! exposure here.

use std::ffi::{c_int, c_void};
use std::sync::OnceLock;

use super::Flat;

/// `JxlBasicInfo`.
#[repr(C)]
struct BasicInfo {
    have_container: c_int,
    xsize: u32,
    ysize: u32,
    bits_per_sample: u32,
    exponent_bits_per_sample: u32,
    intensity_target: f32,
    min_nits: f32,
    relative_to_max_display: c_int,
    linear_below: f32,
    uses_original_profile: c_int,
    have_preview: c_int,
    have_animation: c_int,
    orientation: c_int,
    num_color_channels: u32,
    num_extra_channels: u32,
    alpha_bits: u32,
    alpha_exponent_bits: u32,
    alpha_premultiplied: c_int,
    /// `JxlPreviewHeader`: xsize, ysize.
    preview: [u32; 2],
    /// `JxlAnimationHeader`: tps_numerator, tps_denominator, num_loops,
    /// have_timecodes.
    animation: [u32; 4],
    intrinsic_xsize: u32,
    intrinsic_ysize: u32,
    padding: [u8; 100],
}

/// `JxlPixelFormat`.
#[repr(C)]
struct PixelFormat {
    num_channels: u32,
    data_type: c_int,
    endianness: c_int,
    align: usize,
}

/// Room for a `JxlColorEncoding` (104 bytes on every ABI), which only
/// libjxl itself ever fills in.
#[repr(C, align(8))]
struct ColorEncoding([u8; 256]);

/// `JxlDataType`
const TYPE_FLOAT: c_int = 0;
const TYPE_UINT8: c_int = 2;
const TYPE_UINT16: c_int = 3;
/// `JxlEncoderStatus`
const SUCCESS: c_int = 0;
const NEED_MORE_OUTPUT: c_int = 2;
/// `JXL_ENC_FRAME_SETTING_EFFORT`
const SETTING_EFFORT: c_int = 0;

type Runner =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *const c_void, u32, u32) -> c_int;

pub(super) struct LibJxl {
    /// The symbols below point into these mappings; both live in a
    /// static and are never dropped.
    _lib: libloading::Library,
    _threads: Option<libloading::Library>,
    version: unsafe extern "C" fn() -> u32,
    create: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    get_error: unsafe extern "C" fn(*const c_void) -> c_int,
    init_basic_info: unsafe extern "C" fn(*mut BasicInfo),
    set_basic_info: unsafe extern "C" fn(*mut c_void, *const BasicInfo) -> c_int,
    set_icc_profile: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> c_int,
    set_color_encoding: unsafe extern "C" fn(*mut c_void, *const ColorEncoding) -> c_int,
    color_encoding_srgb: unsafe extern "C" fn(*mut ColorEncoding, c_int),
    frame_settings_create: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void,
    set_frame_lossless: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    set_frame_distance: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    set_option: unsafe extern "C" fn(*mut c_void, c_int, i64) -> c_int,
    add_image_frame:
        unsafe extern "C" fn(*const c_void, *const PixelFormat, *const c_void, usize) -> c_int,
    close_input: unsafe extern "C" fn(*mut c_void),
    process_output: unsafe extern "C" fn(*mut c_void, *mut *mut u8, *mut usize) -> c_int,
    set_parallel_runner: unsafe extern "C" fn(*mut c_void, Runner, *mut c_void) -> c_int,
    /// From libjxl_threads, when it is there too.
    threads: Option<Threads>,
}

struct Threads {
    create: unsafe extern "C" fn(*const c_void, usize) -> *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    runner: Runner,
}

/// (libjxl, libjxl_threads) pairs, newest first.
#[cfg(target_os = "linux")]
const CANDIDATES: &[(&str, &str)] = &[
    ("libjxl.so.0.11", "libjxl_threads.so.0.11"),
    ("libjxl.so.0.10", "libjxl_threads.so.0.10"),
    ("libjxl.so.0.9", "libjxl_threads.so.0.9"),
    ("libjxl.so.0.8", "libjxl_threads.so.0.8"),
    ("libjxl.so.0.7", "libjxl_threads.so.0.7"),
    ("libjxl.so", "libjxl_threads.so"),
];
#[cfg(target_os = "macos")]
const CANDIDATES: &[(&str, &str)] = &[
    ("libjxl.dylib", "libjxl_threads.dylib"),
    // Homebrew's prefix is not on the default search path for app
    // bundles launched from Finder.
    (
        "/opt/homebrew/lib/libjxl.dylib",
        "/opt/homebrew/lib/libjxl_threads.dylib",
    ),
    (
        "/usr/local/lib/libjxl.dylib",
        "/usr/local/lib/libjxl_threads.dylib",
    ),
];
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const CANDIDATES: &[(&str, &str)] = &[
    ("jxl.dll", "jxl_threads.dll"),
    ("libjxl.dll", "libjxl_threads.dll"),
];

/// The oldest release with the API used here.
const MINIMUM_VERSION: u32 = 7_000;

/// The system's libjxl, loaded on first use; `None` when there is none.
/// `SCHIST_LIBJXL` names a library to use instead, or `none` to behave
/// as if there were no libjxl at all.
pub(super) fn get() -> Option<&'static LibJxl> {
    static LOADED: OnceLock<Option<LibJxl>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let wanted = std::env::var("SCHIST_LIBJXL").ok();
            if wanted.as_deref() == Some("none") {
                return None;
            }
            let custom = wanted.map(|path| {
                let threads = std::path::Path::new(&path)
                    .with_file_name(
                        std::path::Path::new(&path)
                            .file_name()
                            .map(|n| n.to_string_lossy().replacen("jxl", "jxl_threads", 1))
                            .unwrap_or_default(),
                    )
                    .into_os_string();
                (std::ffi::OsString::from(path), threads)
            });
            let candidates = custom.into_iter().chain(
                CANDIDATES
                    .iter()
                    .map(|(lib, threads)| ((*lib).into(), (*threads).into())),
            );
            for (name, threads) in candidates {
                match unsafe { libloading::Library::new(&name) } {
                    Ok(lib) => match load(lib, &threads) {
                        Ok(lib) => return Some(lib),
                        Err(err) => log::info!("{}: {err}", name.to_string_lossy()),
                    },
                    Err(err) => log::debug!("{err}"),
                }
            }
            None
        })
        .as_ref()
}

fn load(lib: libloading::Library, threads: &std::ffi::OsStr) -> Result<LibJxl, String> {
    macro_rules! sym {
        ($lib:expr, $name:literal) => {
            *unsafe { $lib.get(concat!($name, "\0").as_bytes()) }
                .map_err(|e| format!("{}: {e}", $name))?
        };
    }
    let threads = unsafe { libloading::Library::new(threads) }.ok();
    let runner = threads.as_ref().and_then(|t| {
        Some(Threads {
            create: *unsafe { t.get(b"JxlThreadParallelRunnerCreate\0") }.ok()?,
            destroy: *unsafe { t.get(b"JxlThreadParallelRunnerDestroy\0") }.ok()?,
            runner: *unsafe { t.get(b"JxlThreadParallelRunner\0") }.ok()?,
        })
    });
    let loaded = LibJxl {
        version: sym!(lib, "JxlEncoderVersion"),
        create: sym!(lib, "JxlEncoderCreate"),
        destroy: sym!(lib, "JxlEncoderDestroy"),
        get_error: sym!(lib, "JxlEncoderGetError"),
        init_basic_info: sym!(lib, "JxlEncoderInitBasicInfo"),
        set_basic_info: sym!(lib, "JxlEncoderSetBasicInfo"),
        set_icc_profile: sym!(lib, "JxlEncoderSetICCProfile"),
        set_color_encoding: sym!(lib, "JxlEncoderSetColorEncoding"),
        color_encoding_srgb: sym!(lib, "JxlColorEncodingSetToSRGB"),
        frame_settings_create: sym!(lib, "JxlEncoderFrameSettingsCreate"),
        set_frame_lossless: sym!(lib, "JxlEncoderSetFrameLossless"),
        set_frame_distance: sym!(lib, "JxlEncoderSetFrameDistance"),
        set_option: sym!(lib, "JxlEncoderFrameSettingsSetOption"),
        add_image_frame: sym!(lib, "JxlEncoderAddImageFrame"),
        close_input: sym!(lib, "JxlEncoderCloseInput"),
        process_output: sym!(lib, "JxlEncoderProcessOutput"),
        set_parallel_runner: sym!(lib, "JxlEncoderSetParallelRunner"),
        threads: runner,
        _threads: threads,
        _lib: lib,
    };
    let version = unsafe { (loaded.version)() };
    if version < MINIMUM_VERSION {
        return Err(format!("libjxl {version} is older than 0.7"));
    }
    Ok(loaded)
}

/// Destroys what libjxl allocated, on every path out.
struct Owned(*mut c_void, unsafe extern "C" fn(*mut c_void));

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { (self.1)(self.0) }
        }
    }
}

/// Encode `flat` at `bits` a sample (8, 16, or 32 for float) and
/// butteraugli `distance` (0 is lossless).
pub(super) fn encode(
    lib: &LibJxl,
    flat: &Flat,
    bits: u32,
    distance: f32,
    effort: u8,
    icc: Option<&[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let lossless = distance <= 0.0;
    let channels: u32 = if flat.opaque { 3 } else { 4 };
    let samples = flat
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| px[..channels as usize].iter().copied());
    let (data, data_type): (Vec<u8>, _) = match bits {
        8 => (
            samples
                .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                .collect(),
            TYPE_UINT8,
        ),
        16 => (
            samples
                .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).to_ne_bytes())
                .collect(),
            TYPE_UINT16,
        ),
        _ => (samples.flat_map(f32::to_ne_bytes).collect(), TYPE_FLOAT),
    };

    unsafe {
        let enc = Owned((lib.create)(std::ptr::null()), lib.destroy);
        anyhow::ensure!(!enc.0.is_null(), "JxlEncoderCreate failed");
        let check = |status: c_int, what: &str| -> anyhow::Result<()> {
            anyhow::ensure!(
                status == SUCCESS,
                "libjxl could not {what} (error {})",
                (lib.get_error)(enc.0)
            );
            Ok(())
        };
        let _runner = match &lib.threads {
            Some(threads) => {
                let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
                let runner = Owned((threads.create)(std::ptr::null(), workers), threads.destroy);
                if !runner.0.is_null() {
                    check(
                        (lib.set_parallel_runner)(enc.0, threads.runner, runner.0),
                        "start its worker threads",
                    )?;
                }
                Some(runner)
            }
            None => None,
        };

        let mut info: BasicInfo = std::mem::zeroed();
        (lib.init_basic_info)(&mut info);
        info.xsize = flat.width;
        info.ysize = flat.height;
        info.bits_per_sample = bits;
        info.exponent_bits_per_sample = if bits == 32 { 8 } else { 0 };
        // Lossless keeps the samples as they are; lossy converts to
        // XYB, the perceptual space the format is built around -- except
        // for a profiled document. libjxl turns such a profile into an
        // equivalent enumerated colour space and goes through XYB from
        // there, and with libjxl 0.7 and a Display P3 profile that
        // shifted saturated colours visibly (red 0 decoded as 0.44, in
        // libjxl's own decoder too). Coding the samples in the
        // document's own space costs some compression and keeps them.
        info.uses_original_profile = (lossless || icc.is_some()) as c_int;
        info.num_color_channels = 3;
        if channels == 4 {
            info.num_extra_channels = 1;
            info.alpha_bits = bits;
            info.alpha_exponent_bits = info.exponent_bits_per_sample;
        }
        check((lib.set_basic_info)(enc.0, &info), "accept the image size")?;
        match icc {
            Some(icc) => check(
                (lib.set_icc_profile)(enc.0, icc.as_ptr(), icc.len()),
                "embed the colour profile",
            )?,
            None => {
                let mut srgb = ColorEncoding([0; 256]);
                (lib.color_encoding_srgb)(&mut srgb, 0);
                check(
                    (lib.set_color_encoding)(enc.0, &srgb),
                    "set the colour space",
                )?
            }
        }

        // Owned by the encoder, freed with it.
        let settings = (lib.frame_settings_create)(enc.0, std::ptr::null());
        anyhow::ensure!(!settings.is_null(), "JxlEncoderFrameSettingsCreate failed");
        if lossless {
            check((lib.set_frame_lossless)(settings, 1), "encode losslessly")?;
        } else {
            check(
                (lib.set_frame_distance)(settings, distance),
                "set the quality",
            )?;
        }
        // 10 needs an expert opt-in on the releases that have it.
        check(
            (lib.set_option)(settings, SETTING_EFFORT, effort.clamp(1, 9) as i64),
            "set the effort",
        )?;
        let format = PixelFormat {
            num_channels: channels,
            data_type,
            endianness: 0,
            align: 0,
        };
        check(
            (lib.add_image_frame)(
                settings,
                &format,
                data.as_ptr() as *const c_void,
                data.len(),
            ),
            "encode the image",
        )?;
        (lib.close_input)(enc.0);

        let mut out = vec![0u8; 1 << 16];
        let mut used = 0;
        loop {
            let mut next = out.as_mut_ptr().add(used);
            let mut available = out.len() - used;
            let status = (lib.process_output)(enc.0, &mut next, &mut available);
            used = out.len() - available;
            match status {
                SUCCESS => break,
                NEED_MORE_OUTPUT => out.resize(out.len() * 2, 0),
                _ => check(status, "write the file")?,
            }
        }
        out.truncate(used);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_color::Depth;
    use schist_core::Document;
    use schist_plugin_api::{CodecPlugin, ExportOptions};

    /// These run against whatever libjxl the machine has, and skip
    /// without one: CI runners do not install it.
    fn lib() -> Option<&'static LibJxl> {
        let lib = get();
        if lib.is_none() {
            eprintln!("skipping: no libjxl on this machine");
        }
        lib
    }

    fn document(alpha: bool) -> Document {
        let (w, h) = (24, 16);
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let a = if alpha && x >= w / 2 { 0.5 } else { 1.0 };
                pixels.extend([x as f32 / w as f32, y as f32 / h as f32, 0.6, a]);
            }
        }
        let mut doc = Document::new("t", w, h, Depth::Sixteen);
        let mut layer = schist_core::Layer::new_raster("l");
        schist_core::blit_rgba_f32(
            &mut layer.as_raster_mut().unwrap().tiles,
            Depth::Sixteen,
            schist_core::IntRect::from_size(w, h),
            &pixels,
        );
        doc.push_layer(layer);
        doc
    }

    fn max_error(a: &Document, b: &Document) -> f32 {
        let a = schist_compositor::composite_region_f32(a, a.canvas_rect());
        let b = schist_compositor::composite_region_f32(b, b.canvas_rect());
        a.iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn lossless_sixteen_bit_with_alpha_is_exact() {
        let Some(lib) = lib() else { return };
        let doc = document(true);
        let flat = super::super::flatten(&doc, &ExportOptions::default(), 16);
        let bytes = encode(lib, &flat, 16, 0.0, 3, None).unwrap();
        let back = super::super::JxlCodec.import(&bytes).unwrap();
        assert_eq!(back.depth, Depth::Sixteen);
        assert!(max_error(&doc, &back) < 1e-4);
    }

    #[test]
    fn lossy_is_close_and_keeps_the_profile() {
        let Some(lib) = lib() else { return };
        let doc = document(true);
        let p3 = schist_colormgmt::Profile::display_p3()
            .icc_bytes()
            .unwrap()
            .to_vec();
        let flat = super::super::flatten(&doc, &ExportOptions::default(), 8);
        let bytes = encode(lib, &flat, 8, 1.0, 5, Some(&p3)).unwrap();
        let back = super::super::JxlCodec.import(&bytes).unwrap();
        assert_eq!(back.depth, Depth::Eight);
        // libjxl stores a profile it can describe exactly (primaries,
        // white point, transfer curve) as that description rather than
        // as the ICC bytes, so what comes back is an equivalent profile.
        let back_profile =
            moxcms::ColorProfile::new_from_slice(back.icc_profile.as_ref().unwrap()).unwrap();
        let p3 = moxcms::ColorProfile::new_from_slice(&p3).unwrap();
        for (a, b) in [
            (p3.red_colorant, back_profile.red_colorant),
            (p3.green_colorant, back_profile.green_colorant),
            (p3.blue_colorant, back_profile.blue_colorant),
        ] {
            assert!((a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs() < 0.01);
        }
        assert!(max_error(&doc, &back) < 0.06, "{}", max_error(&doc, &back));
    }

    #[test]
    fn float_samples_survive() {
        let Some(lib) = lib() else { return };
        let doc = document(false);
        let flat = super::super::flatten(&doc, &ExportOptions::default(), 32);
        let bytes = encode(lib, &flat, 32, 0.0, 3, None).unwrap();
        let back = super::super::JxlCodec.import(&bytes).unwrap();
        assert_eq!(back.depth, Depth::ThirtyTwo);
        assert!(max_error(&doc, &back) < 1e-4);
    }
}
