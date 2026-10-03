//! Controls for layout geometry and the document's named text styles.
use super::*;
use crate::design::controls::{self, Target};
use schist_layout::styles::Align;
use schist_layout::{properties, CharacterStyle, ParagraphStyle};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InspectorSection {
    Basic,
    Font,
    Appearance,
    Typography,
    Decorations,
    Tabs,
    Lists,
    Keeps,
    Hyphenation,
    Style,
    Preferences,
}

struct Inspector {
    rows: std::collections::BTreeMap<InspectorSection, Vec<gpui::AnyElement>>,
    current: InspectorSection,
}
impl Inspector {
    fn new(mut header: Vec<gpui::AnyElement>) -> Self {
        let name = header.remove(1);
        let mut rows = std::collections::BTreeMap::new();
        rows.insert(InspectorSection::Basic, header);
        rows.insert(InspectorSection::Style, vec![name]);
        Self {
            rows,
            current: InspectorSection::Basic,
        }
    }
    fn group(&mut self, section: InspectorSection) {
        self.current = section;
    }
    fn push(&mut self, row: gpui::AnyElement) {
        self.rows.entry(self.current).or_default().push(row);
    }
    fn extend(&mut self, rows: Vec<gpui::AnyElement>) {
        self.rows.entry(self.current).or_default().extend(rows);
    }
    fn render(
        mut self,
        ws: &Workspace,
        paragraph: bool,
        cx: &mut Context<Workspace>,
    ) -> gpui::AnyElement {
        self.rows
            .entry(InspectorSection::Style)
            .or_default()
            .push(style_actions(paragraph, cx));
        let mut body = div().flex().flex_col().gap_2().children(
            self.rows
                .remove(&InspectorSection::Basic)
                .unwrap_or_default(),
        );
        // Font family, face, size, leading and tracking are the everyday Character controls.
        if !paragraph {
            body = body.children(
                self.rows
                    .remove(&InspectorSection::Font)
                    .unwrap_or_default(),
            );
        }
        for (section, rows) in self.rows {
            let (character_id, paragraph_id, label) = match section {
                InspectorSection::Font => ("char-font", "para-font", "design.font_family"),
                InspectorSection::Appearance => ("char-paint", "para-paint", "design.appearance"),
                InspectorSection::Typography => ("char-type", "para-type", "design.advanced_type"),
                InspectorSection::Decorations => {
                    ("char-decorations", "para-decorations", "design.decorations")
                }
                InspectorSection::Lists => ("char-lists", "para-lists", "design.list_type"),
                InspectorSection::Tabs => ("char-tabs", "para-tabs", "design.paragraph_tabs"),
                InspectorSection::Hyphenation => {
                    ("char-hyphen", "para-hyphen", "design.hyphenation")
                }
                InspectorSection::Keeps => ("char-keeps", "para-keeps", "design.keep_options"),
                InspectorSection::Style => ("char-style", "para-style", "design.style_options"),
                InspectorSection::Preferences => (
                    "char-preferences",
                    "para-preferences",
                    "design.text_preferences",
                ),
                InspectorSection::Basic => continue,
            };
            body = body.child(super::design_dock::section(
                ws,
                if paragraph {
                    paragraph_id
                } else {
                    character_id
                },
                label,
                rows,
                cx,
            ));
        }
        body.into_any_element()
    }
}

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
    let inherited = matches!(
        target,
        Target::Character(_) | Target::Paragraph(_) | Target::ObjectStyle(_)
    );
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(div().text_xs().flex_1().min_w_0().child(t(label)))
        .child(
            TextInput::new(id, shown)
                .active(focused)
                .when(inherited, |input| input.placeholder(t("design.inherited")))
                .w(px(108.0))
                .on_focus(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.focus_field(id, value.clone());
                    ws.design.controls.field = Some(target.clone());
                    cx.notify();
                })),
        )
        .into_any_element()
}
fn baseline_value(value: Option<schist_layout::styles::BaselineShift>) -> String {
    match value {
        None => String::new(),
        Some(value) => value
            .explicit_offset()
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| t("design.unsupported_text_position").to_string()),
    }
}
fn number(value: Option<f32>) -> String {
    value.map(|v| format!("{v:.2}")).unwrap_or_default()
}

fn leading_value(value: Option<schist_layout::styles::Leading>) -> String {
    match value {
        Some(schist_layout::styles::Leading::Auto) => t("design.leading_auto").to_string(),
        Some(schist_layout::styles::Leading::Points(points)) => number(Some(points)),
        None => String::new(),
    }
}

