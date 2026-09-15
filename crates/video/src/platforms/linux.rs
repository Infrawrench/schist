//! The system GStreamer 1.x, loaded at runtime. Missing media packages
//! disable preview gracefully; opening the original in an editor still works.
use super::*;
use libloading::Library;
use std::ffi::{c_char, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Ptr = *mut c_void;
type Select = unsafe extern "C" fn(Ptr, Ptr, Ptr, Ptr, Ptr) -> i32;
#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *const c_char,
}
// These are public, ABI-stable GStreamer 1.x structures. Only the buffer's
// PTS is accessed; ownership remains with the GstSample until it is copied.
#[repr(C)]
struct MiniObject {
    kind: usize,
    refcount: i32,
    lockstate: i32,
    flags: u32,
    copy: Ptr,
    dispose: Ptr,
    free: Ptr,
    private_uint: u32,
    private_pointer: Ptr,
}
#[repr(C)]
struct Buffer {
    mini: MiniObject,
    pool: Ptr,
    pts: u64,
    dts: u64,
    duration: u64,
    offset: u64,
    offset_end: u64,
}

// Public prefix of GstVideoMeta, through the row-layout fields we read.
#[repr(C)]
struct VideoMeta {
    meta_flags: u32,
    meta_info: Ptr,
    buffer: Ptr,
    flags: u32,
    format: i32,
    id: i32,
    width: u32,
    height: u32,
    planes: u32,
    offset: [usize; 4],
    stride: [i32; 4],
}

