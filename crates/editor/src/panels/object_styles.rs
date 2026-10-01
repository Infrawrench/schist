//! Shared frame paints and editable paint categories in the Styles panel.
use super::*;
use crate::design::controls::Target;
use schist_layout::{object_styles, properties, ObjectPaint, ObjectStyle, Paint};

pub(super) fn picker(
    ws: &Workspace,
    id: &'static str,
    labels: Vec<String>,
    current: usize,
    apply: impl Fn(&mut Workspace, usize, &mut Context<Workspace>) + Clone + 'static,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let popup = Popup::Field(id);
    ui::dropdown(
        &ws.dropdown,
        ui::Dropdown {
            popup,
            is_open: ws.open_popup == Some(popup),
            current,
            label: labels.get(current).cloned().unwrap_or_default().into(),
            width: ws.view.panel_width.unwrap_or(300.0).max(280.0) - 50.0,
            options: labels
                .into_iter()
                .enumerate()
                .map(|(i, s)| (s.into(), i))
                .collect(),
        },
        move |ws, index, cx| {
            ws.commit_focused_field();
            apply(ws, index, cx);
            cx.notify();
        },
        cx,
    )
    .into_any_element()
}

pub(super) fn paint_picker(
    ws: &Workspace,
    target: Target,
    fill: bool,
    value: Option<Paint>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let style = matches!(
        target,
        Target::ObjectStyle(_) | Target::Paragraph(_) | Target::Character(_)
    );
    let mut values = if style {
        vec![None, Some(Paint::None)]
    } else {
        vec![Some(Paint::None)]
    };
    let mut labels = if style {
        vec![t("design.inherited").into(), t("design.no_paint").into()]
    } else {
        vec![t("design.no_paint").into()]
    };
    for ink in &ws.design.document.inks {
        values.push(Some(Paint::Ink(ink.clone())));
        labels.push(ink.name.clone());
    }
    if !values.contains(&value) {
        values.push(value.clone());
        labels.push(
            value
                .as_ref()
                .and_then(Paint::ink)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| t("design.mixed_paint").into()),
        );
    }
    let current = values.iter().position(|v| *v == value).unwrap_or(0);
    let id = match (&target, fill) {
        (Target::Paragraph(_), true) => "design-paragraph-fill",
        (Target::Paragraph(_), false) => "design-paragraph-stroke",
        (Target::Character(_), true) => "design-character-fill",
        (Target::Character(_), false) => "design-character-stroke",
        (Target::ObjectStyle(_), true) => "design-object-style-fill",
        (Target::ObjectStyle(_), false) => "design-object-style-stroke",
        (_, true) => "design-object-fill",
        (_, false) => "design-object-stroke",
    };
    let text = matches!(target, Target::Paragraph(_) | Target::Character(_));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t(if text && fill {
            "design.text_fill"
        } else if text {
            "design.text_stroke"
        } else if fill {
            "design.frame_fill"
        } else {
            "design.frame_stroke"
        })))
        .child(picker(
            ws,
            id,
            labels,
            current,
            move |ws, index, _| {
                let Some(value) = values.get(index).cloned() else {
                    return;
                };
                match &target {
                    Target::Paragraph(_) | Target::Character(_) => {
                        crate::design::controls::set_text_paint(
                            &mut ws.design,
                            &target,
                            fill,
                            value,
                        );
                    }
                    Target::ObjectStyle(name) => {
                        properties::edit_styles(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            |styles| {
                                if let Some(style) =
                                    styles.objects.iter_mut().find(|s| s.name == *name)
                                {
                                    if fill {
                                        style.paint.fill = value;
                                    } else {
                                        style.paint.stroke = value;
                                    }
                                }
                            },
                        );
                    }
                    Target::Objects(ids) => {
                        let mut paint = ObjectPaint::default();
                        if fill {
                            paint.fill = value;
                        } else {
                            paint.stroke = value;
                        }
                        object_styles::edit_paint(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            ids,
                            &paint,
                        );
                    }
                    _ => {}
                }
            },
            cx,
        ))
        .into_any_element()
}

pub(super) fn selection_paint(
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let objects: Vec<_> = ws
        .design
        .selection
        .iter()
        .filter_map(|id| ws.design.document.object(*id))
        .collect();
    if objects.is_empty() || objects.iter().any(|o| !o.supports_paint()) {
        return Vec::new();
    }
    let paints: Vec<_> = objects
        .iter()
        .map(|o| ws.design.document.styles.object_paint(o))
        .collect();
    [true, false]
        .into_iter()
        .map(|fill| {
            let values: Vec<_> = paints
                .iter()
                .map(|p| if fill { &p.fill } else { &p.stroke })
                .collect();
            let value = if values.iter().all(|v| *v == values[0]) {
                values[0].clone().or(Some(Paint::None))
            } else {
                None
            };
            paint_picker(
                ws,
                Target::Objects(ws.design.selection.clone()),
                fill,
                value,
                cx,
            )
        })
        .collect()
}

