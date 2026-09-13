//! The camera-roll backup's iOS half: the Photos library.
//!
//! An iOS app cannot read the camera roll as files. The backup's source
//! is the library itself, or one of its albums, through the Photos
//! framework: assets are listed by identifier, and each one's original
//! bytes are written out to a staging folder just long enough to be
//! uploaded (`PHAssetResourceManager`, with iCloud fetches allowed for
//! an "optimise storage" library). Reading the library at all needs the
//! user's permission, asked for from the prompt.
//!
//! Two more things only iOS has. A change observer, so a photo taken
//! while Schist is open starts a run; and the background task through
//! which iOS, at a time of its choosing, launches the app to catch up
//! (`BGTaskScheduler`, with the identifier `Info.plist` permits). As in
//! `library_photos.rs`, there is no Objective-C source: the one class is
//! built at runtime.

use super::camera_sync::{Candidate, Locate, LIBRARY_ALBUM};
use anyhow::{anyhow, Result};
use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::NSString;
use schist_i18n::t;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[link(name = "Photos", kind = "framework")]
unsafe extern "C" {}
#[link(name = "BackgroundTasks", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: *const AnyObject;
    static kSecClassInternetPassword: *const AnyObject;
    static kSecAttrServer: *const AnyObject;
    static kSecAttrAccessible: *const AnyObject;
    static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: *const AnyObject;
    static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: *const AnyObject;
    fn SecItemUpdate(query: *const AnyObject, attributes: *const AnyObject) -> i32;
}

/// gpui's credential store defaults to access only while unlocked. A cold
/// background launch must also be able to read the cloud token after the
/// phone locks. Change only Schist Cloud's item, without reading its secret,
/// and restore access only while unlocked when backup is disabled.
pub(crate) fn background_credentials(enabled: bool) -> std::result::Result<(), i32> {
    autoreleasepool(|_| unsafe {
        let query: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
        let _: () = msg_send![query, setObject: kSecClassInternetPassword, forKey: kSecClass];
        let _: () =
            msg_send![query, setObject: &*ns(super::cloud::CREDENTIAL_KEY), forKey: kSecAttrServer];
        let attributes: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
        let accessibility = if enabled {
            kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        } else {
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        };
        let _: () = msg_send![attributes, setObject: accessibility, forKey: kSecAttrAccessible];
        // NSDictionary is toll-free bridged to CFDictionary. A missing item
        // is normal before sign-in and after sign-out; persistence retries
        // this update after creating the item.
        match SecItemUpdate(query, attributes) {
            0 | -25300 => Ok(()),
            status => Err(status),
        }
    })
}

/// The task identifier, as `BGTaskSchedulerPermittedIdentifiers` in
/// `packaging/ios/Info.plist` lists it.
const TASK_ID: &str = "com.infrawrench.schist.camera-sync";
/// `PHAuthorizationStatusAuthorized` / `Limited`.
const AUTHORIZED: isize = 3;
const LIMITED: isize = 4;
/// `PHAccessLevelReadWrite`: what listing and reading the library needs.
const READ_WRITE: isize = 2;
/// `PHAssetMediaTypeImage`.
/// `PHAssetCollectionTypeSmartAlbum` / `Album`, and the user library's
/// subtype; `PHAssetCollectionSubtypeAny` is `NSIntegerMax`.
const TYPE_SMART_ALBUM: isize = 2;
const TYPE_ALBUM: isize = 1;
const SUBTYPE_USER_LIBRARY: isize = 209;
const SUBTYPE_ANY: isize = isize::MAX;
/// `PHAssetResourceTypePhoto`.
const RESOURCE_PHOTO: isize = 1;
/// How long one asset's bytes are waited for: an iCloud original can
/// take a while on a slow link.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(180);
// Read UIKit's sentinel instead of assuming a value for it.
#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    static UIBackgroundTaskInvalid: usize;
}

/// An ObjC pointer that crosses a mutex.
struct ObjPtr(*mut AnyObject);
unsafe impl Send for ObjPtr {}

