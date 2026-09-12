//! The pieces a panel or dialog is laid out with: labelled rows,
//! headings, hairlines, and the modal frame.

use crate::{metrics, palette};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, InteractiveElement as _, IntoElement, ParentElement, Refineable as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};

/// A row with a dim label in a fixed column at the left and the
/// control it names at the right. 26 px high, the label column 110 px.
#[derive(gpui::IntoElement)]
pub struct FieldRow {
    label: SharedString,
    label_width: f32,
    top_aligned: bool,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl FieldRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        FieldRow {
            label: label.into(),
            label_width: 110.0,
            top_aligned: false,
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }

    /// The label column's width, in pixels.
    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width;
        self
    }

    /// For a control taller than a line: the label sits level with its
    /// first line rather than centred, and the row takes the control's
    /// height.
    pub fn top_aligned(mut self) -> Self {
        self.top_aligned = true;
        self
    }
}

impl Styled for FieldRow {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for FieldRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for FieldRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut el = div()
            .flex()
            .flex_row()
            .justify_between()
            .gap_3()
            .when(!self.top_aligned, |d| d.items_center().h(px(26.0)));
        el.style().refine(&self.style);
        el.child(
            div()
                .w(px(self.label_width))
                .flex_none()
                .when(self.top_aligned, |d| d.pt(px(4.0)))
                .text_size(px(12.0))
                .text_color(gpui::rgb(palette().text_dim))
                .child(self.label),
        )
        .children(self.children)
    }
}

/// A small dim caption over a group of controls: a panel's title, a
/// sidebar section, a dialog section. 11 px in the dim text colour.
#[derive(gpui::IntoElement)]
pub struct Heading {
    text: SharedString,
    uppercase: bool,
    color: Option<u32>,
    style: StyleRefinement,
}

impl Heading {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Heading {
            text: text.into(),
            uppercase: false,
            color: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set in capitals, the way panel titles are.
    pub fn uppercase(mut self) -> Self {
        self.uppercase = true;
        self
    }

    /// A colour of the caller's own, for chrome on another palette.
    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }
}

impl Styled for Heading {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Heading {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let text = if self.uppercase {
            SharedString::from(self.text.to_uppercase())
        } else {
            self.text
        };
        let mut el = div()
            .text_size(px(metrics().small_text))
            .text_color(gpui::rgb(self.color.unwrap_or(palette().text_dim)));
        el.style().refine(&self.style);
        el.child(text)
    }
}

/// A one-pixel hairline. Horizontal ones stretch across their column;
/// vertical ones take the row's height.
#[derive(gpui::IntoElement)]
pub struct Divider {
    vertical: bool,
    color: Option<u32>,
    style: StyleRefinement,
}

impl Divider {
    pub fn horizontal() -> Self {
        Divider {
            vertical: false,
            color: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn vertical() -> Self {
        Divider {
            vertical: true,
            color: None,
            style: StyleRefinement::default(),
        }
    }

    /// Between groups of rows in a menu: the popup's edge colour, which
    /// reads on the popup fill where the panel hairline would vanish,
    /// with a little room either side.
    pub fn menu(self) -> Self {
        self.color(palette().edge).my_1()
    }

    /// A colour of the caller's own.
    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }
}

impl Styled for Divider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut el = div()
            .flex_none()
            .bg(gpui::rgb(self.color.unwrap_or(palette().divider)));
        el = if self.vertical {
            el.w(px(1.0)).h_full()
        } else {
            el.h(px(1.0)).w_full()
        };
        el.style().refine(&self.style);
        el
    }
}

/// A dialog: a dimmed backdrop over the whole window, and a centred
/// card with a title, the body the caller adds as children, and a row
/// of action buttons at the bottom right.
///
/// `width` is what the dialog asks for; a window narrower than that (a
/// phone) gets the card at the window's width less a margin, its text
/// rewrapped, and a card taller than the window scrolls its body.
#[derive(gpui::IntoElement)]
pub struct Modal {
    title: SharedString,
    width: f32,
    style: StyleRefinement,
    children: Vec<AnyElement>,
    actions: Vec<AnyElement>,
}

impl Modal {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Modal {
            title: title.into(),
            width: 360.0,
            style: StyleRefinement::default(),
            children: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// The card's width, in pixels.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// A button for the action row; they read left to right in the
    /// order added, with the primary one last.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Several actions at once.
    pub fn actions(mut self, actions: impl IntoIterator<Item = AnyElement>) -> Self {
        self.actions.extend(actions);
        self
    }
}

impl Styled for Modal {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Modal {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Modal {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let p = palette();
        let mut card = div()
            .flex()
            .flex_col()
            .w(px(self.width))
            .max_w_full()
            .max_h_full()
            .p_3()
            .gap_2()
            .rounded_md()
            .bg(gpui::rgb(p.panel_bg))
            .border_1()
            .border_color(gpui::rgb(p.edge))
            .shadow_lg()
            .text_color(gpui::rgb(p.text));
        card.style().refine(&self.style);
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .items_center()
            .justify_center()
            .p_2()
            .bg(gpui::rgba(0x00000080))
            // The backdrop must swallow the pointer, or the canvas
            // underneath keeps its hit box and the active tool edits
            // the document while the dialog is open.
            .occlude()
            .child(
                card.child(
                    div()
                        .text_size(px(13.0))
                        .pb_1()
                        .border_b_1()
                        .border_color(gpui::rgb(p.divider))
                        .child(self.title),
                )
                .child(
                    div()
                        .id("modal-body")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .children(self.children),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_none()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .children(self.actions),
                ),
            )
    }
}
