//! Select > Refine Mask: a self-contained preview and edge controls.

use super::*;
use crate::workspace::mask_refine::Background;
use schist_core::mask_refine::Settings;

pub(super) fn dialog(
    ws: &mut Workspace,
    settings: Settings,
    background: Background,
    original: bool,
    zoom: f32,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let mut preview = div()
        .id("mask-refine-preview")
        .w_full()
        .h(px(285.0))
        .overflow_scroll()
        .bg(gpui::rgb(0x252525));
    if let Some((image, aspect)) = ws.mask_refine_preview(settings, background, original) {
        let h = (620.0 / aspect).min(275.0);
        preview = preview.child(
            gpui::img(image)
                .w(px(h * aspect * zoom))
                .h(px(h * zoom))
                .flex_none(),
        );
    }
    if ws.mask_refine_busy() {
        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(preview)
            .child(t("mask_refine.processing"));
        let actions = ui::button(
            t("common.cancel"),
            false,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        );
        return ui::modal_frame(t("mask_refine.title"), 680.0, body, actions).into_any_element();
    }
    let mut controls = div().flex().flex_col().gap_1();
    for (id, label, value, min, max, suffix) in [
        (
            "mask-refine-radius",
            "mask_refine.radius",
            settings.radius,
            0.0,
            64.0,
            " px",
        ),
        (
            "mask-refine-strength",
            "mask_refine.strength",
            settings.refine * 100.0,
            0.0,
            100.0,
            "%",
        ),
        (
            "mask-refine-smooth",
            "mask_refine.smooth",
            settings.smooth,
            0.0,
            20.0,
            " px",
        ),
        (
            "mask-refine-shift",
            "mask_refine.shift",
            settings.shift,
            -20.0,
            20.0,
            " px",
        ),
        (
            "mask-refine-feather",
            "mask_refine.feather",
            settings.feather,
            0.0,
            40.0,
            " px",
        ),
    ] {
        controls = controls.child(param_slider(
            SliderSpec {
                id,
                label: t(label),
                value,
                min,
                max,
                suffix,
                ..Default::default()
            },
            move |ws, v, _| {
                ws.update_modal(|modal| {
                    if let Modal::MaskRefine { settings, .. } = modal {
                        match id {
                            "mask-refine-radius" => settings.radius = v.round(),
                            "mask-refine-strength" => settings.refine = v / 100.0,
                            "mask-refine-smooth" => settings.smooth = v.round(),
                            "mask-refine-shift" => settings.shift = v.round(),
                            "mask-refine-feather" => settings.feather = v.round(),
                            _ => {}
                        }
                    }
                });
            },
            cx,
        ));
    }
    if ws.mask_refine_rgb() {
        controls = controls.child(param_slider(
            SliderSpec {
                id: "mask-refine-decontaminate",
                label: t("mask_refine.decontaminate"),
                value: settings.decontaminate * 100.0,
                min: 0.0,
                max: 100.0,
                suffix: "%",
                ..Default::default()
            },
            |ws, v, _| {
                ws.update_modal(|modal| {
                    if let Modal::MaskRefine { settings, .. } = modal {
                        settings.decontaminate = v / 100.0;
                    }
                })
            },
            cx,
        ));
    } else {
        controls = controls.child(div().text_size(px(11.0)).child(t("mask_refine.rgb_only")));
    }
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(preview)
        .child(ui::field_row(
            t("mask_refine.background"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("mask-refine-background"),
                    is_open: ws.open_popup == Some(Popup::Field("mask-refine-background")),
                    current: background,
                    label: background.label().into(),
                    width: 170.0,
                    options: Background::ALL
                        .iter()
                        .map(|&bg| (bg.label().into(), bg))
                        .collect(),
                },
                |ws, bg, _| {
                    ws.update_modal(|modal| {
                        if let Modal::MaskRefine { background, .. } = modal {
                            *background = bg;
                        }
                    })
                },
                cx,
            ),
        ))
        .child(param_slider(
            SliderSpec {
                id: "mask-refine-zoom",
                label: t("mask_refine.zoom"),
                value: zoom * 100.0,
                min: 100.0,
                max: 400.0,
                suffix: "%",
                ..Default::default()
            },
            |ws, v, _| {
                ws.update_modal(|modal| {
                    if let Modal::MaskRefine { zoom, .. } = modal {
                        *zoom = v / 100.0;
                    }
                })
            },
            cx,
        ))
        .child(ui::checkbox(
            t("mask_refine.show_original"),
            original,
            |ws, cx| {
                ws.update_modal(|modal| {
                    if let Modal::MaskRefine { original, .. } = modal {
                        *original = !*original;
                    }
                });
                cx.notify();
            },
            cx,
        ))
        .child(controls)
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(ws.mask_refine_note()),
        )
        .child(
            div()
                .text_size(px(11.0))
                .child(t(if settings.decontaminate > 0.0 {
                    "mask_refine.duplicate_note"
                } else {
                    "mask_refine.mask_note"
                })),
        );
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("mask_refine.reset"),
            false,
            |ws, _, cx| {
                ws.update_modal(|modal| {
                    if let Modal::MaskRefine {
                        settings, original, ..
                    } = modal
                    {
                        *settings = Settings::default();
                        *original = false;
                    }
                });
                cx.notify();
            },
            cx,
        ))
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            t("mask_refine.apply"),
            true,
            |ws, _, cx| ws.apply_mask_refine(cx),
            cx,
        ));
    ui::modal_frame(t("mask_refine.title"), 680.0, body, actions).into_any_element()
}
