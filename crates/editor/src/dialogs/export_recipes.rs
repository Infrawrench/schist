//! Saved recipes, with one output's controls visible at a time.
use super::*;
use crate::export_recipes::{
    Editor, Output, Placement, Recipe, Scope, TargetProfile, FLAT_CODECS, MAX_OUTPUTS,
};
use schist_i18n::tf;

fn field(
    state: &DialogState,
    id: &'static str,
    value: String,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let focused = state.focused_field == Some(id);
    let shown = if focused {
        state.field_buffer.clone()
    } else {
        value.clone()
    };
    TextInput::new(id, shown.clone())
        .cursor(if focused {
            state.field_cursor.min(shown.len())
        } else {
            shown.len()
        })
        .selection(if focused {
            state.field_selection.clone()
        } else {
            0..0
        })
        .active(focused)
        .caret_on(state.caret_on)
        .w(px(290.0))
        .on_focus(cx.listener(move |ws, press: &ui::TextPress, _w, cx| {
            if ws.focused_field != Some(id) {
                ws.commit_focused_field();
            }
            ws.press_field(id, value.clone(), press);
            cx.notify();
        }))
        .on_select_to(cx.listener(move |ws, offset: &usize, _w, cx| {
            ws.drag_field(id, *offset);
            cx.notify();
        }))
}
fn edit(ws: &mut Workspace, f: impl FnOnce(&mut Editor)) {
    ws.commit_focused_field();
    ws.update_modal(|modal| {
        if let Modal::ExportRecipes { editor } = modal {
            editor.error = None;
            f(editor);
        }
    });
}

