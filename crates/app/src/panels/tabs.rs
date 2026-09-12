//! The document tab strip.

use super::*;

/// Photoshop-style document tabs: one per open file, the active one lit,
/// a dot marking unsaved changes. Click to switch, middle-click or the ×
/// to close.
pub fn tab_bar(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let active = ws.active_tab();
    let tabs = ws.tab_strip();
    let m = ui::metrics();
    div()
        .flex()
        .flex_row()
        .items_end()
        .h(px(m.tab_h))
        .flex_none()
        .bg(gpui::rgb(palette().deep_bg))
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .overflow_hidden()
        .children(tabs.into_iter().enumerate().map(|(i, (title, dirty))| {
            let is_active = i == active;
            let label: SharedString = if dirty {
                format!("{title} •").into()
            } else {
                title
            };
            let select = cx.entity();
            let close = cx.entity();
            Tab::new(("doc-tab", i), label)
                .h(px(m.tab_h - 1.0))
                .text_size(px(m.small_text))
                .active(is_active)
                .on_select(move |_w, cx| select.update(cx, |ws, cx| ws.select_tab(i, cx)))
                .on_close(move |_w, cx| close.update(cx, |ws, cx| ws.request_close_tab(i, cx)))
        }))
}

// ===== tool options bar =====
