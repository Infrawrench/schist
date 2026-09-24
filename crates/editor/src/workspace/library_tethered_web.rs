//! Browser capture uses explicit WebUSB permission and the browser file store.
use super::*;
use gpui::{AnyElement, StatefulInteractiveElement as _};
use schist_app_platform::web;
use schist_i18n::t;
use schist_ui::Button;

#[derive(Default)]
pub(super) struct Tethered {
    pub open: bool,
    pub busy: bool,
    model: Option<String>,
    message: String,
    paths: Vec<PathBuf>,
    preview: Option<Arc<gpui::Image>>,
}
impl Workspace {
    pub(super) fn open_tethered(&mut self, cx: &mut Context<Self>) {
        self.cloud.show = true;
        self.browser_tethered.open = true;
        cx.notify();
    }
    pub(super) fn close_tethered(&mut self, cx: &mut Context<Self>) {
        drop(web::tethered_request("cancel"));
        self.browser_tethered.open = false;
        cx.notify();
    }
    fn tethered_request(&mut self, operation: &'static str, cx: &mut Context<Self>) {
        if self.browser_tethered.busy {
            return;
        }
        self.browser_tethered.busy = true;
        self.browser_tethered.message = t("common.working").into();
        let request = web::tethered_request(operation);
        cx.spawn(async move |this, cx| {
            let result = request
                .await
                .unwrap_or_else(|_| Err(t("common.cancelled").into()));
            this.update(cx, |ws, cx| {
                let state = &mut ws.browser_tethered;
                state.busy = false;
                match result {
                    Ok(reply) => {
                        if reply.model.is_some() {
                            state.model = reply.model;
                        }
                        if let Some(path) = reply.paths.iter().find(|path| {
                            path.extension()
                                .is_some_and(|ext| ext == "jpg" || ext == "jpeg")
                        }) {
                            state.preview = web::read_file(path).ok().map(|bytes| {
                                Arc::new(gpui::Image::from_bytes(
                                    gpui::ImageFormat::Jpeg,
                                    bytes.as_ref().clone(),
                                ))
                            });
                        } else if !reply.paths.is_empty() {
                            state.preview = None;
                        }
                        state.paths.extend(reply.paths);
                        state.message.clear();
                    }
                    Err(error) => {
                        state.message = error;
                        state.model = None;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}
pub(super) fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let state = &ws.browser_tethered;
    let mut body = div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.))
        .p_4()
        .gap_3()
        .child(div().text_size(px(20.)).child(t("tethered.title")))
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("tethered-refresh", t("common.refresh"))
                        .disabled(state.busy)
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_request("choose", cx))),
                )
                .child(
                    Button::new("tethered-capture", t("tethered.capture"))
                        .disabled(state.busy || state.model.is_none())
                        .on_click(cx.listener(|ws, _, _, cx| ws.tethered_request("capture", cx))),
                )
                .child(
                    Button::new("tethered-cancel", t("common.cancel"))
                        .disabled(!state.busy)
                        .on_click(cx.listener(|_, _, _, _| {
                            drop(web::tethered_request("cancel"));
                        })),
                )
                .child(
                    Button::new("tethered-close", t("common.close"))
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_tethered(cx))),
                ),
        )
        .child(state.model.clone().unwrap_or_default())
        .child(state.message.clone());
    if let Some(preview) = &state.preview {
        use gpui::StyledImage as _;
        body = body.child(
            gpui::img(preview.clone())
                .max_h(px(400.))
                .object_fit(gpui::ObjectFit::Contain),
        );
    }
    for (index, path) in state.paths.iter().enumerate() {
        let download = path.clone();
        let edit = path.clone();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        body = body.child(
            div()
                .flex()
                .gap_2()
                .child(name.clone())
                .child(
                    Button::new(("tethered-save", index), t("common.save_as")).on_click(
                        cx.listener(move |ws, _, _, cx| {
                            let result = web::read_file(&download)
                                .and_then(|bytes| web::download_bytes(&name, &bytes));
                            if let Err(error) = result {
                                ws.browser_tethered.message =
                                    schist_i18n::tf!("tethered.failed", detail = error);
                            }
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    Button::new(("tethered-edit", index), t("common.edit")).on_click(cx.listener(
                        move |ws, _, _, cx| {
                            ws.browser_tethered.open = false;
                            ws.cloud.show = false;
                            ws.load_file(edit.clone(), cx);
                        },
                    )),
                ),
        );
    }
    div()
        .id("tethered-web")
        .flex()
        .flex_col()
        .flex_grow()
        .overflow_y_scroll()
        .child(body)
        .into_any_element()
}
