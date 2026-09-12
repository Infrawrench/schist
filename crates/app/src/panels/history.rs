//! The history panel.

use super::*;

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
    let touch = ui::touch();

    div()
        .flex()
        .flex_col()
        .h(px(ws.view.history_h))
        .flex_none()
        .p_2()
        .gap_1()
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        // On touch the panel's height is the user's: a grip above the
        // title drags it, taller or shorter, and the choice persists.
        .children(touch.then(|| resize_grip(ws, cx)))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(panel_title("History"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .child(icon_button("undo", "edit.undo", cx))
                        .child(icon_button("redo", "edit.redo", cx)),
                ),
        )
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
                // A floor (see the layers panel): the column scrolls. On
                // touch the panel is as tall as its grip was dragged, so
                // the list scrolls inside it rather than past it.
                .min_h(px(if touch { 0.0 } else { 120.0 }))
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
                        .child("Opened")
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

/// The smallest and largest the panel can be dragged to.
const MIN_HISTORY_H: f32 = 120.0;
const MAX_HISTORY_H: f32 = 500.0;

/// The grip along the panel's top edge. A press on it starts the
/// resize; the moves and the release are read at the window, since a
/// finger leaves a 14pt strip the moment it moves.
fn resize_grip(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let dragging = ws.history_resize.is_some();
    let entity = cx.entity();
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(14.0))
        .mt(px(-4.0))
        .cursor(gpui::CursorStyle::ResizeRow)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, ev: &MouseDownEvent, window, cx| {
                window.claim_touch_drag();
                ws.history_resize = Some((f32::from(ev.position.y), ws.view.history_h));
                cx.notify();
            }),
        )
        .child(
            div()
                .w(px(36.0))
                .h(px(4.0))
                .rounded_full()
                .bg(gpui::rgb(if dragging {
                    palette().accent
                } else {
                    palette().text_dim
                })),
        )
        .children(dragging.then(|| {
            canvas(
                |_, _, _| (),
                move |_, (), window, _| {
                    let move_entity = entity.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _w, cx| {
                        if phase != gpui::DispatchPhase::Capture {
                            return;
                        }
                        move_entity.update(cx, |ws, cx| {
                            let Some((start_y, start_h)) = ws.history_resize else {
                                return;
                            };
                            if ev.pressed_button != Some(MouseButton::Left) {
                                ws.history_resize = None;
                                ws.save_view_options();
                                return;
                            }
                            let h = start_h + (start_y - f32::from(ev.position.y));
                            ws.view.history_h = h.clamp(MIN_HISTORY_H, MAX_HISTORY_H);
                            cx.notify();
                        });
                    });
                    let up_entity = entity.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _w, cx| {
                        if phase != gpui::DispatchPhase::Capture || ev.button != MouseButton::Left {
                            return;
                        }
                        up_entity.update(cx, |ws, cx| {
                            if ws.history_resize.take().is_some() {
                                ws.save_view_options();
                                cx.notify();
                            }
                        });
                    });
                },
            )
            .absolute()
            .size_0()
        }))
}
