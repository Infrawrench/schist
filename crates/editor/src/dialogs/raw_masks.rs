//! The Camera Raw dialog's Masks section: local adjustments for a RAW
//! development, listed under the global sliders.

use super::*;
use crate::workspace::raw_masks::NewShape;
use schist_core::raw_masks::LOCAL_CONTROLS;
use schist_core::{MaskCombine, MaskShape};
use schist_i18n::{t, tf};

/// A small button keyed by its own id rather than its label, since the
/// same labels (Brush, Select Sky, …) appear in more than one row.
fn chip(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    on_click: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    schist_ui::Button::new(id, label)
        .active(active)
        .on_click(cx.listener(move |ws, _e, _window, cx| on_click(ws, cx)))
}

fn row() -> gpui::Div {
    div().flex().flex_row().flex_wrap().items_center().gap_1()
}

fn note(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .text_color(gpui::rgb(ui::palette().text_dim))
        .child(text.into())
}

/// The label of a local control: the global slider's own where there is
/// one, so a mask's Exposure reads exactly like the dialog's.
fn control_label(key: &str) -> &'static str {
    t(match key {
        "exposure" => "common.exposure",
        "contrast" => "common.contrast",
        "saturation" => "common.saturation",
        "sharpness" => "filter.param.sharpness",
        "temperature" => "filter.camera_raw.param.temperature",
        "tint" => "filter.camera_raw.param.tint",
        "highlights" => "filter.camera_raw.param.highlights",
        "shadows" => "filter.camera_raw.param.shadows",
        "clarity" => "filter.camera_raw.param.clarity",
        _ => "filter.camera_raw.param.dehaze",
    })
}

/// Slider ids must not collide with the global sliders of the same name.
fn control_id(key: &str) -> &'static str {
    match key {
        "exposure" => "raw_mask.exposure",
        "contrast" => "raw_mask.contrast",
        "saturation" => "raw_mask.saturation",
        "sharpness" => "raw_mask.sharpness",
        "temperature" => "raw_mask.temperature",
        "tint" => "raw_mask.tint",
        "highlights" => "raw_mask.highlights",
        "shadows" => "raw_mask.shadows",
        "clarity" => "raw_mask.clarity",
        _ => "raw_mask.dehaze",
    }
}

