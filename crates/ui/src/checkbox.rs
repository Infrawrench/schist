//! A checkbox with a label to its right.

use crate::{icon, palette, BoolHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Refineable as _, RenderOnce, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled, Window,
};

/// A 14 px box with a label after it; clicking either toggles. The
/// handler is given the *new* state, on release. It takes the state by
/// reference so a `Context::listener` closure fits it directly.
#[derive(gpui::IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
    style: StyleRefinement,
    on_change: Option<BoolHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Checkbox {
            id: id.into(),
            label: label.into(),
            checked,
            disabled: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Faint, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let checked = self.checked;
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .when(self.disabled, |d| d.text_color(gpui::rgb(p.text_faint)));
        el.style().refine(&self.style);
        let mark = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(14.0))
            .rounded_sm()
            .bg(gpui::rgb(if checked { p.accent } else { p.field_bg }))
            .border_1()
            .border_color(gpui::rgb(p.edge))
            .when(self.disabled, |d| d.opacity(0.4))
            .when(checked, |d| d.child(icon("check", 10.0, p.accent_text)));
        let mut el = el.id(self.id).child(mark);
        if !self.disabled {
            el = el.cursor_pointer();
            if let Some(on_change) = self.on_change {
                el = el.on_click(move |_e, window, cx| on_change(&!checked, window, cx));
            }
        }
        el.when(!self.label.is_empty(), |d| d.child(self.label))
    }
}
