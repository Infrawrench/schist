//! Filter parameter dialogs and destructive adjustments.

use super::*;
use schist_i18n::{t, tf};

#[allow(clippy::too_many_arguments)]
pub(super) fn filter_dialog(
    ws: &mut Workspace,
    _state: &DialogState,
    id: &'static str,
    values: schist_plugin_api::FilterValues,
    preview: bool,
    map: Option<std::sync::Arc<schist_plugin_api::FilterImage>>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let raw_development = ws.is_raw_redevelopment(id);
    let (mut name, specs) = ws
        .registry
        .filters()
        .find(|f| f.id() == id)
        .map(|f| (f.name().to_string(), f.params()))
        .unwrap_or_else(|| (id.to_string(), Vec::new()));
    if raw_development {
        name = t("dialog.filter.camera_raw_development").to_string();
    }

    // Scrolls, because Custom is a five-by-five kernel and Lighting
    // Effects has a dozen sliders: a filter dialog is a list of whatever
    // the filter declares, and some filters declare a lot.
    let mut body = div()
        .id("filter-params")
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(420.0))
        .overflow_y_scroll();
    for spec in specs {
        let key = spec.key;
        body = body.child(param_slider(
            SliderSpec {
                id: spec.key,
                label: spec.label,
                value: values.get(spec.key),
                min: spec.min,
                max: spec.max,
                suffix: spec.suffix,
                choices: spec.choices,
            },
            move |ws, v, cx| {
                let mut next = None;
                ws.update_modal(|m| {
                    if let Modal::Filter {
                        values, preview, ..
                    } = m
                    {
                        values.set(key, v);
                        if *preview {
                            next = Some(values.clone());
                        }
                    }
                });
                if let Some(values) = next {
                    ws.preview_filter(id, Some(&values), cx);
                }
            },
            cx,
        ));
    }
    if raw_development {
        body = body.child(ui::button(
            t("dialog.filter.reset_as_shot"),
            false,
            move |ws, _window, cx| {
                let Some(filter) = ws.registry.filters().find(|filter| filter.id() == id) else {
                    return;
                };
                let defaults = schist_plugin_api::FilterValues::defaults(&filter.params());
                let mut next = None;
                ws.update_modal(|modal| {
                    if let Modal::Filter {
                        values, preview, ..
                    } = modal
                    {
                        *values = defaults.clone();
                        if *preview {
                            next = Some(values.clone());
                        }
                    }
                });
                if let Some(values) = next {
                    ws.preview_filter(id, Some(&values), cx);
                }
            },
            cx,
        ));
    }
    // A filter that takes an image gets a row to choose one with. This
    // is Photoshop's "Choose a displacement map" dialog, except that it
    // opens from inside the filter rather than in front of it, so the
    // sliders can be set first and the map swapped without starting
    // over.
    if let Some(label) = ws
        .registry
        .filters()
        .find(|f| f.id() == id)
        .and_then(|f| f.wants_map())
    {
        let chosen = map
            .as_ref()
            .map(|m| tf!("common.dimensions", w = m.width, h = m.height))
            .unwrap_or_else(|| t("common.none").to_string());
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .w(px(150.0))
                        .flex_none()
                        .text_size(px(12.0))
                        .child(SharedString::from(label)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(ui::palette().text_dim))
                        .child(SharedString::from(chosen)),
                )
                .child(ui::button(
                    t("common.choose"),
                    false,
                    move |ws, window, cx| ws.choose_filter_map(id, window, cx),
                    cx,
                )),
        );
    }

    // Anything the filter wants the user to know before running it --
    // for the neural ones, whether they found their model.
    if let Some(note) = ws
        .registry
        .filters()
        .find(|f| f.id() == id)
        .and_then(|f| f.info())
    {
        body = body.child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(SharedString::from(note)),
        );
    }
    body = body
        .child(ui::checkbox(
            t("common.preview"),
            preview,
            move |ws, cx| {
                let mut next = None;
                ws.update_modal(|m| {
                    if let Modal::Filter {
                        values, preview, ..
                    } = m
                    {
                        *preview = !*preview;
                        next = Some((*preview, values.clone()));
                    }
                });
                match next {
                    Some((true, values)) => ws.preview_filter(id, Some(&values), cx),
                    // Unticking shows the untouched pixels again.
                    Some((false, _)) => ws.preview_filter(id, None, cx),
                    None => {}
                }
            },
            cx,
        ))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(if raw_development {
                    t("dialog.filter.raw_note")
                } else {
                    t("dialog.filter.note")
                }),
        );

    let apply_values = values.clone();
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
            t("common.ok"),
            true,
            move |ws, _w, cx| {
                // A RAW-backed layer renders asynchronously. Close its
                // preview dialog first, then let `apply_filter` replace it
                // with a progress modal; the ordinary pixel-filter path
                // remains synchronous and closes afterwards.
                if ws.is_raw_redevelopment(id) {
                    ws.close_modal(cx);
                    ws.apply_filter(id, &apply_values, cx);
                } else {
                    ws.apply_filter(id, &apply_values, cx);
                    ws.close_modal(cx);
                }
            },
            cx,
        ));
    ui::modal_frame(name, 360.0, body, actions)
}

/// Image ▸ Adjustments: the same sliders as the adjustment layers, but
/// previewing writes pixels and OK bakes them in.
pub(super) fn destructive_adjustment_dialog(
    ws: &mut Workspace,
    _state: &DialogState,
    kind: schist_core::AdjustmentKind,
    params: schist_adjustments::Params,
    preview: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let specs = params.param_specs();
    // Curves has no sliders: it needs a graph.
    let curves = matches!(params, schist_adjustments::Params::Curves(_));
    let mut body = div()
        .id("destructive-adjust-body")
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(430.0))
        .overflow_y_scroll();
    if curves {
        body = body.child(crate::curve_editor::render(ws, cx));
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
            move |ws, v, cx| {
                let mut next = None;
                ws.update_modal(|m| {
                    if let Modal::DestructiveAdjustment {
                        params, preview, ..
                    } = m
                    {
                        params.set_param(key, v);
                        if *preview {
                            next = Some((**params).clone());
                        }
                    }
                });
                if let Some(params) = next {
                    ws.preview_destructive_adjustment(Some(&params), cx);
                }
            },
            cx,
        ));
    }
    body = body.child(ui::checkbox(
        t("common.preview"),
        preview,
        move |ws, cx| {
            let mut next = None;
            ws.update_modal(|m| {
                if let Modal::DestructiveAdjustment {
                    params, preview, ..
                } = m
                {
                    *preview = !*preview;
                    next = Some((*preview, (**params).clone()));
                }
            });
            match next {
                Some((true, p)) => ws.preview_destructive_adjustment(Some(&p), cx),
                Some((false, _)) => ws.preview_destructive_adjustment(None, cx),
                None => {}
            }
        },
        cx,
    ));

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
            t("common.ok"),
            true,
            move |ws, _w, cx| {
                let mut run = None;
                ws.update_modal(|m| {
                    if let Modal::DestructiveAdjustment { kind, params, .. } = m {
                        run = Some((*kind, (**params).clone()));
                    }
                });
                ws.modal = None;
                if let Some((kind, params)) = run {
                    ws.commit_destructive_adjustment(kind, &params, cx);
                }
                cx.notify();
            },
            cx,
        ));
    ui::modal_frame(
        ui::adjustment_name(kind),
        if curves { 430.0 } else { 380.0 },
        body,
        actions,
    )
}