pub(super) fn control_bar(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let mut fields = Vec::new();
    if ws.design.selection.is_empty() {
        let page = ws.design.current_page();
        if let Some(p) = ws.design.document.pages.get(page) {
            for (id, label, value) in [
                ("design-prop-bar-page-width", "design.width", p.width),
                ("design-prop-bar-page-height", "design.height", p.height),
            ] {
                fields.push(field(
                    ws,
                    id,
                    label,
                    number(Some(value)),
                    Target::Pages(vec![page]),
                    cx,
                ));
            }
        }
    } else {
        for (id, label) in [
            ("design-prop-x", "design.position_x"),
            ("design-prop-y", "design.position_y"),
            ("design-prop-width", "design.width"),
            ("design-prop-height", "design.height"),
        ] {
            fields.push(object_field(ws, id, label, cx));
        }
    }
    let mut groups = Vec::new();
    let mut fields = fields.into_iter();
    while let Some(first) = fields.next() {
        groups.push(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(160.0))
                .child(first)
                .children(fields.next()),
        );
    }
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .flex_none()
        .bg(gpui::rgb(palette().panel_bg))
        .border_b_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(80.0))
                .text_xs()
                .child(t("design.mode"))
                .child(
                    div()
                        .text_color(gpui::rgb(palette().text_dim))
                        .child(t("design.ruler_pt")),
                ),
        )
        .children(groups)
        .child(
            div().flex().flex_wrap().gap_1().children(
                [
                    ("design_control", "design.properties", "adjust"),
                    ("design_character", "design.character", "character"),
                    ("design_paragraph", "design.paragraph", "type-align-left"),
                ]
                .into_iter()
                .map(|(key, label, glyph)| {
                    div()
                        .id(SharedString::from(format!("design-control-panel-{}", key)))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .h(px(26.0))
                        .cursor_pointer()
                        .text_xs()
                        .hover(|d| d.bg(gpui::rgb(palette().hover)))
                        .child(icon(glyph, 14.0, palette().text))
                        .child(t(label))
                        .on_click(
                            cx.listener(move |ws, _, _, cx| {
                                super::design_dock::select(ws, key, cx)
                            }),
                        )
                }),
            ),
        )
        .into_any_element()
}

fn object_field(
    ws: &Workspace,
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let property = controls::object_property(id).expect("control property");
    let values: Vec<_> = ws
        .design
        .selection
        .iter()
        .filter_map(|id| ws.design.document.object(*id))
        .filter_map(|o| property.value(&o.resolved_appearance(&ws.design.document.styles)))
        .collect();
    let value = values
        .first()
        .copied()
        .filter(|first| values.iter().all(|v| v == first));
    field(
        ws,
        id,
        label,
        number(value),
        Target::Objects(ws.design.selection.clone()),
        cx,
    )
}

