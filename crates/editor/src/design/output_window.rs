use super::output::{self, Options};
use crate::workspace::Workspace;
use gpui::{div, prelude::*, px, *};
use schist_i18n::{t, tf};
use schist_ui as ui;
#[cfg(target_arch = "wasm32")]
use std::path::PathBuf;
use std::sync::Arc;

struct OutputWindow {
    workspace: WeakEntity<Workspace>,
    session: Arc<()>,
    options: Options,
    busy: bool,
    notice: String,
    focus: FocusHandle,
}
impl OutputWindow {
    fn export(&mut self, package: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let session = self.session.clone();
        let snapshot = workspace.update(cx, |ws, cx| {
            if !Arc::ptr_eq(&session, &ws.design.session) {
                return None;
            }
            ws.commit_focused_field();
            ws.design.cancel_gesture();
            cx.notify();
            Some((
                ws.design.document.clone(),
                ws.design.graphics.clone(),
                ws.design_path.clone(),
            ))
        });
        let Some((document, graphics, source_path)) = snapshot else {
            self.notice = t("design.story_session_closed").into();
            cx.notify();
            return;
        };
        let extension = if package { "zip" } else { "pdf" };
        let name = source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("layout");
        let suggested = format!("{name}.{extension}");
        #[cfg(not(target_arch = "wasm32"))]
        let picker = workspace.update(cx, |ws, cx| {
            ws.prompt_for_new_path(
                source_path
                    .as_deref()
                    .and_then(|p| p.parent())
                    .unwrap_or(std::path::Path::new(".")),
                Some(&suggested),
                cx,
            )
        });
        let options = self.options.clone();
        self.busy = true;
        self.notice = t("design.output_busy").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let path = match picker.await {
                Ok(Ok(path)) => path,
                _ => None,
            };
            #[cfg(target_arch = "wasm32")]
            let path = Some(PathBuf::from(suggested));
            let Some(mut path) = path else {
                this.update(cx, |view, cx| {
                    view.busy = false;
                    view.notice.clear();
                    cx.notify();
                })
                .ok();
                return;
            };
            path.set_extension(extension);
            let shown = path.to_string_lossy().into_owned();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (bytes, warnings) = if package {
                        let result = output::package(&document, source_path.as_deref())?;
                        (result.bytes, result.warnings)
                    } else {
                        let result = output::pdf_report(&document, &graphics, &options)?;
                        (result.bytes, result.warnings)
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    output::write_atomic(&path, &bytes)?;
                    #[cfg(target_arch = "wasm32")]
                    crate::web::download_bytes(
                        path.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("layout.pdf"),
                        &bytes,
                    )?;
                    Ok::<_, anyhow::Error>(warnings)
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.notice = match result {
                    Ok(warnings) => format!(
                        "{}{}",
                        tf!("design.output_saved", path = shown),
                        if warnings.is_empty() {
                            String::new()
                        } else {
                            format!("\n{}", warnings.join("; "))
                        }
                    ),
                    Err(error) => tf!("design.output_failed", error = error),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn profile(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let picker = {
            let Some(workspace) = self.workspace.upgrade() else {
                return;
            };
            workspace.update(cx, |ws, cx| {
                ws.prompt_for_paths(
                    PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some(t("design.output_profile").into()),
                    },
                    cx,
                )
            })
        };
        #[cfg(target_arch = "wasm32")]
        let picker = crate::web::pick_file(".icc,.icm");
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            #[cfg(not(target_arch = "wasm32"))]
            let path = picker
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|p| p.into_iter().next());
            #[cfg(target_arch = "wasm32")]
            let path = picker.await.ok().flatten();
            let result = if let Some(path) = path {
                Some(
                    cx.background_executor()
                        .spawn(async move {
                            #[cfg(not(target_arch = "wasm32"))]
                            let bytes = std::fs::read(&path)?;
                            #[cfg(target_arch = "wasm32")]
                            let bytes = crate::web::read_file(&path)?.as_ref().clone();
                            anyhow::ensure!(
                                bytes.len() <= 8 * 1024 * 1024,
                                t("design.output_invalid")
                            );
                            let transform = schist_colormgmt::NativeColorTransform::new(
                                schist_color::ColorMode::Cmyk,
                                Some(&bytes),
                            )?;
                            transform.rgb_to_cmyk_checked(&[[0.5, 0.5, 0.5]])?;
                            Ok::<_, anyhow::Error>((
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                Arc::new(bytes),
                            ))
                        })
                        .await,
                )
            } else {
                None
            };
            this.update(cx, |view, cx| {
                view.busy = false;
                if let Some(result) = result {
                    match result {
                        Ok(profile) => {
                            view.options.profile = Some(profile);
                            view.notice.clear();
                        }
                        Err(error) => view.notice = tf!("design.output_failed", error = error),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
impl Focusable for OutputWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for OutputWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let profile = self
            .options
            .profile
            .as_ref()
            .map(|(name, _)| tf!("design.output_profile_name", name = name))
            .unwrap_or_else(|| t("design.output_preview").to_string());
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(ui::palette().window_bg))
            .text_color(rgb(ui::palette().text))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape"
                    || (event.keystroke.modifiers.platform && event.keystroke.key == "w")
                {
                    window.remove_window();
                }
            }))
            .child(div().text_lg().child(t("design.output_title")))
            .child(div().text_sm().child(t("design.output_snapshot")))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .children([72, 150, 300, 600].into_iter().enumerate().map(|(i, dpi)| {
                        ui::Button::new(("dpi", i), tf!("design.output_resolution", dpi = dpi))
                            .active(self.options.dpi == dpi)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.options.dpi = dpi;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .children([1, 2, 4].into_iter().enumerate().map(|(i, up)| {
                        ui::Button::new(("up", i), tf!("design.output_up", count = up))
                            .active(self.options.up == up)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.options.up = up;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ui::Button::new("marks", t("design.output_marks"))
                            .active(self.options.marks)
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.options.marks = !view.options.marks;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::Button::new("hidden", t("design.output_hidden"))
                            .active(self.options.hidden)
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.options.hidden = !view.options.hidden;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_sm().child(profile))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ui::Button::new("profile", t("design.output_profile"))
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| view.profile(cx))),
                    )
                    .child(
                        ui::Button::new("preview", t("design.output_preview"))
                            .active(self.options.profile.is_none())
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.options.profile = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ui::Button::new("pdf", t("design.export_pdf"))
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| view.export(false, cx))),
                    )
                    .child(
                        ui::Button::new("package", t("design.package"))
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, _, cx| view.export(true, cx))),
                    ),
            )
            .child(div().text_sm().child(t("design.output_package_note")))
            .child(
                div()
                    .id("output-notice")
                    .flex_1()
                    .overflow_y_scroll()
                    .text_sm()
                    .child(self.notice.clone()),
            )
    }
}
impl Workspace {
    pub fn open_design_output(&mut self, cx: &mut Context<Self>) {
        if !self.design_mode() {
            return;
        }
        let workspace = cx.entity().downgrade();
        let session = self.design.session.clone();
        let bounds = Bounds::centered(None, size(px(700.0), px(470.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(t("design.output_title").into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| OutputWindow {
                    workspace,
                    session,
                    options: Options::default(),
                    busy: false,
                    notice: String::new(),
                    focus: cx.focus_handle(),
                });
                window.focus(&view.read(cx).focus);
                view
            },
        ) {
            self.status = tf!("design.output_failed", error = error).into();
        }
        cx.notify();
    }
}
