//! A radio button with a label to its right.

use crate::{palette, ClickHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Refineable as _, RenderOnce, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled, Window,
};

/// One of a set of exclusive choices: a 14 px ring, filled with a dot
/// while it is the choice, and a label after it. Clicking either
/// selects it, on release; selecting the current one does nothing.
#[derive(gpui::IntoElement)]
pub struct Radio {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
    style: StyleRefinement,
    on_select: Option<ClickHandler>,
}

impl Radio {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Radio {
            id: id.into(),
            label: label.into(),
            checked,
            disabled: false,
            style: StyleRefinement::default(),
            on_select: None,
        }
    }

    /// Faint, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_select(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Box::new(handler));
        self
    }
}

impl Styled for Radio {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Radio {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .text_size(px(12.0))
            .when(self.disabled, |d| d.text_color(gpui::rgb(p.text_faint)));
        el.style().refine(&self.style);
        let ring = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(14.0))
            .rounded_full()
            .bg(gpui::rgb(p.field_bg))
            .border_1()
            .border_color(gpui::rgb(if self.checked { p.accent } else { p.edge }))
            .when(self.disabled, |d| d.opacity(0.4))
            .when(self.checked, |d| {
                d.child(div().size(px(6.0)).rounded_full().bg(gpui::rgb(p.accent)))
            });
        let mut el = el.id(self.id).child(ring);
        if !self.disabled {
            el = el.cursor_pointer();
            if let (Some(on_select), false) = (self.on_select, self.checked) {
                el = el.on_click(on_select);
            }
        }
        el.when(!self.label.is_empty(), |d| d.child(self.label))
    }
}
