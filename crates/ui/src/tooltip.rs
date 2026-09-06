//! Hover labels for icon-only controls.

use crate::palette;
use gpui::{
    div, px, AppContext as _, Context, IntoElement, ParentElement as _, SharedString, Styled as _,
};

/// A hover label for an icon-only control: the name, and optionally its
/// keyboard shortcut dimmed after it.
pub struct Tooltip {
    label: SharedString,
    hint: Option<SharedString>,
}

impl gpui::Render for Tooltip {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(gpui::rgb(palette().popup_bg))
            .border_1()
            .border_color(gpui::rgb(palette().panel_edge))
            .text_size(px(11.0))
            .text_color(gpui::rgb(palette().text))
            .child(self.label.clone());
        if let Some(hint) = self.hint.clone() {
            row = row.child(div().text_color(gpui::rgb(palette().text_dim)).child(hint));
        }
        row
    }
}

/// Build a tooltip callback for [`gpui::StatefulInteractiveElement::tooltip`].
pub fn tip(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let label = label.into();
    move |_window, cx| {
        let label = label.clone();
        let hint = hint.clone();
        cx.new(|_| Tooltip { label, hint }).into()
    }
}
