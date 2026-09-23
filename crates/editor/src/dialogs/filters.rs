//! Filter parameter dialogs and destructive adjustments.

use super::*;
use schist_i18n::{t, tf};

#[allow(clippy::too_many_arguments)]
pub(super) fn filter_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    id: &'static str,
    values: schist_plugin_api::FilterValues,
    preview: bool,
    map: Option<std::sync::Arc<schist_plugin_api::FilterImage>>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let raw_development = ws.is_raw_redevelopment(id);
    let canvas_controls = ws.has_filter_canvas_controls();
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
    if id == "filter.lens_correction" {
        body = lens_profile_controls(body, ws, state, &values, cx);
    }
    for spec in specs {
        if id == "filter.lens_correction" && spec.key.starts_with("lp_") {
            continue;
        }
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
                .child(if ws.stack_filter_session.is_some() {
                    t("filter_stack.note")
                } else if raw_development {
                    t("dialog.filter.raw_note")
                } else {
                    t("dialog.filter.note")
                }),
        );

    if canvas_controls {
        body = body.child(
            div()
                .text_size(px(11.0))
                .child(t("dialog.filter.canvas_controls")),
        );
    }
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
                // with a progress modal. Pixel filters can also submit
                // asynchronously in the browser; preserve their Busy modal.
                if ws.is_raw_redevelopment(id) {
                    ws.close_modal(cx);
                    ws.apply_filter(id, &apply_values, cx);
                } else {
                    ws.apply_filter(id, &apply_values, cx);
                    if matches!(ws.modal, Some(Modal::Filter { .. }))
                        && ws.stack_filter_session.is_none()
                    {
                        ws.close_modal(cx);
                    }
                }
            },
            cx,
        ));
    schist_ui::Modal::new(name)
        .width(360.0)
        .dim_background(false)
        .canvas_controls(canvas_controls)
        .backdrop(
            div()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|ws, ev, window, cx| {
                        ws.filter_canvas_down(ev, window, cx);
                    }),
                )
                .on_scroll_wheel(cx.listener(|ws, ev, window, cx| {
                    ws.filter_canvas_scroll(ev, window, cx);
                })),
        )
        .child(body)
        .action(actions)
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
    ui::preview_modal_frame(
        ui::adjustment_name(kind),
        if curves { 430.0 } else { 380.0 },
        body,
        actions,
    )
}

