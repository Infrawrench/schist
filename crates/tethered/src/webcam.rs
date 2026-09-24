//! macOS webcam snapshots through AVFoundation. No camera is opened by discovery.
//! Capture runs on a worker; a serial native queue delivers a single RGB frame.
//! Stop and drain that queue before releasing the session, delegate or buffers.
use super::{cancelled, Camera, Captured, Result, Session};
use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, Encoding, RefEncode};
use objc2_foundation::NSString;
use schist_i18n::t;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::{
    atomic::AtomicBool,
    mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const PREFIX: &str = "avfoundation:";
const TIMEOUT: Duration = Duration::from_secs(30);
const WARMUP: Duration = Duration::from_millis(500);
const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;
const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: &'static NSString;
    static AVCaptureDeviceTypeBuiltInWideAngleCamera: &'static NSString;
    static AVCaptureDeviceTypeExternalUnknown: &'static NSString;
    static AVCaptureSessionPresetHigh: &'static NSString;
}
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetImageBuffer(sample: *const SampleBuffer) -> *mut c_void;
}
#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: &'static NSString;
    fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
    fn CVPixelBufferGetPixelFormatType(buffer: *mut c_void) -> u32;
    fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetDataSize(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *const u8;
}
extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(object: *mut c_void);
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

#[repr(C)]
struct SampleBuffer {
    _opaque: [u8; 0],
}
unsafe impl RefEncode for SampleBuffer {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
}

