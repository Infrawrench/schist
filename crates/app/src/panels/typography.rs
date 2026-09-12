//! A compact Type options bar and the docked Character panel.

use super::*;
use crate::workspace::SideTab;
use schist_plugin_api::{OptionKind, OptionValue, ToolOption};

fn options(ws: &Workspace) -> Vec<ToolOption> {
    ws.registry
        .tools()
        .find(|tool| tool.id() == "type")
        .map(|tool| tool.options())
        .unwrap_or_default()
}

fn option(options: &[ToolOption], key: &str) -> ToolOption {
    options
        .iter()
        .find(|option| option.key == key)
        .unwrap()
        .clone()
}

fn separator() -> Divider {
    Divider::vertical().h(px(20.0)).mx_1()
}

fn compact_button(id: &'static str, label: &'static str, active: bool) -> Button {
    let p = palette();
    Button::bare(id)
        .colors(ButtonColors {
            bg: Some(if active { p.control_bg } else { p.panel_bg }),
            hover: p.hover,
            text: p.text,
            border: Some(if active { p.edge } else { p.panel_bg }),
        })
        .tooltip(label, None)
        .min_w(px(26.0))
        .px_0()
}

fn choice(
    ws: &Workspace,
    option: ToolOption,
    popup_id: &'static str,
    width: f32,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let OptionKind::Choice(labels) = option.kind else {
        unreachable!()
    };
    let key = option.key;
    let current = option.value.index().min(labels.len().saturating_sub(1));
    let popup = Popup::Field(popup_id);
    let spec = ui::Dropdown {
        popup,
        is_open: ws.open_popup == Some(popup),
        current,
        label: labels.get(current).copied().unwrap_or("").into(),
        width,
        options: labels
            .iter()
            .enumerate()
            .map(|(i, label)| ((*label).into(), i))
            .collect(),
    };
    let select = move |ws: &mut Workspace, value, cx: &mut Context<Workspace>| {
        ws.commit_focused_field();
        ws.set_tool_option(key, OptionValue::Choice(value), cx);
    };
    let control = if key == "type-family" {
        ui::font_dropdown(&ws.dropdown, spec, select, cx).into_any_element()
    } else {
        ui::dropdown(&ws.dropdown, spec, select, cx).into_any_element()
    };
    div()
        .id(popup_id)
        .flex_none()
        .tooltip(ui::tip(option.label, None))
        .child(control)
        .into_any_element()
}

fn number(
    ws: &Workspace,
    option: ToolOption,
    field_id: &'static str,
    width: f32,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let OptionKind::Slider { suffix, .. } = option.kind else {
        unreachable!()
    };
    let value = option.value.num();
    let value = if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    let focused = ws.focused_field == Some(field_id);
    let selected = ws.type_field_selected(field_id);
    let shown = if focused {
        ws.field_buffer.clone()
    } else {
        value.clone()
    };
    TextInput::new(field_id, shown)
        .active(focused)
        .selected(selected)
        .suffix(suffix.trim())
        .align_end()
        .w(px(width))
        .text_size(px(11.0))
        .colors(TextInputColors {
            border: Some(palette().edge),
            ..Default::default()
        })
        .tooltip(
            format!(
                "{} · Type a value, or use ↑ / ↓ (Shift for larger steps)",
                option.label
            ),
            None,
        )
        .on_focus(cx.listener(move |ws, _e, _w, cx| {
            ws.commit_focused_field();
            ws.focus_field(field_id, value.clone());
            cx.notify();
        }))
        .into_any_element()
}

fn align_buttons(current: usize, panel: bool, cx: &mut Context<Workspace>) -> gpui::Div {
    div().flex().items_center().gap(px(1.0)).children(
        [
            ("type-align-left", "Align left"),
            ("type-align-center", "Align center"),
            ("type-align-right", "Align right"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, label))| {
            compact_button(
                if panel {
                    [
                        "character-align-left",
                        "character-align-center",
                        "character-align-right",
                    ][i]
                } else {
                    name
                },
                label,
                current == i,
            )
            .on_click(cx.listener(move |ws, _e, _w, cx| {
                ws.commit_focused_field();
                ws.set_tool_option("type-align", OptionValue::Choice(i), cx);
            }))
            .child(icon(name, 16.0, palette().text))
        }),
    )
}

