use super::*;
use crate::workspace::WorkspaceEdit;
use schist_app_settings::workspaces::PANELS;

pub(super) fn dialog(
    ws: &Workspace,
    state: &DialogState,
    primary: Option<WorkspaceEdit>,
    selected: Option<usize>,
    name: String,
    error: Option<String>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let selected = selected.filter(|i| *i < ws.view.workspaces.saved.len());
    let mut options = vec![(SharedString::from(t("workspaces.current")), None)];
    options.extend(
        ws.view
            .workspaces
            .saved
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone().into(), Some(i))),
    );
    let label = selected
        .map(|i| ws.view.workspaces.saved[i].name.clone())
        .unwrap_or_else(|| t("workspaces.current").into());
    let focused = state.focused_field == Some("workspace-name");
    let shown = if focused {
        state.field_buffer.clone()
    } else {
        name.clone()
    };
    let mut body = div()
        .id("workspace-body")
        .flex()
        .flex_col()
        .gap_3()
        .max_h(px(480.0))
        .overflow_y_scroll()
        .child(t("workspaces.help"))
        .child(t("workspaces.keyboard"))
        .child(ui::field_row(
            t("workspaces.title"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("workspace-select"),
                    is_open: state.open_popup == Some(Popup::Field("workspace-select")),
                    current: selected,
                    label: label.into(),
                    width: 300.0,
                    options,
                },
                |ws, selected, _| {
                    ws.commit_focused_field();
                    let name = selected
                        .and_then(|i| ws.view.workspaces.saved.get(i))
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    ws.update_modal(|m| {
                        if let Modal::Workspaces {
                            selected: s,
                            name: n,
                            error,
                            ..
                        } = m
                        {
                            *s = selected;
                            *n = name;
                            *error = None;
                        }
                    });
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.name"),
            TextInput::new("workspace-name", shown.clone())
                .cursor(if focused {
                    state.field_cursor.min(shown.len())
                } else {
                    shown.len()
                })
                .selection(state.field_selection.clone())
                .active(focused)
                .caret_on(state.caret_on)
                .w(px(300.0))
                .on_focus(cx.listener(move |ws, press: &ui::TextPress, _, cx| {
                    ws.press_field("workspace-name", name.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _, cx| {
                    ws.drag_field("workspace-name", *offset);
                    cx.notify();
                })),
        ))
        .child(ui::button(
            t("common.save_as"),
            primary == Some(WorkspaceEdit::Save),
            |ws, _, cx| ws.workspace_edit(WorkspaceEdit::Save, cx),
            cx,
        ));
    if let Some(i) = selected {
        body = body.child(
            div()
                .flex()
                .gap_2()
                .child(ui::button(
                    t("common.apply"),
                    false,
                    move |ws, _, cx| {
                        let layout = ws.view.workspaces.saved[i].layout.clone();
                        ws.apply_workspace(layout, cx);
                    },
                    cx,
                ))
                .child(ui::button(
                    t("common.update"),
                    primary == Some(WorkspaceEdit::Update),
                    |ws, _, cx| ws.workspace_edit(WorkspaceEdit::Update, cx),
                    cx,
                ))
                .child(ui::button(
                    t("common.rename"),
                    primary == Some(WorkspaceEdit::Rename),
                    |ws, _, cx| ws.workspace_edit(WorkspaceEdit::Rename, cx),
                    cx,
                ))
                .child(ui::button(
                    t("common.delete"),
                    primary == Some(WorkspaceEdit::Delete),
                    |ws, _, cx| ws.workspace_edit(WorkspaceEdit::Delete, cx),
                    cx,
                )),
        );
    }
    body = body.child(
        Checkbox::new(
            "workspace-visible",
            t("workspaces.show_panels"),
            ws.view.side_panels,
        )
        .on_change(cx.listener(|ws, _, _, cx| {
            let mut next = ws.view.clone();
            next.side_panels = !next.side_panels;
            ws.commit_workspace_view(next, cx);
        })),
    );
    for (i, key) in PANELS.into_iter().enumerate() {
        let label = t(match key {
            "navigator" => "panel.navigator.title",
            "color" => "common.color",
            "layers" => "common.layers",
            "notes" => "menu.view.notes",
            _ => "panel.history.title",
        });
        body = body.child(
            Checkbox::new(
                ("workspace-panel", i),
                label,
                !ws.view.hidden_panels.iter().any(|k| k == key),
            )
            .on_change(cx.listener(move |ws, _, _, cx| {
                let mut next = ws.view.clone();
                if next.hidden_panels.iter().any(|k| k == key) {
                    next.hidden_panels.retain(|k| k != key);
                } else {
                    next.hidden_panels.push(key.into());
                }
                ws.commit_workspace_view(next, cx);
            })),
        );
    }
    let widths = vec![
        (t("workspaces.automatic").into(), None),
        ("220".into(), Some(220u16)),
        ("260".into(), Some(260)),
        ("300".into(), Some(300)),
        ("400".into(), Some(400)),
    ];
    body = body
        .child(ui::field_row(
            t("common.width"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("workspace-width"),
                    is_open: state.open_popup == Some(Popup::Field("workspace-width")),
                    current: ws.view.panel_width.map(|w| w as u16),
                    label: ws
                        .view
                        .panel_width
                        .map(|w| format!("{w:.0}"))
                        .unwrap_or_else(|| t("workspaces.automatic").into())
                        .into(),
                    width: 140.0,
                    options: widths,
                },
                |ws, width, cx| {
                    let mut next = ws.view.clone();
                    next.panel_width = width.map(f32::from);
                    ws.commit_workspace_view(next, cx);
                },
                cx,
            ),
        ))
        .child(t("workspaces.reset_help"))
        .child(ui::button(
            t("common.reset"),
            primary == Some(WorkspaceEdit::Reset),
            |ws, _, cx| ws.workspace_edit(WorkspaceEdit::Reset, cx),
            cx,
        ));
    ui::modal_frame(
        t("workspaces.title"),
        560.0,
        body,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(error.map(|error| div().child(error)))
            .child(ui::button(
                t("common.close"),
                primary.is_none(),
                |ws, _, cx| ws.close_modal(cx),
                cx,
            )),
    )
}