pub(super) fn control_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() || ws.design.selection.is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    if controls::all_text_frames(&ws.design)
        && ws.design.selection.iter().all(|id| {
            ws.design.document.object(*id).is_some_and(|o| {
                matches!(
                    o.object,
                    schist_layout::LayoutObject::TextFrame {
                        text_path: None,
                        ..
                    }
                )
            })
        })
    {
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
            .is_some_and(|o| o.supports_paint())
    }) {
        fields.extend([
            ("design-prop-fill-tint", "design.fill_tint"),
            ("design-prop-stroke-tint", "design.stroke_tint"),
            ("design-prop-stroke-width", "design.stroke_width"),
        ]);
    }
    let target = Target::Objects(ws.design.selection.clone());
    let mut rows = fields
        .into_iter()
        .map(|(id, label)| {
            let field = object_field(ws, id, label, cx);
            if id != "design-prop-columns" {
                return field;
            }
            let active = ws.design.selection.iter().all(|id| {
                ws.design
                    .document
                    .object(*id)
                    .is_some_and(|object| ws.design.document.styles.frame_balance(object))
            });
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().flex_1().min_w_0().child(field))
                .child(
                    IconButton::new("design-balance-columns", "type-balance-columns")
                        .tooltip(t("design.balance_columns"), None)
                        .active(active)
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.commit_focused_field();
                            controls::toggle_frame_balance(&mut ws.design);
                            cx.notify();
                        })),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let paths: Vec<_> = ws
        .design
        .selection
        .iter()
        .filter_map(|id| match &ws.design.document.object(*id)?.object {
            schist_layout::LayoutObject::TextFrame {
                text_path: Some(path),
                ..
            } => Some(path),
            _ => None,
        })
        .collect();
    if paths.len() == ws.design.selection.len() {
        for (id, label, values) in [
            (
                "design-prop-path-start",
                "design.path_start",
                paths.iter().map(|p| Some(p.start)).collect::<Vec<_>>(),
            ),
            (
                "design-prop-path-end",
                "design.path_end",
                paths.iter().map(|p| p.end).collect(),
            ),
        ] {
            let value = values
                .first()
                .copied()
                .flatten()
                .filter(|v| values.iter().all(|p| *p == Some(*v)));
            rows.push(field(ws, id, label, number(value), target.clone(), cx));
        }
        rows.push(
            div()
                .text_xs()
                .child(t("design.path_end_help"))
                .into_any_element(),
        );
    }
    if let [id] = ws.design.selection.as_slice() {
        let id = *id;
        if ws
            .design
            .document
            .object(id)
            .is_some_and(|o| match &o.object {
                schist_layout::LayoutObject::Shape { path, .. } => {
                    schist_layout::text_path::PathText {
                        path: path.clone(),
                        start: 0.0,
                        end: None,
                    }
                    .engine_path()
                    .is_some()
                }
                _ => false,
            })
            && !ws.design.document.object_locked(id)
        {
            rows.push(
                Button::new("design-attach-path-text", t("design.text_on_path"))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.commit_focused_field();
                        if let Some(frame) = schist_layout::text_path::attach(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            id,
                        ) {
                            crate::design::tools::begin_typing(&mut ws.design, frame.object, 0);
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
    }
    rows.extend(super::object_styles::selection_paint(ws, cx));
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
            width: ws.view.panel_width.unwrap_or(300.0).max(280.0) - 50.0,
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

fn position_picker(
    ws: &Workspace,
    paragraph: bool,
    position: Option<schist_layout::styles::TextPosition>,
    shift: Option<schist_layout::styles::BaselineShift>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_layout::styles::{BaselineShift, TextPosition};
    let values = [
        None,
        Some(TextPosition::Normal),
        Some(TextPosition::Superscript),
        Some(TextPosition::Subscript),
    ];
    let position = position.or(match shift {
        Some(BaselineShift::Superscript) => Some(TextPosition::Superscript),
        Some(BaselineShift::Subscript) => Some(TextPosition::Subscript),
        _ => None,
    });
    let labels = [
        "design.inherited",
        "design.position_normal",
        "design.position_superscript",
        "design.position_subscript",
    ];
    let current = values
        .iter()
        .position(|value| *value == position)
        .unwrap_or(0);
    let popup = Popup::Field(if paragraph {
        "design-paragraph-position"
    } else {
        "design-character-position"
    });
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().text_xs().child(t("design.text_position")))
        .child(ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup,
                is_open: ws.open_popup == Some(popup),
                current,
                label: t(labels[current]).to_string().into(),
                width: 130.0,
                options: labels
                    .iter()
                    .enumerate()
                    .map(|(i, key)| (t(key).to_string().into(), i))
                    .collect(),
            },
            move |ws, index, cx| {
                ws.commit_focused_field();
                if let (Some(name), Some(position)) = (style_name(ws, paragraph), values.get(index))
                {
                    let target = if paragraph {
                        Target::Paragraph(name)
                    } else {
                        Target::Character(name)
                    };
                    controls::set_position(&mut ws.design, &target, *position);
                    cx.notify();
                }
            },
            cx,
        ))
        .into_any_element()
}