macro_rules! api {
    ($($lib:literal => $name:ident($($arg:ty),*) -> $out:ty;)+) => {
        struct Api { $($name: unsafe extern "C" fn($($arg),*) -> $out,)+ _libraries: [Library; 5] }
        impl Api {
            unsafe fn load() -> Result<Self> {
                let libraries = [
                    Library::new("libgstreamer-1.0.so.0")?,
                    Library::new("libgstapp-1.0.so.0")?,
                    Library::new("libgobject-2.0.so.0")?,
                    Library::new("libglib-2.0.so.0")?,
                    Library::new("libgstvideo-1.0.so.0")?,
                ];
                let api = Self { $($name: *libraries[$lib].get(concat!(stringify!($name), "\0").as_bytes())?,)+ _libraries: libraries };
                let mut error = ptr::null_mut();
                if (api.gst_init_check)(ptr::null_mut(), ptr::null_mut(), &mut error) == 0 { return Err(api.error(error)); }
                Ok(api)
            }
        }
    }
}
api! {
    0 => gst_init_check(*mut i32, *mut *mut *mut c_char, *mut *mut GError) -> i32;
    0 => gst_parse_launch(*const c_char, *mut *mut GError) -> Ptr;
    0 => gst_bin_get_by_name(Ptr, *const c_char) -> Ptr;
    0 => gst_util_set_object_arg(Ptr, *const c_char, *const c_char) -> ();
    0 => gst_element_set_state(Ptr, i32) -> i32;
    0 => gst_element_get_state(Ptr, *mut i32, *mut i32, u64) -> i32;
    0 => gst_element_query_duration(Ptr, i32, *mut i64) -> i32;
    0 => gst_element_seek_simple(Ptr, i32, u32, i64) -> i32;
    0 => gst_element_get_bus(Ptr) -> Ptr;
    0 => gst_bus_pop_filtered(Ptr, u32) -> Ptr;
    0 => gst_message_parse_error(Ptr, *mut *mut GError, *mut *mut c_char) -> ();
    0 => gst_sample_get_buffer(Ptr) -> *const Buffer;
    0 => gst_sample_get_caps(Ptr) -> Ptr;
    0 => gst_caps_get_structure(Ptr, u32) -> Ptr;
    0 => gst_structure_get_int(Ptr, *const c_char, *mut i32) -> i32;
    0 => gst_structure_get_fraction(Ptr, *const c_char, *mut i32, *mut i32) -> i32;
    0 => gst_buffer_extract(*const Buffer, usize, Ptr, usize) -> usize;
    0 => gst_plugin_feature_get_plugin_name(Ptr) -> *const c_char;
    0 => gst_mini_object_unref(Ptr) -> ();
    0 => gst_object_unref(Ptr) -> ();
    1 => gst_app_sink_try_pull_sample(Ptr, u64) -> Ptr;
    1 => gst_app_sink_is_eos(Ptr) -> i32;
    2 => g_signal_connect_data(Ptr, *const c_char, Select, Ptr, Ptr, u32) -> usize;
    4 => gst_buffer_get_video_meta(*const Buffer) -> *const VideoMeta;
    3 => g_error_free(*mut GError) -> ();
}
impl Api {
    unsafe fn error(&self, error: *mut GError) -> anyhow::Error {
        let detail = if error.is_null() {
            String::new()
        } else {
            let detail = CStr::from_ptr((*error).message)
                .to_string_lossy()
                .into_owned();
            (self.g_error_free)(error);
            detail
        };
        anyhow::anyhow!("{}: {}", t("video.decode_failed"), detail)
    }
}
fn api() -> Result<&'static Api> {
    static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| unsafe { Api::load().map_err(|e| e.to_string()) })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{}: {}", t("video.install_gstreamer"), e))
}
// Prefer native/GStreamer decoders and never select the FFmpeg-backed
// optional gst-libav plugin, even if it happens to be installed.
unsafe extern "C" fn select_decoder(_: Ptr, _: Ptr, _: Ptr, factory: Ptr, data: Ptr) -> i32 {
    let api = &*(data as *const Api);
    let plugin = (api.gst_plugin_feature_get_plugin_name)(factory);
    if !plugin.is_null() && CStr::from_ptr(plugin) == c"libav" {
        2
    } else {
        0
    }
}
struct Object {
    ptr: Ptr,
    api: &'static Api,
}
impl Object {
    fn new(ptr: Ptr, api: &'static Api) -> Result<Self> {
        ensure!(!ptr.is_null(), "{}", t("video.decode_failed"));
        Ok(Self { ptr, api })
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            (self.api.gst_object_unref)(self.ptr);
        }
    }
}
struct Sample {
    ptr: Ptr,
    api: &'static Api,
}
impl Drop for Sample {
    fn drop(&mut self) {
        unsafe {
            (self.api.gst_mini_object_unref)(self.ptr);
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
    pipeline: Object,
    sink: Object,
    bus: Object,
    job: Arc<Job>,
    start: f64,
    end: f64,
    edge: u32,
    duration: f64,
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
        let path = CString::new(local_file(path)?.as_os_str().as_bytes())?;
        let api = api()?;
        unsafe {
            let mut error = ptr::null_mut();
            // The filename is assigned separately, never parsed as pipeline code.
            let raw = (api.gst_parse_launch)(c"filesrc name=source ! decodebin name=decode caps=video/x-raw ! videoconvert ! videoflip video-direction=auto ! video/x-raw,format=RGBA ! appsink name=frames max-buffers=1 sync=false".as_ptr(), &mut error);
            let pipeline = Object::new(raw, api);
            if !error.is_null() {
                return Err(api.error(error));
            }
            let pipeline = pipeline?;
            let named = |name: &CStr| {
                Object::new((api.gst_bin_get_by_name)(pipeline.ptr, name.as_ptr()), api)
            };
            let source = named(c"source")?;
            let decode = named(c"decode")?;
            let sink = named(c"frames")?;
            let bus = Object::new((api.gst_element_get_bus)(pipeline.ptr), api)?;
            (api.gst_util_set_object_arg)(source.ptr, c"location".as_ptr(), path.as_ptr());
            (api.g_signal_connect_data)(
                decode.ptr,
                c"autoplug-select".as_ptr(),
                select_decoder,
                api as *const Api as Ptr,
                ptr::null_mut(),
                0,
            );
            let mut decoder = Self {
                pipeline,
                sink,
                bus,
                job,
                start: start.max(0.0),
                end: f64::INFINITY,
                edge,
                duration: 0.0,
            };
            ensure!(
                (api.gst_element_set_state)(decoder.pipeline.ptr, 3) != 0,
                "{}",
                t("video.decode_failed")
            ); // PAUSED
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                ensure!(!decoder.job.cancelled(), "{}", t("common.cancel"));
                decoder.check_error()?;
                let state = (api.gst_element_get_state)(
                    decoder.pipeline.ptr,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    100_000_000,
                );
                ensure!(
                    state != 0 && Instant::now() < deadline,
                    "{}",
                    t("video.decode_failed")
                );
                if state != 2 {
                    break;
                } // ASYNC
            }
            let mut nanos = 0;
            ensure!(
                (api.gst_element_query_duration)(decoder.pipeline.ptr, 3, &mut nanos) != 0
                    && nanos > 0,
                "{}",
                t("video.no_duration")
            );
            decoder.duration = nanos as f64 / 1e9;
            decoder.end = span.map_or(decoder.duration, |s| {
                (decoder.start + s).min(decoder.duration)
            });
            // Start at the preceding keyframe and filter by the original PTS
            // below; segment clipping would fabricate a timestamp at the seek.
            if decoder.start > 0.0 {
                ensure!(
                    (api.gst_element_seek_simple)(
                        decoder.pipeline.ptr,
                        3,
                        1 | 2 | 4 | 32, // FLUSH | ACCURATE | KEY_UNIT | SNAP_BEFORE
                        (decoder.start * 1e9).round() as i64
                    ) != 0,
                    "{}",
                    t("video.decode_failed")
                );
            }
            ensure!(
                (api.gst_element_set_state)(decoder.pipeline.ptr, 4) != 0,
                "{}",
                t("video.decode_failed")
            ); // PLAYING
            Ok(decoder)
        }
    }
    unsafe fn check_error(&self) -> Result<()> {
        let api = self.pipeline.api;
        let message = (api.gst_bus_pop_filtered)(self.bus.ptr, 2); // ERROR
        if !message.is_null() {
            let mut error = ptr::null_mut();
            (api.gst_message_parse_error)(message, &mut error, ptr::null_mut());
            (api.gst_mini_object_unref)(message);
            return Err(api.error(error));
        }
        Ok(())
    }
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        let api = self.pipeline.api;
        let deadline = Instant::now() + Duration::from_secs(30);
        unsafe {
            loop {
                if self.job.cancelled() {
                    return Ok(None);
                }
                self.check_error()?;
                let raw = (api.gst_app_sink_try_pull_sample)(self.sink.ptr, 100_000_000);
                if raw.is_null() {
                    self.check_error()?;
                    if (api.gst_app_sink_is_eos)(self.sink.ptr) != 0 {
                        return Ok(None);
                    }
                    ensure!(Instant::now() < deadline, "{}", t("video.decode_failed"));
                    continue;
                }
                let sample = Sample { ptr: raw, api };
                let buffer = (api.gst_sample_get_buffer)(sample.ptr);
                ensure!(
                    !buffer.is_null() && (*buffer).pts != u64::MAX,
                    "{}",
                    t("video.decode_failed")
                );
                let time = (*buffer).pts as f64 / 1e9;
                if time + 0.000001 < self.start {
                    continue;
                }
                if time > self.end + 0.000001 {
                    return Ok(None);
                }
                let caps = (api.gst_sample_get_caps)(sample.ptr);
                ensure!(!caps.is_null(), "{}", t("video.decode_failed"));
                let structure = (api.gst_caps_get_structure)(caps, 0);
                let (mut width, mut height) = (0, 0);
                ensure!(
                    (api.gst_structure_get_int)(structure, c"width".as_ptr(), &mut width) != 0
                        && (api.gst_structure_get_int)(structure, c"height".as_ptr(), &mut height)
                            != 0,
                    "{}",
                    t("video.decode_failed")
                );
                let (width, height) = (u32::try_from(width)?, u32::try_from(height)?);
                let count = pixel_count(width, height)?;
                let mut rgba = vec![0; count * 4];
                let meta = (api.gst_buffer_get_video_meta)(buffer);
                let (offset, stride) = if meta.is_null() {
                    (0, width as i64 * 4)
                } else {
                    ensure!(
                        (*meta).planes == 1 && (*meta).width == width && (*meta).height == height,
                        "{}",
                        t("video.decode_failed")
                    );
                    (i64::try_from((*meta).offset[0])?, (*meta).stride[0] as i64)
                };
                ensure!(
                    stride.unsigned_abs() >= width as u64 * 4,
                    "{}",
                    t("video.decode_failed")
                );
                // GstVideoMeta can describe padded or bottom-up buffers even
                // with RGBA caps. Extract each row without exposing raw pointers.
                let row_size = width as usize * 4;
                for y in 0..height as usize {
                    let at = usize::try_from(offset + y as i64 * stride)?;
                    let row = &mut rgba[y * row_size..(y + 1) * row_size];
                    ensure!(
                        (api.gst_buffer_extract)(buffer, at, row.as_mut_ptr().cast(), row_size)
                            == row_size,
                        "{}",
                        t("video.decode_failed")
                    );
                }
                let (mut numerator, mut denominator) = (1, 1);
                (api.gst_structure_get_fraction)(
                    structure,
                    c"pixel-aspect-ratio".as_ptr(),
                    &mut numerator,
                    &mut denominator,
                );
                return display_frame(
                    Frame {
                        time,
                        width,
                        height,
                        rgba,
                    },
                    numerator as f64 / denominator.max(1) as f64,
                    [1, 0, 0, 1],
                    self.edge,
                )
                .map(Some);
            }
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            (self.pipeline.api.gst_element_set_state)(self.pipeline.ptr, 1);
        }
    } // NULL
}
