//! A document tab.

use crate::{palette, Handler, IconButton};
use gpui::{
    div, px, App, ElementId, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    Refineable as _, RenderOnce, SharedString, StyleRefinement, Styled, Window,
};

/// One tab in a strip: its title, and a close control on the right.
///
/// Selecting switches on the *press*, as every native tab strip does
/// (a tab is a place to be, not an action to confirm); the close button
/// is a button, and commits on release. Middle-click closes too.
#[derive(gpui::IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    active: bool,
    style: StyleRefinement,
    on_select: Option<Handler>,
    on_close: Option<Handler>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Tab {
            id: id.into(),
            label: label.into(),
            active: false,
            style: StyleRefinement::default(),
            on_select: None,
            on_close: None,
        }
    }

    /// The tab whose document is showing.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Styled for Tab {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tab {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .h(px(25.0))
            .pl_2()
            .pr_1()
            .max_w(px(180.0))
            .border_r_1()
            .border_color(gpui::rgb(p.panel_edge))
            .text_size(px(11.0));
        el = if self.active {
            el.bg(gpui::rgb(p.control_bg)).text_color(gpui::rgb(p.text))
        } else {
            el.bg(gpui::rgb(p.panel_bg))
                .text_color(gpui::rgb(p.text_dim))
                .hover(|s| s.bg(gpui::rgb(p.hover)))
        };
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if let Some(on_select) = self.on_select {
            el = el.on_mouse_down(MouseButton::Left, move |_e, window, cx| {
                on_select(window, cx)
            });
        }
        let mut close = IconButton::new("close", "close")
            .size(16.0)
            .icon_size(9.0)
            .color(p.text_dim)
            .consume_press();
        if let Some(on_close) = self.on_close {
            let by_middle = on_close.clone();
            el = el.on_mouse_down(MouseButton::Middle, move |_e, window, cx| {
                by_middle(window, cx)
            });
            close = close.on_click(move |_e, window, cx| on_close(window, cx));
        }
        el.child(
            div()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(self.label),
        )
        .child(close)
    }
}
