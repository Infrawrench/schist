//! Mobile handoff and Android's import mailbox. UI presentation stays on
//! the main thread; media copying and decoding run on background workers.
use super::*;
use schist_i18n::{t, tf};

/// GPUI assigns the root controller before attaching its UIWindow to a scene.
/// UIKit then caches incorrect presentation metrics when the window first
/// becomes visible. Reattach the root immediately after setWindowScene:, before
/// makeKeyAndVisible. Doing this later, at the share action, is too late.
/// Remove this compatibility fix when GPUI initializes the scene before its root.
#[cfg(target_os = "ios")]
pub(crate) fn install_ios_window_scene_fix() {
    use objc2::{
        ffi, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Imp, Sel},
        sel,
    };
    unsafe extern "C-unwind" fn set_scene(this: *mut AnyObject, _: Sel, scene: *mut AnyObject) {
        unsafe {
            let this = &*this;
            let superclass = AnyClass::get(c"UIWindow").unwrap();
            let _: () = msg_send![super(this, superclass), setWindowScene: scene];
            if !scene.is_null() {
                let root: Option<Retained<AnyObject>> = msg_send![this, rootViewController];
                if let Some(root) = root {
                    let _: () =
                        msg_send![this, setRootViewController: std::ptr::null::<AnyObject>()];
                    let _: () = msg_send![this, setRootViewController: &*root];
                }
            }
        }
    }
    if let Some(class) = AnyClass::get(c"GPUIWindow") {
        unsafe {
            // Preserve a future GPUI implementation of this setter.
            ffi::class_addMethod(
                (class as *const AnyClass).cast_mut(),
                sel!(setWindowScene:),
                std::mem::transmute::<
                    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
                    Imp,
                >(set_scene),
                c"v@:@".as_ptr(),
            );
        }
    }
}

