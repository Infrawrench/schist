//! The local photo metadata form, shared by local and cloud persistence.
use super::*;
use gpui::{prelude::FluentBuilder as _, StatefulInteractiveElement as _};
use schist_i18n::{t, tn};
use schist_ui::TextInput;

pub(super) const LABELS: [&str; 6] = [
    "metadata.keywords",
    "metadata.caption",
    "metadata.copyright",
    "metadata.taken",
    "metadata.offset",
    "metadata.gps",
];
const EXAMPLES: [&str; 6] = [
    ";",
    "",
    "©",
    "2026-09-20T14:30:00+02:00",
    "3600",
    "51.5074, -0.1278",
];

pub(super) struct MetadataForm {
    pub count: usize,
    pub values: [String; 6],
    pub enabled: [bool; 6],
    pub error: String,
    pub busy: bool,
}

pub(super) struct MetadataActions {
    pub toggle: fn(&mut Workspace, usize, &mut Context<Workspace>),
    pub save: fn(&mut Workspace, &mut Context<Workspace>),
}

pub(super) fn is_caption(id: &str) -> bool {
    matches!(id, "metadata-caption" | "cloud-meta-caption")
}

/// Absolute dates and offsets describe alternative operations on the same field.
pub(super) fn enable_field(enabled: &mut [bool; 6], index: usize, value: bool) {
    enabled[index] = value;
    if value && index == 3 {
        enabled[4] = false;
    }
    if value && index == 4 {
        enabled[3] = false;
    }
}

pub(super) fn dialog(
    ws: &mut Workspace,
    ids: [&'static str; 6],
    state: MetadataForm,
    actions: MetadataActions,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(tn("common.n_photos", state.count as u64))
        .child(div().text_size(px(11.0)).child(t("metadata.hint")));
    for index in 0..6 {
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(160.0)).child(crate::ui::checkbox(
                    t(LABELS[index]),
                    state.enabled[index],
                    move |ws, cx| {
                        ws.commit_focused_field();
                        (actions.toggle)(ws, index, cx);
                    },
                    cx,
                )))
                .child(
                    field(
                        ids[index],
                        state.values[index].clone(),
                        EXAMPLES[index].into(),
                        ws,
                        cx,
                    )
                    .when(index == 1, |field| field.multiline().h(px(64.0))),
                ),
        );
    }
    body = body.child(
        div()
            .max_h(px(100.0))
            .id("metadata-result")
            .overflow_y_scroll()
            .text_size(px(11.0))
            .child(SharedString::from(state.error)),
    );
    let actions = div()
        .flex()
        .justify_end()
        .gap_2()
        .child(crate::ui::button(
            t("common.close"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .children((!state.busy && state.count > 0).then(|| {
            crate::ui::button(
                t("common.save"),
                true,
                move |ws, _w, cx| (actions.save)(ws, cx),
                cx,
            )
        }))
        .children(state.busy.then(|| div().child(t("common.saving"))));
    crate::ui::modal_frame(t("metadata.title"), 620.0, body, actions).into_any_element()
}

fn field(
    id: &'static str,
    value: String,
    placeholder: String,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> TextInput {
    let focused = ws.focused_field == Some(id);
    let typed = if focused {
        ws.field_buffer.clone()
    } else {
        value
    };
    // A cleared input stays empty while focused; the example is only a placeholder.
    let cursor = if ws.field_buffer.is_empty() {
        typed.len()
    } else {
        ws.field_cursor.min(typed.len())
    };
    TextInput::new(id, typed.clone())
        .cursor(cursor)
        .selection(ws.field_selection())
        .active(focused)
        .caret_on(ws.caret_on())
        .placeholder(placeholder)
        .w(px(360.0))
        .on_focus(
            cx.listener(move |ws, press: &crate::ui::TextPress, _w, cx| {
                if ws.focused_field != Some(id) {
                    ws.commit_focused_field();
                }
                ws.press_field(id, typed.clone(), press);
                cx.notify();
            }),
        )
        .on_select_to(cx.listener(move |ws, offset: &usize, _w, cx| {
            ws.drag_field(id, *offset);
            cx.notify();
        }))
}
