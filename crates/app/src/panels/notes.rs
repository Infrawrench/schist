//! The note tool's options and the notes panel.

use super::*;
use schist_i18n::t;

/// The Note tool's own controls: who is writing, in what colour, and a
/// way to clear the lot -- the three Photoshop puts in its options bar.
pub(super) fn note_options(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let editing = ws.note_edit_buffer(NoteField::Author);
    let author = match editing {
        Some(edit) => edit.text.clone(),
        None => ws.view.note_author.clone(),
    };
    let caret_on = ws.caret_on();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(palette().text_dim))
                .child(t("panel.notes.author")),
        )
        .child(
            TextInput::new("note-author", author)
                .cursor(editing.map_or(0, |edit| edit.cursor))
                .selection(editing.map_or(0..0, |edit| edit.selection()))
                .active(editing.is_some())
                .caret_on(caret_on)
                .placeholder(t("panel.notes.author"))
                .w(px(120.0))
                .on_focus(cx.listener(|ws, press: &ui::TextPress, _w, cx| {
                    // The first press opens the session; it and every
                    // one after put the caret where it landed.
                    if ws.note_edit_buffer(NoteField::Author).is_none() {
                        ws.begin_note_author_edit(cx);
                    }
                    ws.note_edit_press(NoteField::Author, press, cx);
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _w, cx| {
                    ws.note_edit_drag(NoteField::Author, *offset, cx);
                })),
        )
        .child(
            Swatch::new("note-color", swatch_hex(ws.editor.note_color))
                .border_color(gpui::rgb(palette().text_faint))
                .on_click(
                    cx.listener(|ws, _e, _w, cx| ws.open_color_picker(ColorTarget::Note, cx)),
                ),
        )
        .child(ui::button(
            t("panel.notes.clear_all"),
            false,
            |ws, _w, cx| ws.clear_notes(cx),
            cx,
        ))
}

/// A small square button in the Notes panel's header.
pub(super) fn note_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    on_click: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    Button::bare(id)
        .ghost()
        .disabled(!enabled)
        .size(px(20.0))
        .px_0()
        .text_size(px(13.0))
        .on_click(cx.listener(move |ws, _e, _w, cx| on_click(ws, cx)))
        .child(label)
}

/// Photoshop's Notes panel: one note at a time, with its author, its text
/// and a way to walk the rest.
///
/// Absent entirely when there is nothing to show and the Note tool is not
/// out -- the side column is 260px wide and already carries four panels,
/// so an empty fifth would cost the layers panel rows it needs more.
pub(super) fn notes_panel(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<gpui::AnyElement> {
    let doc = ws.doc.as_ref()?;
    let count = doc.notes.len();
    if count == 0 && ws.editor.active_tool != "note" {
        return None;
    }
    let index = ws.active_note().unwrap_or(0);
    let note = doc.notes.get(index);
    let editing = ws.note_edit_buffer(NoteField::Text(index));
    let author = note
        .map(|n| {
            if n.author.is_empty() {
                t("panel.notes.unattributed").to_string()
            } else {
                n.author.clone()
            }
        })
        .unwrap_or_default();
    let body = match editing {
        Some(edit) => edit.text.clone(),
        None => note.map(|n| n.text.clone()).unwrap_or_default(),
    };
    let caret_on = ws.caret_on();

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(panel_title(t("menu.view.notes")))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(note_button(
                    "note-prev",
                    "\u{2039}",
                    count > 1,
                    |ws, cx| ws.step_note(-1, cx),
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(if count == 0 {
                            "0".to_string()
                        } else {
                            format!("{} / {count}", index + 1)
                        }),
                )
                .child(note_button(
                    "note-next",
                    "\u{203A}",
                    count > 1,
                    |ws, cx| ws.step_note(1, cx),
                    cx,
                ))
                .child(
                    IconButton::new("note-delete", "trash")
                        .size(20.0)
                        .icon_size(13.0)
                        .disabled(count == 0)
                        .on_click(cx.listener(move |ws, _e, _w, cx| ws.delete_note(index, cx))),
                ),
        );

    let panel = div()
        .flex()
        .flex_col()
        .flex_none()
        .p_2()
        .gap_1()
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(header);

    let panel = if count == 0 {
        panel.child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(palette().text_faint))
                .child(t("panel.notes.empty")),
        )
    } else {
        panel
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(gpui::rgb(palette().text_dim))
                    .child(author),
            )
            .child(
                // A paragraph, not a line: newlines break.
                TextInput::new("note-body", body)
                    .multiline()
                    .cursor(editing.map_or(0, |edit| edit.cursor))
                    .selection(editing.map_or(0..0, |edit| edit.selection()))
                    .active(editing.is_some())
                    .caret_on(caret_on)
                    .placeholder(t("panel.notes.placeholder"))
                    .h(px(72.0))
                    .overflow_hidden()
                    .on_focus(cx.listener(move |ws, press: &ui::TextPress, _w, cx| {
                        if ws.note_edit_buffer(NoteField::Text(index)).is_none() {
                            ws.begin_note_edit(index, cx);
                        }
                        ws.note_edit_press(NoteField::Text(index), press, cx);
                    }))
                    .on_select_to(cx.listener(move |ws, offset: &usize, _w, cx| {
                        ws.note_edit_drag(NoteField::Text(index), *offset, cx);
                    })),
            )
    };
    Some(panel.into_any_element())
}

// ===== toolbar =====
