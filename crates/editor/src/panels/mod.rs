//! UI chrome: menu bar, tool options bar, toolbar, layers/history/color
//! panels, status bar.
//!
//! These render directly from the Workspace (third-party panel plugins get
//! their seam later; the registry shape in plugin-api reserves it). Icons
//! are monochrome SVGs from the embedded asset source, tinted by text
//! color — no emoji.

use crate::actions::AppItem;
use crate::ui;
use crate::ui::palette;
use crate::workspace::{ColorTarget, ContextTarget, LayerDrop, Modal, NoteField, Popup, Workspace};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    canvas, deferred, div, img, px, AppContext as _, Context, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, RenderImage,
    SharedString, StatefulInteractiveElement as _, Styled, Window,
};
use schist_color::Rgba;
use schist_core::{BlendMode, Layer, LayerId, LayerKind};
use schist_i18n::t;
use schist_ui::{
    menu_separator, Button, ButtonColors, Chip, Divider, DropdownButton, Heading, IconButton, Link,
    ListItem, MenuItem, Popover, Slider, Swatch, Tab, TextInput, TextInputColors,
};
use std::sync::Arc;

#[cfg(not(sandboxed))]
mod ai;
mod brushes;
mod color;
mod context;
mod history;
mod info;
mod layers;
mod menu_bar;
mod menus;
mod navigator;
mod notes;
mod rulers;
mod sliders;
mod status;
mod symmetry;
mod tabs;
mod titlebar;
mod toolbar;
mod typography;

#[cfg(not(sandboxed))]
pub use ai::*;
pub(crate) use color::spot_ink_dialog;
use color::*;
use info::*;

/// The sidebar renders nothing on the web or iOS, where the AI subsystem
/// (which drives locally installed agent CLIs) is compiled out.
#[cfg(sandboxed)]
pub fn ai_sidebar(_ws: &mut Workspace, _cx: &mut Context<Workspace>) -> Option<gpui::AnyElement> {
    None
}
pub use context::*;
use history::*;
use layers::*;
pub use menu_bar::*;
pub(crate) use menus::*;
pub use navigator::*;
use notes::*;
pub use rulers::*;
pub use sliders::*;
pub use status::*;
pub use tabs::*;
pub use titlebar::*;
pub use toolbar::*;

fn swatch_hex(c: Rgba) -> gpui::Rgba {
    let [r, g, b, _] = c.to_u8();
    gpui::rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
}

pub use schist_ui::icon;

trait ActiveExt: Styled + Sized {
    fn when_active(self, active: bool) -> Self {
        if active {
            self.bg(gpui::rgb(palette().accent))
                .text_color(gpui::rgb(palette().accent_text))
        } else {
            self
        }
    }
}
impl<T: Styled> ActiveExt for T {}

// ===== menu bar =====