fn style_actions(paragraph: bool, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    div()
        .flex()
        .gap_1()
        .child(
            IconButton::new(
                if paragraph {
                    "design-apply-para"
                } else {
                    "design-apply-char"
                },
                "check",
            )
            .tooltip(t("design.apply_style"), None)
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
            IconButton::new(
                if paragraph {
                    "design-new-para"
                } else {
                    "design-new-char"
                },
                "plus",
            )
            .tooltip(t("design.new_style"), None)
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
    let mut rows = Inspector::new(vec![
        style_picker(ws, true, &name, cx),
        field(
            ws,
            "design-prop-style-name",
            "design.style_name",
            name.clone(),
            target.clone(),
            cx,
        ),
    ]);
    rows.group(InspectorSection::Font);
    rows.push(field(
        ws,
        "design-prop-paragraph-family",
        "design.font_family",
        style.family.clone().unwrap_or_default(),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Typography);
    rows.push(field(
        ws,
        "design-prop-paragraph-language",
        "design.language",
        controls::language_value(&ws.design.document.styles, &target),
        target.clone(),
        cx,
    ));
    rows.push(
        div()
            .text_xs()
            .child(t("design.language_help"))
            .into_any_element(),
    );
    rows.group(InspectorSection::Appearance);
    for (fill, ink, disabled) in [
        (true, &style.fill, style.fill_disabled),
        (false, &style.stroke, style.stroke_disabled),
    ] {
        let paint = if disabled {
            Some(schist_layout::Paint::None)
        } else {
            ink.clone().map(schist_layout::Paint::Ink)
        };
        rows.push(super::object_styles::paint_picker(
            ws,
            target.clone(),
            fill,
            paint,
            cx,
        ));
    }
    rows.group(InspectorSection::Typography);
    rows.push(no_break_row(ws, &target, style.no_break, cx));
    rows.push(capitalization_row(
        ws,
        &target,
        style.all_caps,
        style.small_caps,
        cx,
    ));
    rows.extend(directional_feature_rows(
        ws,
        &target,
        style.directional_features,
        cx,
    ));
    rows.group(InspectorSection::Decorations);
    rows.extend(super::text_decorations::rows(
        ws,
        &target,
        [
            (&style.underline_style, style.underline),
            (&style.strike_style, style.strikethrough),
        ],
        cx,
    ));
    rows.group(InspectorSection::Appearance);
    rows.push(text_join(ws, &target, style.stroke_join, cx));
    rows.extend(text_paint_flags(
        ws,
        &target,
        style.overprint_fill,
        style.overprint_stroke,
        style.stroke_outside,
        cx,
    ));
    rows.group(InspectorSection::Font);
    rows.push(field(
        ws,
        "design-prop-paragraph-font-style",
        "design.font_style",
        controls::font_style_value(&ws.design.document.styles, &target),
        target.clone(),
        cx,
    ));
    for (id, label, value) in [
        (
            "design-prop-paragraph-stroke-width",
            "design.stroke_width",
            style.stroke_weight,
        ),
        (
            "design-prop-paragraph-stroke-miter",
            "design.text_miter_limit",
            style.stroke_miter_limit,
        ),
        (
            "design-prop-paragraph-stroke-tint",
            "design.stroke_tint",
            style.stroke_tint.map(|v| v * 100.0),
        ),
        (
            "design-prop-paragraph-fill-tint",
            "design.fill_tint",
            style.fill_tint.map(|v| v * 100.0),
        ),
        ("design-prop-size", "design.point_size", style.point_size),
        (
            "design-prop-auto-leading",
            "design.auto_leading_percent",
            style.auto_leading,
        ),
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
        rows.group(if id.contains("stroke") || id.contains("fill-tint") {
            InspectorSection::Appearance
        } else if matches!(
            id,
            "design-prop-before"
                | "design-prop-after"
                | "design-prop-left"
                | "design-prop-right"
                | "design-prop-first"
        ) {
            InspectorSection::Basic
        } else {
            InspectorSection::Font
        });
        rows.push(field(ws, id, label, number(value), target.clone(), cx));
    }
    rows.group(InspectorSection::Typography);
    rows.push(field(
        ws,
        "design-prop-baseline",
        "design.baseline_shift",
        baseline_value(style.baseline_shift),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Font);
    rows.push(field(
        ws,
        "design-prop-leading",
        "design.leading_value",
        leading_value(style.leading),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Tabs);
    rows.extend(super::design_tabs::rows(ws, &name, cx));
    rows.group(InspectorSection::Keeps);
    rows.extend(super::paragraph_keeps::rows(ws, &name, cx));
    rows.group(InspectorSection::Hyphenation);
    rows.extend(super::hyphenation::rows(ws, &name, cx));
    rows.group(InspectorSection::Lists);
    rows.extend(list_fields(
        ws,
        &style
            .list
            .over(&schist_layout::lists::ListStyle::from_legacy(style.bullet)),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Typography);
    rows.push(position_picker(
        ws,
        true,
        style.position,
        style.baseline_shift,
        cx,
    ));
    rows.push(field(
        ws,
        "design-prop-paragraph-features",
        "design.opentype_features",
        crate::design::controls::feature_text(&style.features),
        target.clone(),
        cx,
    ));
    rows.push(
        div()
            .text_xs()
            .child(t("design.opentype_hint"))
            .into_any_element(),
    );
    let active_align = ws
        .design
        .document
        .styles
        .resolve_paragraph(&name)
        .align
        .unwrap_or_default();
    let alignments = [
        (Align::Left, "design.align_left", "type-align-left"),
        (Align::Center, "design.align_center", "type-align-center"),
        (Align::Right, "design.align_right", "type-align-right"),
        (Align::Justify, "design.justify", "type-align-justify"),
    ];
    let buttons = alignments
        .into_iter()
        .enumerate()
        .map(|(index, (align, label, icon))| {
            IconButton::new(("design-align-text", index), icon)
                .tooltip(t(label), None)
                .active(active_align == align)
                .on_click(cx.listener(move |ws, _, _, cx| {
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
                }))
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(div().text_xs().child(t("design.named_style_hint")))
            .child(div().flex().flex_wrap().gap_1().children(buttons))
            .child(rows.render(ws, true, cx))
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
    let mut rows = Inspector::new(vec![
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
    ]);
    rows.group(InspectorSection::Typography);
    rows.push(field(
        ws,
        "design-prop-language",
        "design.language",
        controls::language_value(&ws.design.document.styles, &target),
        target.clone(),
        cx,
    ));
    rows.push(
        div()
            .text_xs()
            .child(t("design.language_help"))
            .into_any_element(),
    );
    rows.group(InspectorSection::Appearance);
    for (fill, ink, disabled) in [
        (true, &style.fill, style.fill_disabled),
        (false, &style.stroke, style.stroke_disabled),
    ] {
        let paint = if disabled {
            Some(schist_layout::Paint::None)
        } else {
            ink.clone().map(schist_layout::Paint::Ink)
        };
        rows.push(super::object_styles::paint_picker(
            ws,
            target.clone(),
            fill,
            paint,
            cx,
        ));
    }
    rows.group(InspectorSection::Typography);
    rows.push(no_break_row(ws, &target, style.no_break, cx));
    rows.push(capitalization_row(
        ws,
        &target,
        style.all_caps,
        style.small_caps,
        cx,
    ));
    rows.extend(directional_feature_rows(
        ws,
        &target,
        style.directional_features,
        cx,
    ));
    rows.group(InspectorSection::Decorations);
    rows.extend(super::text_decorations::rows(
        ws,
        &target,
        [
            (&style.underline_style, style.underline),
            (&style.strike_style, style.strikethrough),
        ],
        cx,
    ));
    rows.group(InspectorSection::Appearance);
    rows.push(text_join(ws, &target, style.stroke_join, cx));
    rows.extend(text_paint_flags(
        ws,
        &target,
        style.overprint_fill,
        style.overprint_stroke,
        style.stroke_outside,
        cx,
    ));
    rows.group(InspectorSection::Font);
    rows.push(field(
        ws,
        "design-prop-font-style",
        "design.font_style",
        controls::font_style_value(&ws.design.document.styles, &target),
        target.clone(),
        cx,
    ));
    for (id, label, value) in [
        (
            "design-prop-char-stroke-width",
            "design.stroke_width",
            style.stroke_weight,
        ),
        (
            "design-prop-char-stroke-miter",
            "design.text_miter_limit",
            style.stroke_miter_limit,
        ),
        (
            "design-prop-char-stroke-tint",
            "design.stroke_tint",
            style.stroke_tint.map(|v| v * 100.0),
        ),
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
        (
            "design-prop-char-tracking",
            "design.tracking",
            style.tracking,
        ),
    ] {
        rows.group(if id.contains("stroke") || id.contains("fill-tint") {
            InspectorSection::Appearance
        } else {
            InspectorSection::Font
        });
        rows.push(field(ws, id, label, number(value), target.clone(), cx));
    }
    rows.group(InspectorSection::Typography);
    rows.push(field(
        ws,
        "design-prop-char-baseline",
        "design.baseline_shift",
        baseline_value(style.baseline_shift),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Font);
    rows.push(field(
        ws,
        "design-prop-char-leading",
        "design.leading_value",
        leading_value(style.leading),
        target.clone(),
        cx,
    ));
    rows.group(InspectorSection::Typography);
    rows.push(position_picker(
        ws,
        false,
        style.position,
        style.baseline_shift,
        cx,
    ));
    rows.push(field(
        ws,
        "design-prop-char-features",
        "design.opentype_features",
        crate::design::controls::feature_text(&style.features),
        target.clone(),
        cx,
    ));
    rows.push(
        div()
            .text_xs()
            .child(t("design.opentype_hint"))
            .into_any_element(),
    );
    rows.group(InspectorSection::Preferences);
    let prefs = ws.design.document.styles.text_preferences;
    for (id, label, value) in [
        (
            "design-prop-small-cap-size",
            "design.small_cap_size",
            prefs.small_cap_size,
        ),
        (
            "design-prop-superscript-size",
            "design.superscript_size",
            prefs.superscript_size,
        ),
        (
            "design-prop-superscript-position",
            "design.superscript_position",
            prefs.superscript_position,
        ),
        (
            "design-prop-subscript-size",
            "design.subscript_size",
            prefs.subscript_size,
        ),
        (
            "design-prop-subscript-position",
            "design.subscript_position",
            prefs.subscript_position,
        ),
    ] {
        rows.push(field(
            ws,
            id,
            label,
            number(Some(value)),
            Target::TextPreferences,
            cx,
        ));
    }
    let resolved = ws.design.document.styles.resolve_character(&name);
    let buttons = [
        ("design.bold", "type-bold", resolved.bold),
        ("design.italic", "type-italic", resolved.italic),
        ("design.underline", "type-underline", resolved.underline),
        (
            "design.strikethrough",
            "type-strike",
            resolved.strikethrough,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, icon, active))| {
        IconButton::new(("design-char-toggle", index), icon)
            .tooltip(t(label), None)
            .active(active.unwrap_or(false))
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.commit_focused_field();
                let Some(name) = style_name(ws, false) else {
                    return;
                };
                properties::edit_styles(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    |styles| {
                        let resolved = styles.resolve_character(&name);
                        if let Some(style) = styles.characters.iter_mut().find(|s| s.name == name) {
                            match index {
                                0 => {
                                    style.font_style = None;
                                    style.bold = Some(!resolved.bold.unwrap_or(false));
                                    style.italic = Some(resolved.italic.unwrap_or(false));
                                }
                                1 => {
                                    style.font_style = None;
                                    style.italic = Some(!resolved.italic.unwrap_or(false));
                                    style.bold = Some(resolved.bold.unwrap_or(false));
                                }
                                2 => style.underline = Some(!resolved.underline.unwrap_or(false)),
                                _ => {
                                    style.strikethrough =
                                        Some(!resolved.strikethrough.unwrap_or(false))
                                }
                            }
                        }
                    },
                );
                cx.notify();
            }))
    })
    .collect::<Vec<_>>();
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(div().text_xs().child(t("design.named_style_hint")))
            .child(div().flex().flex_wrap().gap_1().children(buttons))
            .child(rows.render(ws, false, cx))
            .into_any_element(),
    )
}

fn text_paint_flags(
    ws: &Workspace,
    target: &Target,
    fill: Option<bool>,
    stroke: Option<bool>,
    outside: Option<bool>,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let paragraph = matches!(target, Target::Paragraph(_));
    [
        (
            "fill",
            "design.overprint_fill",
            fill,
            if paragraph {
                "design-paragraph-overprint-fill"
            } else {
                "design-char-overprint-fill"
            },
        ),
        (
            "stroke",
            "design.overprint_stroke",
            stroke,
            if paragraph {
                "design-paragraph-overprint-stroke"
            } else {
                "design-char-overprint-stroke"
            },
        ),
        (
            "outside",
            "design.text_stroke_outside",
            outside,
            if paragraph {
                "design-paragraph-stroke-outside"
            } else {
                "design-char-stroke-outside"
            },
        ),
    ]
    .into_iter()
    .map(|(flag, label, value, id)| {
        let target = target.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().child(t(label)))
            .child(super::object_styles::picker(
                ws,
                id,
                vec![
                    t("design.inherited").into(),
                    t("design.enabled").into(),
                    t("design.disabled").into(),
                ],
                match value {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                },
                move |ws, index, _| {
                    controls::set_text_paint_flag(
                        &mut ws.design,
                        &target,
                        flag,
                        match index {
                            1 => Some(true),
                            2 => Some(false),
                            _ => None,
                        },
                    );
                },
                cx,
            ))
            .into_any_element()
    })
    .collect()
}

fn text_join(
    ws: &mut Workspace,
    target: &Target,
    value: Option<schist_text_engine::TextStrokeJoin>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_text_engine::TextStrokeJoin;
    let values = [
        None,
        Some(TextStrokeJoin::Miter),
        Some(TextStrokeJoin::Round),
        Some(TextStrokeJoin::Bevel),
    ];
    let index = values.iter().position(|v| *v == value).unwrap_or(0);
    let target = target.clone();
    let id = if matches!(target, Target::Paragraph(_)) {
        "design-paragraph-stroke-join"
    } else {
        "design-char-stroke-join"
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.text_stroke_join")))
        .child(super::object_styles::picker(
            ws,
            id,
            [
                "design.inherited",
                "design.join_miter",
                "design.join_round",
                "design.join_bevel",
            ]
            .into_iter()
            .map(|key| t(key).into())
            .collect(),
            index,
            move |ws, index, _| {
                if let Some(value) = values.get(index) {
                    controls::set_text_join(&mut ws.design, &target, *value);
                }
            },
            cx,
        ))
        .into_any_element()
}

fn directional_feature_rows(
    ws: &mut Workspace,
    target: &Target,
    value: schist_layout::directional_features::DirectionalFeatures,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let paragraph = matches!(target, Target::Paragraph(_));
    [
        (false, "design.cjk_kana_forms", value.kana),
        (
            true,
            "design.cjk_proportional_metrics",
            value.proportional_metrics,
        ),
    ]
    .into_iter()
    .map(|(proportional, label, value)| {
        let target = target.clone();
        let id = match (paragraph, proportional) {
            (true, false) => "design-para-kana",
            (true, true) => "design-para-proportional",
            (false, false) => "design-char-kana",
            (false, true) => "design-char-proportional",
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().child(t(label)))
            .child(super::object_styles::picker(
                ws,
                id,
                ["design.inherited", "design.enabled", "design.disabled"]
                    .into_iter()
                    .map(|key| t(key).into())
                    .collect(),
                match value {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                },
                move |ws, index, _| {
                    controls::set_directional_feature(
                        &mut ws.design,
                        &target,
                        proportional,
                        match index {
                            1 => Some(true),
                            2 => Some(false),
                            _ => None,
                        },
                    );
                },
                cx,
            ))
            .into_any_element()
    })
    .collect()
}

fn no_break_row(
    ws: &Workspace,
    target: &Target,
    own: Option<bool>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let resolved = match target {
        Target::Paragraph(name) => {
            ws.design
                .document
                .styles
                .resolve_paragraph(name)
                .character(
                    ws.design
                        .document
                        .styles
                        .resolve_character(&ws.design.document.default_character_style),
                )
                .no_break
        }
        Target::Character(name) => ws.design.document.styles.resolve_character(name).no_break,
        _ => None,
    };
    let active = resolved == Some(true);
    let toggle = target.clone();
    let reset = target.clone();
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            IconButton::new("design-no-break", "link")
                .active(active)
                .tooltip(
                    t("design.no_break"),
                    own.is_none().then(|| t("design.inherited").into()),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    controls::set_no_break(&mut ws.design, &toggle, Some(!active));
                    cx.notify();
                })),
        )
        .child(div().text_xs().flex_1().child(t("design.no_break")))
        .child(
            IconButton::new("design-no-break-inherit", "undo")
                .disabled(own.is_none())
                .tooltip(t("design.inherited"), None)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    controls::set_no_break(&mut ws.design, &reset, None);
                    cx.notify();
                })),
        )
        .into_any_element()
}