fn lens_profile_controls(
    mut body: gpui::Stateful<gpui::Div>,
    ws: &mut Workspace,
    state: &DialogState,
    values: &schist_plugin_api::FilterValues,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<gpui::Div> {
    use schist_filters_core::lens_profiles::{database, profile_id};
    let db = database().read().unwrap_or_else(|e| e.into_inner());
    let current = if values.get("lp_enabled") >= 0.5 {
        profile_id(values)
    } else {
        0
    };
    let mut options = vec![(SharedString::from(t("lens_profiles.manual")), 0u64)];
    options.extend(
        db.profiles
            .iter()
            .map(|p| (SharedString::from(p.name.clone()), p.id)),
    );
    let label = options
        .iter()
        .find(|(_, id)| *id == current)
        .map(|(label, _)| label.clone())
        .unwrap_or_else(|| t("lens_profiles.saved").into());
    drop(db);
    body = body.child(ui::field_row(
        t("lens_profiles.profile"),
        ui::dropdown(
            &state.dropdown,
            ui::Dropdown {
                popup: Popup::Field("lens-profile"),
                is_open: state.open_popup == Some(Popup::Field("lens-profile")),
                current,
                label,
                width: 260.,
                options,
            },
            |ws, id, cx| ws.change_lens_profile(id, cx),
            cx,
        ),
    ));
    for (key, label, min, max, suffix) in [
        (
            "lp_focal",
            t("filter.adaptive_wide_angle.param.focal"),
            0.1,
            10000.,
            " mm",
        ),
        (
            "lp_crop",
            t("filter.adaptive_wide_angle.param.crop"),
            0.1,
            20.,
            "×",
        ),
        ("lp_aperture", t("lens_profiles.aperture"), 0., 128., ""),
        (
            "lp_distance",
            t("lens_profiles.distance"),
            0.,
            1000000.,
            " m",
        ),
    ] {
        body = body.child(param_slider(
            SliderSpec {
                id: key,
                label,
                value: values.get(key),
                min,
                max,
                suffix,
                choices: &[],
            },
            move |ws, v, cx| {
                let mut selected = 0;
                ws.update_modal(|m| {
                    if let Modal::Filter { values, .. } = m {
                        values.set(key, v);
                        selected = if values.get("lp_enabled") >= 0.5 {
                            profile_id(values)
                        } else {
                            0
                        };
                    }
                });
                if selected != 0 {
                    ws.change_lens_profile(selected, cx);
                }
            },
            cx,
        ));
    }
    let supported = ws.lens_vignetting_supported();
    let active = values.get("lp_enabled") >= 0.5;
    let answer = |yes| if yes { t("common.yes") } else { t("common.no") };
    let distortion = active
        && values.get("lp_model") > 0.
        && ["lp_a", "lp_b", "lp_c"].iter().any(|k| values.get(k) != 0.);
    let tca = active
        && (["lp_br", "lp_cr", "lp_bb", "lp_cb"]
            .iter()
            .any(|k| values.get(k) != 0.)
            || values.get("lp_vr") != 1.
            || values.get("lp_vb") != 1.);
    let vignette = active
        && supported
        && values.get("lp_vignette") >= 0.5
        && values.get("lp_vig_available") >= 0.5;
    body = body.child(div().text_size(px(11.)).child(SharedString::from(tf!(
        "lens_profiles.components",
        distortion = answer(distortion),
        tca = answer(tca),
        vignette = answer(vignette)
    ))));
    body = body.child(ui::checkbox(
        t("lens_profiles.vignette"),
        values.get("lp_vignette") >= 0.5 && supported,
        move |ws, cx| {
            let mut next = None;
            ws.update_modal(|m| {
                if let Modal::Filter {
                    values, preview, ..
                } = m
                {
                    values.set(
                        "lp_vignette",
                        if supported && values.get("lp_vignette") < 0.5 {
                            1.
                        } else {
                            0.
                        },
                    );
                    if *preview {
                        next = Some(values.clone());
                    }
                }
            });
            if let Some(values) = next {
                ws.preview_filter("filter.lens_correction", Some(&values), cx);
            }
        },
        cx,
    ));
    let note = if !supported {
        t("lens_profiles.color_context")
    } else if values.get("lp_vig_available") < 0.5 {
        t("lens_profiles.vignette_missing")
    } else {
        t("lens_profiles.note")
    };
    if !supported {
        body = body.child(ui::button(
            t("menu.image.convert_to_profile"),
            false,
            |ws, _window, cx| {
                ws.close_modal(cx);
                ws.open_modal(
                    Modal::Profile {
                        convert: true,
                        selected: 0,
                    },
                    cx,
                );
            },
            cx,
        ));
    }
    body.child(div().text_size(px(11.)).child(note))
        .child(ui::button(
            t("lens_profiles.automatic"),
            false,
            |ws, _window, cx| {
                ws.refresh_exif();
                let mut next = None;
                if let Some(Modal::Filter { values, .. }) = &ws.modal {
                    let mut values = values.clone();
                    values.set("lp_enabled", 0.);
                    ws.seed_lens_profile(&mut values);
                    next = Some(values);
                }
                if let Some(next) = next {
                    if next.get("lp_enabled") < 0.5 {
                        ws.status = t("lens_profiles.no_calibration").into();
                    }
                    let mut preview = false;
                    ws.update_modal(|m| {
                        if let Modal::Filter {
                            values, preview: p, ..
                        } = m
                        {
                            *values = next.clone();
                            preview = *p;
                        }
                    });
                    if preview {
                        ws.preview_filter("filter.lens_correction", Some(&next), cx);
                    }
                }
            },
            cx,
        ))
        .child(ui::button(
            t("lens_profiles.import"),
            false,
            |ws, window, cx| ws.import_lens_profiles(window, cx),
            cx,
        ))
}
