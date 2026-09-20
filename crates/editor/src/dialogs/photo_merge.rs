use super::*;
use crate::workspace::photo_merge::{error_label, mode_label};
use schist_core::DocumentId;
use schist_photo_merge::{Mode, Options};
use schist_ui::{Button, Heading};
use std::sync::atomic::Ordering;

pub(super) fn dialog(
    ws: &mut Workspace,
    documents: Vec<DocumentId>,
    included: Vec<bool>,
    options: Options,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let mut body = div().flex().flex_col().gap_2();
    if let Some(job) = ws.photo_merge_job.as_ref().filter(|job| job.processing) {
        body = body.child(t("common.working")).child(format!(
            "{}%",
            // The processing crate finishes before the result's tiles are
            // installed; keep the dialog below 100% until publication.
            (job.control.progress.load(Ordering::Relaxed) / 10).min(99)
        ));
        return ui::modal_frame(
            t("photo_merge.title"),
            640.0,
            body,
            ui::button(
                t("common.cancel"),
                false,
                |ws, _, cx| ws.close_modal(cx),
                cx,
            ),
        )
        .into_any_element();
    }
    body = body
        .child(ui::field_row(
            t("common.mode"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("photo-merge-mode"),
                    is_open: ws.open_popup == Some(Popup::Field("photo-merge-mode")),
                    current: options.mode,
                    label: mode_label(options.mode).into(),
                    width: 250.0,
                    options: [Mode::Align, Mode::Focus, Mode::Hdr, Mode::Panorama]
                        .into_iter()
                        .map(|m| (mode_label(m).into(), m))
                        .collect(),
                },
                |ws, mode, _| {
                    ws.update_modal(|modal| {
                        if let Modal::PhotoMerge { options, .. } = modal {
                            options.mode = mode;
                        }
                    })
                },
                cx,
            ),
        ))
        .child(
            div()
                .text_size(px(11.0))
                .child(t("photo_merge.translation")),
        )
        .child(Heading::new(t("common.documents")));
    let mut list = div()
        .id("photo-merge-inputs")
        .max_h(px(220.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();
    for (index, id) in documents.iter().enumerate() {
        let Some(doc) = ws.photo_merge_document(*id) else {
            continue;
        };
        let label = format!("{} · {} × {}", doc.title, doc.width, doc.height);
        let mut row = div().flex().flex_row().gap_2().items_center().child(
            Checkbox::new(("photo-merge-document", index), label, included[index]).on_change(
                cx.listener(move |ws, value: &bool, _, cx| {
                    ws.update_modal(|modal| {
                        if let Modal::PhotoMerge { included, .. } = modal {
                            included[index] = *value;
                        }
                    });
                    cx.notify();
                }),
            ),
        );
        if options.mode == Mode::Hdr {
            row = row
                .child(div().flex_1())
                .child(
                    Button::new(("photo-merge-ev-minus", index), "−").on_click(cx.listener(
                        move |ws, _, _, cx| {
                            ws.update_modal(|modal| {
                                if let Modal::PhotoMerge { options, .. } = modal {
                                    options.exposure_ev[index] =
                                        (options.exposure_ev[index] - 1.0 / 3.0).max(-20.0);
                                }
                            });
                            cx.notify();
                        },
                    )),
                )
                .child(format!("{:+.2} EV", options.exposure_ev[index]))
                .child(
                    Button::new(("photo-merge-ev-plus", index), "+").on_click(cx.listener(
                        move |ws, _, _, cx| {
                            ws.update_modal(|modal| {
                                if let Modal::PhotoMerge { options, .. } = modal {
                                    options.exposure_ev[index] =
                                        (options.exposure_ev[index] + 1.0 / 3.0).min(20.0);
                                }
                            });
                            cx.notify();
                        },
                    )),
                );
        }
        list = list.child(row);
    }
    body = body.child(list);
    if matches!(options.mode, Mode::Focus | Mode::Hdr) {
        body = body.child(ui::checkbox(
            t("photo_merge.align"),
            options.align,
            |ws, _| {
                ws.update_modal(|modal| {
                    if let Modal::PhotoMerge { options, .. } = modal {
                        options.align = !options.align;
                    }
                })
            },
            cx,
        ));
    }
    if options.mode != Mode::Panorama {
        body = body.child(ui::checkbox(
            t("photo_merge.crop"),
            options.crop,
            |ws, _| {
                ws.update_modal(|modal| {
                    if let Modal::PhotoMerge { options, .. } = modal {
                        options.crop = !options.crop;
                    }
                })
            },
            cx,
        ));
    }
    if options.mode == Mode::Hdr {
        body = body
            .child(
                div()
                    .text_size(px(11.0))
                    .child(t("photo_merge.exposure_note")),
            )
            .child(ui::checkbox(
                t("photo_merge.tone_map"),
                options.tone_map,
                |ws, _| {
                    ws.update_modal(|modal| {
                        if let Modal::PhotoMerge { options, .. } = modal {
                            options.tone_map = !options.tone_map;
                        }
                    })
                },
                cx,
            ));
        if options.tone_map {
            body = body.child(param_slider(
                SliderSpec {
                    id: "photo-merge-tone",
                    label: t("common.exposure"),
                    value: options.tone_ev,
                    min: -12.0,
                    max: 12.0,
                    suffix: " EV",
                    ..Default::default()
                },
                |ws, value, _| {
                    ws.update_modal(|modal| {
                        if let Modal::PhotoMerge { options, .. } = modal {
                            options.tone_ev = value;
                        }
                    })
                },
                cx,
            ));
        }
    }
    if matches!(options.mode, Mode::Focus | Mode::Panorama) {
        let focus = options.mode == Mode::Focus;
        body = body.child(param_slider(
            SliderSpec {
                id: "photo-merge-radius",
                label: t(if focus {
                    "common.radius"
                } else {
                    "common.feather"
                }),
                value: if focus {
                    options.focus_radius as f32
                } else {
                    options.feather as f32
                },
                min: 1.0,
                max: if focus { 16.0 } else { 512.0 },
                suffix: " px",
                ..Default::default()
            },
            move |ws, value, _| {
                ws.update_modal(|modal| {
                    if let Modal::PhotoMerge { options, .. } = modal {
                        if focus {
                            options.focus_radius = value.round() as u32;
                        } else {
                            options.feather = value.round() as u32;
                        }
                    }
                })
            },
            cx,
        ));
    }
    if options.mode == Mode::Panorama {
        body = body.child(div().text_size(px(11.0)).child(t("photo_merge.order")));
    }
    if let Some(error) = ws.photo_merge_job.as_ref().and_then(|job| job.error) {
        body = body.child(
            div()
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(error_label(error)),
        );
    }
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            t("common.apply"),
            true,
            |ws, _, cx| ws.start_photo_merge(cx),
            cx,
        ));
    ui::modal_frame(t("photo_merge.title"), 640.0, body, actions).into_any_element()
}
