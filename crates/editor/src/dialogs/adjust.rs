//! The adjustment-layer parameter dialog.

use super::*;
use schist_i18n::t;

pub(super) fn adjustment_dialog(
    ws: &mut Workspace,
    layer: schist_core::LayerId,
    params: schist_adjustments::Params,
    original: (Option<String>, Vec<u8>),
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let specs = params.param_specs();
    let title = ui::adjustment_name(params.kind());
    let curves = matches!(params, schist_adjustments::Params::Curves(_));
    let mut body = div().flex().flex_col().gap_1();
    if curves {
        body = body.child(crate::curve_editor::render(ws, cx));
    }
    if let schist_adjustments::Params::ColorLookup(lookup) = &params {
        body = color_lookup_controls(body, lookup, cx);
    }
    for spec in specs {
        let key = spec.key;
        body = body.child(param_slider(
            SliderSpec {
                id: spec.key,
                label: spec.label,
                value: spec.value,
                min: spec.min,
                max: spec.max,
                suffix: spec.suffix,
                ..Default::default()
            },
            move |ws, v, _cx| {
                // Live preview: write straight onto the layer as the
                // slider moves, then commit one history entry on OK.
                let mut updated = None;
                ws.update_modal(|m| {
                    if let Modal::Adjustment { params, .. } = m {
                        params.set_param(key, v);
                        updated = Some(params.clone());
                    }
                });
                if let Some(params) = updated {
                    ws.preview_adjustment(layer, &params);
                }
            },
            cx,
        ));
    }

    let committed = params.clone();
    let cancel_original = original.clone();
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            move |ws, _w, cx| {
                ws.revert_adjustment(layer, cancel_original.clone(), cx);
                ws.close_modal(cx);
            },
            cx,
        ))
        .child(ui::button(
            t("common.ok"),
            true,
            move |ws, _w, cx| {
                ws.commit_adjustment(layer, &committed, original.clone(), cx);
                ws.close_modal(cx);
            },
            cx,
        ));
    ui::modal_frame(title, if curves { 430.0 } else { 360.0 }, body, actions)
}

fn dim_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .text_color(gpui::rgb(ui::palette().text_dim))
        .child(text.into())
}

/// The table row (its name and a Load button), the input choice and a
/// note on what can be loaded.
fn color_lookup_controls(
    body: gpui::Div,
    lookup: &schist_adjustments::ColorLookup,
    cx: &mut Context<Workspace>,
) -> gpui::Div {
    use schist_adjustments::LutInput;
    let loaded = match &lookup.table {
        Some(table) => {
            let size = table.lut().size_label();
            if lookup.name.is_empty() {
                size
            } else {
                format!("{} ({size})", lookup.name)
            }
        }
        None => t("dialog.color_lookup.none").to_string(),
    };
    let table = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .child(SharedString::from(loaded)),
        )
        .child(ui::button(
            t("dialog.color_lookup.load"),
            lookup.table.is_none(),
            |ws, _w, cx| ws.load_color_lookup(cx),
            cx,
        ));
    let mut inputs = div().flex().flex_row().gap_1();
    for (input, label) in LutInput::ALL.into_iter().zip([
        t("dialog.color_lookup.input_document"),
        t("dialog.color_lookup.input_linear"),
    ]) {
        inputs = inputs.child(ui::button(
            label,
            lookup.input == input,
            move |ws, _w, cx| ws.set_color_lookup_input(input, cx),
            cx,
        ));
    }
    body.child(ui::field_row(t("dialog.color_lookup.table"), table))
        .child(ui::field_row(t("dialog.color_lookup.input"), inputs))
        .child(dim_text(t("dialog.color_lookup.note")))
}

/// File ▸ Export ▸ Color Lookup Table.
pub(super) fn export_lut_dialog(
    ws: &mut Workspace,
    size: usize,
    camera_raw: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let _ = ws;
    let mut sizes = div().flex().flex_row().gap_1();
    for choice in crate::workspace::color_lookup::EXPORT_SIZES {
        sizes = sizes.child(ui::button(
            format!("{choice}\u{b3}"),
            choice == size,
            move |ws, _w, cx| {
                ws.update_modal(|m| {
                    if let Modal::ExportLut { size, .. } = m {
                        *size = choice;
                    }
                });
                cx.notify();
            },
            cx,
        ));
    }
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(ui::field_row(t("dialog.export_lut.size"), sizes))
        .child(ui::checkbox(
            t("dialog.export_lut.camera_raw"),
            camera_raw,
            |ws, cx| {
                ws.update_modal(|m| {
                    if let Modal::ExportLut { camera_raw, .. } = m {
                        *camera_raw = !*camera_raw;
                    }
                });
                cx.notify();
            },
            cx,
        ))
        .child(dim_text(t("dialog.export_lut.note")));
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
            t("common.export"),
            true,
            move |ws, _w, cx| {
                ws.close_modal(cx);
                ws.export_lut(size, camera_raw, cx);
            },
            cx,
        ));
    ui::modal_frame(t("dialog.export_lut.title"), 380.0, body, actions)
}