pub(crate) fn keybind_hint(kb: Option<&str>) -> String {
    let Some(kb) = kb else { return String::new() };
    let kb = if cfg!(any(target_os = "macos", target_os = "ios")) {
        kb.to_string()
    } else {
        kb.replace("cmd-", "ctrl-")
    };
    kb.split('-')
        .map(|part| match part {
            "cmd" => t("panel.keys.cmd").to_string(),
            "ctrl" => t("panel.keys.ctrl").to_string(),
            "shift" => t("panel.keys.shift").to_string(),
            "alt" => t("panel.keys.alt").to_string(),
            other if other.len() == 1 => other.to_uppercase(),
            other => {
                let mut c = other.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

pub fn side_panels(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::Stateful<gpui::Div> {
    // Scrollable: the Info tab can be taller than a small window, and
    // without this the panels below it were squeezed into each other —
    // Layers over History, a stray border across the map. Short content
    // still fills the column (the growing panels take the slack); tall
    // content scrolls.
    let order = panel_order(&ws.view.side_panel_order);
    let mut panels = Vec::with_capacity(order.len());
    for kind in order {
        let (label, body, grows) = match kind {
            SidePanel::Navigator => (
                t("panel.navigator.title"),
                navigator(ws, cx).into_any_element(),
                false,
            ),
            SidePanel::Color => (t("common.color"), top_panel(ws, cx), false),
            SidePanel::Layers => (
                t("common.layers"),
                layers_panel(ws, cx).into_any_element(),
                true,
            ),
            SidePanel::Notes => {
                let Some(notes) = notes_panel(ws, cx) else {
                    continue;
                };
                (t("menu.view.notes"), notes, false)
            }
            SidePanel::History => (
                t("panel.history.title"),
                history_panel(ws, cx).into_any_element(),
                false,
            ),
        };
        let key = kind.key();
        let saved_height = ws
            .view
            .side_panel_heights
            .get(key)
            .copied()
            .filter(|height| height.is_finite());
        let resizing = ws
            .side_panel_resize
            .is_some_and(|(drag_key, _, _)| drag_key == key);
        panels.push(movable_panel(
            kind,
            label,
            body,
            grows,
            saved_height,
            resizing,
            cx,
        ));
    }

    div()
        .id("side-panels")
        .flex()
        .flex_col()
        .w(px(ui::metrics().panel_w))
        .flex_none()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .bg(gpui::rgb(palette().panel_bg))
        .border_l_1()
        .border_color(gpui::rgb(palette().panel_edge))
        .children(panels)
        .child(panel_drop_end(cx))
}

const DEFAULT_PANEL_ORDER: [SidePanel; 5] = [
    SidePanel::Navigator,
    SidePanel::Color,
    SidePanel::Layers,
    SidePanel::Notes,
    SidePanel::History,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidePanel {
    Navigator,
    Color,
    Layers,
    Notes,
    History,
}

impl SidePanel {
    fn key(self) -> &'static str {
        match self {
            Self::Navigator => "navigator",
            Self::Color => "color",
            Self::Layers => "layers",
            Self::Notes => "notes",
            Self::History => "history",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        DEFAULT_PANEL_ORDER
            .into_iter()
            .find(|panel| panel.key() == key)
    }
}

fn panel_order(saved: &[String]) -> Vec<SidePanel> {
    let mut order = Vec::with_capacity(DEFAULT_PANEL_ORDER.len());
    for key in saved {
        if let Some(panel) = SidePanel::from_key(key) {
            if !order.contains(&panel) {
                order.push(panel);
            }
        }
    }
    for panel in DEFAULT_PANEL_ORDER {
        if !order.contains(&panel) {
            order.push(panel);
        }
    }
    order
}

#[cfg(test)]
mod panel_order_tests {
    use super::*;

    #[test]
    fn saved_order_is_kept_and_new_panels_are_appended() {
        let saved = ["layers", "color"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            panel_order(&saved),
            vec![
                SidePanel::Layers,
                SidePanel::Color,
                SidePanel::Navigator,
                SidePanel::Notes,
                SidePanel::History,
            ]
        );
    }

    #[test]
    fn unknown_and_duplicate_panel_ids_are_ignored() {
        let saved = ["history", "future-panel", "history", "layers"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            panel_order(&saved),
            vec![
                SidePanel::History,
                SidePanel::Layers,
                SidePanel::Navigator,
                SidePanel::Color,
                SidePanel::Notes,
            ]
        );
    }
}

#[derive(Clone)]
struct PanelDrag {
    panel: SidePanel,
    label: SharedString,
}

struct PanelDragPreview(SharedString);

impl gpui::Render for PanelDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(gpui::rgb(palette().accent))
            .text_color(gpui::rgb(palette().accent_text))
            .child(self.0.clone())
    }
}

fn move_panel(ws: &mut Workspace, source: SidePanel, before: Option<SidePanel>) {
    let mut order = panel_order(&ws.view.side_panel_order);
    order.retain(|panel| *panel != source);
    let at = before
        .and_then(|target| order.iter().position(|panel| *panel == target))
        .unwrap_or(order.len());
    order.insert(at, source);
    ws.view.side_panel_order = order
        .into_iter()
        .map(|panel| panel.key().to_owned())
        .collect();
    ws.save_view_options();
}

fn movable_panel(
    panel: SidePanel,
    label: &'static str,
    body: gpui::AnyElement,
    grows: bool,
    saved_height: Option<f32>,
    resizing: bool,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let key = panel.key();
    let drag = PanelDrag {
        panel,
        label: label.into(),
    };
    let mut wrapper = div()
        .id(SharedString::from(format!("side-panel-{}", panel.key())))
        .flex()
        .flex_col()
        .flex_none()
        .relative()
        .drag_over::<PanelDrag>(|style, _, _, _| {
            style.border_t_2().border_color(gpui::rgb(palette().accent))
        })
        .on_drop(cx.listener(move |ws, drag: &PanelDrag, _window, cx| {
            if drag.panel != panel {
                move_panel(ws, drag.panel, Some(panel));
                cx.notify();
            }
        }))
        .child({
            let entity = cx.entity();
            canvas(
                move |bounds, _window, cx| {
                    entity.update(cx, |ws, _| {
                        ws.side_panel_bounds.insert(key, bounds);
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        })
        .child(
            div()
                .id(SharedString::from(format!(
                    "side-panel-grip-{}",
                    panel.key()
                )))
                .h(px(14.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor(gpui::CursorStyle::OpenHand)
                .text_size(px(10.0))
                .text_color(gpui::rgb(palette().text_faint))
                .child("\u{2807}")
                .on_drag(drag, |drag, _, _, cx| {
                    cx.new(|_| PanelDragPreview(drag.label.clone()))
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!(
                    "side-panel-body-{}",
                    panel.key()
                )))
                .flex()
                .flex_col()
                .flex_grow()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(body),
        )
        .children((panel != SidePanel::History).then(|| panel_resize_grip(panel, resizing, cx)));
    if let Some(height) = saved_height {
        wrapper = wrapper.h(px(height));
    } else if grows {
        wrapper = wrapper.flex_grow().min_h(px(0.0));
    }
    wrapper.into_any_element()
}

fn panel_resize_grip(
    panel: SidePanel,
    dragging: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    const MAX_PANEL_H: f32 = 640.0;
    let key = panel.key();
    let min_height = match panel {
        SidePanel::Layers => 160.0,
        SidePanel::Navigator | SidePanel::Color => 120.0,
        SidePanel::Notes => 100.0,
        SidePanel::History => 120.0,
    };
    let entity = cx.entity();
    div()
        .h(px(10.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor(gpui::CursorStyle::ResizeRow)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &MouseDownEvent, window, cx| {
                window.claim_touch_drag();
                let height = ws
                    .side_panel_bounds
                    .get(key)
                    .map(|bounds| f32::from(bounds.size.height))
                    .or_else(|| ws.view.side_panel_heights.get(key).copied())
                    .unwrap_or(min_height);
                ws.side_panel_resize = Some((key, f32::from(ev.position.y), height));
                cx.notify();
            }),
        )
        .child(
            div()
                .w(px(36.0))
                .h(px(3.0))
                .rounded_full()
                .bg(gpui::rgb(if dragging {
                    palette().accent
                } else {
                    palette().text_dim
                })),
        )
        .children(dragging.then(|| {
            canvas(
                |_, _, _| (),
                move |_, (), window, _| {
                    let move_entity = entity.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _window, cx| {
                        if phase != gpui::DispatchPhase::Capture {
                            return;
                        }
                        move_entity.update(cx, |ws, cx| {
                            let Some((drag_key, start_y, start_h)) = ws.side_panel_resize else {
                                return;
                            };
                            if drag_key != key {
                                return;
                            }
                            if ev.pressed_button != Some(MouseButton::Left) {
                                ws.side_panel_resize = None;
                                ws.save_view_options();
                                return;
                            }
                            let height = start_h + f32::from(ev.position.y) - start_y;
                            ws.view
                                .side_panel_heights
                                .insert(key.to_owned(), height.clamp(min_height, MAX_PANEL_H));
                            cx.notify();
                        });
                    });
                    let up_entity = entity.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
                        if phase != gpui::DispatchPhase::Capture || ev.button != MouseButton::Left {
                            return;
                        }
                        up_entity.update(cx, |ws, cx| {
                            if ws.side_panel_resize.take().is_some() {
                                ws.save_view_options();
                                cx.notify();
                            }
                        });
                    });
                },
            )
            .absolute()
            .size_0()
        }))
}

fn panel_drop_end(cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .h(px(10.0))
        .flex_none()
        .drag_over::<PanelDrag>(|style, _, _, _| {
            style.border_t_2().border_color(gpui::rgb(palette().accent))
        })
        .on_drop(cx.listener(|ws, drag: &PanelDrag, _window, cx| {
            move_panel(ws, drag.panel, None);
            cx.notify();
        }))
}

fn panel_title(name: &'static str) -> impl IntoElement {
    Heading::new(name).uppercase().pb_1()
}

// ===== layers panel =====

fn icon_button(
    icon_name: &'static str,
    command: &'static str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let m = ui::metrics();
    IconButton::new(icon_name, icon_name)
        .size(m.icon_button)
        .icon_size(m.icon_button_icon)
        .on_click(cx.listener(move |ws, _e, _w, cx| ws.run_command(command, cx)))
}
