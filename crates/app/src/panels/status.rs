//! The status bar along the bottom of the window.

use super::*;
use schist_i18n::{t, tf};

pub fn status_bar(ws: &Workspace) -> impl IntoElement {
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
        .child(title)
        .child(zoom)
        .child(brush)
        .child(div().flex_grow())
        .child(ws.status.clone())
}

// ===== context menus =====
