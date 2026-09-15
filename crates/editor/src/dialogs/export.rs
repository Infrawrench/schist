//! Export with explicit encoder settings.

use super::*;
use schist_i18n::t;

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
    let supports_quality = ws
        .registry
        .codecs()
        .find(|c| c.id() == codec_id)
        .map(|c| c.supports_quality())
        .unwrap_or(false);

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
                ws.update_modal(|m| {
                    if let Modal::Export { codec, .. } = m {
                        *codec = value;
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
