//! The Timeline panel: Photoshop-style frame animation.
//!
//! Frames as thumbnails, click to select, drag to reorder; transport
//! (first, previous, play/pause, next, last); the selected frame's delay;
//! the loop count; onion skins; and the active layer's offset in the
//! selected frame. Shown only for a document with a frame animation --
//! Layer ▸ Animation creates one.

use super::*;
use crate::workspace::animation::{delay_label, loop_label, Onion, DELAYS_MS, LOOPS};
use schist_i18n::t;

#[derive(Clone)]
struct FrameDrag {
    index: usize,
}

struct FrameDragPreview(usize);

impl gpui::Render for FrameDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(gpui::rgb(palette().accent))
            .text_color(gpui::rgb(palette().accent_text))
            .child(format!("{}", self.0 + 1))
    }
}

fn transport_button(
    id: &'static str,
    icon_name: &'static str,
    on_click: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let m = ui::metrics();
    IconButton::new(id, icon_name)
        .size(m.icon_button)
        .icon_size(m.icon_button_icon)
        .on_click(cx.listener(move |ws, _e, _w, cx| on_click(ws, cx)))
}

fn small_label(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(ui::metrics().small_text))
        .text_color(gpui::rgb(palette().text_dim))
        .child(text.into())
}

pub(super) fn timeline_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    let doc = ws.doc.as_ref()?;
    let timeline = doc.timeline.as_ref()?.synced(&doc.tree);
    let active_offset = doc.active_layer.map(|id| (id, timeline.current_offset(id)));
    let thumbs = ws.frame_thumbnails();
    if std::mem::take(&mut ws.anim.thumbs_pending) {
        cx.spawn(async move |this, cx| {
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }
    let playing = ws.anim.playing.as_ref().map(|p| p.frame);
    let shown = playing.unwrap_or(timeline.current);
    let current = timeline.current;
    let onion = ws.anim.onion;

    let transport = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(transport_button(
            "anim-first",
            "skip-back",
            |ws, cx| ws.run_command("animation.first_frame", cx),
            cx,
        ))
        .child(transport_button(
            "anim-previous",
            "step-back",
            |ws, cx| ws.run_command("animation.previous_frame", cx),
            cx,
        ))
        .child(transport_button(
            "anim-play",
            if playing.is_some() { "pause" } else { "play" },
            |ws, cx| ws.toggle_playback(cx),
            cx,
        ))
        .child(transport_button(
            "anim-next",
            "step-forward",
            |ws, cx| ws.run_command("animation.next_frame", cx),
            cx,
        ))
        .child(transport_button(
            "anim-last",
            "skip-forward",
            |ws, cx| ws.run_command("animation.last_frame", cx),
            cx,
        ))
        .child(div().flex_grow())
        .child(icon_button("plus", "animation.new_frame", cx))
        .child(icon_button("trash", "animation.delete_frame", cx));

    let frames = div()
        .id("anim-frames")
        .flex()
        .flex_row()
        .gap_1()
        .overflow_x_scroll()
        .pb_1()
        .children(timeline.frames.iter().enumerate().map(|(i, frame)| {
            let thumb = thumbs.get(i).cloned().flatten();
            let selected = i == current;
            div()
                .id(("anim-frame", i))
                .flex()
                .flex_col()
                .items_center()
                .flex_none()
                .w(px(72.0))
                .p_1()
                .rounded_sm()
                .border_1()
                .border_color(gpui::rgb(if i == shown {
                    palette().accent
                } else {
                    palette().divider
                }))
                .when(selected, |d| d.bg(gpui::rgb(palette().control_bg)))
                .cursor(gpui::CursorStyle::PointingHand)
                .on_click(cx.listener(move |ws, _e, _w, cx| ws.select_frame(i, cx)))
                .on_drag(FrameDrag { index: i }, |drag, _, _, cx| {
                    cx.new(|_| FrameDragPreview(drag.index))
                })
                .drag_over::<FrameDrag>(|style, _, _, _| {
                    style.border_color(gpui::rgb(palette().accent))
                })
                .on_drop(cx.listener(move |ws, drag: &FrameDrag, _w, cx| {
                    ws.move_frame(drag.index, i, cx);
                }))
                .child(
                    div()
                        .w(px(64.0))
                        .h(px(64.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui::rgb(palette().field_bg))
                        .children(thumb.map(|t| img(t).max_w(px(64.0)).max_h(px(64.0)))),
                )
                .child(
                    div()
                        .text_size(px(ui::metrics().small_text))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(format!("{}", i + 1)),
                )
                .child(
                    div()
                        .text_size(px(ui::metrics().small_text))
                        .child(delay_label(frame.delay_ms)),
                )
        }));

    let delay_ms = timeline.frames[current].delay_ms;
    let mut delays: Vec<(SharedString, u32)> = DELAYS_MS
        .iter()
        .map(|&ms| (SharedString::from(delay_label(ms)), ms))
        .collect();
    if !DELAYS_MS.contains(&delay_ms) {
        delays.push((delay_label(delay_ms).into(), delay_ms));
    }
    let delay = ui::dropdown(
        &ws.dropdown,
        ui::Dropdown {
            popup: Popup::Field("anim-delay"),
            is_open: ws.open_popup == Some(Popup::Field("anim-delay")),
            current: delay_ms,
            label: delay_label(delay_ms).into(),
            width: 90.0,
            options: delays,
        },
        |ws, ms, cx| ws.set_frame_delay(false, ms, cx),
        cx,
    );
    let plays = timeline.loop_count.plays().unwrap_or(0);
    let mut loops: Vec<(SharedString, u32)> = LOOPS
        .iter()
        .map(|&n| (SharedString::from(loop_label(n)), n))
        .collect();
    if !LOOPS.contains(&plays) {
        loops.insert(loops.len() - 1, (loop_label(plays).into(), plays));
    }
    let looping = ui::dropdown(
        &ws.dropdown,
        ui::Dropdown {
            popup: Popup::Field("anim-loop"),
            is_open: ws.open_popup == Some(Popup::Field("anim-loop")),
            current: plays,
            label: loop_label(plays).into(),
            width: 90.0,
            options: loops,
        },
        |ws, plays, cx| ws.set_frame_loop(plays, cx),
        cx,
    );
    let timing = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(small_label(t("animation.delay.label")))
        .child(delay)
        .child(ui::button(
            t("animation.delay.apply_all"),
            false,
            move |ws, _w, cx| ws.set_frame_delay(true, delay_ms, cx),
            cx,
        ))
        .child(small_label(t("animation.loop.label")))
        .child(looping);

    let counts: Vec<(SharedString, u8)> = (0..=3u8)
        .map(|n| (SharedString::from(n.to_string()), n))
        .collect();
    let opacities: Vec<(SharedString, u8)> = [15u8, 25, 35, 50, 75]
        .iter()
        .map(|&p| (SharedString::from(format!("{p}%")), p))
        .collect();
    let opacity_pct = (onion.opacity * 100.0).round() as u8;
    let onion_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(ui::checkbox(
            t("animation.onion.label"),
            onion.enabled,
            |ws, cx| ws.toggle_onion(cx),
            cx,
        ))
        .child(small_label(t("animation.onion.before")))
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("anim-onion-before"),
                is_open: ws.open_popup == Some(Popup::Field("anim-onion-before")),
                current: onion.before,
                label: onion.before.to_string().into(),
                width: 44.0,
                options: counts.clone(),
            },
            move |ws, n, cx| {
                ws.set_onion(
                    Onion {
                        before: n,
                        enabled: true,
                        ..ws.anim.onion
                    },
                    cx,
                )
            },
            cx,
        ))
        .child(small_label(t("animation.onion.after")))
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("anim-onion-after"),
                is_open: ws.open_popup == Some(Popup::Field("anim-onion-after")),
                current: onion.after,
                label: onion.after.to_string().into(),
                width: 44.0,
                options: counts,
            },
            move |ws, n, cx| {
                ws.set_onion(
                    Onion {
                        after: n,
                        enabled: true,
                        ..ws.anim.onion
                    },
                    cx,
                )
            },
            cx,
        ))
        .child(small_label(t("common.opacity")))
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("anim-onion-opacity"),
                is_open: ws.open_popup == Some(Popup::Field("anim-onion-opacity")),
                current: opacity_pct,
                label: format!("{opacity_pct}%").into(),
                width: 60.0,
                options: opacities,
            },
            move |ws, p, cx| {
                ws.set_onion(
                    Onion {
                        opacity: p as f32 / 100.0,
                        enabled: true,
                        ..ws.anim.onion
                    },
                    cx,
                )
            },
            cx,
        ));

    let offset_row = active_offset.map(|(_, (dx, dy))| {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(small_label(t("animation.offset.label")))
            .child(small_label(format!("{dx}, {dy}")))
            .children(
                [
                    ("anim-left", "←", (-1, 0)),
                    ("anim-right", "→", (1, 0)),
                    ("anim-up", "↑", (0, -1)),
                    ("anim-down", "↓", (0, 1)),
                ]
                .map(|(id, glyph, by)| {
                    note_button(
                        id,
                        glyph,
                        true,
                        move |ws, cx| ws.nudge_frame_offset(Some(by), cx),
                        cx,
                    )
                }),
            )
            .child(ui::button(
                t("common.reset"),
                false,
                |ws, _w, cx| ws.nudge_frame_offset(None, cx),
                cx,
            ))
    });

    let new_layers = ui::checkbox(
        t("animation.new_layers_visible"),
        timeline.new_layers_visible,
        |ws, cx| ws.toggle_new_layers_visible(cx),
        cx,
    );

    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(transport)
            .child(frames)
            .child(timing)
            .child(onion_row)
            .children(offset_row)
            .child(new_layers)
            .into_any_element(),
    )
}