pub(super) fn style_controls(ws: &Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let styles = &ws.design.document.styles.objects;
    let name = ws
        .design
        .controls
        .object_style
        .as_ref()
        .filter(|n| styles.iter().any(|s| s.name == **n))
        .or_else(|| styles.first().map(|s| &s.name))
        .cloned();
    let mut out = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().child(t("design.object_styles")));
    if let Some(style) = name
        .as_deref()
        .and_then(|n| ws.design.document.styles.object_style(n))
        .cloned()
    {
        let names: Vec<_> = styles.iter().map(|s| s.name.clone()).collect();
        let current = names.iter().position(|n| *n == style.name).unwrap_or(0);
        out = out.child(picker(
            ws,
            "design-object-style-picker",
            names.clone(),
            current,
            move |ws, index, _| {
                ws.design.controls.object_style = names.get(index).cloned();
            },
            cx,
        ));
        let target = Target::ObjectStyle(style.name.clone());
        for (id, label, value) in [
            (
                "design-prop-object-name",
                "design.style_name",
                style.name.clone(),
            ),
            (
                "design-prop-object-base",
                "design.based_on",
                style.based_on.clone().unwrap_or_default(),
            ),
            (
                "design-prop-object-stroke-width",
                "design.stroke_width",
                style
                    .paint
                    .stroke_width
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            ),
            (
                "design-prop-object-fill-tint",
                "design.fill_tint",
                style
                    .paint
                    .fill_tint
                    .map(|v| (v * 100.0).to_string())
                    .unwrap_or_default(),
            ),
            (
                "design-prop-object-stroke-tint",
                "design.stroke_tint",
                style
                    .paint
                    .stroke_tint
                    .map(|v| (v * 100.0).to_string())
                    .unwrap_or_default(),
            ),
        ] {
            out = out.child(super::design_controls::field(
                ws,
                id,
                label,
                value,
                target.clone(),
                cx,
            ));
        }
        for fill in [true, false] {
            let name = style.name.clone();
            let value = if fill {
                style.enable_fill
            } else {
                style.enable_stroke
            };
            let labels = [
                "design.inherited",
                "design.style_category_off",
                "design.style_category_on",
            ]
            .map(|k| t(k).to_string())
            .to_vec();
            out = out
                .child(div().text_xs().child(t(if fill {
                    "design.style_fill_category"
                } else {
                    "design.style_stroke_category"
                })))
                .child(picker(
                    ws,
                    if fill {
                        "design-object-enable-fill"
                    } else {
                        "design-object-enable-stroke"
                    },
                    labels,
                    match value {
                        None => 0,
                        Some(false) => 1,
                        Some(true) => 2,
                    },
                    move |ws, index, _| {
                        properties::edit_styles(
                            &mut ws.design.document,
                            &mut ws.design.history,
                            |styles| {
                                if let Some(style) =
                                    styles.objects.iter_mut().find(|s| s.name == name)
                                {
                                    let value = match index {
                                        1 => Some(false),
                                        2 => Some(true),
                                        _ => None,
                                    };
                                    if fill {
                                        style.enable_fill = value;
                                    } else {
                                        style.enable_stroke = value;
                                    }
                                }
                            },
                        );
                    },
                    cx,
                ))
                .child(paint_picker(
                    ws,
                    target.clone(),
                    fill,
                    if fill {
                        style.paint.fill.clone()
                    } else {
                        style.paint.stroke.clone()
                    },
                    cx,
                ));
            let name = style.name.clone();
            let value = if fill {
                style.paint.overprint_fill
            } else {
                style.paint.overprint_stroke
            };
            let labels = ["design.inherited", "design.knockout", "design.overprint"]
                .map(|k| t(k).to_string())
                .to_vec();
            out = out.child(picker(
                ws,
                if fill {
                    "design-object-overprint-fill"
                } else {
                    "design-object-overprint-stroke"
                },
                labels,
                match value {
                    None => 0,
                    Some(false) => 1,
                    Some(true) => 2,
                },
                move |ws, index, _| {
                    properties::edit_styles(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        |styles| {
                            if let Some(style) = styles.objects.iter_mut().find(|s| s.name == name)
                            {
                                let value = match index {
                                    1 => Some(false),
                                    2 => Some(true),
                                    _ => None,
                                };
                                if fill {
                                    style.paint.overprint_fill = value;
                                } else {
                                    style.paint.overprint_stroke = value;
                                }
                            }
                        },
                    );
                },
                cx,
            ));
        }
        let name = style.name;
        out = out.child(
            Button::new("design-apply-object-style", t("design.apply_clear_style")).on_click(
                cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    let name = ws
                        .design
                        .controls
                        .object_style
                        .clone()
                        .unwrap_or_else(|| name.clone());
                    object_styles::apply_style(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        &ws.design.selection,
                        Some(&name),
                    );
                    cx.notify();
                }),
            ),
        );
    }
    out.child(
        Button::new(
            "design-detach-object-style",
            t("design.detach_object_style"),
        )
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.commit_focused_field();
            object_styles::apply_style(
                &mut ws.design.document,
                &mut ws.design.history,
                &ws.design.selection,
                None,
            );
            cx.notify();
        })),
    )
    .child(
        Button::new("design-new-object-style", t("design.new_style")).on_click(cx.listener(
            |ws, _, _, cx| {
                ws.commit_focused_field();
                let mut number = 1;
                let name = loop {
                    let name = schist_i18n::tf!("design.style_number", number = number);
                    if ws.design.document.styles.object_style(&name).is_none() {
                        break name;
                    }
                    number += 1;
                };
                let paint = ws
                    .design
                    .selection
                    .first()
                    .and_then(|id| ws.design.document.object(*id))
                    .map(|o| ws.design.document.styles.object_paint(o))
                    .unwrap_or_default();
                properties::edit_styles(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    |styles| {
                        styles.objects.push(ObjectStyle {
                            name: name.clone(),
                            paint,
                            enable_fill: Some(true),
                            enable_stroke: Some(true),
                            ..Default::default()
                        });
                    },
                );
                ws.design.controls.object_style = Some(name);
                cx.notify();
            },
        )),
    )
    .into_any_element()
}
