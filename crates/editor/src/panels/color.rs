//! The colour panel: foreground/background wells and the swatch palette.

use super::*;
use schist_i18n::t;

pub(super) fn color_wells(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .relative()
        .size(px(30.0))
        .mt_2()
        .child(
            Swatch::new("background-well", swatch_hex(ws.editor.background))
                .absolute()
                .bottom_0()
                .right_0()
                .rounded_none()
                .border_color(gpui::rgb(palette().text_faint))
                .on_click(
                    cx.listener(|ws, _e, _w, cx| ws.open_color_picker(ColorTarget::Background, cx)),
                ),
        )
        .child(
            Swatch::new("foreground-well", swatch_hex(ws.editor.foreground))
                .absolute()
                .top_0()
                .left_0()
                .rounded_none()
                .border_color(gpui::rgb(palette().text))
                .on_click(
                    cx.listener(|ws, _e, _w, cx| ws.open_color_picker(ColorTarget::Foreground, cx)),
                ),
        )
        // The empty corner between the two wells, which is where
        // Photoshop puts the swap arrows too.
        .child(
            IconButton::new("swap-colors", "swap")
                .size(11.0)
                .icon_size(11.0)
                .color(palette().text_dim)
                .absolute()
                .top(px(-1.0))
                .right(px(-1.0))
                .on_click(cx.listener(|ws, _e, _w, cx| {
                    std::mem::swap(&mut ws.editor.foreground, &mut ws.editor.background);
                    cx.notify();
                })),
        )
}

// ===== side panels =====

pub(super) const PALETTE: [u32; 16] = [
    0x000000, 0xFFFFFF, 0x808080, 0xC0C0C0, 0xE81E25, 0xFF7F27, 0xFFF200, 0x22B14C, 0x00A2E8,
    0x3F48CC, 0xA349A4, 0xB97A57, 0xFFAEC9, 0xFFC90E, 0xB5E61D, 0x99D9EA,
];

pub(super) fn color_panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let fg = ws.editor.foreground.to_u8();
    let swatches = palette_controls(ws, cx);
    div()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .child(panel_title(t("common.color")))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|ws, ev: &MouseDownEvent, _w, cx| {
                ws.open_context_menu(ContextTarget::Color, ev.position, cx);
            }),
        )
        .child(native_channels(ws, cx))
        .child(swatches)
        .when(ws.palettes.selected().is_none(), |panel| {
            panel.child(div().flex().flex_row().flex_wrap().gap_1().children(
                PALETTE.iter().enumerate().map(|(i, &hex)| {
                    Swatch::new(("palette-swatch", i), gpui::rgb(hex))
                        .rounded_none()
                        .border_color(gpui::rgb(palette().divider))
                        .on_click(cx.listener(move |ws, ev: &gpui::ClickEvent, _w, cx| {
                            let color = Rgba::from_u8(
                                ((hex >> 16) & 0xFF) as u8,
                                ((hex >> 8) & 0xFF) as u8,
                                (hex & 0xFF) as u8,
                                255,
                            );
                            if ev.modifiers().alt {
                                ws.editor.background = color;
                            } else {
                                ws.editor.foreground = color;
                            }
                            cx.notify();
                        }))
                }),
            ))
        })
        .child(slider(
            "col-r",
            "R",
            format!("{}", fg[0]),
            SliderTarget::ForegroundR,
            ws,
            cx,
        ))
        .child(slider(
            "col-g",
            "G",
            format!("{}", fg[1]),
            SliderTarget::ForegroundG,
            ws,
            cx,
        ))
        .child(slider(
            "col-b",
            "B",
            format!("{}", fg[2]),
            SliderTarget::ForegroundB,
            ws,
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(format!("#{:02X}{:02X}{:02X}", fg[0], fg[1], fg[2])),
                )
                .child(
                    Link::new("open-color-picker", t("panel.color.picker"))
                        .text_size(px(10.0))
                        .text_color(gpui::rgb(palette().text_dim))
                        .on_click(cx.listener(|ws, _e, _w, cx| {
                            ws.open_color_picker(ColorTarget::Foreground, cx)
                        })),
                ),
        )
        // Photoshop's spectrum bar: drag along it to take a hue directly.
        .child(crate::color_picker::hue_ramp(ws, cx))
        .child(spot_ink_launcher(ws, cx))
}

