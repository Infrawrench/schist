//! iPhones and PTP cameras on Apple platforms, through ImageCaptureCore.
#![cfg_attr(target_os = "ios", allow(dead_code))] // iOS uses Photos for bulk import.
//!
//! Those devices never mount as filesystems, so the gallery's
//! DCIM-volume scan cannot see them; ImageCaptureCore is the door Image
//! Capture and Photos use. Like the Quick Look providers, there is no
//! Objective-C source here: one delegate class is assembled with the
//! runtime's class builder, serving as the device-browser delegate, the
//! camera delegate and the download delegate all at once.
//!
//! Threading: everything ObjC-touching runs on the main thread. The
//! browser is started from a UI handler, so ImageCaptureCore delivers
//! its delegate callbacks on the main run loop, which gpui is already
//! pumping. The `Mutex` around [`Shared`] protects only the Rust
//! bookkeeping; the workspace polls it from a timer to draw progress.

pub(super) use super::camera_import::downloaded::KeepFilter;
use super::camera_import::{
    downloaded::{Outcome, Processor},
    ImportDestination,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::NSString;
use schist_i18n::t;
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[link(name = "ImageCaptureCore", kind = "framework")]
extern "C" {
    /// Options key: the directory a requested download lands in.
    static ICDownloadsDirectoryURL: &'static NSString;
    static ICSaveAsFilename: &'static NSString;
    /// Options key in the completion callback: the name the file was
    /// actually saved under (ImageCaptureCore renames on collision).
    static ICSavedFilename: &'static NSString;
    #[cfg(target_os = "macos")]
    static ICCameraDeviceCanTakePicture: &'static NSString;
    #[cfg(target_os = "ios")]
    static ICCameraDeviceCanAcceptPTPCommands: &'static NSString;
}

/// An ObjC pointer that crosses the mutex. Only ever dereferenced on
/// the main thread — the mutex guards the bookkeeping, not the object.
struct ObjPtr(*mut AnyObject);
unsafe impl Send for ObjPtr {}

struct Device {
    id: u64,
    name: String,
    can_capture: bool,
    obj: ObjPtr,
}

/// One import in flight. `keep` decides a downloaded file's fate (the
/// place filter); a file it declines is deleted and counted, so the
/// destination ends up holding exactly what was asked for.
struct Job {
    device_id: u64,
    device: ObjPtr,
    dest: ImportDestination,
    processor: Processor,
    generation: usize,
    /// Downloads requested; `None` until the catalog has been read.
    total: Option<usize>,
    done: usize,
    copied: usize,
    filtered: usize,
    failed: usize,
    locked: bool,
    finished: Option<Result<(), String>>,
}

enum TetheredPhase {
    Opening,
    #[cfg(target_os = "ios")]
    Probing,
    Capturing,
}

struct TetheredJob {
    device_id: u64,
    device: ObjPtr,
    generation: usize,
    staging: tempfile::TempDir,
    baseline: HashSet<usize>,
    queued: HashSet<usize>,
    started: Instant,
    last_added: Option<Instant>,
    total: usize,
    done: usize,
    succeeded: usize,
    warning: Option<String>,
    phase: TetheredPhase,
    finished: Option<Result<(), String>>,
    closed: bool,
}

struct Shared {
    devices: Vec<Device>,
    next_id: u64,
    next_import: usize,
    job: Option<Job>,
    tethered: Option<TetheredJob>,
    started: bool,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    devices: Vec::new(),
    next_id: 1,
    next_import: 1,
    job: None,
    tethered: None,
    started: false,
});

/// What the workspace's progress poll sees.
pub(super) struct ImportStatus {
    pub done: usize,
    pub total: Option<usize>,
    pub locked: bool,
    /// `Some` once everything settled: Ok((copied, filtered, failed)).
    pub finished: Option<Result<(usize, usize, usize), String>>,
}

pub(super) struct TetheredStatus {
    pub finished: Option<Result<(), String>>,
}

pub(super) struct TetheredDownload {
    pub staging: tempfile::TempDir,
    pub warning: Option<String>,
}

fn lock() -> std::sync::MutexGuard<'static, Shared> {
    // A poisoned lock here means a callback panicked; the state is
    // plain counters, safe to keep using.
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn ns_string(ptr: *mut AnyObject) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some((*(ptr as *const NSString)).to_string())
}

