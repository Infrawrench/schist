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
        .child(div().text_size(px(11.0)).child(format!(
            "{} · {} · {} · {}",
            t("common.select"),
            t("tool.transform.name"),
            t("workspace.filters.raw_history"),
            t("filter_stack.title")
        )));
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
                ws.select_recorded_action(index);
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
                    ws.commit_focused_field();
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
                        ws.commit_focused_field();
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
                        ws.commit_focused_field();
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
                    ws.commit_focused_field();
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
                _ if selected_step.filter_parameters().is_some() => {
                    if let Step::Stack {
                        change:
                            crate::workspace::recorded_actions::StackOperation::Set {
                                index: position,
                                ..
                            },
                    } = selected_step
                    {
                        body = body.child(param_slider(
                            SliderSpec {
                                id: "action-effect-index",
                                label: t("common.position"),
                                value: (*position + 1) as f32,
                                min: 1.0,
                                max: schist_core::filter_stack::MAX_EFFECTS as f32,
                                ..Default::default()
                            },
                            move |ws, value, _| {
                                if let Some(Step::Stack {
                                    change:
                                        crate::workspace::recorded_actions::StackOperation::Set {
                                            index: position,
                                            ..
                                        },
                                }) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                {
                                    *position = value.round() as usize - 1;
                                }
                            },
                            cx,
                        ));
                    }
                    let (id, values) = selected_step.filter_parameters().unwrap();
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
                                if let Some(values) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                    .and_then(Step::filter_parameters_mut)
                                {
                                    values.insert(key.into(), value);
                                }
                            },
                            cx,
                        ));
                    }
                }
                Step::SelectLayer { name } => {
                    let focused = state.focused_field == Some("recorded-action-layer");
                    let shown = if focused {
                        state.field_buffer.clone()
                    } else {
                        name.clone()
                    };
                    let original = name.clone();
                    body = body.child(ui::field_row(
                        t("menu.layer"),
                        TextInput::new("recorded-action-layer", shown.clone())
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
                                ws.press_field("recorded-action-layer", original.clone(), press);
                                cx.notify();
                            }))
                            .on_select_to(cx.listener(|ws, offset: &usize, _, cx| {
                                ws.drag_field("recorded-action-layer", *offset);
                                cx.notify();
                            })),
                    ));
                }
                Step::Transform { params } => {
                    for (key, label, value, min, max, suffix) in [
                        (
                            "action-scale-x",
                            t("filter.param.horizontal_scale"),
                            params.scale_x * 100.0,
                            -10000.0,
                            10000.0,
                            "%",
                        ),
                        (
                            "action-scale-y",
                            t("filter.param.vertical_scale"),
                            params.scale_y * 100.0,
                            -10000.0,
                            10000.0,
                            "%",
                        ),
                        (
                            "action-rotation",
                            t("common.angle"),
                            params.rotation,
                            -3600.0,
                            3600.0,
                            "°",
                        ),
                        (
                            "action-offset-x",
                            t("common.x"),
                            params.offset_x * 100.0,
                            -1000.0,
                            1000.0,
                            "%",
                        ),
                        (
                            "action-offset-y",
                            t("common.y"),
                            params.offset_y * 100.0,
                            -1000.0,
                            1000.0,
                            "%",
                        ),
                    ] {
                        body = body.child(param_slider(
                            SliderSpec {
                                id: key,
                                label,
                                value,
                                min,
                                max,
                                suffix,
                                ..Default::default()
                            },
                            move |ws, value, _| {
                                if let Some(Step::Transform { params }) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                {
                                    match key {
                                        "action-scale-x" => params.scale_x = value / 100.0,
                                        "action-scale-y" => params.scale_y = value / 100.0,
                                        "action-rotation" => params.rotation = value,
                                        "action-offset-x" => params.offset_x = value / 100.0,
                                        "action-offset-y" => params.offset_y = value / 100.0,
                                        _ => unreachable!(),
                                    }
                                }
                            },
                            cx,
                        ));
                    }
                    let interpolation = params.interpolation;
                    let filters = [
                        schist_core::Filter::Nearest,
                        schist_core::Filter::Bilinear,
                        schist_core::Filter::Bicubic,
                    ];
                    body = body.child(ui::field_row(
                        t("tool.transform.option.interpolation"),
                        ui::dropdown(
                            &ws.dropdown,
                            ui::Dropdown {
                                popup: Popup::Field("recorded-action-interpolation"),
                                is_open: state.open_popup
                                    == Some(Popup::Field("recorded-action-interpolation")),
                                current: Some(interpolation),
                                label: schist_tools_transform::filter_name(
                                    filters[usize::from(interpolation.min(2))],
                                )
                                .into(),
                                width: 180.0,
                                options: filters
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, f)| {
                                        (
                                            schist_tools_transform::filter_name(f).into(),
                                            Some(i as u8),
                                        )
                                    })
                                    .collect(),
                            },
                            move |ws, value, _| {
                                if let (Some(value), Some(Step::Transform { params })) = (
                                    value,
                                    ws.action_recorder
                                        .draft
                                        .as_mut()
                                        .and_then(|a| a.steps.get_mut(index)),
                                ) {
                                    params.interpolation = value;
                                }
                            },
                            cx,
                        ),
                    ));
                }
                Step::Stack { change } => {
                    use crate::workspace::recorded_actions::StackOperation;
                    let (position, to) = match change {
                        StackOperation::Remove { index, .. }
                        | StackOperation::Enable { index, .. } => (Some(*index), None),
                        StackOperation::Move { index, to, .. } => (Some(*index), Some(*to)),
                        _ => (None, None),
                    };
                    for (key, value) in
                        [("action-effect-index", position), ("action-effect-to", to)]
                    {
                        if let Some(value) = value {
                            body = body.child(param_slider(
                                SliderSpec {
                                    id: key,
                                    label: if key == "action-effect-index" {
                                        t("filter_stack.title")
                                    } else {
                                        t("common.position")
                                    },
                                    value: (value + 1) as f32,
                                    min: 1.0,
                                    max: schist_core::filter_stack::MAX_EFFECTS as f32,
                                    ..Default::default()
                                },
                                move |ws, value, _| {
                                    if let Some(Step::Stack { change }) = ws
                                        .action_recorder
                                        .draft
                                        .as_mut()
                                        .and_then(|a| a.steps.get_mut(index))
                                    {
                                        let value = value.round() as usize - 1;
                                        match change {
                                            StackOperation::Remove { index, .. }
                                            | StackOperation::Enable { index, .. } => {
                                                *index = value
                                            }
                                            StackOperation::Move { index, to, .. } => {
                                                if key == "action-effect-index" {
                                                    *index = value
                                                } else {
                                                    *to = value
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                },
                                cx,
                            ));
                        }
                    }
                    if let StackOperation::Enable { enabled, .. } = change {
                        body = body.child(ui::button(
                            t(if *enabled {
                                "filter_stack.disable"
                            } else {
                                "filter_stack.enable"
                            }),
                            false,
                            move |ws, _, cx| {
                                if let Some(Step::Stack {
                                    change: StackOperation::Enable { enabled, .. },
                                }) = ws
                                    .action_recorder
                                    .draft
                                    .as_mut()
                                    .and_then(|a| a.steps.get_mut(index))
                                {
                                    *enabled = !*enabled;
                                }
                                cx.notify();
                            },
                            cx,
                        ));
                    }
                }
                Step::AddAdjustment { params }
                | Step::SetAdjustment { params }
                | Step::PixelAdjustment { params } => {
                    if params.param_specs().is_empty() {
                        body = body.child(t("actions.preserved_parameters"));
                    }
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
            if ws.cloud.show {
                body = body.child(ui::button(
                    tf!("actions.play_gallery", n = ws.cloud.selected.len()),
                    false,
                    |ws, _, cx| ws.cloud_replay_action_gallery(cx),
                    cx,
                ));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !ws.cloud.show {
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