fn capitalization_row(
    ws: &mut Workspace,
    target: &Target,
    all: Option<bool>,
    small: Option<bool>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_text_engine::Capitalization::*;
    let values = [
        None,
        Some(Normal),
        Some(AllCaps),
        Some(SmallCaps),
        Some(OpenTypeAllSmallCaps),
    ];
    let selected = match (all, small) {
        (None, None) => 0,
        (Some(all), Some(small)) => values
            .iter()
            .position(|v| *v == Some(schist_text_engine::Capitalization::from_flags(all, small)))
            .unwrap(),
        _ => 5,
    };
    let mut labels: Vec<_> = [
        "design.inherited",
        "design.caps_normal",
        "design.caps_all",
        "design.caps_small",
        "design.caps_all_small",
    ]
    .into_iter()
    .map(|key| t(key).into())
    .collect();
    if selected == 5 {
        labels.push(t("design.partial_inheritance").into());
    }
    let id = if matches!(target, Target::Paragraph(_)) {
        "design-para-caps"
    } else {
        "design-char-caps"
    };
    let target = target.clone();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.capitalization")))
        .child(super::object_styles::picker(
            ws,
            id,
            labels,
            selected,
            move |ws, index, _| {
                if let Some(value) = values.get(index) {
                    controls::set_capitalization(&mut ws.design, &target, *value);
                }
            },
            cx,
        ))
        .into_any_element()
}

