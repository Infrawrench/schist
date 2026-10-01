//! One expanded panel at a time, with related panels sharing a tab strip.
use super::*;

impl SidePanel {
    fn design_label(self) -> &'static str {
        t(match self {
            Self::Pages => "design.pages",
            Self::DesignLayers => "common.layers",
            Self::Links => "design.links",
            Self::Control => "design.properties",
            Self::Character => "design.character",
            Self::Paragraph => "design.paragraph",
            Self::Styles => "design.styles",
            Self::Swatches => "design.swatches",
            Self::Stories => "design.stories",
            Self::Preflight => "design.preflight",
            _ => "design.mode",
        })
    }
    fn design_icon(self) -> &'static str {
        match self {
            Self::Pages => "artboard",
            Self::DesignLayers => "duplicate",
            Self::Links => "folder",
            Self::Control => "adjust",
            Self::Character => "character",
            Self::Paragraph => "type-align-left",
            Self::Styles => "type",
            Self::Swatches => "gradient",
            Self::Stories => "note",
            Self::Preflight => "check",
            _ => "settings",
        }
    }
    fn design_group(self) -> u8 {
        match self {
            Self::Pages | Self::DesignLayers | Self::Links => 0,
            Self::Control | Self::Character | Self::Paragraph => 1,
            Self::Styles | Self::Swatches => 2,
            _ => 3,
        }
    }
}

pub(super) fn select(ws: &mut Workspace, key: &'static str, cx: &mut Context<Workspace>) {
    ws.commit_focused_field();
    ws.close_popup(cx);
    let mut next = ws.view.clone();
    next.design_dock.active = key.into();
    next.design_dock.collapsed = false;
    next.hidden_panels.retain(|p| p != key);
    ws.commit_workspace_view(next, cx);
}

pub(super) fn dock(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::Stateful<gpui::Div> {
    let saved: Vec<_> = ws
        .view
        .side_panel_order
        .iter()
        .filter(|key| schist_app_settings::workspaces::DESIGN_PANELS.contains(&key.as_str()))
        .cloned()
        .collect();
    let order = if saved.is_empty() {
        schist_app_settings::workspaces::DESIGN_PANELS
            .into_iter()
            .map(str::to_owned)
            .collect()
    } else {
        saved
    };
    let mut panels: Vec<_> = panel_order(&order, true)
        .into_iter()
        .filter(|p| p.design_only() && !ws.view.hidden_panels.iter().any(|k| k == p.key()))
        .collect();
    panels.sort_by_key(|panel| panel.design_group());
    let active = panels
        .iter()
        .copied()
        .find(|p| p.key() == ws.view.design_dock.active)
        .or_else(|| panels.first().copied());
    let collapsed = ws.view.design_dock.collapsed;
    let p = palette();
    let rail = div()
        .id("design-panel-rail")
        .flex()
        .flex_col()
        .flex_none()
        .w(px(34.0))
        .min_h(px(0.0))
        .overflow_y_scroll()
        .bg(gpui::rgb(p.deep_bg))
        .border_l_1()
        .border_color(gpui::rgb(p.panel_edge))
        .child(
            div()
                .id("design-dock-collapse")
                .flex()
                .items_center()
                .justify_center()
                .h(px(25.0))
                .cursor_pointer()
                .tooltip(ui::tip(
                    t(if collapsed {
                        "design.expand_panels"
                    } else {
                        "design.collapse_panels"
                    }),
                    None,
                ))
                .child(icon(
                    if collapsed {
                        "chevron-right"
                    } else {
                        "chevron-down"
                    },
                    12.0,
                    p.text_dim,
                ))
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.close_popup(cx);
                    let mut next = ws.view.clone();
                    next.design_dock.collapsed = !next.design_dock.collapsed;
                    ws.commit_workspace_view(next, cx);
                })),
        )
        .children(panels.iter().copied().enumerate().map(|(i, panel)| {
            let separated = i > 0 && panels[i - 1].design_group() != panel.design_group();
            div()
                .id(("design-panel-icon", i))
                .flex()
                .items_center()
                .justify_center()
                .h(px(34.0))
                .flex_none()
                .cursor_pointer()
                .when(separated, |d| {
                    d.mt_2().border_t_1().border_color(gpui::rgb(p.panel_edge))
                })
                .when(active == Some(panel) && !collapsed, |d| {
                    d.bg(gpui::rgb(p.selection_bg))
                })
                .hover(|d| d.bg(gpui::rgb(p.hover)))
                .tooltip(ui::tip(panel.design_label(), None))
                .child(icon(panel.design_icon(), 16.0, p.text))
                .on_click(cx.listener(move |ws, _, _, cx| select(ws, panel.key(), cx)))
        }));
    let mut dock = div()
        .id("side-panels")
        .flex()
        .flex_row()
        .flex_none()
        .min_h(px(0.0))
        .w(px(if collapsed || active.is_none() {
            34.0
        } else {
            ws.view.panel_width.unwrap_or(300.0).max(280.0)
        }))
        .bg(gpui::rgb(p.panel_bg))
        .border_l_1()
        .border_color(gpui::rgb(p.panel_edge));
    if let Some(active) = active.filter(|_| !collapsed) {
        let tabs = panels
            .iter()
            .copied()
            .filter(|p| p.design_group() == active.design_group())
            .map(|panel| {
                div()
                    .id(SharedString::from(format!(
                        "design-panel-tab-{}",
                        panel.key()
                    )))
                    .flex()
                    .items_center()
                    .h(px(29.0))
                    .px_2()
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(gpui::rgb(if panel == active {
                        p.accent
                    } else {
                        p.panel_edge
                    }))
                    .when(panel == active, |d| {
                        d.bg(gpui::rgb(p.panel_bg)).text_color(gpui::rgb(p.text))
                    })
                    .child(panel.design_label())
                    .on_click(cx.listener(move |ws, _, _, cx| select(ws, panel.key(), cx)))
            })
            .collect::<Vec<_>>();
        let body = if active == SidePanel::Control {
            design_controls::control_panel(ws, cx).unwrap_or_else(|| document_properties(ws, cx))
        } else {
            panel_content(active, ws, cx)
                .map(|(_, body, _)| body)
                .unwrap_or_else(|| {
                    div()
                        .p_3()
                        .text_xs()
                        .text_color(gpui::rgb(p.text_dim))
                        .child(t("design.panel_empty"))
                        .into_any_element()
                })
        };
        dock = dock.child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h(px(0.0))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .bg(gpui::rgb(p.deep_bg))
                        .children(tabs),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "design-panel-scroll-{}",
                            active.key()
                        )))
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .child(body),
                ),
        );
    }
    dock.child(rail)
}