/// The photo library changed since the last look.
static CHANGED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Background {
    /// The `BGTask` iOS handed over, until it is completed.
    task: Option<ObjPtr>,
    /// Set when the task's expiration handler fires; the run it drives
    /// reads it as its cancel flag.
    cancel: Option<Arc<AtomicBool>>,
    /// A launch the workspace has not yet picked up.
    requested: bool,
    /// The grace period asked for on going to the background, so a run
    /// in flight can finish its chunk.
    grace: Option<usize>,
    enabled: bool,
    active_cancel: Option<Arc<AtomicBool>>,
    registered: bool,
    observing: bool,
    observer: Option<ObjPtr>,
}
static BACKGROUND: Mutex<Background> = Mutex::new(Background {
    task: None,
    cancel: None,
    requested: false,
    grace: None,
    enabled: false,
    active_cancel: None,
    registered: false,
    observing: false,
    observer: None,
});
fn background() -> std::sync::MutexGuard<'static, Background> {
    BACKGROUND.lock().unwrap_or_else(|e| e.into_inner())
}

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}
unsafe fn string_of(object: *mut AnyObject) -> String {
    if object.is_null() {
        return String::new();
    }
    let text: Option<Retained<NSString>> = unsafe { msg_send![object, description] };
    text.map(|t| t.to_string()).unwrap_or_default()
}

/// Whether the library can be read: full access, or the limited
/// selection the user made.
pub(crate) fn limited() -> bool {
    status() == LIMITED
}

pub(crate) fn authorized() -> bool {
    matches!(status(), AUTHORIZED | LIMITED)
}
fn status() -> isize {
    unsafe {
        let Some(class) = AnyClass::get(c"PHPhotoLibrary") else {
            return 0;
        };
        msg_send![class, authorizationStatusForAccessLevel: READ_WRITE]
    }
}

/// Ask for access; `done` hears the answer on the library's own queue.
pub(crate) fn request_authorization(
    done: impl FnOnce(std::result::Result<(), String>) + Send + 'static,
) {
    if authorized() {
        done(Ok(()));
        return;
    }
    let Some(class) = AnyClass::get(c"PHPhotoLibrary") else {
        done(Err(t("library.photos.unavailable").into()));
        return;
    };
    let done: Mutex<Option<Box<dyn FnOnce(std::result::Result<(), String>) + Send>>> =
        Mutex::new(Some(Box::new(done)));
    let handler = RcBlock::new(move |status: isize| {
        if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
            done(match status {
                AUTHORIZED | LIMITED => Ok(()),
                _ => Err(t("cloud.sync.permission_denied").into()),
            });
        }
    });
    unsafe {
        let _: () =
            msg_send![class, requestAuthorizationForAccessLevel: READ_WRITE, handler: &*handler];
    }
}

/// The albums the prompt offers: identifier, title, and how many items
/// the library thinks are in it. Empty until the library can be read.
pub(crate) fn albums() -> Vec<(String, String, u64)> {
    if !authorized() {
        return Vec::new();
    }
    autoreleasepool(|_| unsafe {
        let mut out = Vec::new();
        let Some(collection_class) = AnyClass::get(c"PHAssetCollection") else {
            return out;
        };
        for (kind, subtype) in [
            (TYPE_SMART_ALBUM, SUBTYPE_USER_LIBRARY),
            (TYPE_ALBUM, SUBTYPE_ANY),
        ] {
            let result: *mut AnyObject = msg_send![
                collection_class,
                fetchAssetCollectionsWithType: kind,
                subtype: subtype,
                options: std::ptr::null::<AnyObject>()
            ];
            if result.is_null() {
                continue;
            }
            let count: usize = msg_send![result, count];
            for i in 0..count {
                let album: *mut AnyObject = msg_send![result, objectAtIndex: i];
                let id: Option<Retained<NSString>> = msg_send![album, localIdentifier];
                let title: Option<Retained<NSString>> = msg_send![album, localizedTitle];
                let estimate: usize = msg_send![album, estimatedAssetCount];
                let (Some(id), Some(title)) = (id, title) else {
                    continue;
                };
                // NSNotFound when the library has not counted it.
                let count = if estimate == isize::MAX as usize {
                    0
                } else {
                    estimate as u64
                };
                out.push((id.to_string(), title.to_string(), count));
            }
        }
        out
    })
}

/// A fetch of every image asset in the library, or in one album.
unsafe fn fetch_images(album: &str) -> Result<*mut AnyObject> {
    unsafe {
        let options: *mut AnyObject = msg_send![class!(PHFetchOptions), new];
        let format = ns("mediaType == 1");
        let predicate: *mut AnyObject =
            msg_send![class!(NSPredicate), predicateWithFormat: &*format];
        let _: () = msg_send![options, setPredicate: predicate];
        let assets: *mut AnyObject = if album == LIBRARY_ALBUM {
            msg_send![class!(PHAsset), fetchAssetsWithOptions: options]
        } else {
            let ids: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*ns(album)];
            let collections: *mut AnyObject = msg_send![
                class!(PHAssetCollection),
                fetchAssetCollectionsWithLocalIdentifiers: ids,
                options: std::ptr::null::<AnyObject>()
            ];
            let collection: *mut AnyObject = msg_send![collections, firstObject];
            if collection.is_null() {
                let _: () = msg_send![options, release];
                return Err(anyhow!(t("cloud.sync.album_gone")));
            }
            msg_send![class!(PHAsset), fetchAssetsInAssetCollection: collection, options: options]
        };
        let _: () = msg_send![options, release];
        Ok(assets)
    }
}

