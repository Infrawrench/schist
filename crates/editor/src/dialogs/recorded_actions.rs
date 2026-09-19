//! Saved action manager. Only the selected step's controls are mounted, so
//! slider ids stay unique even when an action repeats the same filter.

use super::*;
use crate::workspace::recorded_actions::Step;
use schist_i18n::{t, tf};

pub(super) fn actions_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    selected: Option<usize>,
    step: Option<usize>,
    name: String,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut body = div()
        .id("recorded-actions-body")
        .flex()
        .flex_col()
        .gap_2()
        .max_h(px(480.0))
        .overflow_y_scroll()
        .child(div().text_size(px(11.0)).child(t("actions.supported_help")))
        .child(
            div()
                .text_size(px(11.0))
                .child(t("actions.unsupported_help")),
        );
    if ws.action_recorder.recording {
        body = body
            .child(div().child(t("actions.recording")))
            .child(ui::button(
                t("actions.stop"),
                true,
                |ws, _, cx| ws.stop_action_recording(cx),
                cx,
            ));
        return ui::modal_frame(
            t("actions.title"),
            560.0,
            body,
            div().child(ui::button(
                t("actions.continue_recording"),
                false,
                |ws, _, cx| ws.close_modal(cx),
                cx,
            )),
        );
    }
    let mut saved = vec![(SharedString::from(t("actions.working_action")), None)];
    saved.extend(
        ws.action_library
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| (a.name.clone().into(), Some(i))),
    );
    let current = selected
        .and_then(|i| ws.action_library.actions.get(i))
        .map(|a| a.name.clone())
        .unwrap_or_else(|| t("actions.working_action").into());
    body = body.child(ui::field_row(
        t("actions.saved_actions"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("recorded-action-select"),
                is_open: state.open_popup == Some(Popup::Field("recorded-action-select")),
                current: selected,
                label: current.into(),
                width: 300.0,
                options: saved,
            },
            |ws, index, _| {
                if let Some(action) = index
                    .and_then(|i| ws.action_library.actions.get(i))
                    .cloned()
                {
                    ws.action_recorder.draft = Some(action.clone());
                    ws.update_modal(|m| {
                        if let Modal::RecordedActions {
                            selected,
                            step,
                            name,
                        } = m
                        {
                            *selected = index;
                            *step = None;
                            *name = action.name;
                        }
                    });
                }
            },
            cx,
        ),
    ));
    body = body.child(ui::button(
        t("actions.start"),
        false,
        |ws, _, cx| ws.start_action_recording(cx),
        cx,
    ));

    if let Some(draft) = ws.action_recorder.draft.clone() {
        let focused = state.focused_field == Some("recorded-action-name");
        let shown = if focused {
            state.field_buffer.clone()
        } else {
            name.clone()
        };
        let committed = name.clone();
        body = body.child(ui::field_row(
            t("common.name"),
            TextInput::new("recorded-action-name", shown.clone())
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
                    ws.press_field("recorded-action-name", committed.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _, cx| {
                    ws.drag_field("recorded-action-name", *offset);
                    cx.notify();
                })),
        ));
        let options = draft
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    SharedString::from(tf!(
                        "actions.step_label",
                        n = i + 1,
                        name = s.label(&ws.registry)
                    )),
                    Some(i),
                )
            })
            .collect();
        let label = step
            .and_then(|i| draft.steps.get(i))
            .map(|s| s.label(&ws.registry))
            .unwrap_or_else(|| t("actions.choose_step").into());
        body = body.child(ui::field_row(
            t("actions.steps"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("recorded-action-step"),
                    is_open: state.open_popup == Some(Popup::Field("recorded-action-step")),
                    current: step,
                    label: label.into(),
                    width: 360.0,
                    options,
                },
                |ws, index, _| {
                    ws.update_modal(|m| {
                        if let Modal::RecordedActions { step, .. } = m {
                            *step = index;
                        }
                    })
                },
                cx,
            ),
        ));
        if let Some((index, selected_step)) = step.and_then(|i| draft.steps.get(i).map(|s| (i, s)))
        {
            let mut reorder = div().flex().flex_row().gap_2();
            if index > 0 {
                reorder = reorder.child(ui::button(
                    t("actions.move_up"),
                    false,
                    move |ws, _, cx| {
                        if let Some(a) = ws.action_recorder.draft.as_mut() {
                            a.steps.swap(index, index - 1);
                        }
                        ws.update_modal(|m| {
                            if let Modal::RecordedActions { step, .. } = m {
                                *step = Some(index - 1);
                            }
                        });
                        cx.notify();
                    },
                    cx,
                ));
            }
            if index + 1 < draft.steps.len() {
                reorder = reorder.child(ui::button(
                    t("actions.move_down"),
                    false,
                    move |ws, _, cx| {
                        if let Some(a) = ws.action_recorder.draft.as_mut() {
                            a.steps.swap(index, index + 1);
                        }
                        ws.update_modal(|m| {
                            if let Modal::RecordedActions { step, .. } = m {
                                *step = Some(index + 1);
                            }
                        });
                        cx.notify();
                    },
                    cx,
                ));
            }
            body = body.child(reorder.child(ui::button(
                t("common.remove"),
                false,
                move |ws, _, cx| {
                    if let Some(a) = ws.action_recorder.draft.as_mut() {
                        if index < a.steps.len() {
                            a.steps.remove(index);
                        }
                    }
                    ws.update_modal(|m| {
                        if let Modal::RecordedActions { step, .. } = m {
                            *step = None;
                        }
                    });
                    cx.notify();
                },
                cx,
            )));
            match selected_step {
                Step::Filter { id, values } => {
                    let specs = ws
                        .registry
                        .filters()
                        .find(|f| f.id() == id)
                        .map(|f| f.params())
                        .unwrap_or_default();
                    for spec in specs {
                        let key = spec.key;
                        body = body.child(param_slider(
                            SliderSpec {
                                id: key,
                                label: spec.label,
                                value: values.get(key).copied().unwrap_or(spec.default),
                                min: spec.min,
                                max: spec.max,
                                suffix: spec.suffix,
                                ..Default::default()
                            },
                            move |ws, value, _| {
                                if let Some(Step::Filter { values, .. }) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                {
                                    values.insert(key.into(), value);
                                }
                            },
                            cx,
                        ));
                    }
                }
                Step::AddAdjustment { params }
                | Step::SetAdjustment { params }
                | Step::PixelAdjustment { params } => {
                    for spec in params.param_specs() {
                        let key = spec.key;
                        body = body.child(param_slider(
                            SliderSpec {
                                id: key,
                                label: t(&format!("actions.param_{key}")),
                                value: spec.value,
                                min: spec.min,
                                max: spec.max,
                                suffix: spec.suffix,
                                ..Default::default()
                            },
                            move |ws, value, _| {
                                if let Some(params) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                    .and_then(Step::adjustment_mut)
                                {
                                    params.set_param(key, value);
                                }
                            },
                            cx,
                        ));
                    }
                }
                Step::Command {
                    id,
                    foreground,
                    background,
                } if id.starts_with("edit.fill_") => {
                    let foreground_selected = id == "edit.fill_foreground";
                    let values = if foreground_selected {
                        foreground
                    } else {
                        background
                    };
                    for (component, key, label) in [
                        (0, "action-red", t("actions.red")),
                        (1, "action-green", t("actions.green")),
                        (2, "action-blue", t("actions.blue")),
                        (3, "action-alpha", t("actions.alpha")),
                    ] {
                        body = body.child(param_slider(
                            SliderSpec {
                                id: key,
                                label,
                                value: values[component],
                                min: 0.0,
                                max: 1.0,
                                suffix: "",
                                ..Default::default()
                            },
                            move |ws, value, _| {
                                if let Some(Step::Command {
                                    foreground,
                                    background,
                                    ..
                                }) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                {
                                    if foreground_selected {
                                        foreground[component] = value;
                                    } else {
                                        background[component] = value;
                                    }
                                }
                            },
                            cx,
                        ));
                    }
                }
                _ => {
                    body = body.child(t("actions.no_parameters"));
                }
            }
        }
        body = body.child(div().text_size(px(11.0)).child(t("actions.replay_help")));
        if !draft.steps.is_empty() {
            body = body.child(ui::button(
                t("actions.play_document"),
                false,
                |ws, _, cx| ws.replay_recorded_action(cx),
                cx,
            ));
            #[cfg(not(target_arch = "wasm32"))]
            {
                body = body
                    .child(div().text_size(px(11.0)).child(t("actions.batch_help")))
                    .child(ui::button(
                        tf!("actions.play_gallery", n = ws.library.selected.len()),
                        false,
                        |ws, window, cx| ws.replay_action_gallery(window, cx),
                        cx,
                    ));
            }
        }
    } else {
        body = body.child(t("actions.empty"));
    }
    let mut controls = div().flex().flex_row().gap_2().child(ui::button(
        t("actions.close"),
        false,
        |ws, _, cx| ws.close_modal(cx),
        cx,
    ));
    if let Some(index) = selected {
        controls = controls.child(ui::button(
            t("actions.delete"),
            false,
            move |ws, _, cx| ws.delete_recorded_action(index, cx),
            cx,
        ));
    }
    if ws.action_recorder.draft.is_some() {
        controls = controls.child(ui::button(
            t("actions.save"),
            true,
            |ws, _, cx| ws.save_recorded_action(cx),
            cx,
        ));
    }
    ui::modal_frame(t("actions.title"), 560.0, body, controls)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn batch_dialog(
    ws: &mut Workspace,
    done: usize,
    total: usize,
    outputs: Vec<std::path::PathBuf>,
    failures: Vec<(std::path::PathBuf, String)>,
    finished: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut body = div()
        .id("recorded-action-results")
        .flex()
        .flex_col()
        .gap_2()
        .max_h(px(420.0))
        .overflow_y_scroll()
        .child(tf!(
            "actions.batch_summary",
            saved = outputs.len(),
            failed = failures.len(),
            total = total
        ));
    if finished && done < total {
        body = body.child(t("actions.batch_cancelled"));
    }
    for path in outputs {
        body = body.child(
            div()
                .text_size(px(11.0))
                .child(tf!("actions.output_saved", path = path.display())),
        );
    }
    for (path, error) in failures {
        body = body.child(div().text_size(px(11.0)).child(tf!(
            "actions.output_failed",
            path = path.display(),
            error = error
        )));
    }
    let cancel = ws.action_recorder.batch_cancel.clone();
    let controls = if finished {
        div().child(ui::button(
            t("actions.close"),
            true,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ))
    } else {
        div().child(ui::button(
            t("actions.cancel_batch"),
            false,
            move |ws, _, cx| {
                if let Some(cancel) = &cancel {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                ws.status = t("actions.cancelling_batch").into();
                cx.notify();
            },
            cx,
        ))
    };
    ui::modal_frame(t("actions.batch_title"), 560.0, body, controls)
}
