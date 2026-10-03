//! Aperture-inspired photo workspace: a left inspector, viewer and browser.
use super::*;
use gpui::{point, size, Bounds, Focusable as _, Pixels, StyledImage as _};
use schist_app_settings::workspaces::{PhotoDisplay, PhotoInspector};
use schist_core::AdjustmentKind;
use schist_i18n::tf;

pub(crate) const VIEWER_BG: u32 = 0x383838;
const BROWSER_BG: u32 = 0x505050;

pub(crate) fn workspace(
    ws: &mut Workspace,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let display = ws.view.photo_layout.display;
    let panels = ws.side_panels_shown(window);
    let panels_page = panels && ui::compact(window);
    let mut body = div().flex().flex_1().min_h_0().min_w_0();
    if panels {
        body = body.child(inspector(ws, cx).when(panels_page, |d| d.w_full()));
    }
    if !panels_page {
        let mut viewer = div().flex().flex_col().flex_1().min_w_0().min_h_0();
        if display != PhotoDisplay::Browser {
            viewer = viewer
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .child(ws.render_canvas(cx))
                        .children(ws.view.rulers.then(|| super::rulers(ws, cx))),
                )
                .child(viewer_controls(ws, cx));
        }
        if display != PhotoDisplay::Viewer {
            viewer = viewer.child(browser(ws, display == PhotoDisplay::Browser, cx));
        }
        body = body.child(viewer).children(super::ai_sidebar(ws, cx));
    }
    div()
        .id("photo-workspace")
        .when(display == PhotoDisplay::Browser || panels_page, |d| {
            d.track_focus(&ws.focus_handle(cx))
        })
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(toolbar(ws, panels, cx))
        .child(body)
        .into_any_element()
}

fn toolbar(ws: &Workspace, panels: bool, cx: &mut Context<Workspace>) -> impl IntoElement {
    let display = ws.view.photo_layout.display;
    let modes = [
        (PhotoDisplay::Browser, t("menu.view.grid").to_owned()),
        (
            PhotoDisplay::Split,
            format!("{} + {}", t("common.photos"), t("common.preview")),
        ),
        (PhotoDisplay::Viewer, t("common.preview").to_owned()),
    ];
    div()
        .flex()
        .items_center()
        .flex_wrap()
        .gap_2()
        .px_3()
        .py_2()
        .flex_none()
        .bg(gpui::rgb(palette().control_bg))
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            IconButton::new("photo-inspector", "adjust")
                .active(panels)
                .tooltip(t("workspaces.show_panels"), None)
                .on_click(cx.listener(|ws, _, window, cx| ws.toggle_side_panels(window, cx))),
        )
        .child(Button::new("photo-open", t("menu.file.open")).on_click(
            cx.listener(|ws, _, window, cx| super::run_app_item(ws, AppItem::Open, window, cx)),
        ))
        .child(
            Button::new("photo-export", t("menu.file.export"))
                .disabled(ws.doc.is_none())
                .on_click(cx.listener(|ws, _, window, cx| {
                    super::run_app_item(ws, AppItem::Export, window, cx)
                })),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .gap_px()
                .children(modes.into_iter().enumerate().map(|(i, (mode, label))| {
                    Button::new(("photo-display", i), label)
                        .active(mode == display)
                        .on_click(cx.listener(move |ws, _, _, cx| ws.set_photo_display(mode, cx)))
                })),
        )
}

fn viewer_controls(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tools: Vec<_> = ["hand", "crop", "zoom"]
        .into_iter()
        .filter_map(|id| {
            ws.registry
                .tools()
                .find(|tool| tool.id() == id)
                .map(|tool| (id, tool.name()))
        })
        .collect();
    div()
        .flex()
        .items_center()
        .flex_wrap()
        .gap_1()
        .px_2()
        .py_1()
        .flex_none()
        .bg(gpui::rgb(palette().control_bg))
        .border_t_1()
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .children(tools.into_iter().map(|(id, label)| {
            IconButton::new(SharedString::from(format!("photo-tool-{id}")), id)
                .active(ws.editor.active_tool == id)
                .tooltip(label, None)
                .on_click(cx.listener(move |ws, _, _, cx| ws.activate_tool(id, cx)))
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.0))
                .px_2()
                .child(ws.doc.as_ref().map(|d| d.title.clone()).unwrap_or_default()),
        )
        .child(
            Button::new("photo-fit", t("menu.view.fit_on_screen"))
                .disabled(ws.doc.is_none())
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.fit_to_view();
                    cx.notify();
                })),
        )
        .child(
            Button::new("photo-actual", t("menu.view.actual_size"))
                .disabled(ws.doc.is_none())
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.set_zoom(1.0);
                    cx.notify();
                })),
        )
        .child(
            div()
                .w(px(46.0))
                .text_size(px(11.0))
                .text_right()
                .child(format!("{:.0}%", ws.viewport_zoom() * 100.0)),
        )
}

