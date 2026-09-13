//! A text box: one line by default, or a paragraph.
//!
//! The box only draws. Its text, caret and selection are the caller's
//! state -- usually a [`crate::LineEdit`], which also answers the
//! keystrokes -- and it takes the keyboard on the *press*, like every
//! click-to-focus field, through [`TextInput::on_focus`]. Every text
//! box in the application looks and behaves like the gallery's search
//! box, which is where this began.
//!
//! The text is one shaped run, which is what lets a press say *where*
//! in the text it landed: the handler is given a [`TextPress`] carrying
//! the byte offset under the pointer, so a click puts the caret there
//! rather than at the end, a drag ([`TextInput::on_select_to`]) sweeps
//! a selection out, and a double click takes a word. The caret is
//! painted over the run at the caret offset, and the selection is a
//! highlight on the run's own glyphs, so it is exactly as tall as the
//! line and is clipped to the box rather than spilling past its edge.

use crate::{metrics, on_press, palette, tip, ClickHandler, IconButton, LineEdit, PressHandler};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, fill, point, px, relative, size, App, Bounds, ClickEvent, CursorStyle, ElementId,
    HighlightStyle, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Pixels, Point, Refineable as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, StyledText, TextLayout, Window,
};
use std::ops::Range;

/// The line box a text box draws its text in, as a multiple of the text
/// size. gpui's default (the golden ratio) leaves a line nearly as tall
/// as a 22 px field, so a selection's fill reached the border and read
/// as spilling out of the box; this is snug around the text with room
/// either side.
const LINE_HEIGHT: f32 = 1.4;

/// Where a press inside a text box landed, and what it should mean.
///
/// A field answers one of these by moving its caret -- see
/// [`LineEdit::press`], which is what every box built on a `LineEdit`
/// does with it.
#[derive(Clone, Copy, Debug)]
pub struct TextPress {
    /// The byte offset in the text nearest the pointer, on a char
    /// boundary.
    pub offset: usize,
    /// Shift was held: the press extends the selection rather than
    /// dropping a fresh caret.
    pub shift: bool,
    /// 1 for a single click, 2 for a double (a word), 3 or more for the
    /// whole line.
    pub clicks: usize,
}

/// What [`TextInput::on_focus`] takes: the press, and where in the text
/// it was.
pub type TextPressHandler = Box<dyn Fn(&TextPress, &mut Window, &mut App) + 'static>;

/// What [`TextInput::on_select_to`] takes: the byte offset the pointer
/// has been dragged to. By reference, like every other event a handler
/// is given, so `cx.listener` can build one.
pub type OffsetHandler = Box<dyn Fn(&usize, &mut Window, &mut App) + 'static>;

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
    /// The fill behind selected text, and the text drawn over it.
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
/// (a byte offset, shown blinking while [`TextInput::active`]), and the
/// selected range. A [`crate::LineEdit`] supplies all of that through
/// [`TextInput::edit`]. The box has no width of its own; give it one
/// with [`Styled`] or let it grow.
#[derive(gpui::IntoElement)]
pub struct TextInput {
    id: ElementId,
    text: String,
    cursor: Option<usize>,
    selection: Option<Range<usize>>,
    select_all: bool,
    active: bool,
    caret_on: bool,
    multiline: bool,
    placeholder: Option<SharedString>,
    suffix: Option<SharedString>,
    align_end: bool,
    colors: Option<TextInputColors>,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    style: StyleRefinement,
    on_focus: Option<TextPressHandler>,
    on_select_to: Option<OffsetHandler>,
    on_clear: Option<ClickHandler>,
}

impl TextInput {
    pub fn new(id: impl Into<ElementId>, text: impl Into<String>) -> Self {
        TextInput {
            id: id.into(),
            text: text.into(),
            cursor: None,
            selection: None,
            select_all: false,
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
            on_select_to: None,
            on_clear: None,
        }
    }

    /// A box showing a [`LineEdit`]: its text, caret, selection and
    /// whether it is active.
    pub fn edit(id: impl Into<ElementId>, edit: &LineEdit) -> Self {
        Self::new(id, edit.text.clone())
            .cursor(edit.cursor)
            .selection(edit.selection())
            .active(edit.active)
            .when(edit.multiline, TextInput::multiline)
    }

    /// Where the caret is, as a byte offset; the end of the text unless
    /// told otherwise.
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// The selected range, as byte offsets: drawn on the selection fill,
    /// and what the next keystroke replaces. An empty range is no
    /// selection, which is the usual case.
    pub fn selection(mut self, selection: Range<usize>) -> Self {
        self.selection = (!selection.is_empty()).then_some(selection);
        self
    }

