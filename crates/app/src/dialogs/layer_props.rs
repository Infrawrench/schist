//! Layer properties.

use super::*;

/// Layer Properties: rename a layer.
pub(super) fn layer_properties(
    state: &DialogState,
    layer: schist_core::LayerId,
    name: String,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let focused = state.focused_field == Some("layer-name");
    let committed = name.clone();
    let shown = if focused && !state.field_buffer.is_empty() {
        state.field_buffer.clone()
    } else {
        name.clone()
    };
    let body = ui::field_row(
        "Name",
        TextInput::new("layer-name", shown.clone())
            // Nothing typed yet: the committed name shows with the
            // caret at its end, as it always did.
            .cursor(if state.field_buffer.is_empty() {
                shown.len()
            } else {
                state.field_cursor.min(shown.len())
            })
            .active(focused)
            .caret_on(state.caret_on)
            .w(px(200.0))
            .on_focus(cx.listener(move |ws, _e, _w, cx| {
                ws.focus_field("layer-name", committed.clone());
                cx.notify();
            })),
    );

    let committed = name;
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            "Cancel",
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            "OK",
            true,
            move |ws, _w, cx| {
                let name = if ws.field_buffer.is_empty() {
                    committed.clone()
                } else {
                    ws.field_buffer.clone()
                };
                ws.rename_layer(layer, name, cx);
                ws.close_modal(cx);
            },
            cx,
        ));
    ui::modal_frame("Layer Properties", 340.0, body, actions)
}
