//! Push buttons: labelled, icon-only, or carrying whatever the caller
//! puts in them.

use crate::{icon, palette, tip, ClickHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, Refineable as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};

/// How a [`Button`] is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonVariant {
    /// The accent fill: a dialog's default action, Send, OK.
    Primary,
    /// The plain grey fill most buttons wear.
    #[default]
    Secondary,
    /// No fill until hovered: toolbar and panel-header controls.
    Ghost,
}

/// Explicit colours for a [`Button`], for chrome that keeps its own
/// palette (the gallery's green import button).
#[derive(Clone, Copy, Debug)]
pub struct ButtonColors {
    /// The fill, or none for a transparent button.
    pub bg: Option<u32>,
    /// The fill while the pointer is over it.
    pub hover: u32,
    pub text: u32,
    /// A one-pixel border, if any.
    pub border: Option<u32>,
}

impl ButtonColors {
    fn of(variant: ButtonVariant) -> Self {
        let p = palette();
        match variant {
            ButtonVariant::Primary => ButtonColors {
                bg: Some(p.accent),
                hover: p.accent_hover,
                text: p.accent_text,
                border: None,
            },
            ButtonVariant::Secondary => ButtonColors {
                bg: Some(p.button_bg),
                hover: p.button_hover,
                text: p.text,
                border: None,
            },
            ButtonVariant::Ghost => ButtonColors {
                bg: None,
                hover: p.hover,
                text: p.text,
                border: None,
            },
        }
    }
}

/// A push button. Fires [`Button::on_click`] when the mouse is released
/// over it (or on Enter/Space while focused), never on the press.
///
/// Defaults to 24 px high with a 12 px label and the secondary fill;
/// every one of those is a [`Styled`] call away from something else.
/// Children added through [`ParentElement`] go after the label, so an
/// icon-and-text button is `Button::new(id, "Save").child(icon(..))`,
/// and a button with no label at all is [`Button::bare`].
#[derive(gpui::IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    variant: ButtonVariant,
    colors: Option<ButtonColors>,
    active: bool,
    disabled: bool,
    consume_press: bool,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// A labelled button with the secondary fill.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::bare(id).label(label)
    }

    /// A button with nothing in it yet: add children for its content.
    pub fn bare(id: impl Into<ElementId>) -> Self {
        Button {
            id: id.into(),
            label: None,
            variant: ButtonVariant::Secondary,
            colors: None,
            active: false,
            disabled: false,
            consume_press: false,
            tooltip: None,
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.label = (!label.is_empty()).then_some(label);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// The accent fill.
    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    /// No fill until hovered.
    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    /// Colours of the caller's own choosing, in place of the variant's.
    pub fn colors(mut self, colors: ButtonColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Lit in the accent, for a toggle that is on or a tool that is the
    /// current one. Hovering an active button does not change it.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Dimmed, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Swallow the press so whatever the button sits in (a selectable
    /// row, a draggable tile) does not also react to it. The click still
    /// fires on release.
    pub fn consume_press(mut self) -> Self {
        self.consume_press = true;
        self
    }

    /// A hover label, with an optional shortcut hint after it.
    pub fn tooltip(mut self, label: impl Into<SharedString>, hint: Option<SharedString>) -> Self {
        self.tooltip = Some((label.into(), hint));
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let colors = self
            .colors
            .unwrap_or_else(|| ButtonColors::of(self.variant));
        let (bg, hover, text) = if self.active {
            (Some(p.accent), p.accent, p.accent_text)
        } else if self.disabled && colors.bg.is_none() {
            (None, colors.hover, p.text_faint)
        } else {
            (colors.bg, colors.hover, colors.text)
        };
        let mut el = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1()
            .h(px(24.0))
            .px_3()
            .rounded_sm()
            .text_size(px(12.0))
            .text_color(gpui::rgb(text))
            .when_some(bg, |d, bg| d.bg(gpui::rgb(bg)))
            .when_some(colors.border, |d, b| {
                d.border_1().border_color(gpui::rgb(b))
            });
        // The caller's own refinements win over the defaults above.
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if self.disabled {
            // A filled button fades as a whole; a ghost one has nothing
            // to fade but its text, handled above.
            if bg.is_some() {
                el = el.opacity(0.4);
            }
        } else {
            el = el.hover(move |s| s.bg(gpui::rgb(hover)));
            if let Some(on_click) = self.on_click {
                el = el
                    .cursor_pointer()
                    // Pressed: a touch dimmer, so the press reads before
                    // the release commits it.
                    .active(|s| s.opacity(0.8))
                    .on_click(on_click);
            }
            if self.consume_press {
                el = el.on_mouse_down(MouseButton::Left, |_e, _window, cx| cx.stop_propagation());
            }
        }
        if let Some((label, hint)) = self.tooltip {
            el = el.tooltip(tip(label, hint));
        }
        el.children(self.label).children(self.children)
    }
}

/// A square button holding one icon: the panel headers' new/delete
/// controls, the ± steppers, a tab's close.
///
/// 22 px square with a 14 px icon by default, no fill until hovered.
#[derive(gpui::IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: SharedString,
    size: f32,
    icon_size: f32,
    color: Option<u32>,
    active: bool,
    disabled: bool,
    consume_press: bool,
    filled: bool,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    style: StyleRefinement,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        IconButton {
            id: id.into(),
            icon: icon.into(),
            size: 22.0,
            icon_size: 14.0,
            color: None,
            active: false,
            disabled: false,
            consume_press: false,
            filled: false,
            tooltip: None,
            style: StyleRefinement::default(),
            on_click: None,
        }
    }

    /// The button's side, in pixels.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// The icon's side, in pixels.
    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }

    /// The icon's tint; the text colour unless told otherwise.
    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    /// The control fill even at rest, as the ± steppers beside a field
    /// have, instead of appearing only under the pointer.
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    /// Lit in the accent.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Faint, and deaf to the pointer.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// See [`Button::consume_press`].
    pub fn consume_press(mut self) -> Self {
        self.consume_press = true;
        self
    }

    pub fn tooltip(mut self, label: impl Into<SharedString>, hint: Option<SharedString>) -> Self {
        self.tooltip = Some((label.into(), hint));
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for IconButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette();
        let color = if self.active {
            p.accent_text
        } else if self.disabled {
            p.text_faint
        } else {
            self.color.unwrap_or(p.text)
        };
        let mut button = Button::bare(self.id)
            .ghost()
            .when(self.filled, |b| {
                b.colors(ButtonColors {
                    bg: Some(p.control_bg),
                    hover: p.button_hover,
                    text: p.text,
                    border: None,
                })
            })
            .active(self.active)
            .disabled(self.disabled)
            .when(self.consume_press, Button::consume_press)
            .when_some(self.tooltip, |b, (label, hint)| b.tooltip(label, hint))
            .when_some(self.on_click, |b, handler| b.on_click(handler))
            .size(px(self.size))
            .px_0()
            .child(icon(&self.icon, self.icon_size, color));
        button.style().refine(&self.style);
        button.render(window, cx)
    }
}