    /// The whole text is selected -- the shorthand for a field that
    /// holds a value typing replaces outright.
    pub fn selected(mut self, selected: bool) -> Self {
        self.select_all = selected;
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

    /// A paragraph rather than a line: the text wraps, newlines break,
    /// and the box grows with its content from a `min_h` the caller
    /// sets.
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

    /// The press that gives the box the keyboard, told where in the text
    /// it landed. Fires on the press, like every click-to-focus field,
    /// so the caret is there before the button comes back up.
    pub fn on_focus(
        mut self,
        handler: impl Fn(&TextPress, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_focus = Some(Box::new(handler));
        self
    }

    /// The pointer dragging across the box with the button down, told
    /// the byte offset it has reached: the caller extends its selection
    /// to there. Only fires while the box is [`TextInput::active`], so
    /// a drag that began somewhere else cannot move this box's caret.
    pub fn on_select_to(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select_to = Some(Box::new(handler));
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
        let text = SharedString::from(self.text);
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
            .line_height(relative(LINE_HEIGHT))
            .text_color(gpui::rgb(colors.text))
            .overflow_hidden()
            .cursor(CursorStyle::IBeam);
        if self.multiline {
            // A paragraph's height is its content's; the caller sets a
            // floor with `min_h`.
            el = el.h_auto().items_start().py_1();
        } else {
            // One line: a value too long for the box is cut off at the
            // edge rather than wrapped onto a second line nothing can
            // show.
            el = el.whitespace_nowrap();
        }
        // The caller's own refinements win over the defaults above.
        el.style().refine(&self.style);
        let mut el = el.id(self.id);
        if let Some((label, hint)) = self.tooltip {
            el = el.tooltip(tip(label, hint));
        }

        // The text as one shaped run, with the selection as a highlight
        // on the glyphs it covers. Its layout is what a press is read
        // against, so the run is built even while the box is empty --
        // an empty layout still answers "the caret goes here".
        let cursor = self.cursor.unwrap_or(text.len()).min(text.len());
        // Only a box with the keyboard shows a selection: several boxes
        // can share one caller's buffer (a dialog's fields do), and the
        // range in it belongs to whichever of them is focused.
        let selection = match (self.select_all, self.selection) {
            _ if !self.active => None,
            (true, _) if !empty => Some(0..text.len()),
            (_, Some(range)) => Some(clamp(&text, range)),
            _ => None,
        }
        .filter(|range| !range.is_empty());
        let run = StyledText::new(text.clone()).with_highlights(selection.map(|range| {
            (
                range,
                HighlightStyle {
                    color: Some(gpui::rgb(colors.selection_text).into()),
                    background_color: Some(gpui::rgb(colors.selection).into()),
                    ..Default::default()
                },
            )
        }));
        let layout = run.layout().clone();

        // The press and the drag, both read against that layout.
        if let Some(on_focus) = self.on_focus {
            let (layout, text) = (layout.clone(), text.clone());
            let press: PressHandler = Box::new(move |ev: &MouseDownEvent, window, cx| {
                let press = TextPress {
                    offset: offset_at(&layout, &text, ev.position),
                    shift: ev.modifiers.shift,
                    clicks: ev.click_count,
                };
                on_focus(&press, window, cx);
            });
            el = on_press(el, press);
        }
        if let (Some(on_select_to), true) = (self.on_select_to, self.active) {
            let (layout, text) = (layout.clone(), text.clone());
            el = el.on_mouse_move(move |ev: &MouseMoveEvent, window, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    on_select_to(&offset_at(&layout, &text, ev.position), window, cx);
                }
            });
        }

        let mut content = div()
            .relative()
            .flex()
            .flex_row()
            .flex_grow()
            .min_w_0()
            .overflow_hidden()
            .when(self.align_end, |d| d.justify_end())
            .when(!self.align_end, |d| d.justify_start())
            .child(run)
            .children(empty.then(|| ghost(self.placeholder.clone(), colors.placeholder)));
        if self.active && self.caret_on {
            content = content.child(caret(layout, cursor, colors.text, self.align_end));
        }
        el = el.child(content);
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
        .flex_none()
        .text_color(gpui::rgb(color))
        .whitespace_nowrap()
        .children(placeholder)
}

/// The caret: a one-pixel bar painted over the run at the caret offset,
/// as tall as the line. It draws nothing of its own layout, so the text
/// does not shuffle as it blinks, and the box's own clipping keeps it
/// inside the border when the text is longer than the box.
fn caret(layout: TextLayout, cursor: usize, color: u32, align_end: bool) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let line_height = layout.line_height();
            // An empty box has no glyph to sit beside: the caret goes
            // against whichever edge the text would have started from.
            let at = layout.position_for_index(cursor).unwrap_or(point(
                if align_end {
                    bounds.right()
                } else {
                    bounds.left()
                },
                bounds.top(),
            ));
            window.paint_quad(fill(
                Bounds::new(at, size(px(1.0), line_height)),
                gpui::rgba((color << 8) | 0xFF),
            ));
        },
    )
    .absolute()
    .inset_0()
}

/// The byte offset in `text` nearest a point in the window.
fn offset_at(layout: &TextLayout, text: &str, at: Point<Pixels>) -> usize {
    // Outside the text, the layout gives the offset it fell short of or
    // ran past, which is where a caret belongs anyway.
    let index = layout
        .index_for_position(at)
        .unwrap_or_else(|index| index)
        .min(text.len());
    // Inside it, that is the character the point is over; the caret
    // lands on whichever of its two edges is nearer, as it must for
    // clicking the right half of the last character to put the caret
    // after it.
    let next = crate::caret_right(text, index);
    match (
        layout.position_for_index(index),
        layout.position_for_index(next),
    ) {
        (Some(before), Some(after))
            if before.y == after.y && (at.x - before.x).abs() > (after.x - at.x).abs() =>
        {
            next
        }
        _ => index,
    }
}

/// A range from the caller, held to the text's char boundaries.
fn clamp(text: &str, range: Range<usize>) -> Range<usize> {
    let end = range.end.min(text.len());
    let start = range.start.min(end);
    let floor = |at: usize| {
        if text.is_char_boundary(at) {
            at
        } else {
            crate::caret_left(text, at)
        }
    };
    floor(start)..floor(end)
}
