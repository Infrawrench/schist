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
    canvas, deferred, div, img, px, Context, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, RenderImage, SharedString,
    StatefulInteractiveElement as _, Styled, Window,
};
use schist_color::Rgba;
use schist_core::{BlendMode, Layer, LayerId, LayerKind};
use schist_ui::{
    menu_separator, Button, ButtonColors, Chip, Divider, Heading, IconButton, Link, ListItem,
    MenuItem, Popover, Slider, Swatch, Tab, TextInput, TextInputColors,
};
use std::sync::Arc;

#[cfg(not(sandboxed))]
mod ai;
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
mod tabs;
mod titlebar;
mod toolbar;
mod typography;

#[cfg(not(sandboxed))]
pub use ai::*;
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

fn keybind_hint(kb: Option<&str>) -> String {
    let Some(kb) = kb else { return String::new() };
    let kb = if cfg!(any(target_os = "macos", target_os = "ios")) {
        kb.to_string()
    } else {
        kb.replace("cmd-", "ctrl-")
    };
    kb.split('-')
        .map(|part| match part {
            "cmd" => "Cmd".to_string(),
            "ctrl" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" => "Alt".to_string(),
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
        .child(navigator(ws, cx))
        .child(top_panel(ws, cx))
        .child(layers_panel(ws, cx))
        .children(notes_panel(ws, cx))
        .child(history_panel(ws, cx))
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