fn inspector(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::Stateful<gpui::Div> {
    let active = ws.view.photo_layout.inspector;
    let tabs = [
        (PhotoInspector::Library, "common.photos"),
        (PhotoInspector::Info, "panel.info.title"),
        (PhotoInspector::Adjustments, "menu.image.adjustments"),
    ];
    let body = match active {
        PhotoInspector::Library => library(ws, cx),
        PhotoInspector::Info => metadata(ws, cx),
        PhotoInspector::Adjustments => adjustments(ws, cx),
    };
    div()
        .id("photo-inspector-panel")
        .flex()
        .flex_col()
        .flex_none()
        .min_h_0()
        .w(px(ws.view.panel_width.unwrap_or(300.0)))
        .bg(gpui::rgb(palette().panel_bg))
        .border_r_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            div()
                .flex()
                .flex_wrap()
                .flex_none()
                .bg(gpui::rgb(palette().control_bg))
                .children(tabs.into_iter().enumerate().map(|(i, (tab, label))| {
                    div()
                        .id(("photo-inspector-tab", i))
                        .flex_1()
                        .px_2()
                        .py_2()
                        .text_size(px(11.0))
                        .text_center()
                        .cursor_pointer()
                        .border_b_2()
                        .border_color(gpui::rgb(if tab == active {
                            palette().accent
                        } else {
                            palette().panel_edge
                        }))
                        .when(tab == active, |d| d.bg(gpui::rgb(palette().panel_bg)))
                        .hover(|d| d.bg(gpui::rgb(palette().hover)))
                        .child(t(label))
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.commit_focused_field();
                            let mut next = ws.view.clone();
                            next.photo_layout.inspector = tab;
                            ws.commit_workspace_view(next, cx);
                        }))
                })),
        )
        .child(
            div()
                .id("photo-inspector-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        )
}

fn section(label: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_2()
        .bg(gpui::rgb(palette().control_bg))
        .border_t_1()
        .border_b_1()
        .border_color(gpui::rgb(palette().divider))
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(icon("chevron-down", 10.0, palette().text_dim))
        .child(label)
}

fn library(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .child(div().p_2().child(
            Button::new("photo-gallery", t("menu.file.browse_gallery")).on_click(cx.listener(
                |ws, _, window, cx| {
                    super::run_app_item(
                        ws,
                        if cfg!(target_arch = "wasm32") {
                            AppItem::CloudBrowse
                        } else {
                            AppItem::OpenGallery
                        },
                        window,
                        cx,
                    )
                },
            )),
        ))
        .child(section(t("library.sidebar.buckets")))
        .children(bucket_rows(ws, cx))
        .child(section(t("workspaces.open_documents")))
        .children(
            ws.tab_strip()
                .into_iter()
                .enumerate()
                .map(|(i, (title, dirty))| {
                    div()
                        .id(("photo-library-document", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .text_size(px(12.0))
                        .cursor_pointer()
                        .when(i == ws.active_tab(), |d| {
                            d.bg(gpui::rgb(palette().selection_bg))
                        })
                        .hover(|d| d.bg(gpui::rgb(palette().hover)))
                        .child(icon("navigator", 14.0, palette().text_dim))
                        .child(div().min_w_0().truncate().child(if dirty {
                            tf!("panel.tabs.dirty", title = title)
                        } else {
                            title.to_string()
                        }))
                        .on_click(cx.listener(move |ws, _, _, cx| ws.select_tab(i, cx)))
                }),
        )
        .into_any_element()
}

fn bucket_rows(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    let mut rows = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    for (index, bucket) in ws.library.buckets.iter().enumerate() {
        let count = bucket
            .contents(|path| ws.library.is_flagged(path))
            .iter()
            .filter(|path| !(ws.view.gallery_hide_nsfw && ws.library.is_flagged(path)))
            .count();
        let name = if bucket.is_smart() {
            format!("\u{2726} {}", bucket.name)
        } else {
            bucket.name.clone()
        };
        rows.push(
            div()
                .id(("photo-bucket", index))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .text_size(px(12.0))
                .cursor_pointer()
                .hover(|d| d.bg(gpui::rgb(palette().hover)))
                .child(icon("folder", 14.0, palette().text_dim))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(count.to_string()),
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_photo_bucket(index, cx)))
                .into_any_element(),
        );
    }
    // Cloud rows already implement bucket browsing, pagination and smart markers.
    rows.extend(crate::workspace::cloud_view::bucket_rows(ws, cx));
    rows
}

fn metadata(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    ws.refresh_exif();
    let exif = ws.exif.as_ref().and_then(|(_, exif)| exif.clone());
    let mut body = div().flex().flex_col().child(super::navigator(ws, cx));
    if let Some(doc) = ws.doc.as_ref() {
        body = body
            .child(div().p_3().text_size(px(12.0)).child(doc.title.clone()))
            .child(div().px_3().pb_3().text_size(px(11.0)).child(tf!(
                "panel.info.size",
                w = doc.width,
                h = doc.height,
                mp = format!("{:.1}", doc.width as f64 * doc.height as f64 / 1_000_000.0)
            )));
    }
    if let Some(exif) = exif {
        body = body.child(super::info::info_panel(ws, &exif, cx));
    }
    body.into_any_element()
}

fn adjustments(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let histogram = histogram(ws);
    let choices = [
        AdjustmentKind::Exposure,
        AdjustmentKind::Levels,
        AdjustmentKind::Curves,
        AdjustmentKind::ColorBalance,
        AdjustmentKind::BrightnessContrast,
        AdjustmentKind::Vibrance,
        AdjustmentKind::HueSaturation,
        AdjustmentKind::BlackWhite,
    ];
    div()
        .flex()
        .flex_col()
        .child(histogram)
        .child(section(t("menu.image.adjustments")))
        .child(
            div()
                .flex()
                .flex_col()
                .p_2()
                .gap_1()
                .children(choices.into_iter().enumerate().map(|(i, kind)| {
                    Button::new(("photo-add-adjustment", i), ui::adjustment_name(kind))
                        .w_full()
                        .disabled(ws.doc.is_none())
                        .on_click(cx.listener(move |ws, _, _, cx| ws.add_adjustment(kind, cx)))
                })),
        )
        .when(
            !ws.view.hidden_panels.iter().any(|key| key == "layers"),
            |d| {
                d.child(section(t("common.layers")))
                    .child(super::layers_panel(ws, cx))
            },
        )
        .when(
            !ws.view.hidden_panels.iter().any(|key| key == "history"),
            |d| {
                d.child(section(t("panel.history.title")))
                    .child(super::history_panel(ws, cx))
            },
        )
        .into_any_element()
}

fn histogram(ws: &mut Workspace) -> gpui::AnyElement {
    let mut channels = [[0f32; 64]; 3];
    if let Some(image) = ws.document_thumbnail() {
        if let Some(bytes) = image.as_bytes(0) {
            for pixel in bytes.as_chunks::<4>().0 {
                for channel in 0..3 {
                    // RenderImage stores BGRA.
                    channels[channel][pixel[2 - channel] as usize / 4] += 1.0;
                }
            }
        }
    }
    let peak = channels.iter().flatten().copied().fold(1.0, f32::max);
    let dimensions = ws
        .doc
        .as_ref()
        .map(|doc| tf!("common.dimensions", w = doc.width, h = doc.height))
        .unwrap_or_default();
    div()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(palette().text_dim))
                .child(t("common.rgb")),
        )
        .child(
            div()
                .h(px(100.0))
                .w_full()
                .bg(gpui::rgb(VIEWER_BG))
                .rounded_sm()
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds: Bounds<Pixels>, _, window, _| {
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            for i in 1..4 {
                                window.paint_quad(gpui::fill(
                                    Bounds {
                                        origin: bounds.origin
                                            + point(px(w * i as f32 / 4.0), px(0.0)),
                                        size: size(px(1.0), px(h)),
                                    },
                                    gpui::rgba(0xFFFFFF18),
                                ));
                            }
                            for (channel, color) in
                                channels.iter().zip([0xF06A6A88, 0x80CF8A88, 0x7D9FF088])
                            {
                                for (i, value) in channel.iter().enumerate() {
                                    let bar = value / peak * (h - 6.0);
                                    window.paint_quad(gpui::fill(
                                        Bounds {
                                            origin: bounds.origin
                                                + point(px(w * i as f32 / 64.0), px(h - bar)),
                                            size: size(px((w / 64.0).max(1.0)), px(bar)),
                                        },
                                        gpui::rgba(color),
                                    ));
                                }
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_center()
                .text_color(gpui::rgb(palette().text_dim))
                .child(dimensions),
        )
        .into_any_element()
}

fn browser(ws: &mut Workspace, grid: bool, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let thumbnails = ws.photo_thumbnails();
    let tabs = ws.tab_strip();
    let active = ws.active_tab();
    let count = crate::workspace::gallery_chrome::photo_count(tabs.len());
    if ws.photo_view.reveal_selection {
        ws.photo_view.scroll.scroll_to_item(active);
        ws.photo_view.reveal_selection = false;
    }
    let mut content = div()
        .id(if grid {
            "photo-grid"
        } else {
            "photo-filmstrip"
        })
        .track_scroll(&ws.photo_view.scroll)
        .flex()
        .gap_3()
        .p_3()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .when(grid, |d| d.flex_wrap().content_start().overflow_y_scroll())
        .when(!grid, |d| d.overflow_x_scroll());
    for (i, ((title, dirty), image)) in tabs.into_iter().zip(thumbnails).enumerate() {
        let label = if dirty {
            tf!("panel.tabs.dirty", title = title)
        } else {
            title.to_string()
        };
        content = content.child(
            div()
                .id(("photo-thumbnail", i))
                .flex()
                .flex_col()
                .flex_none()
                .w(px(152.0))
                .h(px(118.0))
                .p_1()
                .gap_1()
                .rounded_sm()
                .cursor_pointer()
                .border_2()
                .border_color(gpui::rgb(if i == active { 0xD8D8D8 } else { BROWSER_BG }))
                .when(i == active, |d| d.bg(gpui::rgb(0x686868)))
                .hover(|d| d.bg(gpui::rgb(0x626262)))
                .tooltip(ui::tip(label.clone(), None))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(84.0))
                        .w_full()
                        .children(image.map(|image| {
                            img(image)
                                .max_w(px(140.0))
                                .max_h(px(84.0))
                                .object_fit(gpui::ObjectFit::Contain)
                        })),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(0xEEEEEE))
                        .truncate()
                        .child(label),
                )
                .on_click(cx.listener(move |ws, ev: &gpui::ClickEvent, _, cx| {
                    ws.select_tab(i, cx);
                    if ev.click_count() == 2 {
                        ws.set_photo_display(PhotoDisplay::Split, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |ws, _, _, cx| ws.request_close_tab(i, cx)),
                ),
        );
    }
    if ws.tab_count() == 0 {
        content = content.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .text_color(gpui::rgb(0xDDDDDD))
                .child(t("common.no_document_open"))
                .child(
                    Button::new("photo-empty-open", t("menu.file.open")).on_click(cx.listener(
                        |ws, _, window, cx| super::run_app_item(ws, AppItem::Open, window, cx),
                    )),
                ),
        );
    }
    div()
        .flex()
        .flex_col()
        .min_h_0()
        .min_w_0()
        .bg(gpui::rgb(BROWSER_BG))
        .when(grid, |d| d.flex_1())
        .when(!grid, |d| d.h(px(180.0)).flex_none())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .h(px(28.0))
                .flex_none()
                .bg(gpui::rgb(palette().control_bg))
                .text_size(px(11.0))
                .child(t("workspaces.open_documents"))
                .child(count),
        )
        .child(content)
        .into_any_element()
}
