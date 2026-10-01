//! Compact paragraph tab editing. Only the selected stop exposes fields.
use super::*;
use crate::design::controls::{self, Target};

pub(super) fn rows(
    ws: &mut Workspace,
    name: &str,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let tabs = ws
        .design
        .document
        .styles
        .resolve_paragraph(name)
        .list
        .tabs
        .unwrap_or_default();
    let selected = ws.design.controls.tab.min(tabs.len().saturating_sub(1));
    let mut rows = Vec::new();
    if !tabs.is_empty() {
        rows.push(super::object_styles::picker(
            ws,
            "design-tab-selection",
            tabs.iter().map(|tab| tab.position.to_string()).collect(),
            selected,
            move |ws, index, _| ws.design.controls.tab = index,
            cx,
        ));
    }
    let mut actions = div().flex().items_center().gap_1();
    for (index, (alignment, label, icon)) in [
        ("LeftAlign", "design.align_left", "type-align-left"),
        ("CenterAlign", "design.align_center", "type-align-center"),
        ("RightAlign", "design.align_right", "type-align-right"),
        ("CharacterAlign", "design.tab_character", "tab-character"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = name.to_owned();
        actions = actions.child(
            IconButton::new(("design-tab-alignment", index), icon)
                .tooltip(t(label), None)
                .disabled(tabs.is_empty())
                .active(
                    tabs.get(selected)
                        .is_some_and(|tab| tab.alignment == alignment),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    if let Some(target) = controls::tab_target(&ws.design, &name, selected) {
                        controls::set_tab_alignment(&mut ws.design, &target, alignment);
                    }
                    cx.notify();
                })),
        );
    }
    let add_name = name.to_owned();
    let remove_name = name.to_owned();
    let inherit_name = name.to_owned();
    actions = actions
        .child(div().flex_1())
        .child(
            IconButton::new("design-tab-add", "plus")
                .tooltip(t("design.tab_add"), None)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    let count = ws
                        .design
                        .document
                        .styles
                        .resolve_paragraph(&add_name)
                        .list
                        .tabs
                        .map_or(0, |tabs| tabs.len());
                    if controls::add_tab(&mut ws.design, &Target::Paragraph(add_name.clone())) {
                        ws.design.controls.tab = count;
                    }
                    cx.notify();
                })),
        )
        .child(
            IconButton::new("design-tab-remove", "trash")
                .tooltip(t("design.tab_remove"), None)
                .disabled(tabs.is_empty())
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    if let Some(target) = controls::tab_target(&ws.design, &remove_name, selected) {
                        controls::edit_tab(&mut ws.design, &target, |tabs, index| {
                            tabs.remove(index);
                        });
                    }
                    cx.notify();
                })),
        )
        .child(
            IconButton::new("design-tab-inherit", "undo")
                .tooltip(t("design.tab_inherit"), None)
                .disabled(
                    ws.design
                        .document
                        .styles
                        .paragraph(name)
                        .is_none_or(|style| style.list.tabs.is_none()),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    controls::edit_list(
                        &mut ws.design,
                        &Target::Paragraph(inherit_name.clone()),
                        |list| list.tabs = None,
                    );
                    cx.notify();
                })),
        );
    rows.push(actions.into_any_element());
    if let Some(target) = controls::tab_target(&ws.design, name, selected) {
        let tab = &tabs[selected];
        rows.push(super::design_controls::field(
            ws,
            "design-prop-tab-position",
            "design.tab_position",
            tab.position.to_string(),
            target.clone(),
            cx,
        ));
        if tab.alignment == "CharacterAlign" {
            rows.push(super::design_controls::field(
                ws,
                "design-prop-tab-character",
                "design.tab_character",
                tab.alignment_character.clone(),
                target,
                cx,
            ));
        }
    } else {
        rows.push(
            div()
                .text_xs()
                .text_color(gpui::rgb(palette().text_dim))
                .child(t("design.idml_tabs_implicit"))
                .into_any_element(),
        );
    }
    rows
}
