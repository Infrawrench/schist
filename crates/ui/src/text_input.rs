//! A text box: one line by default, or a paragraph.
//!
//! The box only draws. Its text, caret and selection are the caller's
//! state -- usually a [`crate::LineEdit`], which also answers the
//! keystrokes -- and it takes the keyboard on the *press*, like every
//! click-to-focus field, through [`TextInput::on_focus`]. Every text
//! box in the application looks and behaves like the gallery's search
//! box, which is where this began.

use crate::{metrics, on_press, palette, tip, ClickHandler, IconButton, LineEdit, PressHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClickEvent, CursorStyle, ElementId, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Refineable as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};

/// Explicit colours for a [`TextInput`], for chrome that keeps its own
/// palette (the gallery).
#[derive(Clone, Copy, Debug)]
pub struct TextInputColors {
    pub bg: u32,
    /// The border at rest; `None` draws it in the fill, so the box has
    /// an edge only once focused.
    pub border: Option<u32>,
    /// The border while the box has the keyboard.
    pub focus_border: u32,
    pub text: u32,
    pub placeholder: u32,
    /// The fill behind a selected line, and the text drawn over it.
    pub selection: u32,
    pub selection_text: u32,
}

impl Default for TextInputColors {
    fn default() -> Self {
        let p = palette();
        TextInputColors {
            bg: p.field_bg,
            border: None,
            focus_border: p.accent,
            text: p.text,
            placeholder: p.text_dim,
            selection: p.selection_bg,
            selection_text: p.text,
        }
    }
}

/// A text box. 22 px high with 12 px text (a touch target's height and
/// the larger type on a touch screen) on the field fill; the border
/// shows in the accent while it has the keyboard.
///
/// What it draws is decided by the caller's state: the text, a caret
/// (a byte offset, shown blinking while [`TextInput::active`]), and
/// whether the whole line is selected. A [`crate::LineEdit`] supplies
/// all of that through [`TextInput::edit`]. The box has no width of its
/// own; give it one with [`Styled`] or let it grow.
#[derive(gpui::IntoElement)]
pub struct TextInput {
    id: ElementId,
    text: String,
    cursor: Option<usize>,
    selected: bool,
    active: bool,
    caret_on: bool,
    multiline: bool,
    placeholder: Option<SharedString>,
    suffix: Option<SharedString>,
    align_end: bool,
    colors: Option<TextInputColors>,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    style: StyleRefinement,
    on_focus: Option<PressHandler>,
    on_clear: Option<ClickHandler>,
}

impl TextInput {
    pub fn new(id: impl Into<ElementId>, text: impl Into<String>) -> Self {
        TextInput {
            id: id.into(),
            text: text.into(),
            cursor: None,
            selected: false,
            active: false,
            caret_on: true,
            multiline: false,
            placeholder: None,
            suffix: None,
            align_end: false,
            colors: None,
            tooltip: None,
            style: StyleRefinement::default(),
            on_focus: None,
            on_clear: None,
        }
    }

    /// A box showing a [`LineEdit`]: its text, caret, selection and
    /// whether it is active.
    pub fn edit(id: impl Into<ElementId>, edit: &LineEdit) -> Self {
        Self::new(id, edit.text.clone())
            .cursor(edit.cursor)
            .selected(edit.selected)
            .active(edit.active)
    }

    /// Where the caret is, as a byte offset; the end of the text unless
    /// told otherwise.
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// The whole text is selected: drawn on the selection fill, and the
    /// next keystroke replaces it.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The box has the keyboard: it shows its caret and focus border.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Whether this instant of the blink shows the caret. Solid unless
    /// told otherwise; the owner's blink timer drives it.
    pub fn caret_on(mut self, on: bool) -> Self {
        self.caret_on = on;
        self
    }

    /// A paragraph rather than a line: newlines break, and the box grows
    /// with its content from a `min_h` the caller sets.
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Ghosted while the box is empty.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        let placeholder = placeholder.into();
        self.placeholder = (!placeholder.is_empty()).then_some(placeholder);
        self
    }

    /// A dim unit after the text: "px", "%".
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        let suffix = suffix.into();
        self.suffix = (!suffix.is_empty()).then_some(suffix);
        self
    }

    /// Text sits against the right edge, as numbers do.
    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    /// Colours of the caller's own choosing.
    pub fn colors(mut self, colors: TextInputColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// A hover label, with an optional shortcut hint after it.
    pub fn tooltip(mut self, label: impl Into<SharedString>, hint: Option<SharedString>) -> Self {
        self.tooltip = Some((label.into(), hint));
        self
    }

    /// The press that gives the box the keyboard. Fires on the press,
    /// like every click-to-focus field, so the caret is there before
    /// the button comes back up.
    pub fn on_focus(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_focus = Some(Box::new(handler));
        self
    }

    /// Show a ✕ at the right while there is text, firing this when it
    /// is clicked.
    pub fn on_clear(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_clear = Some(Box::new(handler));
        self
    }
}

