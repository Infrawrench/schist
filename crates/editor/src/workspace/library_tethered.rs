//! Tethered capture review keeps its own selection/preview, leaving gallery edits alone.
use super::*;
use gpui::{img, AnyElement, StyledImage as _};
use schist_i18n::{t, tf};
use schist_tethered::{Camera, Captured, Session};
use schist_ui::{Button, DropdownButton};
use std::{path::Path, sync::atomic::AtomicBool};

const CAMERA_POPUP: Popup = Popup::Field("tethered-camera");

#[derive(Default)]
pub(super) struct Tethered {
    pub open: bool,
    cameras: Vec<Camera>,
    selected: Option<Camera>,
    session: Option<Session>,
    loaded: bool,
    pub(super) busy: bool,
    rescan_queued: bool,
    cancel: Arc<AtomicBool>,
    message: String,
    preview: Option<Arc<RenderImage>>,
    last: Option<PathBuf>,
}
impl Drop for Tethered {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn settings_path() -> Option<PathBuf> {
    Some(schist_gallery::library_path()?.with_file_name("tethered-session.json"))
}
type CaptureResult = Result<
    (
        Captured,
        PathBuf,
        Option<schist_preview::Preview>,
        Option<String>,
    ),
    String,
>;
impl Workspace {
    pub(super) fn open_tethered(&mut self, cx: &mut Context<Self>) {
        self.library.tethered.open = true;
        self.cloud.show = false;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        super::library_icc::start_browsing();
        if !self.library.tethered.loaded || self.library.tethered.cameras.is_empty() {
            self.tethered_discover(cx);
        }
        cx.notify();
    }
    pub(super) fn close_tethered(&mut self, cx: &mut Context<Self>) {
        if self.open_popup == Some(CAMERA_POPUP) {
            self.close_popup(cx);
        }
        self.library.tethered.cancel.store(true, Ordering::Release);
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        super::library_icc::cancel_tethered();
        #[cfg(target_os = "android")]
        schist_tethered::cancel_android();
        self.library.tethered.open = false;
        self.library.tethered.selected = None;
        cx.notify();
    }
    fn tethered_begin(&mut self) -> Option<Arc<AtomicBool>> {
        let state = &mut self.library.tethered;
        if state.busy || self.library.importing {
            return None;
        }
        if self.open_popup == Some(CAMERA_POPUP) {
            self.open_popup = None;
        }
        #[cfg(target_os = "android")]
        schist_tethered::begin_android();
        state.busy = true;
        state.cancel = Arc::default();
        state.message = t("common.working").into();
        Some(state.cancel.clone())
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    fn tethered_discover(&mut self, cx: &mut Context<Self>) {
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        self.library.tethered.selected = None;
        self.library.tethered.cameras.clear();
        let load = !self.library.tethered.loaded;
        cx.spawn(async move |this, cx| {
            let (result, settings) = cx
                .background_executor()
                .spawn(async move {
                    let settings = if load {
                        settings_path().map(|path| Session::load(&path))
                    } else {
                        None
                    };
                    (schist_tethered::discover(&cancel), settings)
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.tethered;
                state.busy = false;
                state.loaded = true;
                state.message.clear();
                match result {
                    Ok(cameras) => {
                        if cameras.is_empty() {
                            state.message = t("library.import.no_cameras").into();
                        }
                        state.cameras = cameras;
                    }
                    Err(error) => state.message = error,
                }
                if let Some(settings) = settings {
                    match settings {
                        Ok(session) => state.session = session,
                        Err(error) => state.message = error,
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn tethered_discover(&mut self, cx: &mut Context<Self>) {
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        self.library.tethered.selected = None;
        self.library.tethered.cameras.clear();
        super::library_icc::start_browsing();
        let load = !self.library.tethered.loaded;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let webcams = cx
                .background_executor()
                .spawn(async { schist_tethered::webcam::discover() })
                .await;
            let settings = if load {
                Some(
                    cx.background_executor()
                        .spawn(async move {
                            match settings_path() {
                                Some(path) => Session::load(&path),
                                None => Ok(None),
                            }
                        })
                        .await,
                )
            } else {
                None
            };
            loop {
                let cameras = super::library_icc::tethered_devices();
                #[cfg(target_os = "macos")]
                let cameras = {
                    let mut cameras = cameras;
                    if let Ok(webcams) = &webcams {
                        cameras.extend(webcams.iter().cloned());
                    }
                    cameras
                };
                let cancelled = cancel.load(Ordering::Acquire);
                // Allow ImageCaptureCore its enumeration window even when a
                // built-in webcam appears immediately through AVFoundation.
                let finished = cancelled || std::time::Instant::now() >= deadline;
                this.update(cx, |ws, cx| {
                    let state = &mut ws.library.tethered;
                    state.busy = !finished;
                    state.loaded = true;
                    state.cameras = cameras;
                    if cancelled {
                        state.message = t("common.cancelled").into();
                    } else if finished {
                        state.message.clear();
                        if state.cameras.is_empty() {
                            state.message = t("library.import.no_cameras").into();
                            #[cfg(target_os = "macos")]
                            if let Err(error) = &webcams {
                                state.message = error.clone();
                            }
                        }
                    }
                    if let Some(settings) = settings.clone() {
                        match settings {
                            Ok(Some(session)) => state.session = Some(session),
                            Ok(None) => {}
                            Err(error) => state.message = error,
                        }
                    }
                    cx.notify();
                })
                .ok();
                if finished {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
            }
        })
        .detach();
        cx.notify();
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    fn tethered_connect(&mut self, camera: Camera, cx: &mut Context<Self>) {
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        self.library.tethered.selected = None;
        cx.spawn(async move |this, cx| {
            let candidate = camera.clone();
            let result = cx
                .background_executor()
                .spawn(async move { schist_tethered::connect(&candidate, &cancel) })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.tethered;
                state.busy = false;
                match result {
                    Ok(()) => {
                        if state.open {
                            state.selected = Some(camera);
                        }
                        state.message.clear();
                    }
                    Err(error) => state.message = error,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn tethered_connect(&mut self, camera: Camera, cx: &mut Context<Self>) {
        if self.library.tethered.cameras.contains(&camera) {
            self.library.tethered.selected = Some(camera);
            self.library.tethered.message.clear();
            cx.notify();
        }
    }
    fn tethered_request_capture(&mut self, cx: &mut Context<Self>) {
        let state = &self.library.tethered;
        if state.busy || self.library.importing || !state.open || state.selected.is_none() {
            return;
        }
        if state.session.is_none() {
            self.tethered_session(true, cx);
        } else {
            self.tethered_capture(cx);
        }
    }
    fn tethered_session(&mut self, capture_after: bool, cx: &mut Context<Self>) {
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        let dir = self
            .library
            .tethered
            .session
            .as_ref()
            .map(|session| session.destination.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let name = self
            .library
            .tethered
            .session
            .as_ref()
            .map(|session| session.prefix.clone())
            .unwrap_or_else(|| t("library.volume.camera").into());
        let prompt = self.prompt_for_capture_path(&dir, &name, cx);
        cx.spawn(async move |this, cx| {
            let chosen = prompt.await;
            let cancelled = cancel.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = match chosen {
                        Ok(Ok(Some(path))) => path,
                        Ok(Err(error)) => return Err(error.to_string()),
                        Ok(Ok(None)) | Err(_) => return Ok(None),
                    };
                    if cancel.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let session = Session {
                        destination: path.parent().unwrap_or(Path::new("")).to_path_buf(),
                        prefix: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    };
                    let path =
                        settings_path().ok_or_else(|| t("common.not_available").to_string())?;
                    session.save(&path)?;
                    Ok::<_, String>(Some(session))
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.tethered;
                state.busy = false;
                state.message.clear();
                let ready = match result {
                    Ok(Some(session)) => {
                        state.session = Some(session);
                        true
                    }
                    Ok(None) => false,
                    Err(error) => {
                        state.message = error;
                        false
                    }
                };
                if ready && capture_after && !cancelled.load(Ordering::Acquire) {
                    ws.tethered_request_capture(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    fn tethered_rescan_after_current(&mut self, cx: &mut Context<Self>) {
        if self.library.tethered.rescan_queued {
            return;
        }
        self.library.tethered.rescan_queued = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let waiting = this
                .update(cx, |ws, cx| {
                    if ws.library.scanning {
                        return true;
                    }
                    ws.library.tethered.rescan_queued = false;
                    ws.library_rescan(cx);
                    false
                })
                .unwrap_or(false);
            if !waiting {
                break;
            }
        })
        .detach();
    }
    fn finish_tethered_capture(
        &mut self,
        destination: PathBuf,
        result: CaptureResult,
        cx: &mut Context<Self>,
    ) {
        self.library.tethered.busy = false;
        match result {
            Ok((captured, path, preview, download_warning)) => {
                let state = &mut self.library.tethered;
                state.preview = preview.and_then(|preview| {
                    super::library::rgba_to_render_image(
                        preview.width,
                        preview.height,
                        preview.rgba,
                    )
                });
                state.last = Some(path.clone());
                let warning = captured.warning.or(download_warning);
                state.message = if let Some(error) = warning {
                    tf!("tethered.partial", error = error)
                } else if state.preview.is_none() {
                    t("tethered.preview_failed").into()
                } else {
                    tf!("common.saved_as", name = path.display())
                };
                let already_scanning = self.library.scanning;
                self.finish_camera_import(
                    super::camera_import::ImportDestination::Local(destination),
                    captured.paths.len(),
                    0,
                    0,
                    None,
                    cx,
                );
                if already_scanning {
                    self.tethered_rescan_after_current(cx);
                }
            }
            Err(error) => {
                self.library.tethered.message = error;
                self.library.tethered.selected = None;
            }
        }
        self.status = self.library.tethered.message.clone().into();
        cx.notify();
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
    fn tethered_capture(&mut self, cx: &mut Context<Self>) {
        self.tethered_capture_worker(cx);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "windows",
        target_os = "android",
        target_os = "macos"
    ))]
    fn tethered_capture_worker(&mut self, cx: &mut Context<Self>) {
        let (Some(camera), Some(session)) = (
            self.library.tethered.selected.clone(),
            self.library.tethered.session.clone(),
        ) else {
            return;
        };
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let destination = session.destination.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(target_os = "macos")]
                    let captured = schist_tethered::webcam::capture(&camera, &session, cancel)?;
                    #[cfg(not(target_os = "macos"))]
                    let captured = schist_tethered::capture(&camera, &session, &cancel)?;
                    let path = captured
                        .paths
                        .iter()
                        .find(|path| {
                            path.extension()
                                .is_some_and(|extension| extension == "jpg" || extension == "jpeg")
                        })
                        .or_else(|| captured.paths.first())
                        .unwrap()
                        .clone();
                    let preview = schist_preview::render_file(&path, 1600).ok();
                    Ok::<_, String>((captured, path, preview, None))
                })
                .await;
            this.update(cx, |ws, cx| {
                ws.finish_tethered_capture(destination, result, cx)
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn tethered_capture(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if self
            .library
            .tethered
            .selected
            .as_ref()
            .is_some_and(schist_tethered::webcam::is_camera)
        {
            self.tethered_capture_worker(cx);
            return;
        }
        let (Some(_), Some(session), Some(id)) = (
            self.library.tethered.selected.clone(),
            self.library.tethered.session.clone(),
            self.library
                .tethered
                .selected
                .as_ref()
                .and_then(|camera| camera.id),
        ) else {
            return;
        };
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let staging_session = session.clone();
            let prepared = cx
                .background_executor()
                .spawn(async move { schist_tethered::staging(&staging_session) })
                .await;
            let started = this.update(cx, |ws, cx| {
                let staging = match prepared {
                    Ok(staging) if !cancel.load(Ordering::Acquire) => staging,
                    Ok(_) => return Err(t("common.cancelled").into()),
                    Err(error) => return Err(error),
                };
                super::library_icc::begin_tethered(id, staging)?;
                ws.tethered_poll(session, cancel, cx);
                Ok(())
            });
            if let Ok(Err(error)) = started {
                this.update(cx, |ws, cx| {
                    ws.library.tethered.busy = false;
                    ws.library.tethered.message = error;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn tethered_poll(&mut self, session: Session, cancel: Arc<AtomicBool>, cx: &mut Context<Self>) {
        let destination = session.destination.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            if cancel.load(Ordering::Acquire) {
                super::library_icc::cancel_tethered();
            }
            let Some(status) = super::library_icc::poll_tethered() else {
                this.update(cx, |ws, cx| {
                    ws.library.tethered.busy = false;
                    ws.library.tethered.message = t("common.not_available").into();
                    cx.notify();
                })
                .ok();
                break;
            };
            let Some(result) = status.finished else {
                continue;
            };
            let Some(download) = super::library_icc::take_tethered() else {
                this.update(cx, |ws, cx| {
                    ws.library.tethered.busy = false;
                    ws.library.tethered.message = t("common.not_available").into();
                    cx.notify();
                })
                .ok();
                break;
            };
            if let Err(error) = result {
                this.update(cx, |ws, cx| {
                    ws.library.tethered.busy = false;
                    ws.library.tethered.message = error;
                    ws.library.tethered.selected = None;
                    ws.status = ws.library.tethered.message.clone().into();
                    cx.notify();
                })
                .ok();
                break;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let captured =
                        schist_tethered::publish(download.staging.path(), &session, &cancel)?;
                    let path = captured
                        .paths
                        .iter()
                        .find(|path| {
                            path.extension()
                                .is_some_and(|extension| extension == "jpg" || extension == "jpeg")
                        })
                        .or_else(|| captured.paths.first())
                        .unwrap()
                        .clone();
                    let preview = schist_preview::render_file(&path, 1600).ok();
                    Ok::<_, String>((captured, path, preview, download.warning))
                })
                .await;
            this.update(cx, |ws, cx| {
                ws.finish_tethered_capture(destination.clone(), result, cx)
            })
            .ok();
            break;
        })
        .detach();
    }
}
fn camera_label(camera: &Camera) -> SharedString {
    let show_port = !camera.port.is_empty();
    #[cfg(target_os = "macos")]
    let show_port = show_port && !schist_tethered::webcam::is_camera(camera);
    if show_port {
        format!("{} ({})", camera.model, camera.port).into()
    } else {
        camera.model.clone().into()
    }
}

fn camera_picker(ws: &Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let state = &ws.library.tethered;
    let label = state
        .selected
        .as_ref()
        .map(camera_label)
        .unwrap_or_else(|| {
            t(if state.busy {
                "common.working"
            } else if state.cameras.is_empty() {
                "library.import.no_cameras"
            } else {
                "tethered.select_camera"
            })
            .into()
        });
    if state.busy || ws.library.importing || state.cameras.is_empty() {
        return DropdownButton::new("tethered-camera-disabled", label)
            .w(px(280.))
            .opacity(0.5)
            .cursor(gpui::CursorStyle::Arrow)
            .into_any_element();
    }
    crate::ui::dropdown(
        &ws.dropdown,
        crate::ui::Dropdown {
            popup: CAMERA_POPUP,
            is_open: ws.open_popup == Some(CAMERA_POPUP),
            current: state.selected.clone(),
            label,
            width: 280.,
            options: state
                .cameras
                .iter()
                .map(|camera| (camera_label(camera), Some(camera.clone())))
                .collect(),
        },
        |ws, camera, cx| {
            if let Some(camera) = camera {
                if ws.library.tethered.open && !ws.library.tethered.busy && !ws.library.importing {
                    ws.tethered_connect(camera, cx);
                }
            }
        },
        cx,
    )
    .into_any_element()
}

pub(super) fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let state = &ws.library.tethered;
    let busy = state.busy;
    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .p_3()
        .gap_2()
        .child(div().text_size(px(20.)).child(t("tethered.title")))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(camera_picker(ws, cx))
                .child(
                    Button::new("tethered-refresh", t("common.refresh"))
                        .disabled(busy)
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_discover(cx))),
                )
                .child(
                    Button::new("tethered-session", t("common.save_as"))
                        .disabled(busy || ws.library.importing)
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_session(false, cx))),
                )
                .child(
                    Button::new("tethered-shutter", t("tethered.capture"))
                        .disabled(busy || ws.library.importing || state.selected.is_none())
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_request_capture(cx))),
                )
                .child(
                    Button::new("tethered-cancel", t("common.cancel"))
                        .disabled(!busy)
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.library.tethered.cancel.store(true, Ordering::Release);
                            #[cfg(target_os = "android")]
                            schist_tethered::cancel_android();
                            #[cfg(any(target_os = "macos", target_os = "ios"))]
                            super::library_icc::cancel_tethered();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("tethered-close", t("common.close"))
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_tethered(cx))),
                ),
        )
        .child(t("tethered.session_help"))
        .child(state.message.clone());
    if let Some(session) = &state.session {
        let examples = format!(
            "{}-000001.jpg / {}-000001.raw",
            session.prefix, session.prefix
        );
        #[cfg(target_os = "macos")]
        let examples = if state
            .selected
            .as_ref()
            .is_some_and(schist_tethered::webcam::is_camera)
        {
            format!("{}-000001.jpg", session.prefix)
        } else {
            examples
        };
        body = body
            .child(session.destination.display().to_string())
            .child(examples);
    }
    let preview = div()
        .flex()
        .flex_1()
        .min_h(px(100.))
        .items_center()
        .justify_center()
        .overflow_hidden();
    body = body.child(if let Some(image) = &state.preview {
        preview.child(
            img(image.clone())
                .size_full()
                .object_fit(gpui::ObjectFit::Contain),
        )
    } else {
        preview.child(t(if state.last.is_some() {
            "tethered.preview_failed"
        } else if state.selected.is_none() {
            "tethered.select_camera"
        } else if state.session.is_none() {
            "tethered.choose_destination"
        } else {
            "tethered.preview_empty"
        }))
    });
    if let Some(path) = &state.last {
        let path = path.clone();
        body = body.child(path.display().to_string()).child(
            Button::new("tethered-edit", t("common.edit"))
                .disabled(busy)
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_from_gallery(path.clone(), cx))),
        );
    }
    body.into_any_element()
}