fn list_fields(
    ws: &mut Workspace,
    list: &schist_layout::lists::ListStyle,
    target: Target,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    use schist_layout::lists::ListKind;
    let mut rows = vec![div()
        .text_xs()
        .child(t("design.list_type"))
        .into_any_element()];
    let choices = [
        None,
        Some(ListKind::None),
        Some(ListKind::Bullet),
        Some(ListKind::Numbered),
    ];
    let captured = target.clone();
    rows.push(super::object_styles::picker(
        ws,
        "design-list-kind",
        [
            "design.inherited",
            "design.list_none",
            "design.list_bullets",
            "design.list_numbers",
        ]
        .into_iter()
        .map(|k| t(k).into())
        .collect(),
        choices.iter().position(|v| *v == list.kind).unwrap_or(0),
        move |ws, i, _| {
            if let Some(value) = choices.get(i) {
                controls::set_list_kind(&mut ws.design, &captured, *value);
            }
        },
        cx,
    ));
    if list.kind == Some(ListKind::Numbered) {
        rows.extend(list_sequence_fields(ws, list, target.clone(), cx));
    }
    for (id, label, value) in [
        (
            "design-prop-list-level",
            "design.list_level",
            list.level.map(|v| v.to_string()).unwrap_or_default(),
        ),
        (
            "design-prop-list-bullet",
            "design.list_bullet",
            controls::list_bullet_value(list),
        ),
        (
            "design-prop-list-start",
            "design.list_start",
            list.start.map(|v| v.to_string()).unwrap_or_default(),
        ),
        (
            "design-prop-list-expression",
            "design.list_expression",
            list.expression.clone().unwrap_or_default(),
        ),
    ] {
        rows.push(field(ws, id, label, value, target.clone(), cx));
    }
    rows.extend(list_format_field(ws, list, target.clone(), cx));
    rows.extend(list_restart_field(ws, list, target.clone(), cx));
    let captured = target;
    let choices = [None, Some(true), Some(false)];
    rows.push(super::object_styles::picker(
        ws,
        "design-list-continuation",
        [
            "design.inherited",
            "design.list_continue",
            "design.list_restart",
        ]
        .into_iter()
        .map(|k| t(k).into())
        .collect(),
        choices
            .iter()
            .position(|v| *v == list.continue_numbering)
            .unwrap_or(0),
        move |ws, i, _| {
            if let Some(value) = choices.get(i) {
                controls::edit_list(&mut ws.design, &captured, |list| {
                    list.continue_numbering = *value
                });
            }
        },
        cx,
    ));
    rows.push(
        div()
            .text_xs()
            .child(t("design.list_hint"))
            .into_any_element(),
    );
    rows
}

