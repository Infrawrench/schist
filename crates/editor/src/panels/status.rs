//! The status bar along the bottom of the window.

use super::*;
use schist_i18n::{t, tf};

pub fn support_link(cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id("support-schist")
        .flex()
        .items_center()
        .gap_1()
        .flex_none()
        .cursor_pointer()
        .text_size(px(ui::metrics().small_text))
        .text_color(gpui::rgb(palette().accent))
        .hover(|style| style.text_color(gpui::rgb(palette().accent_hover)))
        // The browser fonts do not contain emoji. Draw the purple heart
        // as an icon so the support control looks the same on every target.
        .child(schist_ui::icon("heart", 14.0, 0xA855F7))
        .child(t("dialog.support.title"))
        .on_click(cx.listener(|ws, _, _, cx| ws.open_modal(Modal::Support, cx)))
}

pub fn status_bar(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let title = ws
        .doc
        .as_ref()
        .map(|d| {
            if d.dirty {
                tf!(
                    "panel.status.document_dirty",
                    title = d.title,
                    w = d.width,
                    h = d.height
                )
            } else {
                tf!(
                    "panel.status.document",
                    title = d.title,
                    w = d.width,
                    h = d.height
                )
            }
        })
        .unwrap_or_else(|| t("common.no_document").to_string());
    let zoom = format!("{:.0}%", ws.zoom * 100.0);
    let brush = format!("{:.0}px", ws.editor.brush_size);
    let m = ui::metrics();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .h(px(m.status_h))
        .flex_none()
        .px_2()
        .bg(gpui::rgb(palette().status_bg))
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .text_size(px(m.small_text))
        .text_color(gpui::rgb(palette().text_dim))
        .child(div().min_w(px(0.0)).truncate().child(title))
        .child(zoom)
        .child(brush)
        .child(if ws.action_recorder.recording {
            t("actions.recording_indicator")
        } else {
            ""
        })
        .child(div().flex_grow())
        .child(div().min_w(px(0.0)).truncate().child(ws.status.clone()))
        .child(support_link(cx))
}

// ===== context menus =====