fn class(name: &'static CStr) -> Result<&'static AnyClass> {
    AnyClass::get(name).ok_or_else(|| t("common.not_available").into())
}

unsafe fn optional_device_type(name: &'static CStr) -> Option<&'static NSString> {
    // RTLD_DEFAULT on Darwin. Newer device types must not raise our deployment
    // target: fall back to ExternalUnknown on macOS versions before 14.
    let symbol = dlsym(-2isize as *mut c_void, name.as_ptr());
    (!symbol.is_null()).then(|| *symbol.cast::<&NSString>())
}

/// Stable IDs keep AVFoundation cameras separate from ImageCaptureCore devices.
pub fn is_camera(camera: &Camera) -> bool {
    camera.id.is_none() && camera.port.starts_with(PREFIX)
}

pub fn discover() -> Result<Vec<Camera>> {
    autoreleasepool(|_| unsafe {
        let mut types = vec![AVCaptureDeviceTypeBuiltInWideAngleCamera];
        types.push(
            optional_device_type(c"AVCaptureDeviceTypeExternal")
                .unwrap_or(AVCaptureDeviceTypeExternalUnknown),
        );
        if let Some(continuity) = optional_device_type(c"AVCaptureDeviceTypeContinuityCamera") {
            types.push(continuity);
        }
        let types: Retained<AnyObject> = msg_send![class(c"NSArray")?,
            arrayWithObjects: types.as_ptr(), count: types.len()];
        let discovery: Retained<AnyObject> = msg_send![class(c"AVCaptureDeviceDiscoverySession")?,
            discoverySessionWithDeviceTypes: &*types,
            mediaType: AVMediaTypeVideo, position: 0isize];
        let devices: Retained<AnyObject> = msg_send![&*discovery, devices];
        let count: usize = msg_send![&*devices, count];
        let mut cameras = Vec::new();
        for index in 0..count {
            let device: *mut AnyObject = msg_send![&*devices, objectAtIndex: index];
            let id: Retained<NSString> = msg_send![device, uniqueID];
            let model: Retained<NSString> = msg_send![device, localizedName];
            cameras.push(Camera {
                id: None,
                model: model.to_string(),
                port: format!("{PREFIX}{id}"),
            });
        }
        Ok(cameras)
    })
}

fn wait<T>(receiver: &Receiver<T>, cancel: &AtomicBool, deadline: Instant) -> Result<T> {
    loop {
        cancelled(cancel)?;
        if Instant::now() >= deadline {
            return Err(t("tethered.timeout").into());
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => return Ok(value),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(t("tethered.no_download").into()),
        }
    }
}

fn authorize(cancel: &AtomicBool) -> Result<()> {
    cancelled(cancel)?;
    unsafe {
        let device = class(c"AVCaptureDevice")?;
        let status: isize = msg_send![device, authorizationStatusForMediaType: AVMediaTypeVideo];
        match status {
            3 => return Ok(()), // AVAuthorizationStatusAuthorized
            0 => {}             // AVAuthorizationStatusNotDetermined
            _ => return Err(t("tethered.camera_permission").into()),
        }
        // Calling requestAccess without this key terminates the process. This
        // also protects embedders and test runners that have no application plist.
        let bundle: Retained<AnyObject> = msg_send![class(c"NSBundle")?, mainBundle];
        let description: Option<Retained<NSString>> = msg_send![&*bundle,
            objectForInfoDictionaryKey: &*NSString::from_str("NSCameraUsageDescription")];
        if description.is_none_or(|description| description.is_empty()) {
            return Err(t("tethered.camera_usage_missing").into());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let completion = RcBlock::new(move |granted: Bool| {
            let _ = sender.try_send(granted.as_bool());
        });
        let _: () = msg_send![device,
            requestAccessForMediaType: AVMediaTypeVideo, completionHandler: &*completion];
        if wait(&receiver, cancel, Instant::now() + Duration::from_secs(120))? {
            Ok(())
        } else {
            Err(t("tethered.camera_permission").into())
        }
    }
}

#[derive(Debug)]
struct Frame {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

fn frame_layout(width: usize, height: usize, stride: usize, length: usize) -> Result<usize> {
    let row = width.checked_mul(4);
    let needed = height
        .checked_sub(1)
        .and_then(|h| h.checked_mul(stride))
        .and_then(|offset| offset.checked_add(row?));
    match (row, needed) {
        (Some(row), Some(needed))
            if width > 0
                && width <= u32::MAX as usize
                && height <= u32::MAX as usize
                && row <= stride
                && needed <= length
                && needed <= MAX_FRAME_BYTES =>
        {
            Ok(needed)
        }
        _ => Err(t("tethered.no_download").into()),
    }
}

fn copy_bgra(width: usize, height: usize, stride: usize, bytes: &[u8]) -> Result<Frame> {
    frame_layout(width, height, stride, bytes.len())?;
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for pixel in bytes[y * stride..y * stride + width * 4].as_chunks::<4>().0 {
            rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
    }
    Ok(Frame {
        width: width as u32,
        height: height as u32,
        rgb,
    })
}

struct LockedBuffer(*mut c_void);
impl Drop for LockedBuffer {
    fn drop(&mut self) {
        unsafe {
            CVPixelBufferUnlockBaseAddress(self.0, 1);
        }
    }
}

unsafe fn copy_sample(sample: *const SampleBuffer) -> Result<Frame> {
    if sample.is_null() {
        return Err(t("tethered.no_download").into());
    }
    let buffer = CMSampleBufferGetImageBuffer(sample);
    if buffer.is_null()
        || CVPixelBufferGetPixelFormatType(buffer) != BGRA
        || CVPixelBufferLockBaseAddress(buffer, 1) != 0
    {
        return Err(t("tethered.no_download").into());
    }
    let _lock = LockedBuffer(buffer);
    let width = CVPixelBufferGetWidth(buffer);
    let height = CVPixelBufferGetHeight(buffer);
    let stride = CVPixelBufferGetBytesPerRow(buffer);
    let length = frame_layout(width, height, stride, CVPixelBufferGetDataSize(buffer))?;
    let base = CVPixelBufferGetBaseAddress(buffer);
    if base.is_null() {
        return Err(t("tethered.no_download").into());
    }
    copy_bgra(
        width,
        height,
        stride,
        std::slice::from_raw_parts(base, length),
    )
}

struct FrameSink {
    sender: Option<SyncSender<Result<Frame>>>,
    ready_at: Option<Instant>,
    cancel: Arc<AtomicBool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Mutex<FrameSink>]
    struct FrameDelegate;

    impl FrameDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn received(&self, _output: *mut AnyObject, sample: *const SampleBuffer, _connection: *mut AnyObject) {
            let mut sink = self.ivars().lock().unwrap_or_else(|error| error.into_inner());
            if sink.sender.is_none() || cancelled(&sink.cancel).is_err() {
                return;
            }
            // Let automatic exposure settle after the first delivered frame.
            let ready_at = *sink.ready_at.get_or_insert_with(|| Instant::now() + WARMUP);
            if Instant::now() < ready_at {
                return;
            }
            if let Some(sender) = sink.sender.take() {
                let _ = sender.try_send(unsafe { copy_sample(sample) });
            }
        }
    }
);

impl FrameDelegate {
    fn new(sender: SyncSender<Result<Frame>>, cancel: Arc<AtomicBool>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Mutex::new(FrameSink {
            sender: Some(sender),
            ready_at: None,
            cancel,
        }));
        unsafe { msg_send![super(this), init] }
    }
}

struct CaptureSession {
    session: Retained<AnyObject>,
    output: Retained<AnyObject>,
    _delegate: Retained<FrameDelegate>,
    queue: *mut c_void,
}
impl CaptureSession {
    unsafe fn new(
        session: Retained<AnyObject>,
        output: Retained<AnyObject>,
        delegate: Retained<FrameDelegate>,
    ) -> Result<Self> {
        let queue = dispatch_queue_create(c"com.infrawrench.schist.webcam".as_ptr(), ptr::null());
        if queue.is_null() {
            return Err(t("common.not_available").into());
        }
        let capture = Self {
            session,
            output,
            _delegate: delegate,
            queue,
        };
        // dispatch_queue_t is an Objective-C object in this method's ABI,
        // although the dispatch C entry points take an opaque pointer.
        let _: () = msg_send![&*capture.output,
            setSampleBufferDelegate: &*capture._delegate, queue: queue.cast::<AnyObject>()];
        Ok(capture)
    }
}
impl Drop for CaptureSession {
    fn drop(&mut self) {
        unsafe {
            // stopRunning is synchronous. Clear the delegate and drain callbacks
            // already enqueued before dropping anything those callbacks can use.
            let _: () = msg_send![&*self.session, stopRunning];
            let _: () = msg_send![&*self.output,
                setSampleBufferDelegate: ptr::null::<AnyObject>(), queue: ptr::null::<AnyObject>()];
            extern "C" fn drained(_: *mut c_void) {}
            dispatch_sync_f(self.queue, ptr::null_mut(), drained);
            dispatch_release(self.queue);
        }
    }
}

unsafe fn native_error(error: *mut AnyObject) -> String {
    if error.is_null() {
        return t("common.not_available").into();
    }
    let detail: Retained<NSString> = msg_send![error, localizedDescription];
    schist_i18n::tf!("tethered.failed", detail = detail.to_string())
}

fn snapshot(camera: &Camera, cancel: Arc<AtomicBool>) -> Result<Frame> {
    let id = camera
        .port
        .strip_prefix(PREFIX)
        .filter(|_| camera.id.is_none())
        .ok_or_else(|| t("library.import.camera_gone").to_string())?;
    authorize(&cancel)?;
    cancelled(&cancel)?;
    unsafe {
        let device: Option<Retained<AnyObject>> = msg_send![class(c"AVCaptureDevice")?,
            deviceWithUniqueID: &*NSString::from_str(id)];
        let device = device.ok_or_else(|| t("library.import.camera_gone").to_string())?;
        let mut error: *mut AnyObject = ptr::null_mut();
        let input: Option<Retained<AnyObject>> = msg_send![class(c"AVCaptureDeviceInput")?,
            deviceInputWithDevice: &*device, error: &mut error];
        let input = input.ok_or_else(|| native_error(error))?;
        let session: Retained<AnyObject> = msg_send![class(c"AVCaptureSession")?, new];
        let output: Retained<AnyObject> = msg_send![class(c"AVCaptureVideoDataOutput")?, new];
        let format: Retained<AnyObject> =
            msg_send![class(c"NSNumber")?, numberWithUnsignedInt: BGRA];
        let settings: Retained<AnyObject> = msg_send![class(c"NSDictionary")?,
            dictionaryWithObject: &*format, forKey: kCVPixelBufferPixelFormatTypeKey];
        let _: () = msg_send![&*output, setVideoSettings: &*settings];
        let _: () = msg_send![&*output, setAlwaysDiscardsLateVideoFrames: true];
        let can_add_input: bool = msg_send![&*session, canAddInput: &*input];
        if !can_add_input {
            return Err(t("tethered.unsupported").into());
        }
        let _: () = msg_send![&*session, addInput: &*input];
        let can_add_output: bool = msg_send![&*session, canAddOutput: &*output];
        if !can_add_output {
            return Err(t("tethered.unsupported").into());
        }
        let _: () = msg_send![&*session, addOutput: &*output];
        let high: bool = msg_send![&*session, canSetSessionPreset: AVCaptureSessionPresetHigh];
        if high {
            let _: () = msg_send![&*session, setSessionPreset: AVCaptureSessionPresetHigh];
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let delegate = FrameDelegate::new(sender, cancel.clone());
        let capture = CaptureSession::new(session, output, delegate)?;
        cancelled(&cancel)?;
        let deadline = Instant::now() + TIMEOUT;
        let _: () = msg_send![&*capture.session, startRunning];
        let running: bool = msg_send![&*capture.session, isRunning];
        if !running {
            return Err(t("common.not_available").into());
        }
        wait(&receiver, &cancel, deadline)?
    }
}

/// Called from a worker, never the UI thread. All native ownership ends before
/// JPEG encoding/publication; cancellation never leaves the webcam streaming.
pub fn capture(camera: &Camera, session: &Session, cancel: Arc<AtomicBool>) -> Result<Captured> {
    let staging = super::staging(session)?;
    let frame = autoreleasepool(|_| snapshot(camera, cancel.clone()))?;
    cancelled(&cancel)?;
    let path = staging.path().join("capture.jpg");
    let file = std::fs::File::create(path).map_err(super::io_error)?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95)
        .encode(
            &frame.rgb,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(super::io_error)?;
    super::publish(staging.path(), session, &cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_bgra_rows_become_rgb_without_alpha_or_padding() {
        let bytes = [
            1, 2, 3, 255, 4, 5, 6, 0, 99, 99, 99, 99, 7, 8, 9, 255, 10, 11, 12, 255,
        ];
        let frame = copy_bgra(2, 2, 12, &bytes).unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.rgb, [3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]);
    }

    #[test]
    fn malformed_or_unbounded_frame_layouts_are_rejected() {
        for (width, height, stride, length) in [
            (0, 1, 4, 4),
            (1, 0, 4, 4),
            (2, 1, 4, 8),
            (1, 2, 8, 11),
            (usize::MAX, 1, 4, 4),
            (1, usize::MAX, 8, usize::MAX),
            (1, 2, MAX_FRAME_BYTES, MAX_FRAME_BYTES + 4),
        ] {
            assert!(frame_layout(width, height, stride, length).is_err());
        }
    }

    #[test]
    fn waiting_obeys_cancellation_timeout_and_disconnect() {
        let (sender, receiver) = mpsc::channel::<()>();
        assert_eq!(
            wait(&receiver, &AtomicBool::new(true), Instant::now() + TIMEOUT).unwrap_err(),
            t("common.cancelled")
        );
        assert_eq!(
            wait(&receiver, &AtomicBool::new(false), Instant::now()).unwrap_err(),
            t("tethered.timeout")
        );
        drop(sender);
        assert_eq!(
            wait(&receiver, &AtomicBool::new(false), Instant::now() + TIMEOUT).unwrap_err(),
            t("tethered.no_download")
        );
    }

    #[test]
    fn delegate_registers_and_drops_without_a_camera() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let delegate = FrameDelegate::new(sender, Arc::default());
        drop(delegate);
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn native_video_output_accepts_and_releases_the_delegate_queue() {
        autoreleasepool(|_| unsafe {
            let (sender, receiver) = mpsc::sync_channel(1);
            let delegate = FrameDelegate::new(sender, Arc::default());
            let session: Retained<AnyObject> = msg_send![class(c"AVCaptureSession").unwrap(), new];
            let output: Retained<AnyObject> =
                msg_send![class(c"AVCaptureVideoDataOutput").unwrap(), new];
            drop(CaptureSession::new(session, output, delegate).unwrap());
            assert!(matches!(
                receiver.try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            ));
        });
    }

    #[test]
    fn image_capture_core_ids_never_route_to_the_webcam_backend() {
        let mut camera = Camera {
            id: None,
            model: "Camera".into(),
            port: format!("{PREFIX}123"),
        };
        assert!(is_camera(&camera));
        camera.id = Some(123);
        assert!(!is_camera(&camera));
        camera.id = None;
        camera.port = "usb:001,002".into();
        assert!(!is_camera(&camera));
    }

    #[test]
    #[ignore = "requires a connected webcam; discovery does not request camera access"]
    fn hardware_discovery() {
        let cameras = discover().unwrap();
        assert!(!cameras.is_empty(), "no webcam connected");
        for camera in cameras {
            assert!(is_camera(&camera));
            assert!(!camera.model.is_empty());
            println!("{}", camera.model);
        }
    }

    #[test]
    #[ignore = "opens the first webcam and may show the macOS camera permission prompt"]
    fn hardware_capture_and_cancel() {
        let camera = discover().unwrap().into_iter().next().expect("a webcam");
        let destination = tempfile::tempdir().unwrap();
        let session = Session {
            destination: destination.path().into(),
            prefix: "webcam-test".into(),
        };
        let captured = capture(&camera, &session, Arc::default()).unwrap();
        assert_eq!(
            captured.paths,
            [destination.path().join("webcam-test-000001.jpg")]
        );
        let decoded = image::open(&captured.paths[0]).unwrap();
        assert!(decoded.width() > 0 && decoded.height() > 0);
        println!("JPEG: {} × {}", decoded.width(), decoded.height());
        let original = std::fs::read(&captured.paths[0]).unwrap();

        // Cancel while a session is opening/streaming, then capture again to
        // check that its native queue and camera ownership were released.
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, std::sync::atomic::Ordering::Release);
        });
        assert_eq!(
            capture(&camera, &session, cancel).unwrap_err(),
            t("common.cancelled")
        );
        worker.join().unwrap();
        let second = capture(&camera, &session, Arc::default()).unwrap();
        assert_eq!(
            second.paths,
            [destination.path().join("webcam-test-000002.jpg")]
        );
        assert_eq!(std::fs::read(&captured.paths[0]).unwrap(), original);
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 2);
    }
}