unsafe fn error_string(error: *mut AnyObject) -> String {
    let description: *mut AnyObject = msg_send![error, localizedDescription];
    ns_string(description).unwrap_or_else(|| t("library.import.unknown_error").into())
}

/// Start watching for cameras. Idempotent; call from the main thread.
pub(super) fn start_browsing() {
    {
        let mut shared = lock();
        if shared.started {
            return;
        }
        shared.started = true;
    }
    let Some(browser_class) = AnyClass::get(c"ICDeviceBrowser") else {
        log::warn!("gallery: ImageCaptureCore is not available");
        return;
    };
    let delegate = delegate();
    unsafe {
        let browser: *mut AnyObject = msg_send![browser_class, new];
        let _: () = msg_send![browser, setDelegate: delegate];
        // Camera devices (0x1) on this machine's own ports (0x100).
        let mask: usize = 0x0000_0001 | 0x0000_0100;
        let _: () = msg_send![browser, setBrowsedDeviceTypeMask: mask];
        let _: () = msg_send![browser, start];
        // The browser lives as long as the app; never released.
    }
    log::info!("gallery: watching for cameras over ImageCaptureCore");
}

/// The connected devices, for the import picker.
pub(super) fn devices() -> Vec<(u64, String)> {
    lock()
        .devices
        .iter()
        .map(|d| (d.id, d.name.clone()))
        .collect()
}

pub(super) fn tethered_devices() -> Vec<schist_tethered::Camera> {
    lock()
        .devices
        .iter()
        .filter(|device| device.can_capture)
        .map(|device| schist_tethered::Camera {
            id: Some(device.id),
            model: device.name.clone(),
            port: String::new(),
        })
        .collect()
}

/// Open the device and start pulling its photos into `dest`. The rest
/// happens in delegate callbacks; poll [`poll_import`] for progress.
pub(super) fn begin_import(
    id: u64,
    dest: ImportDestination,
    keep: Option<KeepFilter>,
) -> Result<(), String> {
    std::fs::create_dir_all(dest.path()).map_err(|e| e.to_string())?;
    let device = {
        let mut shared = lock();
        if shared.job.is_some() || shared.tethered.is_some() {
            return Err(t("library.import.already_running").into());
        }
        let Some(device) = shared.devices.iter().find(|d| d.id == id) else {
            return Err(t("library.import.camera_gone").into());
        };
        let ptr = device.obj.0;
        let generation = shared.next_import;
        shared.next_import += 1;
        let processor = Processor::new(dest.clone(), keep);
        shared.job = Some(Job {
            device_id: id,
            device: ObjPtr(ptr),
            dest,
            processor,
            generation,
            total: None,
            done: 0,
            copied: 0,
            filtered: 0,
            failed: 0,
            locked: false,
            finished: None,
        });
        ptr
    };
    let delegate = delegate();
    unsafe {
        let _: () = msg_send![device, setDelegate: delegate];
        let _: () = msg_send![device, requestOpenSession];
    }
    log::info!("gallery: opening camera session");
    Ok(())
}

pub(super) fn begin_tethered(id: u64, staging: tempfile::TempDir) -> Result<(), String> {
    let mut shared = lock();
    if shared.job.is_some() || shared.tethered.is_some() {
        return Err(t("library.import.already_running").into());
    }
    let Some(device) = shared.devices.iter().find(|device| device.id == id) else {
        return Err(t("library.import.camera_gone").into());
    };
    if !device.can_capture {
        return Err(t("tethered.unsupported").into());
    }
    let ptr = device.obj.0;
    let retained =
        unsafe { Retained::retain(ptr) }.ok_or_else(|| t("common.not_available").to_string())?;
    let owned = ObjPtr(Retained::into_raw(retained));
    let generation = shared.next_import;
    shared.next_import += 1;
    shared.tethered = Some(TetheredJob {
        device_id: id,
        device: owned,
        generation,
        staging,
        baseline: HashSet::new(),
        queued: HashSet::new(),
        started: Instant::now(),
        last_added: None,
        total: 0,
        done: 0,
        succeeded: 0,
        warning: None,
        phase: TetheredPhase::Opening,
        finished: None,
        closed: false,
    });
    drop(shared);
    let delegate = delegate();
    unsafe {
        let _: () = msg_send![ptr, setDelegate: delegate];
        let _: () = msg_send![ptr, requestOpenSession];
    }
    Ok(())
}