impl Workspace {
    pub(crate) fn pause_mobile_video(&mut self, cx: &mut Context<Self>) {
        if let Some(video) = self.library.video.as_mut() {
            if video.playing {
                video.pause();
                cx.notify();
            }
        }
    }
    pub(super) fn share_video(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.library.video.as_ref().map(|v| v.path.clone()) else {
            return;
        };
        self.pause_mobile_video(cx);
        #[cfg(target_os = "ios")]
        let result = share_ios(&path);
        #[cfg(target_os = "android")]
        let result = crate::video::platform::share(&path);
        if let Err(error) = result {
            self.status = tf!("video.editor_failed", error = error).into();
            if let Some(viewer) = self.library.video.as_mut() {
                viewer.set_message(tf!("video.editor_failed", error = error));
            }
            cx.notify();
        }
    }
    #[cfg(target_os = "android")]
    pub(super) fn import_mobile_media(&mut self, cx: &mut Context<Self>) {
        let destination = match self
            .library
            .import_cloud
            .as_ref()
            .map(|target| target.destination(&self.cloud))
            .transpose()
        {
            Ok(destination) => destination,
            Err(error) => {
                self.status = error.to_string().into();
                cx.notify();
                return;
            }
        };
        match crate::video::platform::begin_import(destination.as_ref().map(|d| d.path())) {
            Ok(()) => {
                self.library.media_import_destination = destination;
                self.library.importing = true;
                self.status = t("video.importing").into();
            }
            Err(error) => self.status = tf!("video.import_failed", error = error).into(),
        }
        cx.notify();
    }
    #[cfg(target_os = "android")]
    pub(super) fn watch_media_imports(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut import_failed = 0;
            let mut imported = 0;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let events = cx
                    .background_executor()
                    .spawn(async { crate::video::platform::take_imports() })
                    .await;
                if this
                    .update(cx, |ws, cx| {
                        let events = match events {
                            Ok(events) => events,
                            Err(error) => {
                                log::warn!("media import mailbox: {error:#}");
                                return;
                            }
                        };
                        if events.is_empty() {
                            return;
                        }
                        let mut changed = false;
                        for event in events {
                            if let Some(path) = event.path {
                                if !event.open {
                                    imported += 1;
                                }
                                if event.open || ws.library.media_import_destination.is_none() {
                                    if let Some(folder) = path.parent() {
                                        if !ws.library.folders.iter().any(|f| f == folder) {
                                            ws.library.folders.push(folder.to_path_buf());
                                            ws.library.save();
                                        }
                                    }
                                    changed = true;
                                    if event.open {
                                        ws.load_file(path, cx);
                                    } else {
                                        ws.library.open = true;
                                    }
                                }
                            }
                            if let Some(error) = event.error {
                                if !event.open {
                                    import_failed += 1;
                                }
                                log::warn!("media import/handoff: {error}");
                                ws.status = t("video.import_failed_simple").into();
                                if let Some(video) = ws.library.video.as_mut() {
                                    video.set_message(t("video.import_failed_simple").into());
                                }
                            }
                            if event.done && !event.open {
                                ws.library.importing = false;
                                if let Some(destination) =
                                    ws.library.media_import_destination.take()
                                {
                                    ws.finish_camera_import(
                                        destination,
                                        imported,
                                        0,
                                        import_failed,
                                        None,
                                        cx,
                                    );
                                } else if import_failed == 0 {
                                    ws.status = t("common.ready").into();
                                }
                                import_failed = 0;
                                imported = 0;
                            }
                        }
                        if changed {
                            ws.library_rescan(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(target_os = "ios")]
fn share_ios(path: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, Encode, Encoding};
    use objc2_foundation::NSString;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    unsafe impl Encode for Point {
        const ENCODING: Encoding =
            Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding =
            Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }
    let path = path.canonicalize()?;
    unsafe {
        let controller = super::library_photos::presenting_controller()
            .context(t("library.import.no_window"))?;
        let path = NSString::from_str(path.to_str().context(t("video.invalid_file"))?);
        let url: Retained<AnyObject> =
            msg_send![AnyClass::get(c"NSURL").unwrap(), fileURLWithPath: &*path];
        let items: Retained<AnyObject> =
            msg_send![AnyClass::get(c"NSArray").unwrap(), arrayWithObject: &*url];
        let class =
            AnyClass::get(c"UIActivityViewController").context(t("video.editor_unavailable"))?;
        let activity: *mut AnyObject = msg_send![class, alloc];
        let activity: *mut AnyObject = msg_send![activity, initWithActivityItems: &*items, applicationActivities: std::ptr::null::<AnyObject>()];
        let activity = Retained::from_raw(activity).context(t("video.editor_unavailable"))?;
        let device: *mut AnyObject = msg_send![AnyClass::get(c"UIDevice").unwrap(), currentDevice];
        let idiom: isize = msg_send![device, userInterfaceIdiom];
        if idiom == 1 {
            // iPad requires a popover anchored in the presenting view.
            let _: () = msg_send![&*activity, setModalPresentationStyle: 7isize];
            let popover: *mut AnyObject = msg_send![&*activity, popoverPresentationController];
            anyhow::ensure!(!popover.is_null(), "{}", t("video.editor_unavailable"));
            let view: *mut AnyObject = msg_send![controller, view];
            let bounds: Rect = msg_send![view, bounds];
            let anchor = Rect {
                origin: Point {
                    x: bounds.size.width / 2.0,
                    y: bounds.size.height / 2.0,
                },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            };
            let _: () = msg_send![popover, setSourceView: view];
            let _: () = msg_send![popover, setSourceRect: anchor];
            let _: () = msg_send![popover, setPermittedArrowDirections: 0usize];
        } else {
            // Phones use a modal share sheet.
            let _: () = msg_send![&*activity, setModalPresentationStyle: 1isize];
        }
        let _: () = msg_send![controller, presentViewController: &*activity, animated: Bool::YES, completion: std::ptr::null::<AnyObject>()];
    }
    Ok(())
}