impl Styled for TextInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextInput {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let colors = self.colors.unwrap_or_default();
        let m = metrics();
        let text = self.text;
        let empty = text.is_empty();
        let border = if self.active {
            colors.focus_border
        } else {
            colors.border.unwrap_or(colors.bg)
        };
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .h(px(m.icon_button))
            .px_1()
            .rounded_sm()
            .bg(gpui::rgb(colors.bg))
            .border_1()
            .border_color(gpui::rgb(border))
            .text_size(px(m.text))
            .text_color(gpui::rgb(colors.text))
            .overflow_hidden()
            .cursor(CursorStyle::IBeam);
        if self.multiline {
            // A paragraph's height is its content's; the caller sets a
            // floor with `min_h`.
            el = el.h_auto().items_start().py_1();
        }
        // The caller's own refinements win over the defaults above.
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if let Some(on_focus) = self.on_focus {
            el = on_press(el, on_focus);
        }
        if let Some((label, hint)) = self.tooltip {
            el = el.tooltip(tip(label, hint));
        }

        // The content: a selection, a caret in the text (with the
        // placeholder ghosted beside it while empty), or the plain text.
        let cursor = self.cursor.unwrap_or(text.len()).min(text.len());
        let content = if self.selected && !empty {
            div()
                .rounded_sm()
                .px(px(1.0))
                .bg(gpui::rgb(colors.selection))
                .text_color(gpui::rgb(colors.selection_text))
                .child(SharedString::from(text.clone()))
                .into_any_element()
        } else if self.active {
            let run = if self.multiline {
                caret_paragraph(&text, cursor, self.caret_on, colors.text).into_any_element()
            } else {
                caret_run(&text[..cursor], &text[cursor..], self.caret_on, colors.text)
                    .into_any_element()
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(run)
                .children(empty.then(|| ghost(self.placeholder.clone(), colors.placeholder)))
                .into_any_element()
        } else if empty {
            ghost(self.placeholder.clone(), colors.placeholder).into_any_element()
        } else if self.multiline {
            lines(&text).into_any_element()
        } else {
            div()
                .child(SharedString::from(text.clone()))
                .into_any_element()
        };
        el = el.child(
            div()
                .flex()
                .flex_row()
                .flex_grow()
                .min_w_0()
                .overflow_hidden()
                .when(self.align_end, |d| d.justify_end())
                .when(!self.align_end, |d| d.justify_start())
                .child(content),
        );
        if let Some(suffix) = self.suffix {
            el = el.child(
                div()
                    .flex_none()
                    .text_color(gpui::rgb(colors.placeholder))
                    .child(suffix),
            );
        }
        if let (Some(on_clear), false) = (self.on_clear, empty) {
            el = el.child(
                IconButton::new("clear", "close")
                    .size(16.0)
                    .icon_size(9.0)
                    .color(colors.placeholder)
                    .consume_press()
                    .on_click(on_clear),
            );
        }
        el
    }
}

/// The ghosted placeholder, or nothing.
fn ghost(placeholder: Option<SharedString>, color: u32) -> impl IntoElement {
    div()
        .text_color(gpui::rgb(color))
        .whitespace_nowrap()
        .children(placeholder)
}

/// One line of a paragraph. An empty line is given a space so it keeps
/// its height; a text run with nothing in it lays out as nothing.
fn line(line: &str) -> SharedString {
    if line.is_empty() {
        SharedString::from(" ")
    } else {
        SharedString::from(line.to_string())
    }
}

/// A paragraph with no caret: one child per line, so a single text run
/// does not flow the whole thing onto one line.
fn lines(text: &str) -> impl IntoElement {
    div().flex().flex_col().children(text.split('\n').map(line))
}

/// Text split around a caret bar that blinks. The bar keeps its
/// one-pixel slot while off, so the text does not shuffle as it
/// blinks; `color` is the field's text colour.
fn caret_run(before: &str, after: &str, on: bool, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .max_w_full()
        .overflow_hidden()
        .children((!before.is_empty()).then(|| {
            div()
                .flex_none()
                .child(SharedString::from(before.to_string()))
        }))
        .child(div().flex_none().w(px(1.0)).h(px(13.0)).bg(if on {
            gpui::rgba((color << 8) | 0xFF)
        } else {
            gpui::rgba(0x00000000)
        }))
        .children((!after.is_empty()).then(|| {
            div()
                .flex_none()
                .child(SharedString::from(after.to_string()))
        }))
}

/// A paragraph with the caret on whichever line holds byte `cursor`.
fn caret_paragraph(text: &str, cursor: usize, on: bool, color: u32) -> impl IntoElement {
    let before = &text[..cursor];
    let after = &text[cursor..];
    // The caret's line is the last line of `before` joined to the first
    // of `after`; whole lines either side draw plainly.
    let (head, line_before) = match before.rfind('\n') {
        Some(i) => (Some(&before[..i]), &before[i + 1..]),
        None => (None, before),
    };
    let (line_after, tail) = match after.find('\n') {
        Some(i) => (&after[..i], Some(&after[i + 1..])),
        None => (after, None),
    };
    let mut col = div().flex().flex_col();
    if let Some(head) = head {
        col = col.children(head.split('\n').map(line));
    }
    col = col.child(caret_run(line_before, line_after, on, color));
    if let Some(tail) = tail {
        col = col.children(tail.split('\n').map(line));
    }
    col
}
