//! The OpenEXR rows of the export dialog: sample type, compression,
//! layers and alpha.

use super::*;
use schist_plugin_api::{ExportOptions, ExrCompression};

fn compression_label(c: ExrCompression) -> &'static str {
    t(match c {
        ExrCompression::None => "dialog.export.exr_compression.none",
        ExrCompression::Rle => "dialog.export.exr_compression.rle",
        ExrCompression::Zips => "dialog.export.exr_compression.zips",
        ExrCompression::Zip => "dialog.export.exr_compression.zip",
        ExrCompression::Piz => "dialog.export.exr_compression.piz",
        ExrCompression::Pxr24 => "dialog.export.exr_compression.pxr24",
        ExrCompression::B44 => "dialog.export.exr_compression.b44",
        ExrCompression::B44a => "dialog.export.exr_compression.b44a",
    })
}

fn update(ws: &mut Workspace, change: impl FnOnce(&mut ExportOptions)) {
    ws.update_modal(|m| {
        if let Modal::Export { options, .. } = m {
            change(options);
        }
    });
}

pub(super) fn exr_export_rows(
    ws: &mut Workspace,
    state: &DialogState,
    options: ExportOptions,
    mut body: gpui::Div,
    cx: &mut Context<Workspace>,
) -> gpui::Div {
    let float = options.bit_depth >= 32;
    let samples = [
        (SharedString::from(t("dialog.export.exr_half")), 16u8),
        (SharedString::from(t("dialog.export.exr_float")), 32u8),
    ];
    body = body.child(ui::field_row(
        t("dialog.export.exr_samples"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("export-exr-samples"),
                is_open: state.open_popup == Some(Popup::Field("export-exr-samples")),
                current: if float { 32u8 } else { 16 },
                label: samples[usize::from(float)].0.clone(),
                width: 150.0,
                options: samples.to_vec(),
            },
            |ws, value, _cx| update(ws, |o| o.bit_depth = value),
            cx,
        ),
    ));
    body = body.child(ui::field_row(
        t("dialog.export.exr_compression"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("export-exr-compression"),
                is_open: state.open_popup == Some(Popup::Field("export-exr-compression")),
                current: options.exr.compression,
                label: compression_label(options.exr.compression).into(),
                width: 150.0,
                options: ExrCompression::ALL
                    .into_iter()
                    .map(|c| (SharedString::from(compression_label(c)), c))
                    .collect(),
            },
            |ws, value, _cx| update(ws, |o| o.exr.compression = value),
            cx,
        ),
    ));
    body = body.child(ui::field_row(
        t("dialog.export.exr_layers"),
        ui::checkbox(
            t("dialog.export.exr_layers_note"),
            options.exr.layered,
            |ws, _cx| update(ws, |o| o.exr.layered = !o.exr.layered),
            cx,
        ),
    ));
    body = body.child(ui::field_row(
        t("dialog.export.exr_alpha"),
        ui::checkbox(
            t("dialog.export.exr_alpha_note"),
            options.exr.alpha,
            |ws, _cx| update(ws, |o| o.exr.alpha = !o.exr.alpha),
            cx,
        ),
    ));
    if ws.doc.as_ref().is_some_and(|d| d.depth != Depth::ThirtyTwo) {
        body = body.child(t("dialog.export.exr_linear_note"));
    }
    body
}
