//! The floating panel a menu, dropdown or picker opens in, and the rows
//! it holds.

use crate::{icon, metrics, on_press, palette, ClickHandler, Divider, ListItem, PressHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement, Refineable as _, RenderOnce, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};

/// A popup's frame: absolutely positioned where the caller puts it
/// (`left`, `top`, `w` through [`Styled`]), on the popup fill with a
/// hairline edge and a shadow, and opaque to the pointer so nothing
/// beneath it reacts. A press outside it fires [`Popover::on_dismiss`].
///
/// Wrap it in [`gpui::deferred`] to draw it over its siblings.
#[derive(gpui::IntoElement)]
pub struct Popover {
    id: ElementId,
    in_flow: bool,
    scroll: Option<ScrollHandle>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_dismiss: Option<PressHandler>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Popover {
            id: id.into(),
            in_flow: false,
            scroll: None,
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_dismiss: None,
        }
    }

    /// Laid out by its parent rather than placed absolutely: for a frame
    /// inside [`gpui::anchored`], which positions it.
    pub fn in_flow(mut self) -> Self {
        self.in_flow = true;
        self
    }

    /// Scroll a long list inside the frame; pair it with `max_h`.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    /// A press anywhere outside the frame.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Popover {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .when(!self.in_flow, |d| d.absolute())
            .flex()
            .flex_col()
            .py_1()
            .bg(gpui::rgb(p.popup_bg))
            .text_color(gpui::rgb(p.text))
            .border_1()
            .border_color(gpui::rgb(p.edge))
            .rounded_sm()
            .shadow_lg();
        el.style().refine(&self.style);
        let mut el = el.id(self.id).occlude();
        if let Some(handle) = &self.scroll {
            el = el.overflow_y_scroll().track_scroll(handle);
        }
        if let Some(on_dismiss) = self.on_dismiss {
            el = el.on_mouse_down_out(on_dismiss);
        }
        el.children(self.children)
    }
}

/// A menu row: a label, an optional shortcut hint at the right, and a
/// check gutter for the rows that toggle something. Lights in the
/// accent under the pointer and fires on release. Its height and type
/// come from [`metrics`], so it is a finger's target on touch.
#[derive(gpui::IntoElement)]
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    hint: Option<SharedString>,
    checked: Option<bool>,
    submenu: bool,
    highlighted: bool,
    disabled: bool,
    style: StyleRefinement,
    on_click: Option<ClickHandler>,
    on_hover: Option<crate::BoolHandler>,
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        MenuItem {
            id: id.into(),
            label: label.into(),
            hint: None,
            checked: None,
            submenu: false,
            highlighted: false,
            disabled: false,
            style: StyleRefinement::default(),
            on_click: None,
            on_hover: None,
        }
    }

    /// The shortcut, dimmed at the right.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        let hint = hint.into();
        self.hint = (!hint.is_empty()).then_some(hint);
        self
    }

    /// A row that toggles something: reserves the check gutter, and
    /// shows a tick when `Some(true)`.
    pub fn checked(mut self, checked: Option<bool>) -> Self {
        self.checked = checked;
        self
    }

    /// A row that opens another menu: a chevron at the right.
    pub fn submenu(mut self) -> Self {
        self.submenu = true;
        self
    }

    /// Drawn as if hovered: the row the keyboard is on, or whose
    /// submenu is open.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// See [`ListItem::on_hover`].
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl Styled for MenuItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MenuItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette();
        let m = metrics();
        let mut row = ListItem::new(self.id)
            .h(px(m.menu_row_h))
            .justify_between()
            .accent_hover()
            .highlighted(self.highlighted)
            .disabled(self.disabled)
            .when_some(self.on_click, |r, h| r.on_click(h))
            .when_some(self.on_hover, |r, h| r.on_hover(h));
        row.style().refine(&self.style);
        let label = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .when(self.checked.is_some(), |d| {
                // A fixed gutter, so labels line up whether or not the
                // row is checkable.
                d.child(
                    div().w(px(12.0)).flex_none().children(
                        self.checked
                            .unwrap_or(false)
                            .then(|| icon("check", 10.0, p.text)),
                    ),
                )
            })
            .child(div().text_size(px(m.row_text)).child(self.label));
        let trailing = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_size(px(m.small_text - 1.0))
            .text_color(gpui::rgb(p.text_dim))
            .children(self.hint)
            .when(self.submenu, |d| {
                d.child(icon("chevron-right", 10.0, p.text_dim))
            });
        row.child(label).child(trailing).render(window, cx)
    }
}

/// The hairline between groups of menu rows.
pub fn menu_separator() -> Divider {
    Divider::horizontal().menu()
}

/// The button a dropdown opens from: the current value with a chevron
/// after it, on the field fill. Opens on the *press*, as native popup
/// buttons do (the release then lands on a row of the list).
///
/// Children go before the label, for an icon or a preview.
#[derive(gpui::IntoElement)]
pub struct DropdownButton {
    id: ElementId,
    label: SharedString,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_press: Option<PressHandler>,
}

impl DropdownButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        DropdownButton {
            id: id.into(),
            label: label.into(),
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_press: None,
        }
    }

    pub fn on_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_press = Some(Box::new(handler));
        self
    }
}

impl Styled for DropdownButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for DropdownButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DropdownButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_1()
            .h(px(20.0))
            .px_1()
            .rounded_sm()
            .bg(gpui::rgb(p.field_bg))
            .text_size(px(11.0))
            .text_color(gpui::rgb(p.text))
            .cursor_pointer();
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if let Some(handler) = self.on_press {
            el = on_press(el, handler);
        }
        el.children(self.children)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.label),
            )
            .child(icon("chevron-down", 11.0, p.text_dim))
    }
}
