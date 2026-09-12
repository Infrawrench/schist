//! A numeric field with ± steppers beside it.

use crate::{ClickHandler, IconButton, PressHandler, TextInput};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseDownEvent,
    ParentElement as _, Refineable as _, RenderOnce, SharedString, StyleRefinement, Styled, Window,
};

/// A number's box, right-aligned with its unit after it, and a minus
/// and a plus button. Click the box to type into it; the steppers fire
/// on release like any button.
///
/// The box shows whatever text the caller gives it -- the committed
/// value, or the digits typed so far while it is focused -- so it
/// draws from the same state as every other field.
#[derive(gpui::IntoElement)]
pub struct NumberField {
    id: ElementId,
    text: String,
    suffix: Option<SharedString>,
    focused: bool,
    style: StyleRefinement,
    on_focus: Option<PressHandler>,
    on_decrement: Option<ClickHandler>,
    on_increment: Option<ClickHandler>,
}

impl NumberField {
    pub fn new(id: impl Into<ElementId>, text: impl Into<String>) -> Self {
        NumberField {
            id: id.into(),
            text: text.into(),
            suffix: None,
            focused: false,
            style: StyleRefinement::default(),
            on_focus: None,
            on_decrement: None,
            on_increment: None,
        }
    }

    /// The unit after the number: "px", "%".
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        let suffix = suffix.into();
        self.suffix = (!suffix.is_empty()).then_some(suffix);
        self
    }

    /// The box has the keyboard.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The press on the box that gives it the keyboard.
    pub fn on_focus(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_focus = Some(Box::new(handler));
        self
    }

    pub fn on_decrement(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_decrement = Some(Box::new(handler));
        self
    }

    pub fn on_increment(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_increment = Some(Box::new(handler));
        self
    }
}

impl Styled for NumberField {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NumberField {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut field = TextInput::new("value", self.text)
            .align_end()
            .active(self.focused)
            .when_some(self.suffix, TextInput::suffix)
            .when_some(self.on_focus, |f, h| f.on_focus(h))
            .w(px(62.0))
            .h(px(20.0))
            .text_size(px(11.0));
        // The caller's refinements shape the box, not the row.
        field.style().refine(&self.style);
        let step = |name: &'static str, handler: Option<ClickHandler>| {
            IconButton::new(name, name)
                .filled()
                .size(18.0)
                .icon_size(11.0)
                .when_some(handler, |b, h| b.on_click(h))
        };
        div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(field)
            .child(step("minus", self.on_decrement))
            .child(step("plus", self.on_increment))
    }
}