pub(super) fn poll_tethered() -> Option<TetheredStatus> {
    let completion = {
        let shared = lock();
        let job = shared.tethered.as_ref()?;
        if let Some(result) = &job.finished {
            Some(result.clone())
        } else if job.started.elapsed() >= Duration::from_secs(120) {
            Some(Err(t("tethered.timeout").into()))
        } else if job.succeeded > 0
            && job.done >= job.total
            && job
                .last_added
                .is_some_and(|last| last.elapsed() >= Duration::from_secs(2))
        {
            Some(Ok(()))
        } else if job.total > 0 && job.done >= job.total && job.succeeded == 0 {
            Some(Err(t("tethered.no_download").into()))
        } else {
            None
        }
    };
    if let Some(result) = completion.clone() {
        conclude_tethered(result);
    }
    let shared = lock();
    let job = shared.tethered.as_ref()?;
    Some(TetheredStatus {
        finished: job.closed.then(|| job.finished.clone()).flatten(),
    })
}

pub(super) fn cancel_tethered() {
    conclude_tethered(Err(t("common.cancelled").into()));
}

pub(super) fn take_tethered() -> Option<TetheredDownload> {
    let job = {
        let mut shared = lock();
        let job = shared.tethered.take()?;
        if job.finished.is_none() || !job.closed {
            shared.tethered = Some(job);
            return None;
        }
        job
    };
    unsafe {
        drop(Retained::from_raw(job.device.0));
    }
    Some(TetheredDownload {
        staging: job.staging,
        warning: job.warning,
    })
}

fn conclude_tethered(result: Result<(), String>) {
    let device = {
        let mut shared = lock();
        let Some(job) = shared.tethered.as_mut() else {
            return;
        };
        if job.finished.is_some() {
            return;
        }
        job.finished = Some(result);
        job.device.0
    };
    unsafe {
        let _: () = msg_send![device, cancelDownload];
        let _: () = msg_send![device, requestCloseSession];
    }
}

extern "C" fn did_close_session(
    _this: *mut AnyObject,
    _sel: Sel,
    device: *mut AnyObject,
    _error: *mut AnyObject,
) {
    let mut shared = lock();
    if let Some(job) = shared
        .tethered
        .as_mut()
        .filter(|job| job.device.0 == device)
    {
        job.closed = true;
        if job.finished.is_none() {
            job.finished = Some(Err(t("library.import.camera_disconnected").into()));
        }
    }
}

/// A snapshot of the running import, or `None` when there is none.
pub(super) fn poll_import() -> Option<ImportStatus> {
    let complete = {
        let mut shared = lock();
        let job = shared.job.as_mut()?;
        for outcome in job.processor.results.try_iter() {
            job.done += 1;
            match outcome {
                Outcome::Copied => job.copied += 1,
                Outcome::Filtered => job.filtered += 1,
                Outcome::Failed => job.failed += 1,
            }
        }
        if let Some(total) = job.total {
            job.dest
                .expect(total.saturating_sub(job.filtered + job.failed));
        }
        job.finished.is_none() && job.total.is_some_and(|total| job.done >= total)
    };
    if complete {
        conclude(Ok(()));
    }
    let shared = lock();
    let job = shared.job.as_ref()?;
    Some(ImportStatus {
        done: job.done,
        total: job.total,
        locked: job.locked,
        finished: job.finished.as_ref().map(|r| match r {
            Ok(()) => Ok((job.copied, job.filtered, job.failed)),
            Err(e) => Err(e.clone()),
        }),
    })
}

/// Forget the finished import so the next one can start.
pub(super) fn finish_import() {
    lock().job = None;
}

/// End the job, closing the camera session.
fn conclude(result: Result<(), String>) {
    let device = {
        let mut shared = lock();
        let Some(job) = shared.job.as_mut() else {
            return;
        };
        if job.finished.is_some() {
            return;
        }
        job.finished = Some(result);
        job.device.0
    };
    unsafe {
        let _: () = msg_send![device, requestCloseSession];
    }
}