pub(super) fn export_recipes_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    editor: Editor,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2();
    let mut saved: Vec<(SharedString, Option<usize>)> = editor
        .book
        .recipes
        .iter()
        .enumerate()
        .map(|(i, recipe)| (recipe.name.clone().into(), Some(i)))
        .collect();
    saved.push((t("export_recipes.new").into(), None));
    let selected_name = editor
        .selected
        .and_then(|i| editor.book.recipes.get(i))
        .map(|r| r.name.clone())
        .unwrap_or_else(|| t("export_recipes.new").into());
    body = body
        .child(ui::field_row(
            t("export_recipes.saved_recipes"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("recipe-saved"),
                    is_open: state.open_popup == Some(Popup::Field("recipe-saved")),
                    current: editor.selected,
                    label: selected_name.into(),
                    width: 250.0,
                    options: saved,
                },
                |ws, selected, _cx| {
                    edit(ws, |editor| {
                        editor.selected = selected;
                        editor.draft = selected
                            .and_then(|i| editor.book.recipes.get(i))
                            .cloned()
                            .unwrap_or_else(|| Recipe {
                                name: t("export_recipes.new").into(),
                                ..Default::default()
                            });
                        editor.output = 0;
                        editor.show_finishing = editor
                            .draft
                            .outputs
                            .first()
                            .is_some_and(|output| output.finishing != Default::default());
                    })
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("common.name"),
            field(state, "recipe-name", editor.draft.name.clone(), cx),
        ))
        .child(ui::field_row(
            t("export_recipes.source"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("recipe-scope"),
                    is_open: state.open_popup == Some(Popup::Field("recipe-scope")),
                    current: editor.draft.scope,
                    label: editor.draft.scope.label().into(),
                    width: 250.0,
                    options: [Scope::Document, Scope::Artboards, Scope::Slices]
                        .into_iter()
                        .map(|scope| (scope.label().into(), scope))
                        .collect(),
                },
                |ws, scope, _cx| edit(ws, |editor| editor.draft.scope = scope),
                cx,
            ),
        ));
    if !editor.photos.is_empty() {
        body = body.child(SharedString::from(tf!(
            "export_recipes.photos",
            count = editor.photos.len()
        )));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if editor.cloud_assets.is_empty() {
        body = body
            .child(ui::field_row(
                t("export_recipes.destination"),
                field(
                    state,
                    "recipe-destination",
                    editor.draft.destination.to_string_lossy().into_owned(),
                    cx,
                ),
            ))
            .child(ui::button(
                t("export_recipes.choose_folder"),
                false,
                |ws, window, cx| ws.choose_recipe_folder(window, cx),
                cx,
            ));
    }
    #[cfg(target_arch = "wasm32")]
    if editor.cloud_assets.is_empty() {
        body = body.child(t("export_recipes.browser_destination"));
    }

    if !editor.cloud_assets.is_empty() {
        body = body.child(format!(
            "{} · {}",
            t("menu.file.schist_cloud"),
            tf!("export_recipes.photos", count = editor.cloud_assets.len())
        ));
    }

    let output = editor
        .draft
        .outputs
        .get(editor.output)
        .cloned()
        .unwrap_or_default();
    let outputs = editor
        .draft
        .outputs
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let name = ws
                .registry
                .codecs()
                .find(|c| c.id() == o.codec)
                .map(|c| c.name())
                .unwrap_or(&o.codec);
            (
                SharedString::from(tf!(
                    "export_recipes.output_item",
                    index = i + 1,
                    format = name
                )),
                i,
            )
        })
        .collect();
    body = body.child(ui::field_row(
        t("export_recipes.output"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("recipe-output"),
                is_open: state.open_popup == Some(Popup::Field("recipe-output")),
                current: editor.output,
                label: tf!("export_recipes.output_number", index = editor.output + 1).into(),
                width: 250.0,
                options: outputs,
            },
            |ws, index, _cx| {
                edit(ws, |editor| {
                    editor.output = index;
                    editor.show_finishing =
                        editor.draft.outputs[index].finishing != Default::default();
                })
            },
            cx,
        ),
    ));
    let mut output_actions = div().flex().gap_2();
    if editor.draft.outputs.len() < MAX_OUTPUTS {
        output_actions = output_actions.child(ui::button(
            t("export_recipes.add_output"),
            false,
            |ws, _window, cx| {
                edit(ws, |editor| {
                    editor.draft.outputs.push(Output::default());
                    editor.output = editor.draft.outputs.len() - 1;
                    editor.show_finishing = false;
                });
                cx.notify();
            },
            cx,
        ));
    }
    if editor.draft.outputs.len() > 1 {
        output_actions = output_actions.child(ui::button(
            t("export_recipes.remove_output"),
            false,
            |ws, _window, cx| {
                edit(ws, |editor| {
                    editor.draft.outputs.remove(editor.output);
                    editor.output = editor.output.min(editor.draft.outputs.len() - 1);
                    editor.show_finishing =
                        editor.draft.outputs[editor.output].finishing != Default::default();
                });
                cx.notify();
            },
            cx,
        ));
    }
    body = body.child(output_actions);
    let formats: Vec<(SharedString, String)> = ws
        .registry
        .codecs()
        .filter(|c| c.can_export() && FLAT_CODECS.contains(&c.id()))
        .map(|c| (c.name().into(), c.id().into()))
        .collect();
    let label = formats
        .iter()
        .find(|(_, id)| id == &output.codec)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| output.codec.clone().into());
    body = body
        .child(ui::field_row(
            t("dialog.export.format"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("recipe-format"),
                    is_open: state.open_popup == Some(Popup::Field("recipe-format")),
                    current: output.codec.clone(),
                    label,
                    width: 250.0,
                    options: formats,
                },
                |ws, codec, _cx| {
                    edit(ws, |editor| {
                        editor.draft.outputs[editor.output].codec = codec
                    })
                },
                cx,
            ),
        ))
        .child(ui::field_row(
            t("export_recipes.max_edge"),
            field(state, "recipe-max-edge", output.max_edge.to_string(), cx),
        ))
        .child(
            div()
                .text_size(px(11.0))
                .child(t("export_recipes.size_note")),
        )
        .child(ui::field_row(
            t("export_recipes.filename"),
            field(state, "recipe-template", output.template.clone(), cx),
        ))
        .child(
            div()
                .text_size(px(11.0))
                .child(t("export_recipes.template_note")),
        )
        .child(
            div()
                .text_size(px(11.0))
                .child(t("export_recipes.depth_note")),
        );
    if ws
        .registry
        .codecs()
        .find(|c| c.id() == output.codec)
        .is_some_and(|c| c.supports_quality())
    {
        body = body.child(param_slider(
            SliderSpec {
                id: "recipe-quality",
                label: t("common.quality"),
                value: output.quality as f32,
                min: 1.0,
                max: 100.0,
                suffix: "",
                ..Default::default()
            },
            |ws, value, _cx| {
                edit(ws, |editor| {
                    editor.draft.outputs[editor.output].quality = value.clamp(1.0, 100.0) as u8
                })
            },
            cx,
        ));
    } else if output.codec == "codec.webp" {
        body = body.child(t("export_recipes.lossless_webp"));
    }
    body = body
        .child(ui::field_row(
            t("export_finishing.watermark"),
            field(state, "recipe-watermark", output.finishing.text.clone(), cx),
        ))
        .child(ui::checkbox(
            t("common.more"),
            editor.show_finishing,
            |ws, _cx| {
                edit(ws, |editor| {
                    editor.show_finishing = !editor.show_finishing;
                })
            },
            cx,
        ));
    if editor.show_finishing {
        if !output.finishing.text.trim().is_empty() {
            for (id, label, value, min, max) in [
                (
                    "recipe-watermark-size",
                    t("common.size"),
                    output.finishing.text_size,
                    0.5,
                    25.0,
                ),
                (
                    "recipe-watermark-opacity",
                    t("common.opacity"),
                    output.finishing.opacity * 100.0,
                    0.0,
                    100.0,
                ),
            ] {
                body = body.child(param_slider(
                    SliderSpec {
                        id,
                        label,
                        value,
                        min,
                        max,
                        suffix: "%",
                        ..Default::default()
                    },
                    move |ws, value, _cx| {
                        edit(ws, |editor| {
                            let finishing = &mut editor.draft.outputs[editor.output].finishing;
                            match id {
                                "recipe-watermark-size" => finishing.text_size = value,
                                _ => finishing.opacity = value / 100.0,
                            }
                        })
                    },
                    cx,
                ));
            }
            body = body
                .child(ui::field_row(
                    t("common.position"),
                    ui::dropdown(
                        &ws.dropdown,
                        ui::Dropdown {
                            popup: Popup::Field("recipe-watermark-position"),
                            is_open: state.open_popup
                                == Some(Popup::Field("recipe-watermark-position")),
                            current: output.finishing.placement,
                            label: output.finishing.placement.label().into(),
                            width: 250.0,
                            options: [
                                Placement::TopLeft,
                                Placement::TopRight,
                                Placement::Center,
                                Placement::BottomLeft,
                                Placement::BottomRight,
                            ]
                            .into_iter()
                            .map(|p| (p.label().into(), p))
                            .collect(),
                        },
                        |ws, value, _cx| {
                            edit(ws, |editor| {
                                editor.draft.outputs[editor.output].finishing.placement = value
                            })
                        },
                        cx,
                    ),
                ))
                .child(ui::field_row(
                    t("common.color"),
                    ui::dropdown(
                        &ws.dropdown,
                        ui::Dropdown {
                            popup: Popup::Field("recipe-watermark-color"),
                            is_open: state.open_popup
                                == Some(Popup::Field("recipe-watermark-color")),
                            current: output.finishing.white,
                            label: t(if output.finishing.white {
                                "common.white"
                            } else {
                                "common.black"
                            })
                            .into(),
                            width: 250.0,
                            options: vec![
                                (t("common.white").into(), true),
                                (t("common.black").into(), false),
                            ],
                        },
                        |ws, value, _cx| {
                            edit(ws, |editor| {
                                editor.draft.outputs[editor.output].finishing.white = value
                            })
                        },
                        cx,
                    ),
                ));
        }
        body = body.child(param_slider(
            SliderSpec {
                id: "recipe-sharpen",
                label: t("filter.camera_raw.param.sharpening"),
                value: output.finishing.sharpen * 100.0,
                min: 0.0,
                max: 200.0,
                suffix: "%",
                ..Default::default()
            },
            |ws, value, _cx| {
                edit(ws, |editor| {
                    editor.draft.outputs[editor.output].finishing.sharpen = value / 100.0
                })
            },
            cx,
        ));
        let mut profiles: Vec<_> = [
            TargetProfile::Original,
            TargetProfile::Srgb,
            TargetProfile::DisplayP3,
        ]
        .into_iter()
        .map(|p| (p.label().into(), p))
        .collect();
        if !output.finishing.custom_icc.is_empty() {
            profiles.push((
                output.finishing.custom_name.clone().into(),
                TargetProfile::Custom,
            ));
        }
        let profile_label: SharedString = if output.finishing.profile == TargetProfile::Custom {
            output.finishing.custom_name.clone().into()
        } else {
            output.finishing.profile.label().into()
        };
        let profile_control = div().flex().gap_2().child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: Popup::Field("recipe-profile"),
                is_open: state.open_popup == Some(Popup::Field("recipe-profile")),
                current: output.finishing.profile,
                label: profile_label,
                width: 250.0,
                options: profiles,
            },
            |ws, value, _cx| {
                edit(ws, |editor| {
                    editor.draft.outputs[editor.output].finishing.profile = value
                })
            },
            cx,
        ));
        #[cfg(not(target_arch = "wasm32"))]
        let profile_control = profile_control.child(ui::button(
            t("common.browse"),
            false,
            |ws, window, cx| ws.choose_recipe_profile(window, cx),
            cx,
        ));
        body = body.child(ui::field_row(
            t("dialog.profile.convert_title"),
            profile_control,
        ));
        body = body.child(ui::field_row(
            t("metadata.title"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: Popup::Field("recipe-metadata"),
                    is_open: state.open_popup == Some(Popup::Field("recipe-metadata")),
                    current: output.finishing.retain_copyright,
                    label: t(if output.finishing.retain_copyright {
                        "metadata.copyright"
                    } else {
                        "common.none"
                    })
                    .into(),
                    width: 250.0,
                    options: vec![
                        (t("common.none").into(), false),
                        (t("metadata.copyright").into(), true),
                    ],
                },
                |ws, value, _cx| {
                    edit(ws, |editor| {
                        editor.draft.outputs[editor.output]
                            .finishing
                            .retain_copyright = value
                    })
                },
                cx,
            ),
        ));
        if output.finishing.retain_copyright {
            body = body.child(ui::field_row(
                t("metadata.copyright"),
                field(
                    state,
                    "recipe-copyright",
                    output.finishing.copyright.clone(),
                    cx,
                ),
            ));
        }
    }
    if let Some(error) = editor.error {
        body = body.child(
            div()
                .text_color(gpui::rgb(ui::palette().text))
                .child(SharedString::from(error)),
        );
    }
    let mut actions = div().flex().flex_wrap().gap_2().child(ui::button(
        t("common.cancel"),
        false,
        |ws, _w, cx| ws.close_modal(cx),
        cx,
    ));
    if editor.selected.is_some() {
        actions = actions.child(ui::button(
            t("export_recipes.delete"),
            false,
            |ws, _w, cx| ws.delete_export_recipe(cx),
            cx,
        ));
    }
    actions = actions
        .child(ui::button(
            t("common.save"),
            false,
            |ws, _w, cx| {
                ws.save_export_recipe(cx);
            },
            cx,
        ))
        .child(ui::button(
            t("export_recipes.save_and_run"),
            true,
            |ws, _w, cx| ws.run_export_recipe(cx),
            cx,
        ));
    ui::modal_frame(t("export_recipes.title"), 550.0, body, actions)
}
