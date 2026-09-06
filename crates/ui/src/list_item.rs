//! One row of a menu, a dropdown, a flyout or a panel list.

use crate::{palette, BoolHandler, ClickHandler};
use gpui::{
    div, px, AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, Refineable as _, RenderOnce, StatefulInteractiveElement as _,
    StyleRefinement, Styled, Window,
};

/// A clickable row. The caller supplies its content through
/// [`ParentElement`]; the row lays it out as a horizontal strip, 24 px
/// high with 8 px side padding, lights on hover, and fires
/// [`ListItem::on_click`] on release.
#[derive(gpui::IntoElement)]
pub struct ListItem {
    id: ElementId,
    selected: bool,
    highlighted: bool,
    accent_hover: bool,
    disabled: bool,
    consume_press: bool,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
    on_hover: Option<BoolHandler>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        ListItem {
            id: id.into(),
            selected: false,
            highlighted: false,
            accent_hover: false,
            disabled: false,
            consume_press: false,
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_click: None,
            on_hover: None,
        }
    }

    /// The row that is the current value: accent fill, and unmoved by
    /// the pointer.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The row the keyboard has walked to: drawn as if hovered.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    /// Hover in the accent rather than the quiet row hover, the way
    /// menus do.
    pub fn accent_hover(mut self) -> Self {
        self.accent_hover = true;
        self
    }

    /// Faint, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// See [`crate::Button::consume_press`].
    pub fn consume_press(mut self) -> Self {
        self.consume_press = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Told `true` as the pointer arrives and `false` as it leaves; a
    /// menu row opens its submenu from this.
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl Styled for ListItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .px_2()
            .h(px(24.0))
            .text_size(px(12.0));
        if self.selected {
            el = el
                .bg(gpui::rgb(p.accent))
                .text_color(gpui::rgb(p.accent_text));
        } else if self.highlighted {
            el = el.bg(gpui::rgb(p.hover));
        }
        if self.disabled {
            el = el.text_color(gpui::rgb(p.text_faint));
        }
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if !self.disabled {
            el = el.cursor_pointer();
            if !self.selected {
                let accent = self.accent_hover;
                el = el.hover(move |s| {
                    if accent {
                        s.bg(gpui::rgb(p.accent))
                            .text_color(gpui::rgb(p.accent_text))
                    } else {
                        s.bg(gpui::rgb(p.hover))
                    }
                });
            }
            if let Some(on_click) = self.on_click {
                el = el.on_click(on_click);
            }
            if let Some(on_hover) = self.on_hover {
                el = el.on_hover(on_hover);
            }
            if self.consume_press {
                el = el.on_mouse_down(MouseButton::Left, |_e, _window, cx| cx.stop_propagation());
            }
        }
        el.children(self.children)
    }
}