/// The one delegate instance, creating its class on first use.
fn delegate() -> *mut AnyObject {
    static INSTANCE: OnceLock<usize> = OnceLock::new();
    *INSTANCE.get_or_init(|| {
        let superclass = AnyClass::get(c"NSObject").expect("NSObject exists");
        let mut builder =
            ClassBuilder::new(c"SchistImageCapture", superclass).expect("class name is free");
        unsafe {
            // Device browser.
            builder.add_method(
                sel!(deviceBrowser:didAddDevice:moreComing:),
                did_add_device as extern "C" fn(_, _, _, _, _),
            );
            // Not a typo: additions announce "moreComing", removals
            // "moreGoing". Registering the wrong spelling is an
            // unrecognized-selector abort the moment a camera unplugs.
            builder.add_method(
                sel!(deviceBrowser:didRemoveDevice:moreGoing:),
                did_remove_device as extern "C" fn(_, _, _, _, _),
            );
            // Device session.
            builder.add_method(
                sel!(device:didOpenSessionWithError:),
                did_open_session as extern "C" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(device:didCloseSessionWithError:),
                did_close_session as extern "C" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(didRemoveDevice:),
                device_went_away as extern "C" fn(_, _, _),
            );
            // Camera catalog. The ready callback is the one that
            // matters; the rest are required by the delegate protocol
            // and deliberately do nothing.
            builder.add_method(
                sel!(deviceDidBecomeReadyWithCompleteContentCatalog:),
                device_ready as extern "C" fn(_, _, _),
            );
            builder.add_method(
                sel!(cameraDevice:didAddItems:),
                did_add_items as extern "C" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(cameraDevice:didRemoveItems:),
                two_args_noop as extern "C" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(cameraDevice:didRenameItems:),
                two_args_noop as extern "C" fn(_, _, _, _),
            );
            builder.add_method(
                sel!(cameraDeviceDidChangeCapability:),
                camera_capability_changed as extern "C" fn(_, _, _),
            );
            // Four selector arguments each: (device, payload, item,
            // error). objc2 checks the count when the class is built,
            // so a miscount here is a launch-time abort, not a bug that
            // waits for a callback.
            builder.add_method(
                sel!(cameraDevice:didReceiveThumbnail:forItem:error:),
                four_args_noop as extern "C" fn(_, _, _, _, _, _),
            );
            builder.add_method(
                sel!(cameraDevice:didReceiveMetadata:forItem:error:),
                four_args_noop as extern "C" fn(_, _, _, _, _, _),
            );
            // A passcode-locked iPhone: say so instead of hanging.
            builder.add_method(
                sel!(cameraDeviceDidEnableAccessRestriction:),
                access_restricted as extern "C" fn(_, _, _),
            );
            builder.add_method(
                sel!(cameraDeviceDidRemoveAccessRestriction:),
                access_granted as extern "C" fn(_, _, _),
            );
            // Downloads.
            builder.add_method(
                sel!(didDownloadFile:error:options:contextInfo:),
                did_download as extern "C" fn(_, _, _, _, _, _),
            );
            #[cfg(target_os = "ios")]
            builder.add_method(
                sel!(didSendPTPCommand:inData:response:error:contextInfo:),
                did_send_ptp as extern "C" fn(_, _, _, _, _, _, _),
            );
        }
        let class = builder.register();
        let instance: *mut AnyObject = unsafe { msg_send![class, new] };
        instance as usize
    }) as *mut AnyObject
}

extern "C" fn two_args_noop(
    _this: *mut AnyObject,
    _sel: Sel,
    _a: *mut AnyObject,
    _b: *mut AnyObject,
) {
}
extern "C" fn four_args_noop(
    _this: *mut AnyObject,
    _sel: Sel,
    _a: *mut AnyObject,
    _b: *mut AnyObject,
    _c: *mut AnyObject,
    _d: *mut AnyObject,
) {
}

extern "C" fn did_add_device(
    _this: *mut AnyObject,
    _sel: Sel,
    _browser: *mut AnyObject,
    device: *mut AnyObject,
    _more: Bool,
) {
    if device.is_null() {
        return;
    }
    unsafe {
        // Owned for as long as it stays connected.
        let Some(retained) = Retained::retain(device) else {
            return;
        };
        let raw = Retained::into_raw(retained);
        let name_obj: *mut AnyObject = msg_send![raw, name];
        let name = ns_string(name_obj).unwrap_or_else(|| t("library.volume.camera").into());
        let can_capture = can_take_picture(raw);
        let mut shared = lock();
        let id = shared.next_id;
        shared.next_id += 1;
        log::info!("gallery: camera connected: {name}");
        shared.devices.push(Device {
            id,
            name,
            can_capture,
            obj: ObjPtr(raw),
        });
    }
}

