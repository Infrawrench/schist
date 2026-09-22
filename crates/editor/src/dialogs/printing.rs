use super::*;
use crate::printing::{Editor, Options};

fn edit(ws: &mut Workspace, f: impl FnOnce(&mut Options)) {
    ws.update_modal(|modal| {
        if let Modal::Printing { editor } = modal {
            if !editor.running {
                editor.error = None;
                f(&mut editor.options);
            }
        }
    });
}
pub(super) fn printing_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    editor: Editor,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let o = &editor.options;
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(t("printing.color_note"));
    if editor.running {
        body = body.child(t("printing.running"));
    } else {
        for (id, label, value, values) in [
            (
                "print-paper",
                "printing.paper",
                usize::from(o.letter),
                vec![
                    (t("printing.a4").to_string(), 0),
                    (t("printing.letter").to_string(), 1),
                ],
            ),
            (
                "print-margin",
                "printing.margin",
                o.margin as usize,
                [5, 10, 15, 20, 25, 30]
                    .into_iter()
                    .map(|n| (schist_i18n::tf!("printing.margin_value", value = n), n))
                    .collect(),
            ),
            (
                "print-cols",
                "printing.columns",
                o.columns,
                (1..=5).map(|n| (n.to_string(), n)).collect(),
            ),
            (
                "print-rows",
                "printing.rows",
                o.rows,
                (1..=6).map(|n| (n.to_string(), n)).collect(),
            ),
            (
                "print-dpi",
                "printing.dpi",
                o.dpi as usize,
                [72, 150, 300, 600]
                    .into_iter()
                    .map(|n| (n.to_string(), n))
                    .collect(),
            ),
        ] {
            let selected_label = values
                .iter()
                .find(|(_, n)| *n == value)
                .map(|(s, _)| s.clone())
                .unwrap_or_default();
            body = body.child(ui::field_row(
                t(label),
                ui::dropdown(
                    &ws.dropdown,
                    ui::Dropdown {
                        popup: Popup::Field(id),
                        is_open: state.open_popup == Some(Popup::Field(id)),
                        current: value,
                        label: selected_label.into(),
                        width: 180.0,
                        options: values.into_iter().map(|(s, n)| (s.into(), n)).collect(),
                    },
                    move |ws, value, _cx| {
                        edit(ws, |o| match id {
                            "print-paper" => o.letter = value == 1,
                            "print-margin" => o.margin = value as f32,
                            "print-cols" => o.columns = value,
                            "print-rows" => o.rows = value,
                            "print-dpi" => o.dpi = value as u32,
                            _ => {}
                        })
                    },
                    cx,
                ),
            ));
        }
        body = body
            .child(ui::checkbox(
                t("printing.landscape"),
                o.landscape,
                |ws, _| edit(ws, |o| o.landscape = !o.landscape),
                cx,
            ))
            .child(ui::checkbox(
                t("printing.actual_size"),
                o.actual_size,
                |ws, _| edit(ws, |o| o.actual_size = !o.actual_size),
                cx,
            ))
            .child(ui::checkbox(
                t("printing.captions"),
                o.captions,
                |ws, _| edit(ws, |o| o.captions = !o.captions),
                cx,
            ))
            .child(SharedString::from(schist_i18n::tf!(
                "printing.pages",
                count = o.pages(editor.photos.len().max(1))
            )))
            .child(t("printing.sizing_note"));
    }
    if let Some(error) = editor.error {
        body = body.child(SharedString::from(error));
    }
    let mut actions = div().flex().flex_row().gap_2().child(ui::button(
        t("common.cancel"),
        false,
        |ws, _, cx| ws.close_modal(cx),
        cx,
    ));
    if !editor.running {
        actions = actions.child(ui::button(
            t("printing.save_pdf"),
            true,
            |ws, window, cx| ws.run_printing(false, window, cx),
            cx,
        ));
        #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
        {
            actions = actions.child(ui::button(
                t("printing.open_print"),
                false,
                |ws, window, cx| ws.run_printing(true, window, cx),
                cx,
            ));
        }
    }
    ui::modal_frame(t("printing.title"), 590.0, body, actions)
}
