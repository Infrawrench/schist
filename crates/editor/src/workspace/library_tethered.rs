//! Tethered capture review keeps its own selection/preview, leaving gallery edits alone.
use super::*;
use gpui::{img, AnyElement, StyledImage as _};
use schist_i18n::{t, tf};
use schist_tethered::{Camera, Gphoto, Session};
use schist_ui::{Button, Link};
use std::{path::Path, sync::atomic::AtomicBool};

#[derive(Default)]
pub(super) struct Tethered {
    pub open: bool,
    cameras: Vec<Camera>,
    selected: Option<Camera>,
    session: Option<Session>,
    loaded: bool,
    busy: bool,
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
impl Workspace {
    pub(super) fn open_tethered(&mut self, cx: &mut Context<Self>) {
        self.library.tethered.open = true;
        self.cloud.show = false;
        if !self.library.tethered.loaded || self.library.tethered.cameras.is_empty() {
            self.tethered_discover(cx);
        }
        cx.notify();
    }
    pub(super) fn close_tethered(&mut self, cx: &mut Context<Self>) {
        self.library.tethered.cancel.store(true, Ordering::Release);
        self.library.tethered.open = false;
        self.library.tethered.selected = None;
        cx.notify();
    }
    fn tethered_begin(&mut self) -> Option<Arc<AtomicBool>> {
        let state = &mut self.library.tethered;
        if state.busy {
            return None;
        }
        state.busy = true;
        state.cancel = Arc::default();
        state.message = t("common.working").into();
        Some(state.cancel.clone())
    }
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
                        settings_path().map(|p| Session::load(&p))
                    } else {
                        None
                    };
                    (schist_tethered::discover(&Gphoto, &cancel), settings)
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
                        Err(e) => state.message = e,
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    fn tethered_connect(&mut self, camera: Camera, cx: &mut Context<Self>) {
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        self.library.tethered.selected = None;
        cx.spawn(async move |this, cx| {
            let candidate = camera.clone();
            let result = cx
                .background_executor()
                .spawn(async move { schist_tethered::connect(&Gphoto, &candidate, &cancel) })
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
                    Err(e) => state.message = e,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    fn tethered_session(&mut self, cx: &mut Context<Self>) {
        if self.library.tethered.busy {
            return;
        }
        let dir = self
            .library
            .tethered
            .session
            .as_ref()
            .map(|s| s.destination.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let name = self
            .library
            .tethered
            .session
            .as_ref()
            .map(|s| s.prefix.clone())
            .unwrap_or_else(|| t("library.volume.camera").into());
        let prompt = self.prompt_for_new_path(&dir, Some(&name), cx);
        let Some(cancel) = self.tethered_begin() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let chosen = prompt.await;
            let result = cx
                .background_executor()
                .spawn(async move {
                    let Ok(Ok(Some(path))) = chosen else {
                        return Ok(None);
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
                match result {
                    Ok(Some(s)) => state.session = Some(s),
                    Ok(None) => {}
                    Err(e) => state.message = e,
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
    fn tethered_capture(&mut self, cx: &mut Context<Self>) {
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
                    let captured = schist_tethered::capture(&Gphoto, &camera, &session, &cancel)?;
                    let paths = captured.paths;
                    // Prefer the JPEG from RAW+JPEG; all delivered originals import.
                    let path = paths
                        .iter()
                        .find(|p| p.extension().is_some_and(|e| e == "jpg" || e == "jpeg"))
                        .or_else(|| paths.first())
                        .unwrap()
                        .clone();
                    let preview = schist_preview::render_file(&path, 1600).ok();
                    Ok::<_, String>((paths, path, preview, captured.warning))
                })
                .await;
            this.update(cx, |ws, cx| {
                ws.library.tethered.busy = false;
                match result {
                    Ok((paths, path, preview, warning)) => {
                        let state = &mut ws.library.tethered;
                        state.preview = preview.and_then(|p| {
                            super::library::rgba_to_render_image(p.width, p.height, p.rgba)
                        });
                        state.last = Some(path.clone());
                        state.message = if let Some(error) = warning {
                            tf!("tethered.partial", error = error)
                        } else if state.preview.is_none() {
                            t("tethered.preview_failed").into()
                        } else {
                            tf!("common.saved_as", name = path.display())
                        };
                        let already_scanning = ws.library.scanning;
                        ws.finish_camera_import(
                            super::camera_import::ImportDestination::Local(destination),
                            paths.len(),
                            0,
                            0,
                            None,
                            cx,
                        );
                        if already_scanning {
                            ws.tethered_rescan_after_current(cx);
                        }
                    }
                    Err(e) => {
                        ws.library.tethered.message = e;
                        // Force an explicit re-probe after an error/unplug.
                        ws.library.tethered.selected = None;
                    }
                }
                // Completion can arrive after Close; keep partial-save and
                // preview errors visible in the ordinary workspace status too.
                ws.status = ws.library.tethered.message.clone().into();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
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
                .gap_2()
                .child(
                    Button::new("tethered-refresh", t("common.refresh"))
                        .disabled(busy)
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_discover(cx))),
                )
                .child(
                    Button::new("tethered-session", t("common.save_as"))
                        .disabled(busy)
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_session(cx))),
                )
                .child(
                    Button::new("tethered-shutter", t("tethered.capture"))
                        .disabled(busy || state.selected.is_none() || state.session.is_none())
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_capture(cx))),
                )
                .child(
                    Button::new("tethered-cancel", t("common.cancel"))
                        .disabled(!busy)
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.library.tethered.cancel.store(true, Ordering::Release);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("tethered-close", t("common.close"))
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_tethered(cx))),
                )
                .child(
                    Link::new("tethered-help", t("common.help"))
                        .url("https://github.com/Infrawrench/schist/blob/main/docs/tethered.md"),
                ),
        )
        .child(t("tethered.session_help"))
        .child(state.message.clone());
    if let Some(session) = &state.session {
        body = body
            .child(session.destination.display().to_string())
            .child(format!(
                "{}-000001.jpg / {}-000001.raw",
                session.prefix, session.prefix
            ));
    }
    for (index, camera) in state.cameras.iter().enumerate() {
        let candidate = camera.clone();
        let selected = state.selected.as_ref() == Some(camera);
        body = body.child(
            Button::new(
                ("tethered-camera", index),
                format!(
                    "{}{} ({})",
                    if selected { "✓ " } else { "" },
                    camera.model,
                    camera.port
                ),
            )
            .disabled(busy)
            .on_click(cx.listener(move |ws, _, _, cx| ws.tethered_connect(candidate.clone(), cx))),
        );
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
        preview.child(t("library.cell.no_preview"))
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
