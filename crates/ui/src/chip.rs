//! Small pills: a choice among a few, and a count or a mark.

use crate::{palette, Button, ButtonColors, ClickHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, ElementId, IntoElement, ParentElement, Refineable as _,
    RenderOnce, SharedString, StyleRefinement, Styled, Window,
};

/// Colours for a [`Chip`] on another palette.
#[derive(Clone, Copy, Debug)]
pub struct ChipColors {
    pub bg: u32,
    pub hover: u32,
    pub text: u32,
    pub selected_bg: u32,
    pub selected_text: u32,
}

impl Default for ChipColors {
    fn default() -> Self {
        let p = palette();
        ChipColors {
            bg: p.control_bg,
            hover: p.hover,
            text: p.text,
            selected_bg: p.selection_bg,
            selected_text: p.text,
        }
    }
}

/// A rounded pill among a row of them, one of which is selected: the
/// gallery's GROUP BY, a panel's tabs, a set of presets. 20 px high with
/// 11 px text; fires on release.
///
/// Children go after the label, for a count or a ✕.
#[derive(gpui::IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    selected: bool,
    colors: Option<ChipColors>,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Chip {
            id: id.into(),
            label: label.into(),
            selected: false,
            colors: None,
            tooltip: None,
            style: StyleRefinement::default(),
            children: Vec::new(),
            on_click: None,
        }
    }

    /// The chip that is the current choice: filled, and unmoved by the
    /// pointer.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn colors(mut self, colors: ChipColors) -> Self {
        self.colors = Some(colors);
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

impl Styled for Chip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Chip {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = self.colors.unwrap_or_default();
        let colors = if self.selected {
            ButtonColors {
                bg: Some(c.selected_bg),
                hover: c.selected_bg,
                text: c.selected_text,
                border: None,
            }
        } else {
            ButtonColors {
                bg: Some(c.bg),
                hover: c.hover,
                text: c.text,
                border: None,
            }
        };
        let mut button = Button::new(self.id, self.label)
            .colors(colors)
            .when_some(self.tooltip, |b, (label, hint)| b.tooltip(label, hint))
            .when_some(self.on_click, |b, h| b.on_click(h))
            .h(px(20.0))
            .px_2()
            .rounded_md()
            .text_size(px(11.0));
        button.style().refine(&self.style);
        button.extend(self.children);
        button.render(window, cx)
    }
}

/// A small round mark: a count on a tile, a "?" beside an unnamed
/// group. Filled in the accent with 10 px text; [`Badge::outlined`]
/// draws a ring in the dim text colour instead.
#[derive(gpui::IntoElement)]
pub struct Badge {
    text: SharedString,
    outlined: bool,
    bg: Option<u32>,
    color: Option<u32>,
    style: StyleRefinement,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Badge {
            text: text.into(),
            outlined: false,
            bg: None,
            color: None,
            style: StyleRefinement::default(),
        }
    }

    /// A ring rather than a fill.
    pub fn outlined(mut self) -> Self {
        self.outlined = true;
        self
    }

    /// The fill (or, outlined, the ring) and the text colour.
    pub fn colors(mut self, bg: u32, text: u32) -> Self {
        self.bg = Some(bg);
        self.color = Some(text);
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut el = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full();
        el = if self.outlined {
            let color = self.color.unwrap_or(p.text_dim);
            el.size(px(20.0))
                .border_1()
                .border_color(gpui::rgb(self.bg.unwrap_or(color)))
                .text_size(px(11.0))
                .text_color(gpui::rgb(color))
        } else {
            el.min_w(px(18.0))
                .h(px(18.0))
                .px_1()
                .bg(gpui::rgb(self.bg.unwrap_or(p.accent)))
                .text_size(px(10.0))
                .text_color(gpui::rgb(self.color.unwrap_or(p.accent_text)))
        };
        el.style().refine(&self.style);
        el.child(self.text)
    }
}