pub(super) fn type_options_bar(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let options = options(ws);
    let editing = ws.tool_captures_keys();
    div()
        .id("type-options-bar")
        .flex()
        .items_center()
        .gap_2()
        .h(px(36.0))
        .w_full()
        .min_w_0()
        .flex_none()
        .px_2()
        .bg(gpui::rgb(palette().panel_bg))
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            compact_button("type-tool-indicator", "Type tool (T)", false).child(icon(
                "type",
                17.0,
                palette().text,
            )),
        )
        .child(separator())
        .child(choice(
            ws,
            option(&options, "type-family"),
            "type-family",
            170.0,
            cx,
        ))
        .child(choice(
            ws,
            option(&options, "type-style"),
            "type-style",
            110.0,
            cx,
        ))
        .child(number(
            ws,
            option(&options, "type-size"),
            "type-size",
            68.0,
            cx,
        ))
        .child(separator())
        .child(align_buttons(
            option(&options, "type-align").value.index(),
            false,
            cx,
        ))
        .child(separator())
        .child(
            compact_button("type-color", "Text color", false)
                .on_click(cx.listener(|ws, _e, _w, cx| {
                    ws.commit_focused_field();
                    ws.open_color_picker(ColorTarget::Foreground, cx);
                }))
                .child(
                    div()
                        .w(px(22.0))
                        .h(px(14.0))
                        .rounded_sm()
                        .border_1()
                        .border_color(gpui::rgb(palette().text_faint))
                        .bg(swatch_hex(ws.editor.foreground)),
                ),
        )
        .child(separator())
        .child(
            compact_button(
                "show-character",
                "Character panel",
                ws.side_tab.unwrap_or(SideTab::Character) == SideTab::Character,
            )
            .on_click(cx.listener(|ws, _e, _w, cx| {
                ws.commit_focused_field();
                ws.side_tab = Some(
                    if ws.side_tab.unwrap_or(SideTab::Character) == SideTab::Character {
                        SideTab::Color
                    } else {
                        SideTab::Character
                    },
                );
                cx.notify();
            }))
            .child(icon("character", 16.0, palette().text)),
        )
        .child(separator())
        .child(
            compact_button("type-cancel", "Cancel text edit", false)
                .disabled(!editing)
                .on_click(cx.listener(move |ws, _e, _w, cx| {
                    if editing {
                        ws.commit_focused_field();
                        ws.cancel_gesture(cx);
                    }
                }))
                .child(icon("close", 16.0, palette().text)),
        )
        .child(
            compact_button("type-commit", "Commit text edit", false)
                .disabled(!editing)
                .on_click(cx.listener(move |ws, _e, _w, cx| {
                    if editing {
                        ws.commit_focused_field();
                        ws.commit_gesture(cx);
                    }
                }))
                .child(icon("check", 17.0, palette().text)),
        )
        .into_any_element()
}

fn labeled(label: &'static str, child: gpui::AnyElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(10.0))
                .text_color(gpui::rgb(palette().text_dim))
                .child(label),
        )
        .child(child)
}

pub(super) fn character_panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let options = options(ws);
    let path = option(&options, "type-path").value.bool();
    let row = || div().flex().items_center().gap_2();
    let section = |label: &'static str| {
        Heading::new(label)
            .text_size(px(10.0))
            .mt_1()
            .pt_2()
            .border_t_1()
            .border_color(gpui::rgb(palette().divider))
    };
    let mut panel = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .child(
            row()
                .child(choice(
                    ws,
                    option(&options, "type-family"),
                    "character-family",
                    144.0,
                    cx,
                ))
                .child(choice(
                    ws,
                    option(&options, "type-style"),
                    "character-style",
                    92.0,
                    cx,
                )),
        )
        .child(
            row()
                .child(labeled(
                    "Size",
                    number(
                        ws,
                        option(&options, "type-size"),
                        "character-size",
                        118.0,
                        cx,
                    ),
                ))
                .child(labeled(
                    "Leading",
                    number(
                        ws,
                        option(&options, "type-leading"),
                        "character-leading",
                        118.0,
                        cx,
                    ),
                )),
        )
        .child(
            row()
                .child(labeled(
                    "Tracking",
                    number(
                        ws,
                        option(&options, "type-tracking"),
                        "character-tracking",
                        118.0,
                        cx,
                    ),
                ))
                .child(labeled(
                    "Alignment",
                    align_buttons(option(&options, "type-align").value.index(), true, cx)
                        .into_any_element(),
                )),
        )
        .child(section("OpenType"))
        .child(
            row().children(
                [
                    ("type-kern", "AV"),
                    ("type-liga", "fi"),
                    ("type-dlig", "st"),
                    ("type-smcp", "Tt"),
                ]
                .into_iter()
                .map(|(key, glyph)| {
                    let option = option(&options, key);
                    let on = option.value.bool();
                    compact_button(key, option.label, on)
                        .w(px(55.0))
                        .h(px(28.0))
                        .text_size(px(14.0))
                        .when(on, |d| d.border_color(gpui::rgb(palette().accent)))
                        .on_click(cx.listener(move |ws, _e, _w, cx| {
                            ws.commit_focused_field();
                            ws.set_tool_option(key, OptionValue::Bool(!on), cx);
                        }))
                        .child(glyph)
                }),
            ),
        )
        .child(section("Text on a path"))
        .child(
            div().flex().gap_1().children(
                [(false, "Straight"), (true, "On path")]
                    .into_iter()
                    .map(|(on, label)| {
                        compact_button(
                            if on {
                                "character-on-path"
                            } else {
                                "character-straight"
                            },
                            if on {
                                "Use a copy of the active path"
                            } else {
                                "Use the text layer's ordinary baseline"
                            },
                            path == on,
                        )
                        .w(px(120.0))
                        .text_size(px(11.0))
                        .on_click(cx.listener(move |ws, _e, _w, cx| {
                            ws.commit_focused_field();
                            ws.set_tool_option("type-path", OptionValue::Bool(on), cx);
                        }))
                        .child(label)
                    }),
            ),
        );
    if path {
        panel = panel.child(labeled(
            "Path offset",
            number(
                ws,
                option(&options, "type-path-offset"),
                "character-path-offset",
                118.0,
                cx,
            ),
        ));
    }
    panel.into_any_element()
}