extern "C" fn did_remove_device(
    _this: *mut AnyObject,
    _sel: Sel,
    _browser: *mut AnyObject,
    device: *mut AnyObject,
    _more: Bool,
) {
    forget_device(device);
}

extern "C" fn camera_capability_changed(_this: *mut AnyObject, _sel: Sel, device: *mut AnyObject) {
    let can_capture = unsafe { can_take_picture(device) };
    let mut shared = lock();
    if let Some(device) = shared
        .devices
        .iter_mut()
        .find(|candidate| candidate.obj.0 == device)
    {
        device.can_capture = can_capture;
    }
}

unsafe fn can_take_picture(device: *mut AnyObject) -> bool {
    let capabilities: *mut AnyObject = msg_send![device, capabilities];
    #[cfg(target_os = "macos")]
    let capability = ICCameraDeviceCanTakePicture;
    #[cfg(target_os = "ios")]
    let capability = ICCameraDeviceCanAcceptPTPCommands;
    #[cfg(target_os = "ios")]
    {
        let responds: bool = msg_send![device, respondsToSelector: sel!(requestSendPTPCommand:outData:sendCommandDelegate:didSendCommand:contextInfo:)];
        if !responds {
            return false;
        }
    }
    !capabilities.is_null() && unsafe { msg_send![capabilities, containsObject: capability] }
}

extern "C" fn did_add_items(
    _this: *mut AnyObject,
    _sel: Sel,
    device: *mut AnyObject,
    items: *mut AnyObject,
) {
    if items.is_null() {
        return;
    }
    let (staging, generation) = {
        let mut shared = lock();
        let Some(job) = shared.tethered.as_mut() else {
            return;
        };
        if job.device.0 != device
            || job.finished.is_some()
            || !matches!(job.phase, TetheredPhase::Capturing)
        {
            return;
        }
        (job.staging.path().to_path_buf(), job.generation)
    };
    let count: usize = unsafe { msg_send![items, count] };
    let pending: Vec<*mut AnyObject> = unsafe {
        (0..count)
            .map(|index| -> *mut AnyObject { msg_send![items, objectAtIndex: index] })
            .filter(|item| !item.is_null())
            .filter(|item| {
                let class = AnyClass::get(c"ICCameraFile").expect("ICCameraFile exists");
                let is_file: bool = msg_send![*item, isKindOfClass: class];
                is_file
            })
            .collect()
    };
    let pending = {
        let mut shared = lock();
        let Some(job) = shared.tethered.as_mut() else {
            return;
        };
        if job.finished.is_some() {
            return;
        }
        let pending: Vec<_> = pending
            .into_iter()
            .filter(|item| !job.baseline.contains(&(*item as usize)))
            .filter(|item| job.queued.insert(*item as usize))
            .collect();
        if !pending.is_empty() {
            job.total += pending.len();
            job.last_added = Some(Instant::now());
        }
        pending
    };
    let delegate = delegate();
    let dest_ns = NSString::from_str(&staging.to_string_lossy());
    let url_class = AnyClass::get(c"NSURL").expect("NSURL exists");
    let dict_class = AnyClass::get(c"NSMutableDictionary").expect("NSMutableDictionary exists");
    unsafe {
        let url: *mut AnyObject =
            msg_send![url_class, fileURLWithPath: &*dest_ns, isDirectory: Bool::YES];
        for file in pending {
            let name: *mut AnyObject = msg_send![file, name];
            let name = ns_string(name).unwrap_or_default();
            let path = match schist_tethered::download_path(&staging, file as usize, &name) {
                Ok(path) => path,
                Err(error) => {
                    conclude_tethered(Err(error));
                    return;
                }
            };
            let filename = NSString::from_str(&path.file_name().unwrap().to_string_lossy());
            let options: *mut AnyObject = msg_send![dict_class, dictionary];
            let _: () = msg_send![options, setObject: url, forKey: ICDownloadsDirectoryURL];
            let _: () = msg_send![options, setObject: &*filename, forKey: ICSaveAsFilename];
            let _: () = msg_send![
                device,
                requestDownloadFile: file,
                options: options,
                downloadDelegate: delegate,
                didDownloadSelector: sel!(didDownloadFile:error:options:contextInfo:),
                contextInfo: generation as *mut c_void
            ];
        }
    }
}

