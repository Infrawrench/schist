//! The photo library on iOS and iPadOS, through the system picker.
//!
//! An iOS app cannot read the camera roll as files, and Schist's gallery
//! is built on files: paths, sidecars beside them, thumbnails decoded
//! from them. `PHPickerViewController` bridges the two without a single
//! permission prompt: the user picks photos in the system's own browser,
//! the picker hands over each one's original file (HEIC stays HEIC), and
//! the copy lands in a folder the gallery watches like any other.
//!
//! Like the macOS ImageCaptureCore bridge, there is no Objective-C
//! source: the one delegate class is built with the runtime's class
//! builder. The picker calls it on the main thread; the per-photo file
//! handlers run on the item providers' own queues and touch only the
//! filesystem and the counters behind [`lock`].

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::NSString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// The picker's classes only exist once the framework is linked in.
#[link(name = "PhotosUI", kind = "framework")]
unsafe extern "C" {}
#[link(name = "Photos", kind = "framework")]
unsafe extern "C" {}

/// An ObjC pointer that crosses the mutex; only dereferenced on the
/// main thread.
struct ObjPtr(*mut AnyObject);
unsafe impl Send for ObjPtr {}

/// One import in flight: what was picked, and how much has landed.
struct Job {
    dest: PathBuf,
    /// Photos picked; `None` until the picker has returned.
    total: Option<usize>,
    done: usize,
    copied: usize,
    failed: usize,
    /// Set once the picker was dismissed with nothing (or everything
    /// finished) so the poller can wrap up.
    finished: bool,
}

#[derive(Default)]
struct Shared {
    job: Option<Job>,
    /// The delegate, kept alive for the picker (which only holds it
    /// weakly) until the job is collected.
    delegate: Option<ObjPtr>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    job: None,
    delegate: None,
});

fn lock() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) struct ImportStatus {
    pub done: usize,
    pub total: Option<usize>,
    /// `Some` once everything settled: `(copied, failed)`.
    pub finished: Option<(usize, usize)>,
}

fn delegate_class() -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let superclass = AnyClass::get(c"NSObject").expect("NSObject");
        let mut builder =
            ClassBuilder::new(c"SchistPhotoPickerDelegate", superclass).expect("class name free");
        unsafe {
            builder.add_method(
                sel!(picker:didFinishPicking:),
                did_finish_picking as unsafe extern "C-unwind" fn(_, _, _, _),
            );
        }
        if let Some(protocol) = AnyProtocol::get(c"PHPickerViewControllerDelegate") {
            builder.add_protocol(protocol);
        }
        builder.register()
    })
}

/// The view controller to present from: the key window's root, or
/// whatever it has presented on top.
unsafe fn presenting_controller() -> Option<*mut AnyObject> {
    unsafe {
        let app_class = AnyClass::get(c"UIApplication")?;
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        let scenes: *mut AnyObject = msg_send![app, connectedScenes];
        let scenes: *mut AnyObject = msg_send![scenes, allObjects];
        let count: usize = msg_send![scenes, count];
        let window_scene = AnyClass::get(c"UIWindowScene")?;
        let mut fallback: Option<*mut AnyObject> = None;
        for i in 0..count {
            let scene: *mut AnyObject = msg_send![scenes, objectAtIndex: i];
            let is_window_scene: bool = msg_send![scene, isKindOfClass: window_scene];
            if !is_window_scene {
                continue;
            }
            let windows: *mut AnyObject = msg_send![scene, windows];
            let n: usize = msg_send![windows, count];
            for j in 0..n {
                let window: *mut AnyObject = msg_send![windows, objectAtIndex: j];
                let root: *mut AnyObject = msg_send![window, rootViewController];
                if root.is_null() {
                    continue;
                }
                let is_key: bool = msg_send![window, isKeyWindow];
                if is_key {
                    return Some(topmost(root));
                }
                fallback.get_or_insert(root);
            }
        }
        fallback.map(|root| topmost(root))
    }
}

unsafe fn topmost(mut controller: *mut AnyObject) -> *mut AnyObject {
    unsafe {
        loop {
            let presented: *mut AnyObject = msg_send![controller, presentedViewController];
            if presented.is_null() {
                return controller;
            }
            controller = presented;
        }
    }
}

/// Show the picker; what it picks is copied into `dest`. Poll
/// [`poll_import`] for progress.
pub(super) fn begin_import(dest: PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    {
        let mut shared = lock();
        if shared.job.is_some() {
            return Err("an import is already running".into());
        }
        shared.job = Some(Job {
            dest,
            total: None,
            done: 0,
            copied: 0,
            failed: 0,
            finished: false,
        });
    }
    let result = unsafe { present_picker() };
    if result.is_err() {
        let mut shared = lock();
        shared.job = None;
        shared.delegate = None;
    }
    result
}

