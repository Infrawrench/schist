//! The file picker Schist draws where the platform has none (Android);
//! its state and the prompts that open it are in
//! `workspace/file_picker.rs`.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use super::*;
use crate::workspace::file_picker::{places, PickerKind, NAME_FIELD};
use schist_ui::ListItem;

pub(super) fn file_picker(
    ws: &mut Workspace,
    state: &DialogState,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let Some(picker) = ws.file_picker.as_ref() else {
        return div().into_any_element();
    };
    // The dialog's body scrolls rather than shrinks, so the listing is
    // sized to what the window leaves for it -- less with the software
    // keyboard up -- keeping the name field and the buttons in view.
    // 300pt is the rest of the dialog: title, places, location, the name
    // row, the buttons and the padding between.
    let list_height = (ws.visible_height - 300.0).clamp(120.0, 320.0);
    let pal = ui::palette();
    let m = ui::metrics();
    let kind = picker.kind;
    let title = picker.title.clone();

    // The folders a tap reaches.
    let mut chips = div().flex().flex_row().flex_wrap().gap_1();
    for (i, (label, dir)) in places().into_iter().enumerate() {
        let here = dir == picker.dir;
        chips = chips.child(
            div()
                .id(("picker-place", i))
                .px_2()
                .py_1()
                .rounded_md()
                .text_size(px(m.small_text))
                .bg(gpui::rgb(if here {
                    pal.selection_bg
                } else {
                    pal.control_bg
                }))
                .cursor_pointer()
                .hover(|s| s.bg(gpui::rgb(pal.hover)))
                .on_click(cx.listener(move |ws, _e, _w, cx| ws.picker_navigate(dir.clone(), cx)))
                .child(label),
        );
    }

    // Where we are, and the way up.
    let at_root = picker.dir.parent().is_none();
    let location = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .id("picker-up")
                .px_2()
                .py_1()
                .rounded_md()
                .text_size(px(m.small_text))
                .bg(gpui::rgb(pal.control_bg))
                .text_color(gpui::rgb(if at_root { pal.text_faint } else { pal.text }))
                .cursor_pointer()
                .hover(|s| s.bg(gpui::rgb(pal.hover)))
                .on_click(cx.listener(|ws, _e, _w, cx| ws.picker_up(cx)))
                .child("↑ Up"),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(m.small_text))
                .text_color(gpui::rgb(pal.text_dim))
                .child(ui::shown_path(&picker.dir)),
        );

    // The listing.
    let mut list = div()
        .id("picker-list")
        .flex()
        .flex_col()
        // Tall when there is room, and the first thing to give when
        // there is not (a software keyboard up), so the name field and
        // the buttons stay in view.
        .h(px(list_height))
        .overflow_y_scroll()
        .rounded_md()
        .border_1()
        .border_color(gpui::rgb(pal.edge))
        .bg(gpui::rgb(pal.field_bg));
    let note = |text: String| {
        div()
            .p_3()
            .text_size(px(m.small_text))
            .text_color(gpui::rgb(pal.text_dim))
            .child(text)
    };
    if let Some(error) = &picker.error {
        list = list.child(note(error.clone()));
    } else if picker.entries.is_empty() {
        list = list.child(note("Nothing here".into()));
    } else {
        for (i, entry) in picker.entries.iter().enumerate() {
            let selected = picker.selected.contains(&entry.path);
            // The row is sized on the item itself: a taller child would
            // overflow its 24pt hit area, and a finger tapping the
            // overflow would miss.
            list = list.child(
                ListItem::new(("picker-row", i))
                    .selected(selected)
                    .on_click(cx.listener(move |ws, _e, _w, cx| ws.picker_tap(i, cx)))
                    .h(px(m.menu_row_h))
                    .px_3()
                    .gap_2()
                    .text_size(px(m.row_text))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(entry.name.clone()),
                    )
                    .when(entry.is_dir, |row| {
                        row.child(div().text_color(gpui::rgb(pal.text_faint)).child("›"))
                    }),
            );
        }
    }

    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(chips)
        .child(location)
        .child(list);
    if kind == PickerKind::Save {
        let focused = state.focused_field == Some(NAME_FIELD);
        let committed = picker.name.clone();
        let shown = if focused {
            state.field_buffer.clone()
        } else {
            committed.clone()
        };
        body = body.child(ui::field_row(
            "Name",
            TextInput::new(NAME_FIELD, shown.clone())
                .cursor(if focused {
                    state.field_cursor.min(shown.len())
                } else {
                    shown.len()
                })
                .active(focused)
                .caret_on(state.caret_on)
                .w(px(260.0))
                .on_focus(cx.listener(move |ws, _e, _w, cx| {
                    ws.focus_field(NAME_FIELD, committed.clone());
                    cx.notify();
                })),
        ));
    }

    let primary = match kind {
        PickerKind::Save => "Save",
        PickerKind::Open { files: true, .. } => "Open",
        PickerKind::Open { .. } => "Choose This Folder",
    };
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
            primary,
            true,
            |ws, _w, cx| ws.picker_confirm(cx),
            cx,
        ));
    ui::modal_frame(title, 460.0, body, actions).into_any_element()
}