/// Every image in the album, by identifier. The name is filled in when
/// the asset is written out: asking the library for each original
/// filename up front would cost a resource fetch per photo, and the
/// ledger only needs the identifier and the modification date.
pub(crate) fn list_assets(album: &str) -> Result<Vec<Candidate>> {
    if !authorized() {
        return Err(anyhow!(t("cloud.sync.permission_denied")));
    }
    autoreleasepool(|_| unsafe {
        let assets = fetch_images(album)?;
        let count: usize = msg_send![assets, count];
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let asset: *mut AnyObject = msg_send![assets, objectAtIndex: i];
            let id: Option<Retained<NSString>> = msg_send![asset, localIdentifier];
            let Some(id) = id else { continue };
            let modified: *mut AnyObject = msg_send![asset, modificationDate];
            let stamp: f64 = if modified.is_null() {
                0.0
            } else {
                msg_send![modified, timeIntervalSince1970]
            };
            let id = id.to_string();
            out.push(Candidate {
                identity: format!("{id}|{}", stamp as i64),
                name: id.clone(),
                relative: None,
                locate: Locate::Asset(id),
            });
        }
        Ok(out)
    })
}

/// Write the asset's original bytes under `staging`, in a folder of
/// their own so two "IMG_0001.HEIC" never meet, named as the camera
/// named them. Waits for the write; iCloud fetches are allowed.
pub(crate) fn export_original(
    id: &str,
    _name: &str,
    staging: &Path,
    cancel: &Arc<AtomicBool>,
) -> Result<PathBuf> {
    autoreleasepool(|_| unsafe {
        let ids: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: &*ns(id)];
        let fetched: *mut AnyObject = msg_send![
            class!(PHAsset),
            fetchAssetsWithLocalIdentifiers: ids,
            options: std::ptr::null::<AnyObject>()
        ];
        let asset: *mut AnyObject = msg_send![fetched, firstObject];
        anyhow::ensure!(!asset.is_null(), t("cloud.sync.asset_gone"));
        let resources: *mut AnyObject =
            msg_send![class!(PHAssetResource), assetResourcesForAsset: asset];
        let count: usize = msg_send![resources, count];
        let mut chosen: *mut AnyObject = std::ptr::null_mut();
        for i in 0..count {
            let resource: *mut AnyObject = msg_send![resources, objectAtIndex: i];
            let kind: isize = msg_send![resource, type];
            if kind == RESOURCE_PHOTO {
                chosen = resource;
                break;
            }
        }
        anyhow::ensure!(!chosen.is_null(), t("cloud.sync.asset_gone"));
        let filename: Option<Retained<NSString>> = msg_send![chosen, originalFilename];
        let filename = filename
            .map(|f| f.to_string())
            .filter(|f| !f.is_empty() && !f.contains('/') && f != "." && f != "..")
            .unwrap_or_else(|| "photo".into());
        let folder = staging.join(schist_cloud::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&folder)?;
        let target = folder.join(&filename);
        let _ = std::fs::remove_file(&target);
        let url: *mut AnyObject =
            msg_send![class!(NSURL), fileURLWithPath: &*ns(&target.to_string_lossy())];
        let options: *mut AnyObject = msg_send![class!(PHAssetResourceRequestOptions), new];
        let _: () = msg_send![options, setNetworkAccessAllowed: Bool::YES];
        let manager: *mut AnyObject = msg_send![class!(PHAssetResourceManager), defaultManager];
        let (tx, rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
        let cleanup = folder.clone();
        let handler = RcBlock::new(move |error: *mut AnyObject| {
            let result = if error.is_null() {
                Ok(())
            } else {
                let description: Option<Retained<NSString>> =
                    msg_send![error, localizedDescription];
                Err(description
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| string_of(error)))
            };
            if tx.send(result).is_err() {
                // A cancelled/timed-out export can complete after its caller
                // leaves. Its unique directory is safe to remove here.
                let _ = std::fs::remove_dir_all(&cleanup);
            }
        });
        let _: () = msg_send![
            manager,
            writeDataForAssetResource: chosen,
            toFile: url,
            options: options,
            completionHandler: &*handler
        ];
        let _: () = msg_send![options, release];
        let deadline = std::time::Instant::now() + EXPORT_TIMEOUT;
        loop {
            if cancel.load(Ordering::Relaxed) || std::time::Instant::now() >= deadline {
                // Drop the receiver before checking for a completion that raced
                // cancellation; either this cleanup or the callback removes it.
                drop(rx);
                let _ = std::fs::remove_dir_all(&folder);
                return Err(anyhow!(if cancel.load(Ordering::Relaxed) {
                    t("cloud.upload.cancelled")
                } else {
                    t("cloud.sync.export_timed_out")
                }));
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(())) => return Ok(target),
                Ok(Err(error)) => {
                    let _ = std::fs::remove_dir_all(&folder);
                    return Err(anyhow!(error));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => return Err(error.into()),
            }
        }
    })
}

