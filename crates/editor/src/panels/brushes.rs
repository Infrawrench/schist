//! Brush dynamics and the user's saved recipes, available for every paint tool.
use super::*;
use schist_plugin_api::{BrushPreset, BrushTip};

// Availability here describes host routing, not attached tablet capabilities.
// The per-sample status below remains unavailable until real data arrives.
const PEN_TILT_HOST: bool = cfg!(any(
    target_arch = "wasm32",
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
));

const NAME: &str = "brush-preset-name";
const POPUP: Popup = Popup::Field("brush-settings");

pub(super) fn brush_controls(ws: &Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let open = ws.open_popup == Some(POPUP);
    div()
        .relative()
        .child(
            DropdownButton::new("brush-settings", t("dialog.new_doc.preset")).on_press(
                cx.listener(|ws, _e, _w, cx| {
                    ws.commit_focused_field();
                    ws.toggle_popup(POPUP, cx);
                }),
            ),
        )
        .children(open.then(|| deferred(brush_popover(ws, cx))))
        .into_any_element()
}

fn brush_popover(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let dynamics = ws.editor.brush_dynamics;
    let focused = ws.focused_field == Some(NAME);
    let name = if focused {
        ws.field_buffer.clone()
    } else {
        ws.brush_preset_name.clone()
    };
    let normalized = BrushPreset::capture(name.clone(), &ws.editor).name;
    let existing = ws
        .brush_library
        .presets
        .iter()
        .any(|p| p.name == normalized);
    let full = ws.brush_library.presets.len() >= schist_app_settings::brushes::MAX_PRESETS;
    let mut popup = Popover::new("brush-settings-popup")
        .top(px(ui::metrics().icon_button + 6.0))
        .left_0()
        .w(px(320.0))
        .max_h(px((ws.visible_height - 140.0).clamp(180.0, 650.0)))
        .track_scroll(&ws.brush_scroll)
        .p_3()
        .gap_2()
        .on_dismiss(cx.listener(|ws, _e, _w, cx| {
            ws.commit_focused_field();
            ws.close_popup(cx);
        }))
        .child(div().text_size(px(12.0)).child(t("filter.param.texture")))
        .child(
            div().flex().flex_wrap().gap_1().children(
                [
                    (BrushTip::Round, "common.none"),
                    (BrushTip::Grain, "filter.param.grain"),
                    (BrushTip::Bristles, "filter.oil_paint.param.bristle"),
                    (BrushTip::Bitmap, "common.bitmap"),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (tip, key))| {
                    Button::new(("brush-tip", i), t(key))
                        .active(dynamics.tip == tip)
                        .disabled(tip == BrushTip::Bitmap && ws.editor.brush_bitmap.is_none())
                        .on_click(cx.listener(move |ws, _e, _w, cx| {
                            ws.editor.brush_dynamics.tip = tip;
                            cx.notify();
                        }))
                }),
            ),
        );
    for (id, label, display, target) in [
        (
            "brush-spacing",
            t("common.spacing"),
            format!("{:.0}%", dynamics.spacing * 100.0),
            SliderTarget::BrushSpacing,
        ),
        (
            "brush-scatter",
            t("filter.spatter.name"),
            format!("{:.0}%", dynamics.scatter * 100.0),
            SliderTarget::BrushScatter,
        ),
        (
            "brush-smoothing",
            t("common.smoothing"),
            format!("{:.0}px", dynamics.stabilization),
            SliderTarget::BrushStabilization,
        ),
    ] {
        popup = popup.child(slider_stretch(id, label, display, target, ws, cx));
    }
    popup
        .child(div().text_size(px(12.0)).child(t("common.pressure")))
        .child(slider_stretch(
            "brush-pressure",
            t("common.size"),
            format!("γ{:.2}", dynamics.pressure_gamma),
            SliderTarget::BrushPressure,
            ws,
            cx,
        ))
        .child(
            Button::new("brush-pressure-opacity", t("common.opacity"))
                .active(dynamics.pressure_opacity)
                .on_click(cx.listener(|ws, _e, _w, cx| {
                    ws.editor.brush_dynamics.pressure_opacity =
                        !ws.editor.brush_dynamics.pressure_opacity;
                    cx.notify();
                })),
        )
        .child(div().text_size(px(12.0)).child(t("common.rotation")))
        .child(slider_stretch(
            "brush-rotation",
            t("common.angle"),
            format!("{:.0}°", dynamics.rotation),
            SliderTarget::BrushRotation,
            ws,
            cx,
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("brush-tilt", t("panels.brushes.pen_tilt"))
                        .active(dynamics.tilt_rotation && PEN_TILT_HOST)
                        .disabled(!PEN_TILT_HOST)
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            ws.editor.brush_dynamics.tilt_rotation =
                                !ws.editor.brush_dynamics.tilt_rotation;
                            cx.notify();
                        })),
                )
                .children(
                    (!PEN_TILT_HOST || ws.editor.pen_tilt.is_none())
                        .then(|| div().text_size(px(11.0)).child(t("common.not_available"))),
                ),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("brush-import", t("common.import"))
                        .on_click(cx.listener(|ws, _e, _w, cx| ws.import_brushes(cx))),
                )
                .child(
                    Button::new("brush-export", t("common.export"))
                        .disabled(ws.brush_library.presets.is_empty())
                        .on_click(cx.listener(|ws, _e, _w, cx| ws.export_brushes(cx))),
                ),
        )
        .child(div().text_size(px(12.0)).child(t("dialog.new_doc.preset")))
        .child(
            div()
                .id("brush-preset-list")
                .max_h(px(180.0))
                .overflow_y_scroll()
                .children(
                    ws.brush_library
                        .presets
                        .iter()
                        .enumerate()
                        .map(|(index, preset)| {
                            let preset = preset.clone();
                            ListItem::new(("brush-preset", index))
                                .selected(preset.name == ws.brush_preset_name)
                                .child(preset.name.clone())
                                .on_click(cx.listener(move |ws, _e, _w, cx| {
                                    ws.commit_focused_field();
                                    preset.apply(&mut ws.editor);
                                    ws.brush_preset_name = preset.name.clone();
                                    cx.notify();
                                }))
                        }),
                ),
        )
        .child(
            TextInput::new(NAME, name.clone())
                .active(focused)
                .cursor(if focused { ws.field_cursor } else { name.len() })
                .selection(if focused { ws.field_selection() } else { 0..0 })
                .caret_on(ws.caret_on())
                .placeholder(t("common.name"))
                .w_full()
                .on_focus(cx.listener(move |ws, press: &ui::TextPress, _w, cx| {
                    if ws.focused_field != Some(NAME) {
                        ws.commit_focused_field();
                    }
                    ws.press_field(NAME, name.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _w, cx| {
                    ws.drag_field(NAME, *offset);
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new(
                        "brush-preset-save",
                        t(if existing {
                            "common.update"
                        } else {
                            "common.save"
                        }),
                    )
                    .disabled(normalized.is_empty() || (full && !existing))
                    .on_click(cx.listener(|ws, _e, _w, cx| {
                        ws.commit_focused_field();
                        let mut library = ws.brush_library.clone();
                        if library.save_preset(BrushPreset::capture(
                            ws.brush_preset_name.clone(),
                            &ws.editor,
                        )) && library.save()
                        {
                            ws.brush_library = library;
                            ws.cloud_workflows_changed();
                        } else {
                            ws.status = t("common.failed").into();
                        }
                        cx.notify();
                    })),
                )
                .child(
                    Button::new("brush-preset-delete", t("common.delete"))
                        .disabled(!existing)
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            ws.commit_focused_field();
                            let mut library = ws.brush_library.clone();
                            library.delete(&ws.brush_preset_name);
                            if library.save() {
                                ws.brush_library = library;
                                ws.cloud_workflows_changed();
                                ws.brush_preset_name.clear();
                            } else {
                                ws.status = t("common.failed").into();
                            }
                            cx.notify();
                        })),
                ),
        )
}
