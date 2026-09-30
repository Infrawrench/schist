//! Layout layers, independent of raster layers and their tools.
use super::*;
use schist_layout::{structure, LayoutEdit};

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
        rows.push(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(div().flex_1().text_sm().child(name))
                        .child(
                            Button::new(
                                ("layout-layer-visible", id.0),
                                t(if visible {
                                    "design.hide"
                                } else {
                                    "design.show"
                                }),
                            )
                            .on_click(cx.listener(
                                move |ws, _, _, cx| {
                                    structure::change_layer(
                                        &mut ws.design.document,
                                        &mut ws.design.history,
                                        id,
                                        |layer| layer.visible = !layer.visible,
                                    );
                                    ws.design.selection.clear();
                                    ws.design.typing = None;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            Button::new(
                                ("layout-layer-lock", id.0),
                                t(if locked {
                                    "design.unlock"
                                } else {
                                    "design.lock"
                                }),
                            )
                            .on_click(cx.listener(
                                move |ws, _, _, cx| {
                                    structure::change_layer(
                                        &mut ws.design.document,
                                        &mut ws.design.history,
                                        id,
                                        |layer| layer.locked = !layer.locked,
                                    );
                                    ws.design.typing = None;
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            Button::new(("layout-layer-up", id.0), t("design.raise")).on_click(
                                cx.listener(move |ws, _, _, cx| {
                                    structure::move_layer(
                                        &mut ws.design.document,
                                        &mut ws.design.history,
                                        id,
                                        -1,
                                    );
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            Button::new(("layout-layer-down", id.0), t("design.lower")).on_click(
                                cx.listener(move |ws, _, _, cx| {
                                    structure::move_layer(
                                        &mut ws.design.document,
                                        &mut ws.design.history,
                                        id,
                                        1,
                                    );
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            Button::new(
                                ("layout-layer-move", id.0),
                                t("design.move_selection_here"),
                            )
                            .on_click(cx.listener(
                                move |ws, _, _, cx| {
                                    structure::move_objects_to_layer(
                                        &mut ws.design.document,
                                        &mut ws.design.history,
                                        &ws.design.selection,
                                        id,
                                    );
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .children(
                    document
                        .objects
                        .iter()
                        .rev()
                        .filter(|object| {
                            object.page == page && document.object_layer(object.id) == id
                        })
                        .map(|object| {
                            let object_id = object.id;
                            let selected = ws.design.selection.contains(&object_id);
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .pl_2()
                                .child(
                                    div()
                                        .id(("layout-object", object_id.0))
                                        .flex_1()
                                        .text_sm()
                                        .when(selected, |row| {
                                            row.bg(gpui::rgb(palette().selection_bg))
                                        })
                                        .child(object.name.clone())
                                        .on_click(cx.listener(move |ws, _, _, cx| {
                                            ws.design.selection = vec![object_id];
                                            ws.design.typing = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new(
                                        ("layout-object-lock", object_id.0),
                                        t(if object.locked {
                                            "design.unlock"
                                        } else {
                                            "design.lock"
                                        }),
                                    )
                                    .on_click(cx.listener(
                                        move |ws, _, _, cx| {
                                            if ws.design.document.layer_locked(id) {
                                                return;
                                            }
                                            let Some(object) = ws.design.document.object(object_id)
                                            else {
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
                                        },
                                    )),
                                )
                        }),
                )
                .into_any_element(),
        );
    }
    for parent in &document.parents {
        if !parent.applied_to.contains(&page) || parent.hidden {
            continue;
        }
        rows.push(
            div()
                .flex()
                .flex_col()
                .text_xs()
                .text_color(gpui::rgb(palette().text_dim))
                .child(format!("{}: {}", t("design.parent_items"), parent.name))
                .children(
                    parent
                        .objects
                        .iter()
                        .filter(|object| object.tracks_parent(page))
                        .map(|object| div().pl_2().child(format!("┄ {}", object.object.name))),
                )
                .into_any_element(),
        );
    }
    Some(
        div()
            .flex()
            .flex_col()
            .p_2()
            .gap_2()
            .child(
                Button::new("layout-new-layer", t("design.new_layer")).on_click(cx.listener(
                    |ws, _, _, cx| {
                        let name = schist_i18n::tf!(
                            "design.layer_number",
                            number = ws.design.document.layers.len() + 1
                        );
                        structure::add_layer(&mut ws.design.document, &mut ws.design.history, name);
                        cx.notify();
                    },
                )),
            )
            .child(div().text_xs().child(schist_i18n::tf!(
                "design.layer_page",
                page = document.page_number(page)
            )))
            .children(rows)
            .into_any_element(),
    )
}
