//! Video preview and still-frame editing. The viewer owns cancellation;
//! closing it or seeking invalidates every pending result and decode.
use super::gallery_chrome::pal;
use super::*;
use crate::video::{self, Frame, Info, Job};
use gpui::{img, StatefulInteractiveElement as _, StyledImage as _};
use schist_i18n::{t, tf};

fn video_button(
    label: impl Into<SharedString>,
    green: bool,
    action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    super::gallery_chrome::gallery_button(label, green, action, cx)
        .h(px(if crate::ui::touch() { 44.0 } else { 24.0 }))
        .text_size(px(if crate::ui::touch() { 14.0 } else { 12.0 }))
}

pub struct VideoViewer {
    pub path: PathBuf,
    info: Option<Info>,
    time: f64,
    image: Option<Arc<RenderImage>>,
    image_painted: bool,
    pub(super) playing: bool,
    ended: bool,
    busy: bool,
    message: String,
    job: Arc<Job>,
    retired: Arc<std::sync::Mutex<Vec<Arc<RenderImage>>>>,
}
impl Drop for VideoViewer {
    fn drop(&mut self) {
        self.job.cancel();
        if let Some(image) = self.image.take().filter(|_| self.image_painted) {
            self.retired.lock().unwrap().push(image);
        }
    }
}
impl VideoViewer {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    pub(super) fn pause(&mut self) {
        self.restart();
        self.busy = false;
    }
    #[cfg(any(target_os = "ios", target_os = "android"))]
    pub(super) fn set_message(&mut self, message: String) {
        self.message = message;
    }
    fn restart(&mut self) -> Arc<Job> {
        self.job.cancel();
        self.job = Arc::new(Job::default());
        self.playing = false;
        self.busy = true;
        self.message.clear();
        self.job.clone()
    }
    fn show(&mut self, frame: Frame) {
        self.ended = false;
        self.time = frame.time;
        if let Some(image) = self.image.take().filter(|_| self.image_painted) {
            self.retired.lock().unwrap().push(image);
        }
        self.image_painted = false;
        self.image = super::library::rgba_to_render_image(frame.width, frame.height, frame.rgba);
        self.busy = false;
    }
}

#[derive(Clone, Copy)]
enum FrameAction {
    Seek(f64),
    Previous,
    Sharper,
    Edit,
}