pub(super) fn raw_mask_controls(
    mut body: gpui::Stateful<gpui::Div>,
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<gpui::Div> {
    let Some(editor) = ws.raw_masks.as_ref() else {
        return body;
    };
    body = body.child(
        div()
            .pt_2()
            .text_size(px(12.0))
            .child(t("dialog.raw_masks.title")),
    );

    // New masks.
    let mut new = row().child(
        div()
            .text_size(px(11.0))
            .child(t("dialog.raw_masks.new_mask")),
    );
    for (i, shape) in NewShape::ALL.into_iter().enumerate() {
        new = new.child(chip(
            ("raw-mask-new", i),
            shape.label(),
            false,
            move |ws, cx| ws.add_raw_mask(shape, false, cx),
            cx,
        ));
    }
    body = body.child(new);

    if editor.masks.is_empty() {
        return body.child(note(t("dialog.raw_masks.hint_empty")));
    }

    // The masks, one chip each.
    let mut list = row();
    for i in 0..editor.masks.len() {
        list = list.child(chip(
            ("raw-mask", i),
            tf!("dialog.raw_masks.mask_n", n = i + 1),
            editor.selected == Some(i),
            move |ws, cx| ws.select_raw_mask(i, cx),
            cx,
        ));
    }
    body = body.child(list);

    let Some(mask) = editor.mask().cloned() else {
        return body;
    };
    let selected_component = editor.component;
    let brush = editor.brush;
    let overlay = editor.overlay;
    let detecting = editor.detecting;

    // Components: select one to edit it; the selected one shows how it
    // combines with those above it.
    for (i, component) in mask.components.iter().enumerate() {
        let selected = i == selected_component;
        let mut line = row().child(chip(
            ("raw-mask-component", i),
            NewShape::of(&component.shape).label(),
            selected,
            move |ws, cx| ws.select_raw_mask_component(i, cx),
            cx,
        ));
        if selected {
            if i > 0 {
                for (j, (combine, label)) in [
                    (MaskCombine::Add, t("common.add")),
                    (MaskCombine::Subtract, t("tool.select.mode.subtract")),
                    (MaskCombine::Intersect, t("tool.select.mode.intersect")),
                ]
                .into_iter()
                .enumerate()
                {
                    line = line.child(chip(
                        ("raw-mask-combine", j),
                        label,
                        component.combine == combine,
                        move |ws, cx| ws.set_raw_mask_combine(i, combine, cx),
                        cx,
                    ));
                }
            }
            line = line
                .child(chip(
                    "raw-mask-component-invert",
                    t("common.invert"),
                    component.invert,
                    move |ws, cx| ws.edit_raw_mask_component(i, cx, |c| c.invert = !c.invert),
                    cx,
                ))
                .child(chip(
                    "raw-mask-component-remove",
                    t("common.remove"),
                    false,
                    move |ws, cx| ws.remove_raw_mask_component(i, cx),
                    cx,
                ));
        }
        body = body.child(line);
    }

    // Add to the selected mask.
    let mut add = row().child(
        div()
            .text_size(px(11.0))
            .child(t("dialog.raw_masks.add_to_mask")),
    );
    for (i, shape) in NewShape::ALL.into_iter().enumerate() {
        add = add.child(chip(
            ("raw-mask-add", i),
            shape.label(),
            false,
            move |ws, cx| ws.add_raw_mask(shape, true, cx),
            cx,
        ));
    }
    body = body.child(add);
    if detecting {
        body = body.child(note(t("dialog.raw_masks.detecting")));
    }

    // Tools for the selected component.
    match mask.components.get(selected_component).map(|c| &c.shape) {
        Some(MaskShape::Brush { .. }) => {
            for (key, label, value, min, max, suffix) in [
                (
                    "raw_mask.brush_size",
                    t("common.size"),
                    brush.size,
                    0.2,
                    25.0,
                    "%",
                ),
                (
                    "raw_mask.brush_feather",
                    t("common.feather"),
                    brush.feather,
                    0.0,
                    100.0,
                    "",
                ),
                (
                    "raw_mask.brush_flow",
                    t("common.flow"),
                    brush.flow,
                    1.0,
                    100.0,
                    "",
                ),
            ] {
                body = body.child(param_slider(
                    SliderSpec {
                        id: key,
                        label,
                        value,
                        min,
                        max,
                        suffix,
                        choices: &[],
                    },
                    move |ws, v, cx| {
                        ws.set_raw_mask_brush(|b| match key {
                            "raw_mask.brush_size" => b.size = v,
                            "raw_mask.brush_feather" => b.feather = v,
                            _ => b.flow = v,
                        });
                        cx.notify();
                    },
                    cx,
                ));
            }
            body = body
                .child(ui::checkbox(
                    t("dialog.raw_masks.erase"),
                    brush.erase,
                    |ws, _cx| ws.set_raw_mask_brush(|b| b.erase = !b.erase),
                    cx,
                ))
                .child(note(t("dialog.raw_masks.hint_brush")));
        }
        Some(MaskShape::Radial { feather, .. }) => {
            body = body
                .child(param_slider(
                    SliderSpec {
                        id: "raw_mask.radial_feather",
                        label: t("common.feather"),
                        value: *feather,
                        min: 0.0,
                        max: 100.0,
                        suffix: "",
                        choices: &[],
                    },
                    move |ws, v, cx| {
                        ws.edit_raw_mask_component(selected_component, cx, |c| {
                            if let MaskShape::Radial { feather, .. } = &mut c.shape {
                                *feather = v.clamp(0.0, 100.0);
                            }
                        })
                    },
                    cx,
                ))
                .child(note(t("dialog.raw_masks.hint_gradient")));
        }
        Some(MaskShape::Linear { .. }) => {
            body = body.child(note(t("dialog.raw_masks.hint_gradient")));
        }
        _ => {}
    }

    body = body
        .child(ui::checkbox(
            t("dialog.raw_masks.invert_mask"),
            mask.invert,
            |ws, cx| ws.edit_raw_mask(cx, |m| m.invert = !m.invert),
            cx,
        ))
        .child(ui::checkbox(
            t("dialog.raw_masks.overlay"),
            overlay,
            |ws, cx| ws.toggle_raw_mask_overlay(cx),
            cx,
        ))
        .child(
            div()
                .pt_1()
                .text_size(px(12.0))
                .child(t("dialog.raw_masks.local")),
        );

    // The mask's own development controls.
    for (key, min, max) in LOCAL_CONTROLS {
        body = body.child(param_slider(
            SliderSpec {
                id: control_id(key),
                label: control_label(key),
                value: mask.adjustments.get(key),
                min,
                max,
                suffix: if key == "exposure" { " EV" } else { "" },
                choices: &[],
            },
            move |ws, v, cx| ws.edit_raw_mask(cx, |m| m.adjustments.set(key, v)),
            cx,
        ));
    }
    body.child(chip(
        "raw-mask-delete",
        t("dialog.raw_masks.delete_mask"),
        false,
        |ws, cx| ws.delete_raw_mask(cx),
        cx,
    ))
}