fn list_sequence_fields(
    ws: &mut Workspace,
    list: &schist_layout::lists::ListStyle,
    target: Target,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let default = "NumberingList/$ID/[Default]";
    let mut choices = vec![None, Some(default.to_owned())];
    let mut labels = vec![
        t("design.inherited").to_owned(),
        t("design.list_default").to_owned(),
    ];
    for resource in &ws.design.document.styles.numbering_lists {
        if resource.id != default {
            choices.push(Some(resource.id.clone()));
            labels.push(resource.name.clone());
        }
    }
    // Retain an unresolved imported identity until explicitly replaced.
    if let Some(id) = &list.list {
        if !choices.iter().any(|choice| choice.as_ref() == Some(id)) {
            choices.push(Some(id.clone()));
            labels.push(id.clone());
        }
    }
    let selected = choices
        .iter()
        .position(|choice| *choice == list.list)
        .unwrap_or(0);
    let captured = target.clone();
    let mut rows = vec![
        div()
            .text_xs()
            .child(t("design.list_sequence"))
            .into_any_element(),
        super::object_styles::picker(
            ws,
            "design-list-sequence",
            labels,
            selected,
            move |ws, index, _| {
                if let Some(value) = choices.get(index) {
                    controls::edit_list(&mut ws.design, &captured, |list| {
                        list.list = value.clone()
                    });
                }
            },
            cx,
        ),
    ];
    let Target::Paragraph(name) = &target else {
        return rows;
    };
    let resolved = ws.design.document.styles.resolve_paragraph(name).list;
    let id = schist_layout::list_counters::sequence_id(&resolved);
    let enabled = ws
        .design
        .document
        .styles
        .numbering_lists
        .iter()
        .find(|r| r.id == id)
        .is_some_and(|r| r.across_stories);
    rows.push(
        div()
            .text_xs()
            .child(t("design.list_across_stories"))
            .into_any_element(),
    );
    rows.push(super::object_styles::picker(
        ws,
        "design-list-across-stories",
        vec![t("common.off").into(), t("common.on").into()],
        usize::from(enabled),
        move |ws, index, _| {
            if index < 2 {
                controls::set_list_across_stories(&mut ws.design, &target, index == 1);
            }
        },
        cx,
    ));
    rows
}