fn document_properties(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let page = ws.design.document.pages.get(ws.design.current_page());
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .child(div().text_sm().child(t("design.page")))
        .children(page.map(|page| {
            div()
                .text_xs()
                .text_color(gpui::rgb(palette().text_dim))
                .child(schist_i18n::tf!(
                    "design.document_size",
                    width = format!("{:.1}", page.width),
                    height = format!("{:.1}", page.height)
                ))
        }))
        .child(
            Button::new("design-properties-pages", t("design.page_options")).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.design.controls.expanded.insert("page-setup");
                    select(ws, "pages", cx);
                },
            )),
        )
        .child(
            Button::new("design-properties-place", t("design.place_graphic")).on_click(
                cx.listener(|ws, _, _, cx| {
                    ws.pick_design_graphic(
                        crate::workspace::design_graphics::Destination::Page(
                            ws.design.current_page(),
                        ),
                        cx,
                    )
                }),
            ),
        )
        .into_any_element()
}

/// Closing a disclosure commits the captured field before its control disappears.
pub(super) fn section(
    ws: &Workspace,
    id: &'static str,
    label: &'static str,
    body: Vec<gpui::AnyElement>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let open = ws.design.controls.expanded.contains(id);
    div()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .child(
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.0))
                .cursor_pointer()
                .text_xs()
                .text_color(gpui::rgb(palette().text_dim))
                .child(icon(
                    if open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    11.0,
                    palette().text_dim,
                ))
                .child(t(label))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.close_popup(cx);
                    if !ws.design.controls.expanded.remove(id) {
                        ws.design.controls.expanded.insert(id);
                    }
                    cx.notify();
                })),
        )
        .when(open, |d| {
            d.child(div().flex().flex_col().gap_2().pb_2().children(body))
        })
        .into_any_element()
}