/// ICDeviceDelegate's own removal notice; arrives for open sessions.
extern "C" fn device_went_away(_this: *mut AnyObject, _sel: Sel, device: *mut AnyObject) {
    forget_device(device);
}

fn forget_device(device: *mut AnyObject) {
    let mut shared = lock();
    let Some(at) = shared.devices.iter().position(|d| d.obj.0 == device) else {
        return;
    };
    let gone = shared.devices.remove(at);
    log::info!("gallery: camera disconnected: {}", gone.name);
    if let Some(job) = shared.job.as_mut() {
        if job.device_id == gone.id && job.finished.is_none() {
            job.finished = Some(Err(t("library.import.camera_disconnected").into()));
        }
    }
    if let Some(job) = shared.tethered.as_mut() {
        if job.device_id == gone.id && job.finished.is_none() {
            job.finished = Some(Err(t("library.import.camera_disconnected").into()));
        }
        if job.device_id == gone.id {
            job.closed = true;
        }
    }
    drop(shared);
    // Balances the retain in `did_add_device`.
    drop(unsafe { Retained::from_raw(gone.obj.0) });
}

extern "C" fn did_open_session(
    _this: *mut AnyObject,
    _sel: Sel,
    device: *mut AnyObject,
    error: *mut AnyObject,
) {
    if error.is_null() {
        log::info!("gallery: camera session open; waiting for its catalog");
        return;
    }
    let what = unsafe { error_string(error) };
    log::warn!("gallery: camera session failed to open: {what}");
    let (importing, tethering) = {
        let shared = lock();
        (
            shared
                .job
                .as_ref()
                .is_some_and(|job| job.device.0 == device && job.finished.is_none()),
            shared
                .tethered
                .as_ref()
                .is_some_and(|job| job.device.0 == device && job.finished.is_none()),
        )
    };
    if importing {
        conclude(Err(what));
    } else if tethering {
        conclude_tethered(Err(what));
        // Opening failed, so there is no session whose close can be awaited.
        if let Some(job) = lock().tethered.as_mut() {
            job.closed = true;
        }
    }
}

extern "C" fn access_restricted(_this: *mut AnyObject, _sel: Sel, _device: *mut AnyObject) {
    log::info!("gallery: the camera is passcode-locked");
    if let Some(job) = lock().job.as_mut() {
        job.locked = true;
    }
}

extern "C" fn access_granted(_this: *mut AnyObject, _sel: Sel, _device: *mut AnyObject) {
    log::info!("gallery: the camera was unlocked");
    if let Some(job) = lock().job.as_mut() {
        job.locked = false;
    }
}