fn list_format_field(
    ws: &mut Workspace,
    list: &schist_layout::lists::ListStyle,
    target: Target,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    use schist_layout::list_numbering::CounterFormat;
    let choices = std::iter::once(None)
        .chain(CounterFormat::ALL.into_iter().map(Some))
        .collect::<Vec<_>>();
    let mut labels = [
        "design.inherited",
        "design.list_decimal",
        "design.list_roman_upper",
        "design.list_roman_lower",
        "design.list_letters_upper",
        "design.list_letters_lower",
        "design.list_zero_one",
        "design.list_zero_two",
        "design.list_zero_three",
        "design.list_no_number",
    ]
    .into_iter()
    .map(|key| t(key).into())
    .collect::<Vec<String>>();
    let selected = if let Some(native) = &list.format {
        if let Some(format) = native.counter_format() {
            choices.iter().position(|v| *v == Some(format)).unwrap_or(0)
        } else {
            labels.push(native.value().to_owned());
            labels.len() - 1
        }
    } else {
        0
    };
    vec![
        div()
            .text_xs()
            .child(t("design.list_format"))
            .into_any_element(),
        super::object_styles::picker(
            ws,
            "design-list-format",
            labels,
            selected,
            move |ws, i, _| {
                if let Some(format) = choices.get(i) {
                    controls::set_list_format(&mut ws.design, &target, *format);
                }
            },
            cx,
        ),
    ]
}

fn list_restart_field(
    ws: &mut Workspace,
    list: &schist_layout::lists::ListStyle,
    target: Target,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let choices = [None, Some(true), Some(false)];
    let mut labels = [
        "design.inherited",
        "design.list_restart_higher",
        "design.list_keep_level",
    ]
    .into_iter()
    .map(|key| t(key).into())
    .collect::<Vec<String>>();
    let selected = if let Some(policy) = list.restart_policy.as_ref().filter(|p| {
        list.apply_restart_policy != Some(false)
            && (p.policy != "AnyPreviousLevel" || p.lower != 0 || p.upper != 0)
    }) {
        labels.push(policy.policy.clone());
        labels.len() - 1
    } else {
        choices
            .iter()
            .position(|v| *v == list.apply_restart_policy)
            .unwrap_or(0)
    };
    vec![
        div()
            .text_xs()
            .child(t("design.list_restart_policy"))
            .into_any_element(),
        super::object_styles::picker(
            ws,
            "design-list-restart-policy",
            labels,
            selected,
            move |ws, i, _| {
                if let Some(value) = choices.get(i) {
                    controls::set_list_restart_policy(&mut ws.design, &target, *value);
                }
            },
            cx,
        ),
    ]
}