unsafe fn present_picker() -> Result<(), String> {
    unsafe {
        let config_class = AnyClass::get(c"PHPickerConfiguration")
            .ok_or("the photo picker needs iOS 14 or later")?;
        let picker_class =
            AnyClass::get(c"PHPickerViewController").ok_or("the photo picker is unavailable")?;
        let filter_class =
            AnyClass::get(c"PHPickerFilter").ok_or("the photo picker is unavailable")?;
        let controller = presenting_controller().ok_or("no window to present the picker from")?;

        let config: *mut AnyObject = msg_send![config_class, new];
        // 0 is unlimited.
        let _: () = msg_send![config, setSelectionLimit: 0isize];
        let images: *mut AnyObject = msg_send![filter_class, imagesFilter];
        let _: () = msg_send![config, setFilter: images];
        // PHPickerConfigurationAssetRepresentationModeCurrent: the original
        // bytes as they are, HEIC included, with the user's edits applied.
        let _: () = msg_send![config, setPreferredAssetRepresentationMode: 1isize];

        let picker: *mut AnyObject = msg_send![picker_class, alloc];
        let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];
        let _: () = msg_send![config, release];
        let delegate: *mut AnyObject = msg_send![delegate_class(), new];
        let _: () = msg_send![picker, setDelegate: delegate];
        lock().delegate = Some(ObjPtr(delegate));
        let _: () = msg_send![controller, presentViewController: picker, animated: true, completion: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![picker, release];
        Ok(())
    }
}

/// `PHPickerViewControllerDelegate`: the user is done choosing. Each
/// result's original file is requested and copied in its completion
/// handler; the picker is dismissed at once.
unsafe extern "C-unwind" fn did_finish_picking(
    _this: *mut AnyObject,
    _sel: Sel,
    picker: *mut AnyObject,
    results: *mut AnyObject,
) {
    unsafe {
        let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: std::ptr::null::<AnyObject>()];
        let count: usize = msg_send![results, count];
        {
            let mut shared = lock();
            let Some(job) = shared.job.as_mut() else {
                return;
            };
            job.total = Some(count);
            if count == 0 {
                job.finished = true;
                return;
            }
        }
        let image_type = NSString::from_str("public.image");
        for i in 0..count {
            let result: *mut AnyObject = msg_send![results, objectAtIndex: i];
            let provider: *mut AnyObject = msg_send![result, itemProvider];
            let has_image: bool =
                msg_send![provider, hasItemConformingToTypeIdentifier: &*image_type];
            if provider.is_null() || !has_image {
                record(None);
                continue;
            }
            // The URL is only valid inside the handler: copy there.
            let handler = RcBlock::new(move |url: *mut AnyObject, _error: *mut AnyObject| {
                let copied = (!url.is_null()).then(|| copy_in(url)).flatten();
                record(copied);
            });
            // Returns the NSProgress for the load, which nothing here reads.
            let _progress: *mut AnyObject = msg_send![provider, loadFileRepresentationForTypeIdentifier: &*image_type, completionHandler: &*handler];
        }
    }
}

/// Copy the file at `url` into the job's folder under its own name (a
/// counter keeps a second "IMG_0001.HEIC" from overwriting the first).
unsafe fn copy_in(url: *mut AnyObject) -> Option<()> {
    unsafe {
        let path: Option<Retained<NSString>> = msg_send![url, path];
        let path = PathBuf::from(path?.to_string());
        let dest = lock().job.as_ref()?.dest.clone();
        let name = path.file_name()?.to_os_string();
        let mut target = dest.join(&name);
        let stem = path.file_stem()?.to_string_lossy().into_owned();
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
        let mut n = 1;
        while target.exists() {
            n += 1;
            let mut candidate = format!("{stem}-{n}");
            if let Some(ext) = &ext {
                candidate.push('.');
                candidate.push_str(ext);
            }
            target = dest.join(candidate);
        }
        std::fs::copy(&path, &target).ok().map(|_| ())
    }
}

/// One result settled, copied or not.
fn record(copied: Option<()>) {
    let mut shared = lock();
    let Some(job) = shared.job.as_mut() else {
        return;
    };
    job.done += 1;
    if copied.is_some() {
        job.copied += 1;
    } else {
        job.failed += 1;
    }
    if job.total.is_some_and(|total| job.done >= total) {
        job.finished = true;
    }
}

pub(super) fn poll_import() -> Option<ImportStatus> {
    let shared = lock();
    let job = shared.job.as_ref()?;
    Some(ImportStatus {
        done: job.done,
        total: job.total,
        finished: job.finished.then_some((job.copied, job.failed)),
    })
}

/// Forget the finished job, releasing the delegate.
pub(super) fn finish_import() {
    let mut shared = lock();
    shared.job = None;
    if let Some(delegate) = shared.delegate.take() {
        unsafe {
            let _: () = msg_send![delegate.0, release];
        }
    }
}