/// The catalog is complete: queue every media file for download, except
/// the ones the destination already holds at the same size.
extern "C" fn device_ready(_this: *mut AnyObject, _sel: Sel, device: *mut AnyObject) {
    let tethering = {
        let shared = lock();
        shared.tethered.as_ref().is_some_and(|job| {
            job.device.0 == device
                && job.finished.is_none()
                && matches!(job.phase, TetheredPhase::Opening)
        })
    };
    if tethering {
        let files: *mut AnyObject = unsafe { msg_send![device, mediaFiles] };
        let count: usize = if files.is_null() {
            0
        } else {
            unsafe { msg_send![files, count] }
        };
        let mut baseline = HashSet::new();
        for index in 0..count {
            let file: *mut AnyObject = unsafe { msg_send![files, objectAtIndex: index] };
            if !file.is_null() {
                baseline.insert(file as usize);
            }
        }
        if !unsafe { can_take_picture(device) } {
            conclude_tethered(Err(t("tethered.unsupported").into()));
            return;
        }
        let started = {
            let mut shared = lock();
            let Some(job) = shared.tethered.as_mut() else {
                return;
            };
            if job.device.0 != device || job.finished.is_some() {
                return;
            }
            job.baseline = baseline;
            #[cfg(target_os = "macos")]
            {
                job.phase = TetheredPhase::Capturing;
            }
            #[cfg(target_os = "ios")]
            {
                job.phase = TetheredPhase::Probing;
            }
            true
        };
        if started {
            #[cfg(target_os = "macos")]
            unsafe {
                let _: () = msg_send![device, requestTakePicture];
            }
            #[cfg(target_os = "ios")]
            send_ptp(device, 0x1001, &[]);
        }
        return;
    }
    let (dest, generation, local) = {
        let shared = lock();
        match shared.job.as_ref() {
            Some(job)
                if job.device.0 == device && job.finished.is_none() && job.total.is_none() =>
            {
                (
                    job.dest.path().to_path_buf(),
                    job.generation,
                    job.dest.is_local(),
                )
            }
            _ => return,
        }
    };
    let delegate = delegate();
    let mut queued = 0usize;
    let mut already = 0usize;
    unsafe {
        let files: *mut AnyObject = msg_send![device, mediaFiles];
        let count: usize = if files.is_null() {
            0
        } else {
            msg_send![files, count]
        };
        let dest_ns = NSString::from_str(&dest.to_string_lossy());
        let url_class = AnyClass::get(c"NSURL").expect("NSURL exists");
        let dict_class = AnyClass::get(c"NSMutableDictionary").expect("NSMutableDictionary exists");
        for i in 0..count {
            let file: *mut AnyObject = msg_send![files, objectAtIndex: i];
            if file.is_null() {
                continue;
            }
            let name_obj: *mut AnyObject = msg_send![file, name];
            let Some(name) = ns_string(name_obj) else {
                continue;
            };
            let size: i64 = msg_send![file, fileSize];
            let existing = local
                && std::fs::metadata(dest.join(&name))
                    .map(|m| m.len() as i64 == size)
                    .unwrap_or(false);
            if existing {
                already += 1;
                continue;
            }
            let options: *mut AnyObject = msg_send![dict_class, dictionary];
            let url: *mut AnyObject =
                msg_send![url_class, fileURLWithPath: &*dest_ns, isDirectory: Bool::YES];
            let _: () = msg_send![options, setObject: url, forKey: ICDownloadsDirectoryURL];
            let _: () = msg_send![
                device,
                requestDownloadFile: file,
                options: options,
                downloadDelegate: delegate,
                didDownloadSelector: sel!(didDownloadFile:error:options:contextInfo:),
                contextInfo: generation as *mut c_void
            ];
            queued += 1;
        }
        log::info!(
            "gallery: camera catalog ready — {count} files, {queued} to download, {already} already here"
        );
    }
    if let Some(job) = lock().job.as_mut() {
        job.total = Some(queued);
        job.dest.expect(queued);
    }
    if queued == 0 {
        conclude(Ok(()));
    }
}

/// One download settled. Queue filesystem work without blocking the main loop.
/// The poller counts a download only after the background filter has finished.
extern "C" fn did_download(
    _this: *mut AnyObject,
    _sel: Sel,
    file: *mut AnyObject,
    error: *mut AnyObject,
    options: *mut AnyObject,
    context: *mut c_void,
) {
    let tethering = {
        let shared = lock();
        shared
            .tethered
            .as_ref()
            .is_some_and(|job| job.generation == context as usize && job.finished.is_none())
    };
    if tethering {
        let failed = if error.is_null() {
            let shared = lock();
            let job = shared.tethered.as_ref().unwrap();
            let name = unsafe {
                let name: *mut AnyObject = msg_send![file, name];
                ns_string(name).unwrap_or_default()
            };
            let expected: u64 = unsafe { msg_send![file, fileSize] };
            let valid = schist_tethered::download_path(job.staging.path(), file as usize, &name)
                .ok()
                .and_then(|path| std::fs::symlink_metadata(path).ok())
                .is_some_and(|meta| {
                    meta.is_file() && meta.len() > 0 && (expected == 0 || meta.len() == expected)
                });
            (!valid).then(|| t("tethered.no_download").to_string())
        } else {
            Some(unsafe { error_string(error) })
        };
        let mut shared = lock();
        let Some(job) = shared.tethered.as_mut() else {
            return;
        };
        if job.finished.is_some() {
            return;
        }
        job.done += 1;
        match failed {
            Some(error) => {
                log::warn!("gallery: a tethered download failed: {error}");
                drop(shared);
                // A failed native transfer may have left nonempty partial data.
                // Never publish the staging directory after such a callback.
                conclude_tethered(Err(error));
            }
            None => job.succeeded += 1,
        }
        return;
    }
    let (dest, sender) = {
        let shared = lock();
        let Some(job) = shared.job.as_ref() else {
            return;
        };
        if job.generation != context as usize || job.finished.is_some() {
            return;
        }
        (job.dest.path().to_path_buf(), job.processor.sender.clone())
    };
    let path = unsafe {
        let saved: *mut AnyObject = if options.is_null() {
            std::ptr::null_mut()
        } else {
            msg_send![options, objectForKey: ICSavedFilename]
        };
        let name = ns_string(saved).or_else(|| {
            if file.is_null() {
                None
            } else {
                let name_obj: *mut AnyObject = msg_send![file, name];
                ns_string(name_obj)
            }
        });
        name.map(|n| dest.join(n))
    };
    let failed = if error.is_null() {
        None
    } else {
        Some(unsafe { error_string(error) })
    };
    if let Some(error) = failed {
        log::warn!("gallery: a download failed: {error}");
        let _ = sender.send(None);
    } else {
        let _ = sender.send(path);
    }
}