fn palette_controls(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    use crate::workspace::palettes::SEARCH_FIELD;

    let selected = ws.palettes.selected();
    let popup = Popup::Field("color-palette");
    let options = std::iter::once((t("common.default").into(), 0))
        .chain(
            ws.palettes
                .entries
                .iter()
                .enumerate()
                .map(|(i, p)| (p.name.clone().into(), i + 1)),
        )
        .collect();
    let picker = ui::dropdown(
        &ws.dropdown,
        ui::Dropdown {
            popup,
            is_open: ws.open_popup == Some(popup),
            current: ws.palettes.active,
            label: selected.map_or_else(|| t("common.default").into(), |p| p.name.clone().into()),
            width: 0.0,
            options,
        },
        |ws, value, cx| ws.select_palette(value, cx),
        cx,
    );
    let mut root = div().flex().flex_col().gap_1().child(picker).child(
        div()
            .flex()
            .gap_2()
            .child(
                Link::new("import-palette", t("common.import"))
                    .text_size(px(10.0))
                    .on_click(cx.listener(|ws, _, _, cx| ws.import_palette(cx))),
            )
            .when(selected.is_some(), |row| {
                row.child(
                    Link::new("remove-palette", t("common.remove"))
                        .text_size(px(10.0))
                        .on_click(cx.listener(|ws, _, _, cx| ws.remove_palette(cx))),
                )
            }),
    );
    if let Some(selected) = selected {
        let active = ws.focused_field == Some(SEARCH_FIELD);
        let query = if active {
            &ws.field_buffer
        } else {
            &ws.palette_search
        };
        root = root.child(
            TextInput::new(SEARCH_FIELD, query.clone())
                .placeholder(t("common.search"))
                .active(active)
                .caret_on(ws.caret_on())
                .cursor(if active { ws.field_cursor } else { 0 })
                .selection(if active { ws.field_selection() } else { 0..0 })
                .w_full()
                .on_focus(cx.listener(|ws, press: &ui::TextPress, _, cx| {
                    ws.press_field(SEARCH_FIELD, ws.palette_search.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(|ws, offset: &usize, _, cx| {
                    ws.drag_field(SEARCH_FIELD, *offset);
                    cx.notify();
                })),
        );
        let query = query.trim().to_lowercase();
        let indices: Vec<usize> = selected
            .swatches
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                query.is_empty()
                    || s.name.to_lowercase().contains(&query)
                    || s.group.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        let count = indices.len();
        let entity = cx.entity().downgrade();
        let rows = gpui::uniform_list("imported-palette", count, move |range, _, cx| {
            entity
                .update(cx, |ws, cx| {
                    let Some(palette) = ws.palettes.selected() else {
                        return Vec::new();
                    };
                    range
                        .filter_map(|row| {
                            let index = *indices.get(row)?;
                            let swatch = palette.swatches.get(index)?;
                            let color = swatch.color.to_rgb();
                            let [r, g, b, _] = color.to_u8();
                            let hex = format!("#{r:02X}{g:02X}{b:02X}");
                            let label = if swatch.name.is_empty() {
                                hex.clone()
                            } else {
                                swatch.name.clone()
                            };
                            let tip = if swatch.group.is_empty() {
                                format!("{label} · {hex}")
                            } else {
                                format!("{} / {label} · {hex}", swatch.group)
                            };
                            Some(
                                Button::bare(("named-swatch", index))
                                    .ghost()
                                    .w_full()
                                    .h(px(26.0))
                                    .px_1()
                                    .gap_2()
                                    .justify_start()
                                    .tooltip(tip, None)
                                    .child(Swatch::new(
                                        ("named-swatch-color", index),
                                        swatch_hex(color),
                                    ))
                                    .child(
                                        div().text_size(px(11.0)).truncate().child(label.clone()),
                                    )
                                    .on_click(cx.listener(
                                        move |ws, ev: &gpui::ClickEvent, _, cx| {
                                            ws.commit_focused_field();
                                            if ev.modifiers().alt {
                                                ws.editor.background = color;
                                            } else {
                                                ws.editor.foreground = color;
                                            }
                                            ws.status = label.clone().into();
                                            cx.notify();
                                        },
                                    )),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .h(px(156.0))
        .w_full();
        root = if count == 0 {
            root.child(
                div()
                    .text_size(px(11.0))
                    .child(t("dialog.file_picker.empty")),
            )
        } else {
            root.child(div().overflow_hidden().child(rows))
        };
    }
    root
}

fn native_channels(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let mut panel = div().flex().flex_col().gap_1();
    let Some(doc) = ws.doc.as_ref() else {
        return panel;
    };
    let keys: &[&str] = match doc.mode {
        schist_color::ColorMode::Cmyk => &[
            "common.cyan",
            "common.magenta",
            "common.yellow",
            "common.black",
        ],
        schist_color::ColorMode::Lab => &["common.lightness", "a*", "b*"],
        _ => return panel,
    };
    // CIELAB axis symbols are mathematical identifiers, like R/G/B above.
    let label = |key: &'static str| {
        if matches!(key, "a*" | "b*") {
            key
        } else {
            t(key)
        }
    };
    let composite = t(if doc.mode == schist_color::ColorMode::Cmyk {
        "common.cmyk"
    } else {
        "common.lab"
    });
    let selected = doc.active_channel.filter(|&c| c < keys.len());
    let popup = Popup::Field("native-channel");
    let options = std::iter::once((composite.into(), 0))
        .chain(
            keys.iter()
                .enumerate()
                .map(|(c, key)| (label(key).into(), c + 1)),
        )
        .collect();
    panel = panel
        .child(panel_title(t("common.channels")))
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup,
                is_open: ws.open_popup == Some(popup),
                current: selected.map_or(0, |c| c + 1),
                label: selected.map_or_else(|| composite.into(), |c| label(keys[c]).into()),
                width: 0.0,
                options,
            },
            |ws, value, cx| {
                if let Some(doc) = ws.doc.as_mut() {
                    doc.active_channel = value.checked_sub(1);
                    doc.active_ink = None;
                    doc.ink_preview = schist_core::InkPreview::Process;
                    doc.damage_all();
                }
                ws.after_change(cx);
            },
            cx,
        ));
    if let Some(channel) = selected {
        let display = if doc.mode == schist_color::ColorMode::Lab && channel > 0 {
            format!("{:.1}", ws.editor.native_channel_value * 255.0 - 128.0)
        } else {
            format!("{:.1}%", ws.editor.native_channel_value * 100.0)
        };
        panel = panel
            .child(slider(
                "native-channel-value",
                t("common.value"),
                display,
                SliderTarget::NativeChannelValue,
                ws,
                cx,
            ))
            .child(
                Link::new("fill-native-channel", t("common.fill")).on_click(cx.listener(
                    |ws, _, _, cx| {
                        let Some(doc) = ws.doc.as_mut() else {
                            return;
                        };
                        let (Some(layer), Some(channel)) = (doc.active_layer, doc.active_channel)
                        else {
                            return;
                        };
                        let mut edit = doc.begin_edit(t("common.fill"));
                        edit.fill_native_channel(layer, channel, ws.editor.native_channel_value);
                        edit.commit();
                        ws.after_change(cx);
                    },
                )),
            );
    }
    panel
}

fn spot_ink_launcher(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let Some(doc) = &ws.doc else { return div() };
    let selected = doc.active_ink.and_then(|id| {
        doc.ink_channels
            .iter()
            .find(|channel| channel.info.id == id && channel.info.spot)
    });
    div()
        .pt_1()
        .mt_1()
        .border_t_1()
        .border_color(gpui::rgb(palette().divider))
        .child(
            Button::bare("open-spot-ink")
                .ghost()
                .w_full()
                .min_w(px(0.0))
                .px_2()
                .justify_start()
                .gap_2()
                .child(t("panels.ink.spot"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(
                            selected.map_or_else(String::new, |channel| channel.info.name.clone()),
                        ),
                )
                .child("…")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.open_modal(Modal::SpotInk, cx);
                })),
        )
}

pub(crate) fn spot_ink_dialog(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let body = spot_channels(ws, cx);
    let actions = ui::button(
        t("common.done"),
        true,
        |ws, _, cx| {
            ws.commit_focused_field();
            ws.close_modal(cx);
            ws.after_change(cx);
        },
        cx,
    );
    ui::preview_modal_frame(t("panels.ink.spot"), 440.0, body, actions)
}

fn spot_channels(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    use schist_core::{InkChannel, InkPreview};
    let mut panel = div().flex().flex_col().gap_2();
    let Some(doc) = &ws.doc else { return div() };
    let spots: Vec<_> = doc
        .ink_channels
        .iter()
        .filter(|c| c.info.spot)
        .map(|c| c.info.clone())
        .collect();
    let selected = doc
        .active_ink
        .and_then(|id| spots.iter().position(|c| c.id == id));
    panel = panel.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(panel_title(t("common.channel")))
            .child(
                Button::new("new-spot-channel", t("common.new"))
                    .ghost()
                    .px_2()
                    .disabled(doc.mode.channels() + doc.ink_channels.len() >= 55)
                    .on_click(cx.listener(|ws, _, _, cx| {
                        ws.commit_focused_field();
                        let fg = ws.editor.foreground;
                        if let Some(doc) = ws.doc.as_mut() {
                            let channel =
                                InkChannel::spot(t("panels.ink.spot").into(), [fg.r, fg.g, fg.b]);
                            let id = channel.info.id;
                            let mut edit = doc.begin_edit(t("panels.ink.spot"));
                            edit.change_ink_channels(|channels| channels.push(channel));
                            edit.commit();
                            doc.active_ink = Some(id);
                            doc.active_channel = None;
                            doc.ink_preview = InkPreview::Separation(id);
                            ws.editor.active_tool = "brush";
                        }
                        ws.after_change(cx);
                    })),
            ),
    );
    if spots.is_empty() {
        return panel;
    }
    let ids: Vec<_> = spots.iter().map(|c| c.id).collect();
    let popup = Popup::Field("spot-channel");
    let mut options = vec![(t("common.color").into(), 0)];
    options.extend(
        spots
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone().into(), i + 1)),
    );
    let mut channels =
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(div().flex_1().min_w(px(0.0)).child(ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup,
                    is_open: ws.open_popup == Some(popup),
                    current: selected.map_or(0, |i| i + 1),
                    label: selected.map_or_else(
                        || t("common.color").into(),
                        |i| spots[i].name.clone().into(),
                    ),
                    width: 0.0,
                    options,
                },
                move |ws, value, cx| {
                    ws.commit_focused_field();
                    if let Some(doc) = ws.doc.as_mut() {
                        doc.active_ink = value.checked_sub(1).and_then(|i| ids.get(i).copied());
                        doc.active_channel = None;
                        doc.ink_preview = doc
                            .active_ink
                            .map_or(InkPreview::Process, InkPreview::Separation);
                        doc.damage_all();
                    }
                    ws.after_change(cx);
                },
                cx,
            )));
    if let Some(index) = selected {
        let info = &spots[index];
        let id = info.id;
        channels = channels
            .child(
                IconButton::new("show-spot", if info.visible { "eye" } else { "eye-off" })
                    .size(ui::metrics().icon_button)
                    .tooltip(
                        t(if info.visible {
                            "common.hide"
                        } else {
                            "common.show"
                        }),
                        None,
                    )
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        if let Some(doc) = ws.doc.as_mut() {
                            let mut edit = doc.begin_edit(t("common.preview"));
                            edit.change_ink_channels(|channels| {
                                if let Some(c) = channels.iter_mut().find(|c| c.info.id == id) {
                                    c.info.visible = !c.info.visible;
                                }
                            });
                            edit.commit();
                        }
                        ws.after_change(cx);
                    })),
            )
            .child(
                IconButton::new("delete-spot", "trash")
                    .size(ui::metrics().icon_button)
                    .tooltip(t("common.delete"), None)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.commit_focused_field();
                        if let Some(doc) = ws.doc.as_mut() {
                            let mut edit = doc.begin_edit(t("common.delete"));
                            edit.change_ink_channels(|channels| {
                                channels.retain(|c| c.info.id != id)
                            });
                            edit.commit();
                            doc.active_ink = None;
                            doc.ink_preview = InkPreview::Overprint;
                        }
                        ws.after_change(cx);
                    })),
            );
    }
    panel = panel.child(channels);
    if let Some(index) = selected {
        let info = &spots[index];
        let id = info.id;
        let focused = ws.focused_field == Some("spot-name");
        let name = if focused {
            ws.field_buffer.clone()
        } else {
            info.name.clone()
        };
        panel = panel
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div().flex_1().min_w(px(0.0)).child(
                            TextInput::new("spot-name", name.clone())
                                .active(focused)
                                .cursor(if focused { ws.field_cursor } else { name.len() })
                                .selection(if focused { ws.field_selection() } else { 0..0 })
                                .caret_on(ws.caret_on())
                                .placeholder(t("common.name"))
                                .w_full()
                                .on_focus(cx.listener(move |ws, press: &ui::TextPress, _, cx| {
                                    if ws.focused_field != Some("spot-name") {
                                        ws.commit_focused_field();
                                    }
                                    ws.press_field("spot-name", name.clone(), press);
                                    cx.notify();
                                }))
                                .on_select_to(cx.listener(|ws, offset: &usize, _, cx| {
                                    ws.drag_field("spot-name", *offset);
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        Button::new("rename-spot", t("common.rename"))
                            .ghost()
                            .px_2()
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.commit_focused_field();
                                ws.after_change(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Swatch::new(
                            "spot-ink-color",
                            swatch_hex(Rgba::new(info.color[0], info.color[1], info.color[2], 1.0)),
                        )
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.commit_focused_field();
                            let color = ws.doc.as_ref().and_then(|doc| {
                                doc.ink_channels
                                    .iter()
                                    .find(|channel| channel.info.id == id)
                                    .map(|channel| channel.info.color)
                            });
                            if let Some([r, g, b]) = color {
                                ws.open_color_picker_on(
                                    ColorTarget::SpotInk(id),
                                    Rgba::new(r, g, b, 1.0),
                                    cx,
                                );
                            }
                        })),
                    )
                    .child(
                        Button::new("spot-display-color", t("common.foreground_color"))
                            .ghost()
                            .px_2()
                            .flex_1()
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                let fg = ws.editor.foreground;
                                if let Some(doc) = ws.doc.as_mut() {
                                    let mut edit = doc.begin_edit(t("common.color"));
                                    edit.change_ink_channels(|channels| {
                                        if let Some(c) =
                                            channels.iter_mut().find(|c| c.info.id == id)
                                        {
                                            c.info.color = [fg.r, fg.g, fg.b];
                                            c.info.original_display = None;
                                        }
                                    });
                                    edit.commit();
                                }
                                ws.after_change(cx);
                            })),
                    ),
            );
    }
    let preview_popup = Popup::Field("ink-preview");
    let preview = match doc.ink_preview {
        InkPreview::Process => 0,
        InkPreview::Overprint => 1,
        InkPreview::Separation(_) => 2,
    };
    let mut previews = vec![
        (t("common.color").into(), 0),
        (t("panels.ink.overprint").into(), 1),
    ];
    if selected.is_some() {
        previews.push((t("common.channel").into(), 2));
    }
    let mut preview_controls = div()
        .flex()
        .flex_col()
        .gap_1()
        .pt_2()
        .border_t_1()
        .border_color(gpui::rgb(palette().divider))
        .child(
            div()
                .text_size(px(ui::metrics().small_text))
                .text_color(gpui::rgb(palette().text_dim))
                .child(t("common.preview")),
        )
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: preview_popup,
                is_open: ws.open_popup == Some(preview_popup),
                current: preview,
                label: t(match preview {
                    1 => "panels.ink.overprint",
                    2 => "common.channel",
                    _ => "common.color",
                })
                .into(),
                width: 0.0,
                options: previews,
            },
            |ws, value, cx| {
                if let Some(doc) = ws.doc.as_mut() {
                    doc.ink_preview = match value {
                        1 => InkPreview::Overprint,
                        2 => doc
                            .active_ink
                            .map_or(InkPreview::Process, InkPreview::Separation),
                        _ => InkPreview::Process,
                    };
                    doc.damage_all();
                }
                ws.after_change(cx);
            },
            cx,
        ));
    if let Some(index) = selected {
        let info = &spots[index];
        let id = info.id;
        preview_controls = preview_controls.child(slider_stretch(
            "spot-solidity",
            t("common.opacity"),
            if info.solidity == 0.0 {
                t("common.transparent").into()
            } else {
                format!("{:.0}%", info.solidity * 100.0)
            },
            SliderTarget::SpotSolidity(id),
            ws,
            cx,
        ));
        panel =
            panel.child(preview_controls).child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pt_2()
                    .border_t_1()
                    .border_color(gpui::rgb(palette().divider))
                    .child(div().flex_1().min_w(px(0.0)).child(slider_stretch(
                        "spot-coverage",
                        t("common.value"),
                        format!("{:.0}%", ws.editor.native_channel_value * 100.0),
                        SliderTarget::NativeChannelValue,
                        ws,
                        cx,
                    )))
                    .child(Button::new("fill-spot", t("common.fill")).px_2().on_click(
                        cx.listener(move |ws, _, _, cx| {
                            if let Some(doc) = ws.doc.as_mut() {
                                let mut edit = doc.begin_edit(t("common.fill"));
                                edit.fill_ink(id, ws.editor.native_channel_value);
                                edit.commit();
                            }
                            ws.after_change(cx);
                        }),
                    )),
            );
    } else {
        panel = panel.child(preview_controls);
    }
    panel
}
