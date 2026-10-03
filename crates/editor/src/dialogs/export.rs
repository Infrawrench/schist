//! Export with explicit encoder settings.

use super::*;
use schist_i18n::{t, tf};

pub(super) fn export_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    codec_id: &'static str,
    options: schist_plugin_api::ExportOptions,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let codecs: Vec<(SharedString, &'static str)> = ws
        .registry
        .codecs()
        .filter(|c| c.can_export())
        .map(|c| (SharedString::from(c.name().to_string()), c.id()))
        .collect();
    let current_name = codecs
        .iter()
        .find(|(_, id)| *id == codec_id)
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| "PNG".into());
    let codec = ws.registry.codecs().find(|c| c.id() == codec_id);
    let supports_quality = codec.is_some_and(|c| c.supports_quality());
    let supports_effort = codec.is_some_and(|c| c.supports_effort());
    let bit_depths: &'static [u8] = codec.map_or(&[8], |c| c.bit_depths());

    let mut body = div().flex().flex_col().gap_1().child(ui::field_row(
        t("dialog.export.format"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("export-format"),
                is_open: state.open_popup == Some(Popup::Field("export-format")),
                current: codec_id,
                label: (current_name),
                width: 150.0,
                options: codecs,
            },
            |ws, value, _cx| {
                let depths = ws
                    .registry
                    .codecs()
                    .find(|c| c.id() == value)
                    .map_or(&[8u8][..], |c| c.bit_depths());
                ws.update_modal(|m| {
                    if let Modal::Export { codec, options } = m {
                        *codec = value;
                        // A depth the new format cannot write falls back
                        // to the deepest one it can below it.
                        if !depths.contains(&options.bit_depth) {
                            options.bit_depth = depths
                                .iter()
                                .copied()
                                .filter(|&d| d <= options.bit_depth)
                                .max()
                                .unwrap_or(8);
                        }
                    }
                });
            },
            cx,
        ),
    ));
    if supports_quality {
        body = body.child(param_slider(
            SliderSpec {
                id: "export-quality",
                label: t("common.quality"),
                value: options.quality as f32,
                min: 1.0,
                max: 100.0,
                suffix: "",
                ..Default::default()
            },
            |ws, v, _cx| {
                ws.update_modal(|m| {
                    if let Modal::Export { options, .. } = m {
                        options.quality = v.clamp(1.0, 100.0) as u8;
                    }
                });
            },
            cx,
        ));
    }
    if supports_effort {
        body = body.child(param_slider(
            SliderSpec {
                id: "export-effort",
                label: t("dialog.export.effort"),
                value: options.effort as f32,
                min: 1.0,
                max: 10.0,
                suffix: "",
                ..Default::default()
            },
            |ws, v, _cx| {
                ws.update_modal(|m| {
                    if let Modal::Export { options, .. } = m {
                        options.effort = v.round().clamp(1.0, 10.0) as u8;
                    }
                });
            },
            cx,
        ));
    }
    if bit_depths.len() > 1 {
        let label = |bits: u8| SharedString::from(tf!("common.bits_per_channel", n = bits));
        body = body.child(ui::field_row(
            t("common.bit_depth"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("export-depth"),
                    is_open: state.open_popup == Some(Popup::Field("export-depth")),
                    current: options.bit_depth,
                    label: label(options.bit_depth),
                    width: 150.0,
                    options: bit_depths.iter().map(|&b| (label(b), b)).collect(),
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::Export { options, .. } = m {
                            options.bit_depth = value;
                        }
                    });
                },
                cx,
            ),
        ));
    }
    body = body.child(ui::field_row(
        t("dialog.export.dither"),
        ui::checkbox(
            t("dialog.export.dither_note"),
            options.dither,
            |ws, _cx| {
                ws.update_modal(|m| {
                    if let Modal::Export { options, .. } = m {
                        options.dither = !options.dither;
                    }
                });
            },
            cx,
        ),
    ));

    body = body.child(ui::button(
        t("export_recipes.open"),
        false,
        |ws, _window, cx| ws.open_export_recipes(Vec::new(), cx),
        cx,
    ));

    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            t("dialog.export.export_ellipsis"),
            true,
            move |ws, window, cx| {
                ws.close_modal(cx);
                ws.export_with(codec_id, options, window, cx);
            },
            cx,
        ));
    ui::modal_frame(t("common.export"), 360.0, body, actions)
}