// ---------------------------------------------------------------------
// The observer class: library changes and the app's background moves.
// ---------------------------------------------------------------------

fn observer_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let superclass = AnyClass::get(c"NSObject").expect("NSObject");
        let mut builder =
            ClassBuilder::new(c"SchistCameraSyncObserver", superclass).expect("class name free");
        unsafe {
            builder.add_method(
                sel!(photoLibraryDidChange:),
                photo_library_did_change as unsafe extern "C-unwind" fn(_, _, _),
            );
            builder.add_method(
                sel!(appDidEnterBackground:),
                app_did_enter_background as unsafe extern "C-unwind" fn(_, _, _),
            );
            builder.add_method(
                sel!(appWillEnterForeground:),
                app_will_enter_foreground as unsafe extern "C-unwind" fn(_, _, _),
            );
        }
        if let Some(protocol) = AnyProtocol::get(c"PHPhotoLibraryChangeObserver") {
            builder.add_protocol(protocol);
        }
        builder.register()
    })
}
unsafe extern "C-unwind" fn photo_library_did_change(
    _this: *mut AnyObject,
    _sel: Sel,
    _change: *mut AnyObject,
) {
    CHANGED.store(true, Ordering::Relaxed);
}
/// The observer object, made once.
fn observer() -> *mut AnyObject {
    let mut state = background();
    if let Some(ObjPtr(observer)) = state.observer {
        return observer;
    }
    let observer: *mut AnyObject = unsafe { msg_send![observer_class(), new] };
    state.observer = Some(ObjPtr(observer));
    observer
}

/// Watch the library, so a new photo starts a run. Idempotent.
pub(crate) fn observe_library() {
    if !authorized() || background().observing {
        return;
    }
    unsafe {
        let library: *mut AnyObject = msg_send![class!(PHPhotoLibrary), sharedPhotoLibrary];
        if library.is_null() {
            return;
        }
        let _: () = msg_send![library, registerChangeObserver: observer()];
    }
    background().observing = true;
}
/// Whether the library changed since the last call.
pub(crate) fn take_change() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

// ---------------------------------------------------------------------
// Background execution.
// ---------------------------------------------------------------------

