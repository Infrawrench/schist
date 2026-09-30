//! Controls for layout geometry and the document's named text styles.
use super::*;
use crate::design::controls::{self, Target};
use schist_layout::styles::Align;
use schist_layout::{properties, CharacterStyle, ParagraphStyle};

pub(super) fn field(
    ws: &Workspace,
    id: &'static str,
    label: &'static str,
    value: String,
    target: Target,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let focused = ws.focused_field == Some(id);
    let shown = if focused {
        ws.field_buffer.clone()
    } else {
        value.clone()
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().text_xs().flex_1().child(t(label)))
        .child(
            TextInput::new(id, shown)
                .active(focused)
                .w(px(130.0))
                .on_focus(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.focus_field(id, value.clone());
                    ws.design.controls.field = Some(target.clone());
                    cx.notify();
                })),
        )
        .into_any_element()
}
fn number(value: Option<f32>) -> String {
    value.map(|v| format!("{v:.2}")).unwrap_or_default()
}

pub(super) fn control_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() || ws.design.selection.is_empty() {
        return None;
    }
    let mut fields = vec![
        ("design-prop-x", "design.position_x"),
        ("design-prop-y", "design.position_y"),
        ("design-prop-width", "design.width"),
        ("design-prop-height", "design.height"),
    ];
    if controls::all_text_frames(&ws.design) {
        fields.extend([
            ("design-prop-columns", "design.columns"),
            ("design-prop-gutter", "design.gutter"),
            ("design-prop-inset", "design.inset"),
        ]);
    }
    if ws.design.selection.iter().all(|id| {
        ws.design
            .document
            .object(*id)
            .is_some_and(|o| matches!(o.object, schist_layout::LayoutObject::Shape { .. }))
    }) {
        fields.extend([
            ("design-prop-fill-tint", "design.fill_tint"),
            ("design-prop-stroke-tint", "design.stroke_tint"),
        ]);
    }
    let target = Target::Objects(ws.design.selection.clone());
    let rows = fields
        .into_iter()
        .map(|(id, label)| {
            let property = controls::object_property(id).expect("control property");
            let values: Vec<_> = ws
                .design
                .selection
                .iter()
                .filter_map(|id| ws.design.document.object(*id))
                .filter_map(|o| property.value(o))
                .collect();
            let first = values.first().copied();
            let value = first.filter(|first| values.iter().all(|v| v == first));
            field(ws, id, label, number(value), target.clone(), cx)
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(div().text_xs().child(t("design.control_units")))
            .children(rows)
            .into_any_element(),
    )
}

fn style_name(ws: &Workspace, paragraph: bool) -> Option<String> {
    let doc = &ws.design.document;
    if paragraph {
        ws.design
            .controls
            .paragraph
            .as_ref()
            .filter(|name| doc.styles.paragraph(name).is_some())
            .cloned()
            .or_else(|| {
                doc.styles
                    .paragraphs
                    .first()
                    .map(|style| style.name.clone())
            })
    } else {
        ws.design
            .controls
            .character
            .as_ref()
            .filter(|name| doc.styles.character(name).is_some())
            .cloned()
            .or_else(|| {
                doc.styles
                    .characters
                    .first()
                    .map(|style| style.name.clone())
            })
    }
}

fn style_picker(
    ws: &Workspace,
    paragraph: bool,
    selected: &str,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let names: Vec<_> = if paragraph {
        ws.design
            .document
            .styles
            .paragraphs
            .iter()
            .map(|s| s.name.clone())
            .collect()
    } else {
        ws.design
            .document
            .styles
            .characters
            .iter()
            .map(|s| s.name.clone())
            .collect()
    };
    let current = names.iter().position(|name| name == selected).unwrap_or(0);
    let popup = Popup::Field(if paragraph {
        "design-paragraph-style"
    } else {
        "design-character-style"
    });
    ui::dropdown(
        &ws.dropdown,
        ui::Dropdown {
            popup,
            is_open: ws.open_popup == Some(popup),
            current,
            label: selected.to_owned().into(),
            width: 240.0,
            options: names
                .iter()
                .enumerate()
                .map(|(i, name)| (name.clone().into(), i))
                .collect(),
        },
        move |ws, index, cx| {
            ws.commit_focused_field();
            if paragraph {
                ws.design.controls.paragraph = names.get(index).cloned();
            } else {
                ws.design.controls.character = names.get(index).cloned();
            }
            cx.notify();
        },
        cx,
    )
    .into_any_element()
}

fn style_actions(paragraph: bool, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    div()
        .flex()
        .gap_1()
        .child(
            Button::new(
                if paragraph {
                    "design-apply-para"
                } else {
                    "design-apply-char"
                },
                t("design.apply_style"),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                let Some(name) = style_name(ws, paragraph) else {
                    return;
                };
                if paragraph {
                    schist_layout::authoring::set_paragraph_style(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        &ws.design.selection,
                        &name,
                    );
                } else if let Some(typing) = ws.design.typing {
                    let text = schist_layout::authoring::text_of(&ws.design.document, typing.story);
                    let (start, end) = super::styles::word_around(&text, typing.at);
                    schist_layout::authoring::set_character_style(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        typing.story,
                        start..end,
                        &name,
                    );
                }
                cx.notify();
            })),
        )
        .child(
            Button::new(
                if paragraph {
                    "design-new-para"
                } else {
                    "design-new-char"
                },
                t("design.new_style"),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                let mut n = 1;
                let name = loop {
                    let name = schist_i18n::tf!("design.style_number", number = n);
                    let exists = if paragraph {
                        ws.design.document.styles.paragraph(&name).is_some()
                    } else {
                        ws.design.document.styles.character(&name).is_some()
                    };
                    if !exists {
                        break name;
                    }
                    n += 1;
                };
                properties::edit_styles(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    |styles| {
                        if paragraph {
                            styles.add_paragraph(ParagraphStyle {
                                name: name.clone(),
                                ..Default::default()
                            });
                        } else {
                            styles.add_character(CharacterStyle {
                                name: name.clone(),
                                ..Default::default()
                            });
                        }
                    },
                );
                if paragraph {
                    ws.design.controls.paragraph = Some(name);
                } else {
                    ws.design.controls.character = Some(name);
                }
                cx.notify();
            })),
        )
        .into_any_element()
}