impl Workspace {
    pub(super) fn open_video(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.viewer_unpick();
        self.library.viewer = None;
        self.library.context = None;
        self.library.search.active = false;
        self.library.map_view = false;
        self.library.open = true;
        self.cloud.show = false;
        self.note_recent(&path);
        let job = Arc::new(Job::default());
        self.library.video = Some(VideoViewer {
            path: path.clone(),
            info: None,
            time: 0.0,
            image: None,
            image_painted: false,
            playing: false,
            ended: false,
            busy: true,
            message: t("video.loading").into(),
            job: job.clone(),
            retired: self.library.video_retired.clone(),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let work = job.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let info = video::probe(&path, work.clone())?;
                    let frame = video::frame(&path, 0.0, video::PREVIEW_EDGE, work)?;
                    anyhow::Ok((info, frame))
                })
                .await;
            this.update(cx, |ws, cx| {
                let Some(viewer) = ws
                    .library
                    .video
                    .as_mut()
                    .filter(|v| Arc::ptr_eq(&v.job, &job))
                else {
                    return;
                };
                viewer.busy = false;
                match result {
                    Ok((info, frame)) => {
                        viewer.info = Some(info);
                        viewer.message.clear();
                        viewer.show(frame);
                    }
                    Err(err) => viewer.message = format!("{err:#}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn close_video(&mut self, cx: &mut Context<Self>) {
        self.library.video = None;
        cx.notify();
    }

    fn video_frame(&mut self, action: FrameAction, cx: &mut Context<Self>) {
        let Some(viewer) = self.library.video.as_mut() else {
            return;
        };
        let Some(info) = viewer.info else { return };
        let time = viewer.time;
        let path = viewer.path.clone();
        let job = viewer.restart();
        viewer.message = match action {
            FrameAction::Sharper => t("video.searching"),
            FrameAction::Edit => t("video.capturing"),
            _ => t("video.loading"),
        }
        .into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let work = job.clone();
            let source = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    match action {
                        FrameAction::Seek(at) => video::seek(
                            &source,
                            at.clamp(0.0, (info.duration - 0.001).max(0.0)),
                            video::PREVIEW_EDGE,
                            work,
                        ),
                        FrameAction::Previous => video::previous(&source, time, work),
                        FrameAction::Sharper => {
                            let at = video::sharper(&source, time, info.duration, work.clone())?;
                            video::frame(&source, at, video::PREVIEW_EDGE, work)
                        }
                        FrameAction::Edit => video::frame(&source, time, 0, work),
                    }
                })
                .await;
            this.update(cx, |ws, cx| {
                let Some(viewer) = ws
                    .library
                    .video
                    .as_mut()
                    .filter(|v| Arc::ptr_eq(&v.job, &job))
                else {
                    return;
                };
                viewer.busy = false;
                viewer.message.clear();
                match result {
                    Ok(frame) if matches!(action, FrameAction::Edit) => {
                        ws.install_document(frame_document(&path, frame));
                    }
                    Ok(frame) => {
                        if matches!(action, FrameAction::Sharper) {
                            viewer.message = if (frame.time - time).abs() < 0.001 {
                                t("video.already_sharpest").into()
                            } else {
                                tf!("video.found_frame", time = time_label(frame.time))
                            };
                        }
                        viewer.show(frame);
                    }
                    Err(err) => viewer.message = format!("{err:#}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn video_toggle_play(&mut self, cx: &mut Context<Self>) {
        let Some(viewer) = self.library.video.as_mut() else {
            return;
        };
        if viewer.info.is_none() {
            return;
        }
        if viewer.playing {
            viewer.restart();
            viewer.busy = false;
            cx.notify();
            return;
        }
        let start = if viewer.ended { 0.0 } else { viewer.time };
        let path = viewer.path.clone();
        let job = viewer.restart();
        viewer.playing = true;
        // One-frame backpressure bounds memory even when decode outruns display.
        let (mut tx, mut rx) = futures::channel::mpsc::channel::<anyhow::Result<Frame>>(1);
        let worker_job = job.clone();
        cx.background_executor()
            .spawn(async move {
                use futures::SinkExt;
                let result = (|| -> anyhow::Result<()> {
                    let mut decoder = video::Decoder::open(
                        &path,
                        start,
                        None,
                        video::PREVIEW_EDGE,
                        worker_job.clone(),
                    )?;
                    while let Some(frame) = decoder.next()? {
                        if worker_job.cancelled() {
                            break;
                        }
                        if futures::executor::block_on(tx.send(Ok(frame))).is_err() {
                            break;
                        }
                    }
                    Ok(())
                })();
                if let Err(err) = result {
                    let _ = tx.send(Err(err)).await;
                }
            })
            .detach();
        cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let mut clock: Option<(web_time::Instant, f64)> = None;
            while let Some(result) = rx.next().await {
                if job.cancelled() {
                    break;
                }
                if let Ok(frame) = &result {
                    let (began, first) =
                        *clock.get_or_insert((web_time::Instant::now(), frame.time));
                    let wait = (frame.time - first - began.elapsed().as_secs_f64()).max(0.0);
                    // Short waits keep cancellation responsive across long gaps in VFR media.
                    let until = web_time::Instant::now() + std::time::Duration::from_secs_f64(wait);
                    while web_time::Instant::now() < until && !job.cancelled() {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(10))
                            .await;
                    }
                }
                if job.cancelled() {
                    break;
                }
                let keep = this
                    .update(cx, |ws, cx| {
                        let visible = ws.library.open && !ws.cloud.show && !ws.library.map_view;
                        let Some(viewer) = ws
                            .library
                            .video
                            .as_mut()
                            .filter(|v| Arc::ptr_eq(&v.job, &job))
                        else {
                            return false;
                        };
                        if !visible {
                            viewer.restart();
                            viewer.busy = false;
                            return false;
                        }
                        match result {
                            Ok(frame) => viewer.show(frame),
                            Err(err) => {
                                viewer.message = format!("{err:#}");
                                viewer.playing = false;
                                viewer.busy = false;
                            }
                        }
                        cx.notify();
                        viewer.playing
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
            let reached_end = !job.cancelled();
            job.cancel();
            this.update(cx, |ws, cx| {
                if let Some(v) = ws
                    .library
                    .video
                    .as_mut()
                    .filter(|v| Arc::ptr_eq(&v.job, &job))
                {
                    if v.playing && reached_end {
                        v.ended = true;
                    }
                    v.playing = false;
                    v.busy = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    fn choose_video_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.library.video.as_ref().map(|v| v.path.clone()) else {
            return;
        };
        let rx = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: cfg!(target_os = "macos"),
                multiple: false,
                prompt: Some(t("video.choose_editor").into()),
            },
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(editor) = paths.into_iter().next() {
                    this.update(cx, |ws, cx| {
                        ws.library.video_editor = Some(editor.clone());
                        ws.library.save();
                        ws.launch_video_editor(editor, path, cx);
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    fn launch_video_editor(&mut self, editor: PathBuf, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(viewer) = self.library.video.as_mut() {
            viewer.restart();
            viewer.busy = false;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { video::open_editor(&editor, &path) })
                .await;
            this.update(cx, |ws, cx| {
                if let Err(err) = result {
                    let message = tf!("video.editor_failed", error = err);
                    if let Some(viewer) = ws.library.video.as_mut() {
                        viewer.message = message.clone();
                    }
                    ws.status = message.into();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn video_enter(&mut self, cx: &mut Context<Self>) -> bool {
        if self.library.video.is_none() || self.library.search.active {
            return false;
        }
        self.video_frame(FrameAction::Edit, cx);
        true
    }

    pub(super) fn video_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.library.video.is_none()
            || self.focused_field.is_some()
            || self.library.search.active
        {
            return false;
        }
        if ev.is_held && ev.keystroke.key == "space" {
            return true;
        }
        match ev.keystroke.key.as_str() {
            "space" => self.video_toggle_play(cx),
            "left" => self.video_frame(FrameAction::Previous, cx),
            "right" => {
                let at = self.library.video.as_ref().unwrap().time + 0.001;
                self.video_frame(FrameAction::Seek(at), cx);
            }
            "enter" => self.video_frame(FrameAction::Edit, cx),
            _ => return false,
        }
        true
    }

    pub(super) fn render_video(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let viewer = self.library.video.as_mut().unwrap();
        // Frames replaced while minimized were never in the atlas and can
        // be dropped immediately instead of accumulating until the next paint.
        viewer.image_painted = viewer.image.is_some();
        let name = viewer
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let image = viewer.image.clone();
        let time = viewer.time;
        let duration = viewer.info.map(|i| i.duration).unwrap_or(0.0);
        let ready = viewer.info.is_some() && !viewer.busy;
        let playing = viewer.playing;
        let message = viewer.message.clone();
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let editor = self.library.video_editor.clone();
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let path = viewer.path.clone();
        let mut controls = div()
            .flex_none()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(video_button(
                t("video.back"),
                false,
                |ws, _, cx| ws.close_video(cx),
                cx,
            ));
        if ready || playing {
            controls = controls
                .child(video_button(
                    t(if playing { "video.pause" } else { "video.play" }),
                    false,
                    |ws, _, cx| ws.video_toggle_play(cx),
                    cx,
                ))
                .child(video_button(
                    t("video.previous"),
                    false,
                    |ws, _, cx| ws.video_frame(FrameAction::Previous, cx),
                    cx,
                ))
                .child(video_button(
                    t("video.next"),
                    false,
                    |ws, _, cx| {
                        let at = ws
                            .library
                            .video
                            .as_ref()
                            .map(|v| v.time + 0.001)
                            .unwrap_or(0.0);
                        ws.video_frame(FrameAction::Seek(at), cx)
                    },
                    cx,
                ))
                .child(video_button(
                    t("video.sharper"),
                    false,
                    |ws, _, cx| ws.video_frame(FrameAction::Sharper, cx),
                    cx,
                ))
                .child(video_button(
                    t("video.edit_frame"),
                    true,
                    |ws, _, cx| ws.video_frame(FrameAction::Edit, cx),
                    cx,
                ));
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            if let Some(editor) = editor {
                controls = controls.child(video_button(
                    t("video.open_editor"),
                    false,
                    move |ws, _, cx| ws.launch_video_editor(editor.clone(), path.clone(), cx),
                    cx,
                ));
            }
            controls = controls.child(video_button(
                t("video.choose_editor"),
                false,
                |ws, w, cx| ws.choose_video_editor(w, cx),
                cx,
            ));
        }
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            controls = controls.child(video_button(
                t("video.open_another_app"),
                false,
                |ws, _, cx| ws.share_video(cx),
                cx,
            ));
        }
        let mut body = div()
            .id("video-viewer")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .gap_2()
            .p_3()
            .bg(gpui::rgb(pal().grid_bg))
            .text_color(gpui::rgb(pal().text))
            .text_size(px(12.0))
            .child(div().truncate().child(name))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(if crate::ui::touch() { 120.0 } else { 0.0 }))
                    .overflow_hidden()
                    .bg(gpui::rgb(0))
                    .children(image.map(|image| {
                        img(image)
                            .absolute()
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain)
                    })),
            )
            .child(controls);
        if duration > 0.0 {
            body = body.child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::slider_track(
                        "video-seek",
                        (time / duration) as f32,
                        (f32::from(window.viewport_size().width) - 48.0).clamp(80.0, 320.0),
                        move |ws, ratio, cx| {
                            ws.video_frame(FrameAction::Seek(ratio as f64 * duration), cx)
                        },
                        cx,
                    ))
                    .child(format!("{} / {}", time_label(time), time_label(duration))),
            );
        }
        body.child(
            div()
                .text_color(gpui::rgb(pal().text_dim))
                .child(t(if crate::ui::touch() {
                    "video.preview_help_touch"
                } else {
                    "video.preview_help"
                })),
        )
        .child(div().child(message))
        .into_any_element()
    }
}

pub(super) fn time_label(time: f64) -> String {
    let ms = (time.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Capturing is a new image, never a save target for the source movie.
fn frame_document(path: &std::path::Path, frame: Frame) -> Document {
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let title = tf!(
        "video.frame_title",
        name = name,
        time = time_label(frame.time)
    );
    let mut doc = Document::new(title, frame.width, frame.height, Depth::Eight);
    let mut layer = Layer::new_raster(t("video.frame_layer").to_string());
    blit_rgba8(
        &mut layer.as_raster_mut().unwrap().tiles,
        Depth::Eight,
        IntRect::from_size(frame.width, frame.height),
        &frame.rgba,
    );
    doc.push_layer(layer);
    doc.dirty = true;
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_capture_is_an_unsaved_image_document() {
        let doc = frame_document(
            std::path::Path::new("/movies/holiday.mov"),
            Frame {
                time: 12.375,
                width: 2,
                height: 1,
                rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            },
        );
        assert!(
            doc.path.is_none(),
            "saving must ask for an image destination"
        );
        assert!(doc.dirty, "closing must offer to save the captured frame");
        assert_eq!((doc.width, doc.height), (2, 1));
        assert!(doc.title.contains("00:00:12.375"));
    }
}
