//! The history panel.

use super::*;
use schist_i18n::t;

pub(super) fn history_panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let (undo_entries, redo_entries): (Vec<String>, Vec<String>) = ws
        .doc
        .as_ref()
        .map(|d| {
            (
                d.history.entries().iter().map(|e| e.name.clone()).collect(),
                // Most-recently-undone first == next redo first.
                d.history
                    .redo_entries()
                    .iter()
                    .rev()
                    .map(|e| e.name.clone())
                    .collect(),
            )
        })
        .unwrap_or_default();
    let n_undo = undo_entries.len() as i32;
    #[cfg(not(target_arch = "wasm32"))]
    let versions = ws.version_history_original().map(|original| {
        schist_ui::Button::new("history-saved-versions", t("versions.open"))
            .on_click(cx.listener(move |ws, _ev, _window, cx| {
                ws.open_version_history(original.clone(), cx);
            }))
            .into_any_element()
    });
    #[cfg(target_arch = "wasm32")]
    let versions: Option<gpui::AnyElement> = None;

    div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        .overflow_hidden()
        .p_2()
        .gap_1()
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            div().flex().flex_row().items_center().justify_end().child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(icon_button("undo", "edit.undo", cx))
                    .child(icon_button("redo", "edit.redo", cx)),
            ),
        )
        .children(versions)
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|ws, ev: &MouseDownEvent, _w, cx| {
                ws.open_context_menu(ContextTarget::History, ev.position, cx);
            }),
        )
        .child(
            div()
                .id("history-scroll")
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .flex_grow()
                // The list gives way to the panel's chosen height and
                // scrolls inside it instead of painting over the status bar.
                .min_h(px(0.0))
                // The state the document opened in. The panel could walk
                // back to "one edit applied" but never to "none": the
                // topmost row still leaves the first edit in place, so
                // getting all the way back needed one more cmd-Z.
                // Photoshop's panel has this row too.
                .child({
                    let is_current = n_undo == 0;
                    let m = ui::metrics();
                    ListItem::new("history-opened")
                        .px_1()
                        .h(px(m.history_row_h))
                        .text_size(px(m.small_text))
                        .rounded_sm()
                        .selected(is_current)
                        .text_color(gpui::rgb(palette().text_dim))
                        .on_click(cx.listener(move |ws, _e, _w, cx| ws.history_jump(-n_undo, cx)))
                        .child(t("panel.history.opened"))
                })
                .children(undo_entries.into_iter().enumerate().map(|(i, name)| {
                    // Jump so entry i becomes the last applied edit.
                    let steps = (i as i32 + 1) - n_undo;
                    let is_current = i as i32 + 1 == n_undo;
                    let m = ui::metrics();
                    ListItem::new(("history-undo", i))
                        .px_1()
                        .h(px(m.history_row_h))
                        .text_size(px(m.small_text))
                        .rounded_sm()
                        .selected(is_current)
                        .on_click(cx.listener(move |ws, _e, _w, cx| ws.history_jump(steps, cx)))
                        .child(name)
                }))
                .children(redo_entries.into_iter().enumerate().map(|(j, name)| {
                    let steps = j as i32 + 1;
                    let m = ui::metrics();
                    ListItem::new(("history-redo", j))
                        .px_1()
                        .h(px(m.history_row_h))
                        .text_size(px(m.small_text))
                        .rounded_sm()
                        .text_color(gpui::rgb(palette().text_faint))
                        .on_click(cx.listener(move |ws, _e, _w, cx| ws.history_jump(steps, cx)))
                        .child(name)
                })),
        )
}

// ===== status bar =====