pub(super) fn paragraph_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let name = style_name(ws, true)?;
    let style = ws.design.document.styles.paragraph(&name)?.clone();
    let target = Target::Paragraph(name.clone());
    let mut rows = vec![
        style_picker(ws, true, &name, cx),
        field(
            ws,
            "design-prop-style-name",
            "design.style_name",
            name.clone(),
            target.clone(),
            cx,
        ),
    ];
    rows.push(field(
        ws,
        "design-prop-paragraph-family",
        "design.font_family",
        style.family.clone().unwrap_or_default(),
        target.clone(),
        cx,
    ));
    for (id, label, value) in [
        (
            "design-prop-paragraph-fill-tint",
            "design.fill_tint",
            style.fill_tint.map(|v| v * 100.0),
        ),
        ("design-prop-size", "design.point_size", style.point_size),
        ("design-prop-leading", "design.leading", style.leading),
        ("design-prop-tracking", "design.tracking", style.tracking),
        (
            "design-prop-before",
            "design.space_before",
            style.space_before,
        ),
        ("design-prop-after", "design.space_after", style.space_after),
        ("design-prop-left", "design.indent_left", style.left_indent),
        (
            "design-prop-right",
            "design.indent_right",
            style.right_indent,
        ),
        (
            "design-prop-first",
            "design.indent_first",
            style.first_line_indent,
        ),
    ] {
        rows.push(field(ws, id, label, number(value), target.clone(), cx));
    }
    let alignments = [
        (Align::Left, "design.align_left"),
        (Align::Center, "design.align_center"),
        (Align::Right, "design.align_right"),
        (Align::Justify, "design.justify"),
    ];
    let buttons = alignments
        .into_iter()
        .enumerate()
        .map(|(index, (align, label))| {
            Button::new(("design-align-text", index), t(label)).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    let Some(name) = style_name(ws, true) else {
                        return;
                    };
                    properties::edit_styles(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        |styles| {
                            if let Some(style) =
                                styles.paragraphs.iter_mut().find(|s| s.name == name)
                            {
                                style.align = Some(align);
                            }
                        },
                    );
                    cx.notify();
                },
            ))
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(div().text_xs().child(t("design.named_style_hint")))
            .children(rows)
            .child(div().flex().flex_wrap().gap_1().children(buttons))
            .child(style_actions(true, cx))
            .into_any_element(),
    )
}

pub(super) fn character_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let name = style_name(ws, false)?;
    let style = ws.design.document.styles.character(&name)?.clone();
    let target = Target::Character(name.clone());
    let mut rows = vec![
        style_picker(ws, false, &name, cx),
        field(
            ws,
            "design-prop-char-name",
            "design.style_name",
            name.clone(),
            target.clone(),
            cx,
        ),
        field(
            ws,
            "design-prop-family",
            "design.font_family",
            style.family.clone().unwrap_or_default(),
            target.clone(),
            cx,
        ),
    ];
    for (id, label, value) in [
        (
            "design-prop-char-fill-tint",
            "design.fill_tint",
            style.fill_tint.map(|v| v * 100.0),
        ),
        (
            "design-prop-char-size",
            "design.point_size",
            style.point_size,
        ),
        ("design-prop-char-leading", "design.leading", style.leading),
        (
            "design-prop-char-tracking",
            "design.tracking",
            style.tracking,
        ),
    ] {
        rows.push(field(ws, id, label, number(value), target.clone(), cx));
    }
    let buttons = ["design.bold", "design.italic", "design.underline"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            Button::new(("design-char-toggle", index), t(label)).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    let Some(name) = style_name(ws, false) else {
                        return;
                    };
                    properties::edit_styles(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        |styles| {
                            let resolved = styles.resolve_character(&name);
                            if let Some(style) =
                                styles.characters.iter_mut().find(|s| s.name == name)
                            {
                                match index {
                                    0 => {
                                        style.bold = Some(!resolved.bold.unwrap_or(false));
                                        style.italic = Some(resolved.italic.unwrap_or(false));
                                    }
                                    1 => {
                                        style.italic = Some(!resolved.italic.unwrap_or(false));
                                        style.bold = Some(resolved.bold.unwrap_or(false));
                                    }
                                    _ => {
                                        style.underline = Some(!resolved.underline.unwrap_or(false))
                                    }
                                }
                            }
                        },
                    );
                    cx.notify();
                },
            ))
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(div().text_xs().child(t("design.named_style_hint")))
            .children(rows)
            .child(div().flex().flex_wrap().gap_1().children(buttons))
            .child(style_actions(false, cx))
            .into_any_element(),
    )
}