/// Register the background task's handler and the app-state observers.
/// Must run before the app finishes launching — it is called from
/// `main`, before gpui starts UIKit.
pub(crate) fn register_background_task() {
    if background().registered {
        return;
    }
    unsafe {
        let Some(scheduler_class) = AnyClass::get(c"BGTaskScheduler") else {
            log::warn!("camera sync: BackgroundTasks unavailable; no background runs");
            return;
        };
        let scheduler: *mut AnyObject = msg_send![scheduler_class, sharedScheduler];
        let handler = RcBlock::new(move |task: *mut AnyObject| {
            // retain returns id; a void result fails objc2's debug signature check.
            let _: *mut AnyObject = msg_send![task, retain];
            let cancel = background()
                .active_cancel
                .clone()
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
            let expiring = cancel.clone();
            let expiration = RcBlock::new(move || {
                expiring.store(true, Ordering::Relaxed);
                finish_background(false);
            });
            let _: () = msg_send![task, setExpirationHandler: &*expiration];
            let mut state = background();
            if let Some(ObjPtr(previous)) = state.task.take() {
                let _: () = msg_send![previous, setTaskCompletedWithSuccess: Bool::NO];
                let _: () = msg_send![previous, release];
            }
            state.task = Some(ObjPtr(task));
            state.cancel = Some(cancel);
            state.requested = true;
        });
        let registered: Bool = msg_send![
            scheduler,
            registerForTaskWithIdentifier: &*ns(TASK_ID),
            usingQueue: std::ptr::null::<AnyObject>(),
            launchHandler: &*handler
        ];
        if !registered.as_bool() {
            log::warn!("camera sync: could not register the background task");
            return;
        }
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        let observer = observer();
        for (name, selector) in [
            (
                "UIApplicationDidEnterBackgroundNotification",
                sel!(appDidEnterBackground:),
            ),
            (
                "UIApplicationWillEnterForegroundNotification",
                sel!(appWillEnterForeground:),
            ),
        ] {
            let _: () = msg_send![
                center,
                addObserver: observer,
                selector: selector,
                name: &*ns(name),
                object: std::ptr::null::<AnyObject>()
            ];
        }
    }
    background().registered = true;
}
/// A launch iOS made for the backup, if one is waiting: the flag its
/// expiration sets, for the run to read as its cancel.
pub(crate) fn take_background_request() -> Option<Arc<AtomicBool>> {
    let mut state = background();
    if !state.requested {
        return None;
    }
    state.requested = false;
    state.cancel.clone()
}
/// The run the task asked for is over: tell iOS, and ask for the next.
pub(crate) fn finish_background(success: bool) {
    let task = background().task.take();
    if let Some(ObjPtr(task)) = task {
        unsafe {
            let _: () = msg_send![task, setTaskCompletedWithSuccess: Bool::new(success)];
            let _: () = msg_send![task, release];
        }
    }
    {
        let mut state = background();
        state.cancel = None;
        state.requested = false;
    }
    submit_background_request();
}
/// Ask iOS for a processing launch when it suits: on a network, power
/// or not.
fn submit_background_request() {
    if !background().registered || !background().enabled {
        return;
    }
    unsafe {
        let request: *mut AnyObject = msg_send![class!(BGProcessingTaskRequest), alloc];
        let request: *mut AnyObject = msg_send![request, initWithIdentifier: &*ns(TASK_ID)];
        let date: *mut AnyObject =
            msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 900.0f64];
        let _: () = msg_send![request, setEarliestBeginDate: date];
        let _: () = msg_send![request, setRequiresNetworkConnectivity: Bool::YES];
        let _: () = msg_send![request, setRequiresExternalPower: Bool::NO];
        let scheduler: *mut AnyObject = msg_send![class!(BGTaskScheduler), sharedScheduler];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: Bool = msg_send![scheduler, submitTaskRequest: request, error: &mut error];
        if !ok.as_bool() {
            log::warn!(
                "camera sync: background request refused: {}",
                string_of(error)
            );
        }
        let _: () = msg_send![request, release];
    }
}
unsafe extern "C-unwind" fn app_did_enter_background(
    _this: *mut AnyObject,
    _sel: Sel,
    _note: *mut AnyObject,
) {
    if !background().enabled {
        return;
    }
    if background().active_cancel.is_some() {
        unsafe {
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let expiration = RcBlock::new(move || {
                if let Some(cancel) = background().active_cancel.clone() {
                    cancel.store(true, Ordering::Relaxed);
                }
                end_grace();
            });
            let grace: usize =
                msg_send![app, beginBackgroundTaskWithExpirationHandler: &*expiration];
            end_grace();
            if grace != UIBackgroundTaskInvalid {
                background().grace = Some(grace);
            }
        }
    }
    submit_background_request();
}
unsafe extern "C-unwind" fn app_will_enter_foreground(
    _this: *mut AnyObject,
    _sel: Sel,
    _note: *mut AnyObject,
) {
    end_grace();
    CHANGED.store(true, Ordering::Relaxed);
}
fn end_grace() {
    let grace = background().grace.take();
    if let Some(grace) = grace {
        unsafe {
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let _: () = msg_send![app, endBackgroundTask: grace];
        }
    }
}
pub(crate) fn set_enabled(enabled: bool) {
    background().enabled = enabled;
    if enabled {
        observe_library();
        submit_background_request();
    } else {
        if let Some(cancel) = background().active_cancel.clone() {
            cancel.store(true, Ordering::Relaxed);
        }
        run_finished(false);
        if background().registered {
            unsafe {
                let scheduler: *mut AnyObject = msg_send![class!(BGTaskScheduler), sharedScheduler];
                let _: () = msg_send![scheduler, cancelTaskRequestWithIdentifier: &*ns(TASK_ID)];
            }
        }
    }
}
pub(crate) fn run_started(cancel: Arc<AtomicBool>) {
    background().active_cancel = Some(cancel);
}
pub(crate) fn run_finished(success: bool) {
    background().active_cancel = None;
    end_grace();
    if background().task.is_some() {
        finish_background(success);
    }
}
