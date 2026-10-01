//! Compact layout-layer tree. Layer drags insert; object drags move the
//! selection between layers. Both use one reversible model operation.
use super::*;
use schist_layout::{structure, LayerId, LayoutEdit, LayoutObject, ObjectId};

#[derive(Clone)]
struct LayerDrag {
    id: LayerId,
    label: SharedString,
}
#[derive(Clone)]
struct ObjectDrag {
    ids: Vec<ObjectId>,
    label: SharedString,
}
struct DragPreview(SharedString);
impl gpui::Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .text_xs()
            .bg(gpui::rgb(palette().panel_bg))
            .child(self.0.clone())
    }
}

fn prepare(ws: &mut Workspace) {
    ws.commit_focused_field();
    ws.design.cancel_gesture();
    ws.design.typing = None;
}

pub(super) fn design_layers_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() || ws.design.document.pages.is_empty() {
        return None;
    }
    let page = ws.design.current_page();
    let document = &ws.design.document;
    let mut rows = Vec::new();
    for (index, id) in document.layers.iter().copied().enumerate() {
        let name = document
            .layer_properties
            .iter()
            .find(|layer| layer.id == id)
            .map(|layer| layer.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| schist_i18n::tf!("design.layer_number", number = index + 1));
        let visible = document.layer_visible(id);
        let locked = document.layer_locked(id);
        let collapsed = ws.design.controls.collapsed_layers.contains(&id);
        let selected = ws
            .design
            .selection
            .iter()
            .any(|object| document.object_layer(*object) == id);
        let header = div()
            .id(("layout-layer", id.0))
            .flex()
            .items_center()
            .h(px(27.0))
            .border_b_1()
            .border_color(gpui::rgb(palette().panel_edge))
            .child(
                IconButton::new(
                    ("layout-layer-visible", id.0),
                    if visible { "eye" } else { "eye-off" },
                )
                .size(22.0)
                .icon_size(13.0)
                .consume_press()
                .color(if visible {
                    palette().text_dim
                } else {
                    palette().text_faint
                })
                .tooltip(
                    t(if visible {
                        "design.hide"
                    } else {
                        "design.show"
                    }),
                    None,
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    prepare(ws);
                    structure::change_layer(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        id,
                        |layer| layer.visible = !layer.visible,
                    );
                    ws.design.selection.clear();
                    cx.notify();
                })),
            )
            .child(
                IconButton::new(
                    ("layout-layer-lock", id.0),
                    if locked { "lock" } else { "unlock" },
                )
                .size(22.0)
                .icon_size(12.0)
                .consume_press()
                .color(if locked {
                    palette().text
                } else {
                    palette().text_faint
                })
                .tooltip(
                    t(if locked {
                        "design.unlock"
                    } else {
                        "design.lock"
                    }),
                    None,
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    prepare(ws);
                    structure::change_layer(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        id,
                        |layer| layer.locked = !layer.locked,
                    );
                    cx.notify();
                })),
            )
            .child(
                IconButton::new(
                    ("layout-layer-fold", id.0),
                    if collapsed {
                        "chevron-right"
                    } else {
                        "chevron-down"
                    },
                )
                .size(18.0)
                .icon_size(10.0)
                .consume_press()
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    if !ws.design.controls.collapsed_layers.remove(&id) {
                        ws.design.controls.collapsed_layers.insert(id);
                    }
                    cx.notify();
                })),
            )
            .child(
                div()
                    .id(("layout-layer-name", id.0))
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .truncate()
                    .cursor_pointer()
                    .tooltip(ui::tip(name.clone(), None))
                    .child(name.clone())
                    .on_drag(
                        LayerDrag {
                            id,
                            label: name.into(),
                        },
                        |drag, _, _, cx| cx.new(|_| DragPreview(drag.label.clone())),
                    ),
            )
            .child(
                div()
                    .w(px(7.0))
                    .h(px(7.0))
                    .mr_2()
                    .when(selected, |s| s.bg(gpui::rgb(palette().accent))),
            )
            .can_drop(move |drag, _, _| {
                drag.downcast_ref::<LayerDrag>()
                    .is_some_and(|drag| drag.id != id)
                    || (!locked && drag.is::<ObjectDrag>())
            })
            .drag_over::<LayerDrag>(|s, _, _, _| {
                s.border_t_2().border_color(gpui::rgb(palette().accent))
            })
            .drag_over::<ObjectDrag>(|s, _, _, _| s.bg(gpui::rgb(palette().selection_bg)))
            .on_drop(cx.listener(move |ws, drag: &LayerDrag, _, cx| {
                prepare(ws);
                structure::place_layer(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    drag.id,
                    Some(id),
                );
                cx.notify();
            }))
            .on_drop(cx.listener(move |ws, drag: &ObjectDrag, _, cx| {
                prepare(ws);
                if structure::move_objects_to_layer(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    &drag.ids,
                    id,
                ) {
                    ws.design.controls.collapsed_layers.remove(&id);
                    ws.design.selection.clone_from(&drag.ids);
                }
                cx.notify();
            }));
        rows.push(header.into_any_element());
        if collapsed {
            continue;
        }
        for object in document
            .objects
            .iter()
            .rev()
            .filter(|object| object.page == page && document.object_layer(object.id) == id)
        {
            let object_id = object.id;
            let selected = ws.design.selection.contains(&object_id);
            let object_locked = object.locked;
            let icon = match object.object {
                LayoutObject::TextFrame { .. } => "type",
                LayoutObject::GraphicFrame { .. } => "frame",
                LayoutObject::Shape { .. } => "shape-rect",
                LayoutObject::Group { .. } => "folder",
                LayoutObject::Note { .. } => "note",
            };
            let ids = if selected {
                ws.design.selection.clone()
            } else {
                vec![object_id]
            };
            let can_drag = !ids.iter().any(|id| document.object_locked(*id));
            rows.push(
                div()
                    .id(("layout-object", object_id.0))
                    .flex()
                    .items_center()
                    .h(px(25.0))
                    .min_w_0()
                    .pl(px(22.0))
                    .pr_2()
                    .gap_1()
                    .when(selected, |s| s.bg(gpui::rgb(palette().selection_bg)))
                    .hover(move |s| {
                        if selected {
                            s
                        } else {
                            s.bg(gpui::rgb(palette().hover))
                        }
                    })
                    .child(
                        IconButton::new(
                            ("layout-object-lock", object_id.0),
                            if object_locked || locked {
                                "lock"
                            } else {
                                "unlock"
                            },
                        )
                        .size(22.0)
                        .icon_size(12.0)
                        .consume_press()
                        .disabled(locked)
                        .color(if object_locked {
                            palette().text
                        } else {
                            palette().text_faint
                        })
                        .tooltip(
                            t(if object_locked {
                                "design.unlock"
                            } else {
                                "design.lock"
                            }),
                            None,
                        )
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            cx.stop_propagation();
                            prepare(ws);
                            if ws.design.document.layer_locked(id) {
                                return;
                            }
                            let Some(object) = ws.design.document.object(object_id) else {
                                return;
                            };
                            let before = schist_layout::snapshot_object(object);
                            let mut after = before.clone();
                            after.locked = !after.locked;
                            ws.design.history.apply(
                                &mut ws.design.document,
                                LayoutEdit::ObjectChanged {
                                    id: object_id.0,
                                    before,
                                    after,
                                },
                            );
                            cx.notify();
                        })),
                    )
                    .child(schist_ui::icon(icon, 12.0, palette().text_dim))
                    .child(
                        div()
                            .id(("layout-object-name", object_id.0))
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .truncate()
                            .tooltip(ui::tip(object.name.clone(), None))
                            .child(object.name.clone())
                            .when(can_drag, |row| {
                                row.on_drag(
                                    ObjectDrag {
                                        ids,
                                        label: object.name.clone().into(),
                                    },
                                    |drag, _, _, cx| cx.new(|_| DragPreview(drag.label.clone())),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |ws, ev: &gpui::ClickEvent, _, cx| {
                        prepare(ws);
                        if ev.modifiers().shift {
                            if ws.design.selection.contains(&object_id) {
                                ws.design.selection.retain(|id| *id != object_id);
                            } else {
                                ws.design.selection.push(object_id);
                            }
                        } else {
                            ws.design.selection = vec![object_id];
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
    }
    // The last insertion slot also lets a layer be dragged below the final layer.
    rows.push(
        div()
            .id("layout-layer-bottom")
            .h(px(12.0))
            .drag_over::<LayerDrag>(|s, _, _, _| {
                s.border_t_2().border_color(gpui::rgb(palette().accent))
            })
            .on_drop(cx.listener(|ws, drag: &LayerDrag, _, cx| {
                prepare(ws);
                structure::place_layer(
                    &mut ws.design.document,
                    &mut ws.design.history,
                    drag.id,
                    None,
                );
                cx.notify();
            }))
            .into_any_element(),
    );
    for parent in &document.parents {
        if !parent.applied_to.contains(&page) || parent.hidden {
            continue;
        }
        rows.push(
            div()
                .text_xs()
                .text_color(gpui::rgb(palette().text_dim))
                .px_2()
                .py_1()
                .child(div().truncate().child(format!(
                    "{}: {}",
                    t("design.parent_items"),
                    parent.name
                )))
                .children(
                    parent
                        .objects
                        .iter()
                        .filter(|object| object.tracks_parent(page))
                        .map(|object| {
                            div()
                                .pl_2()
                                .truncate()
                                .child(format!("┄ {}", object.object.name))
                        }),
                )
                .into_any_element(),
        );
    }
    Some(
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .h(px(30.0))
                    .text_xs()
                    .text_color(gpui::rgb(palette().text_dim))
                    .child(schist_i18n::tf!(
                        "design.layer_page",
                        page = document.page_number(page)
                    ))
                    .child(
                        IconButton::new("layout-new-layer", "layer-new")
                            .size(22.0)
                            .icon_size(14.0)
                            .tooltip(t("design.new_layer"), None)
                            .on_click(cx.listener(|ws, _, _, cx| {
                                prepare(ws);
                                let name = schist_i18n::tf!(
                                    "design.layer_number",
                                    number = ws.design.document.layers.len() + 1
                                );
                                structure::add_layer(
                                    &mut ws.design.document,
                                    &mut ws.design.history,
                                    name,
                                );
                                cx.notify();
                            })),
                    ),
            )
            .children(rows)
            .into_any_element(),
    )
}
