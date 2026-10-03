//! Camera Raw's colour grading wheels.
//!
//! Each region's hue and saturation are one point on a colour wheel:
//! the angle is the hue (red to the right, turning anticlockwise as on
//! Lightroom's wheels) and the distance from the centre the saturation.
//! The luminance, blending and balance values stay ordinary sliders, and
//! the filter still declares hue and saturation as parameters, so
//! recorded actions and presets see plain numbers.

use super::*;
use crate::workspace::PickerDrag;
use gpui::{canvas, img, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, RenderImage};
use schist_filters_core::color_grading::WHEEL_KEYS;
use schist_i18n::t;
use smallvec::smallvec;
use std::cell::RefCell;
use std::sync::Arc;

const WHEEL: f32 = 72.0;
const WHEEL_IDS: [&str; 4] = [
    "grade-wheel-shadows",
    "grade-wheel-midtones",
    "grade-wheel-highlights",
    "grade-wheel-global",
];

/// Whether a Camera Raw value is drawn as a wheel rather than a slider.
pub(super) fn is_wheel_key(key: &str) -> bool {
    WHEEL_KEYS.iter().any(|k| k[0] == key || k[1] == key)
}

thread_local! {
    /// Built once: GPUI renders on one thread.
    static IMAGE: RefCell<Option<Arc<RenderImage>>> = const { RefCell::new(None) };
}

fn wheel_image() -> Arc<RenderImage> {
    IMAGE.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                let n = (WHEEL * 2.0) as usize;
                let mut bgra = vec![0u8; n * n * 4];
                for y in 0..n {
                    for x in 0..n {
                        let dx = (x as f32 + 0.5) / n as f32 - 0.5;
                        let dy = 0.5 - (y as f32 + 0.5) / n as f32;
                        let r = (dx.hypot(dy) * 2.0).min(1.0);
                        // A one-pixel soft edge instead of a jagged one.
                        let alpha = ((1.0 - dx.hypot(dy) * 2.0) * n as f32 / 2.0).clamp(0.0, 1.0);
                        let hue = dy.atan2(dx).to_degrees().rem_euclid(360.0) / 360.0;
                        let (cr, cg, cb) = crate::color_picker::hsv_to_rgb(hue, r * 0.85, 0.9);
                        let d = (y * n + x) * 4;
                        bgra[d] = (cb * 255.0).round() as u8;
                        bgra[d + 1] = (cg * 255.0).round() as u8;
                        bgra[d + 2] = (cr * 255.0).round() as u8;
                        bgra[d + 3] = (alpha * 255.0).round() as u8;
                    }
                }
                let buffer =
                    image::RgbaImage::from_raw(n as u32, n as u32, bgra).expect("sized above");
                Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]))
            })
            .clone()
    })
}

/// Set a wheel's hue and saturation from a pointer position and preview.
fn take_wheel(
    ws: &mut Workspace,
    id: &'static str,
    wheel: usize,
    at: gpui::Point<gpui::Pixels>,
    cx: &mut Context<Workspace>,
) {
    let Some((x, y)) = ws.box_position(WHEEL_IDS[wheel], at) else {
        return;
    };
    let (dx, dy) = (x - 0.5, y - 0.5);
    let saturation = ((dx.hypot(dy) * 2.0).min(1.0) * 100.0).round();
    let hue = dy.atan2(dx).to_degrees().rem_euclid(360.0).round() % 360.0;
    let [hue_key, sat_key, _] = WHEEL_KEYS[wheel];
    let mut next = None;
    ws.update_modal(|m| {
        if let Modal::Filter {
            values, preview, ..
        } = m
        {
            values.set(hue_key, hue);
            values.set(sat_key, saturation);
            if *preview {
                next = Some(values.clone());
            }
        }
    });
    if let Some(values) = next {
        ws.preview_filter(id, Some(&values), cx);
    }
    cx.notify();
}

fn wheel(
    ws: &mut Workspace,
    id: &'static str,
    index: usize,
    values: &schist_plugin_api::FilterValues,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let [hue_key, sat_key, _] = WHEEL_KEYS[index];
    let (hue, sat) = (
        values.get(hue_key).to_radians(),
        values.get(sat_key) / 100.0,
    );
    let (mx, my) = (
        (0.5 + 0.5 * sat * hue.cos()) * WHEEL,
        (0.5 - 0.5 * sat * hue.sin()) * WHEEL,
    );
    let entity = cx.entity();
    let caption = t([
        "filter.camera_raw.grading_shadows",
        "filter.camera_raw.grading_midtones",
        "filter.camera_raw.grading_highlights",
        "filter.camera_raw.grading_global",
    ][index]);
    let disc = div()
        .relative()
        .size(px(WHEEL))
        .flex_none()
        .child(img(wheel_image()).absolute().size_full())
        .child(
            div()
                .absolute()
                .left(px(mx - 5.0))
                .top(px(my - 5.0))
                .size(px(10.0))
                .rounded_full()
                .border_2()
                .border_color(gpui::rgb(0xFFFFFF))
                .child(
                    div()
                        .size_full()
                        .rounded_full()
                        .border_1()
                        .border_color(gpui::rgb(0x000000)),
                ),
        )
        .child(
            canvas(
                move |bounds, _window, cx| {
                    entity.update(cx, |ws, _| {
                        ws.record_slider_bounds(WHEEL_IDS[index], bounds)
                    });
                },
                |_, _: (), _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                ws.picker_drag = Some(PickerDrag::GradeWheel(index));
                take_wheel(ws, id, index, ev.position, cx);
            }),
        )
        .on_mouse_move(cx.listener(move |ws, ev: &MouseMoveEvent, _w, cx| {
            if ev.pressed_button == Some(MouseButton::Left)
                && ws.picker_drag == Some(PickerDrag::GradeWheel(index))
            {
                take_wheel(ws, id, index, ev.position, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _e: &MouseUpEvent, _w, _cx| ws.picker_drag = None),
        );
    let _ = ws;
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(disc)
        .child(div().text_size(px(11.0)).child(SharedString::from(caption)))
}

/// The heading and the four wheels, in a row.
pub(super) fn wheels(
    ws: &mut Workspace,
    id: &'static str,
    values: &schist_plugin_api::FilterValues,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut row = div().flex().flex_row().justify_between().py_1();
    for index in 0..WHEEL_KEYS.len() {
        row = row.child(wheel(ws, id, index, values, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pt_2()
        .child(
            div()
                .text_size(px(12.0))
                .child(SharedString::from(t("filter.camera_raw.grading_title"))),
        )
        .child(row)
}
