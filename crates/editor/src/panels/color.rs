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