#[cfg(target_os = "ios")]
fn send_ptp(device: *mut AnyObject, operation: u16, parameters: &[u32]) {
    let generation = {
        let shared = lock();
        let Some(job) = shared.tethered.as_ref() else {
            return;
        };
        job.generation
    };
    let packet = schist_tethered::ptp::command(
        operation,
        (generation as u32)
            .wrapping_mul(2)
            .wrapping_add(u32::from(operation == 0x100e)),
        parameters,
    );
    unsafe {
        let class = AnyClass::get(c"NSData").expect("NSData exists");
        let data: *mut AnyObject =
            msg_send![class, dataWithBytes: packet.as_ptr().cast::<c_void>(), length: packet.len()];
        let _: () = msg_send![device,
            requestSendPTPCommand: data,
            outData: std::ptr::null::<AnyObject>(),
            sendCommandDelegate: delegate(),
            didSendCommand: sel!(didSendPTPCommand:inData:response:error:contextInfo:),
            contextInfo: generation as *mut c_void];
    }
}

#[cfg(target_os = "ios")]
unsafe fn data_bytes<'a>(data: *mut AnyObject) -> &'a [u8] {
    if data.is_null() {
        return &[];
    }
    let len: usize = msg_send![data, length];
    if len == 0 {
        return &[];
    }
    let bytes: *const u8 = msg_send![data, bytes];
    std::slice::from_raw_parts(bytes, len)
}

#[cfg(target_os = "ios")]
extern "C" fn did_send_ptp(
    _this: *mut AnyObject,
    _sel: Sel,
    _command: *mut AnyObject,
    data: *mut AnyObject,
    response: *mut AnyObject,
    error: *mut AnyObject,
    context: *mut c_void,
) {
    let device = {
        let shared = lock();
        let Some(job) = shared.tethered.as_ref() else {
            return;
        };
        if job.generation != context as usize || job.finished.is_some() {
            return;
        }
        job.device.0
    };
    if !error.is_null() {
        conclude_tethered(Err(unsafe { error_string(error) }));
        return;
    }
    if !schist_tethered::ptp::response_ok(unsafe { data_bytes(response) }) {
        conclude_tethered(Err(t("tethered.unsupported").into()));
        return;
    }
    let probing = lock()
        .tethered
        .as_ref()
        .is_some_and(|job| matches!(job.phase, TetheredPhase::Probing));
    if probing {
        match schist_tethered::ptp::supports_capture(unsafe { data_bytes(data) }) {
            Ok(true) => {
                if let Some(job) = lock().tethered.as_mut() {
                    job.phase = TetheredPhase::Capturing;
                }
                send_ptp(device, 0x100e, &[0, 0]);
            }
            _ => conclude_tethered(Err(t("tethered.unsupported").into())),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn delegate_selector_signatures_register_without_panicking() {
        // objc2 verifies selector arity when ClassBuilder registers each method.
        assert!(!super::delegate().is_null());
    }
}
