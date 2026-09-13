//! The keyboard model behind a text box.
//!
//! GPUI ships no text editor, and a full one needs an IME handler, so
//! Schist's fields are plain state plus a keystroke function: the view
//! that owns a [`LineEdit`] hands it each [`gpui::KeyDownEvent`] while
//! the box is active, and draws it with [`crate::TextInput`]. Every box
//! built this way types, selects, pastes and moves its caret the same
//! way, which is the point.
//!
//! The selection is a caret and an anchor, the way every text box works:
//! the caret is the end that moves, the anchor is where the selection
//! started, and the two are equal when nothing is selected. A click puts
//! both at the same place, a drag or a shifted key moves the caret and
//! leaves the anchor, and ⌘A puts them at the two ends.

use crate::TextPress;
use std::ops::Range;

/// A text box's state: the text, a caret on a char boundary, the other
/// end of its selection, and whether it is taking keystrokes.
#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    pub text: String,
    /// Byte position of the caret, always on a char boundary -- arrows
    /// move it, typing inserts at it.
    pub cursor: usize,
    /// The still end of the selection, also on a char boundary. Equal to
    /// `cursor` when nothing is selected, which is the usual case.
    pub anchor: usize,
    pub active: bool,
    /// A paragraph rather than a line: Enter breaks it instead of
    /// submitting, and a pasted paragraph keeps its breaks.
    pub multiline: bool,
}

/// What a keystroke did to a [`LineEdit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEditKey {
    /// Not for the box; let it propagate.
    Ignored,
    /// The caret or selection moved; the text is unchanged.
    Moved,
    /// The text changed.
    Changed,
    /// Enter: the box gave the keyboard back.
    Submitted,
}

impl LineEdit {
    /// An empty paragraph box rather than a line.
    pub fn multiline() -> Self {
        LineEdit {
            multiline: true,
            ..Default::default()
        }
    }

    /// A box that already has the keyboard, holding `text` with the
    /// caret at its end -- a rename or a note opening on what it is
    /// about to edit.
    pub fn focused(text: impl Into<String>) -> Self {
        let mut edit = LineEdit {
            text: text.into(),
            ..Default::default()
        };
        edit.focus();
        edit
    }

    /// Focus with the caret at the end and nothing selected -- what a
    /// field gets when the keyboard reaches it by some route other than
    /// a click (a dialog opening, Tab, the gallery's `/`).
    pub fn focus(&mut self) {
        self.active = true;
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// Empty and inactive.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.active = false;
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// The selected range, low end first, always on char boundaries and
    /// inside the text. Empty when there is no selection.
    pub fn selection(&self) -> Range<usize> {
        let a = snap(&self.text, self.cursor);
        let b = snap(&self.text, self.anchor);
        a.min(b)..a.max(b)
    }

    /// Whether anything is selected.
    pub fn has_selection(&self) -> bool {
        !self.selection().is_empty()
    }

    /// The selected text, empty when nothing is.
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Everything, as ⌘A leaves it: the caret at the end, the anchor at
    /// the start.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Drop the caret at a byte offset, losing any selection.
    pub fn place_caret(&mut self, at: usize) {
        self.cursor = snap(&self.text, at);
        self.anchor = self.cursor;
    }

    /// Move the caret to a byte offset and leave the anchor, extending
    /// the selection -- a drag, or a shifted key.
    pub fn extend_to(&mut self, at: usize) {
        self.cursor = snap(&self.text, at);
    }

    /// Select the word (or run of spaces, or single mark) around a byte
    /// offset, as a double-click does.
    pub fn select_word_at(&mut self, at: usize) {
        let word = word_at(&self.text, at);
        self.anchor = word.start;
        self.cursor = word.end;
    }

    /// A press inside the box: the caret lands where it was clicked, and
    /// a shifted, double or triple click selects from there.
    pub fn press(&mut self, press: &TextPress) {
        self.active = true;
        match press.clicks {
            0 | 1 if press.shift => self.extend_to(press.offset),
            0 | 1 => self.place_caret(press.offset),
            2 => self.select_word_at(press.offset),
            _ => self.select_all(),
        }
    }

    /// Move the caret to a byte offset, taking the anchor with it or
    /// leaving it behind: the difference between a key and a shifted
    /// key, and the one thing every caret move here comes down to.
    pub fn move_caret(&mut self, at: usize, extend: bool) {
        if extend {
            self.extend_to(at);
        } else {
            self.place_caret(at);
        }
    }

    /// An arrow key: one character along, or -- with a selection up and
    /// no Shift -- to that side of it, which is where the caret is
    /// left when a selection is arrowed away rather than typed over.
    pub fn arrow(&mut self, forward: bool, extend: bool) {
        if self.has_selection() && !extend {
            let selection = self.selection();
            self.place_caret(if forward {
                selection.end
            } else {
                selection.start
            });
            return;
        }
        let at = if forward {
            caret_right(&self.text, self.cursor).min(self.text.len())
        } else {
            caret_left(&self.text, self.cursor)
        };
        self.move_caret(at, extend);
    }

    /// Cut out whatever is selected, leaving the caret in its place.
    /// Returns whether anything went.
    pub fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = range.start;
        true
    }

    /// Type `text` in, over the selection if there is one.
    pub fn insert(&mut self, text: &str) {
        self.delete_selection();
        let at = snap(&self.text, self.cursor);
        self.text.insert_str(at, text);
        self.cursor = at + text.len();
        self.anchor = self.cursor;
    }

    /// A keystroke while the box is active. `cx` is for the clipboard.
    pub fn key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut gpui::App) -> LineEditKey {
        if !self.active {
            return LineEditKey::Ignored;
        }
        let primary = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
        let shift = ev.keystroke.modifiers.shift;
        // Keep the caret and anchor on the rails whatever changed the
        // text underneath them.
        self.cursor = snap(&self.text, self.cursor);
        self.anchor = snap(&self.text, self.anchor);
        match ev.keystroke.key.as_str() {
            "a" if primary => {
                self.select_all();
                LineEditKey::Moved
            }
            "c" if primary && self.has_selection() => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                    self.selected_text().to_string(),
                ));
                LineEditKey::Moved
            }
            "x" if primary && self.has_selection() => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                    self.selected_text().to_string(),
                ));
                self.delete_selection();
                LineEditKey::Changed
            }
            "v" if primary => {
                let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return LineEditKey::Moved;
                };
                // One line: a pasted paragraph flattens rather than
                // breaking the box. A paragraph box keeps the breaks.
                let multiline = self.multiline;
                let pasted: String = pasted
                    .chars()
                    .map(|c| match c {
                        '\n' if multiline => '\n',
                        c if c.is_control() => ' ',
                        c => c,
                    })
                    .collect();
                self.insert(&pasted);
                LineEditKey::Changed
            }
            // ⌘←/⌘→: the ends of the line.
            "left" if primary => {
                self.move_caret(0, shift);
                LineEditKey::Moved
            }
            "right" if primary => {
                self.move_caret(self.text.len(), shift);
                LineEditKey::Moved
            }
            "left" | "right" => {
                self.arrow(ev.keystroke.key == "right", shift);
                LineEditKey::Moved
            }
            "up" if self.multiline => {
                self.move_caret(caret_up(&self.text, self.cursor), shift);
                LineEditKey::Moved
            }
            "down" if self.multiline => {
                self.move_caret(caret_down(&self.text, self.cursor), shift);
                LineEditKey::Moved
            }
            "home" | "up" => {
                self.move_caret(0, shift);
                LineEditKey::Moved
            }
            "end" | "down" => {
                self.move_caret(self.text.len(), shift);
                LineEditKey::Moved
            }
            "backspace" => {
                if !self.delete_selection() && self.cursor > 0 {
                    let from = caret_left(&self.text, self.cursor);
                    self.text.replace_range(from..self.cursor, "");
                    self.cursor = from;
                    self.anchor = from;
                }
                LineEditKey::Changed
            }
            "delete" => {
                if !self.delete_selection() && self.cursor < self.text.len() {
                    let to = caret_right(&self.text, self.cursor);
                    self.text.replace_range(self.cursor..to, "");
                }
                LineEditKey::Changed
            }
            // A paragraph box breaks the line; a one-line box is done.
            "enter" if self.multiline && !primary => {
                self.insert("\n");
                LineEditKey::Changed
            }
            "enter" => {
                self.active = false;
                self.anchor = self.cursor;
                LineEditKey::Submitted
            }
            _ => {
                // A ⌘ (or Ctrl) chord the box has no use for is a
                // command, not text -- the keystroke still arrives
                // carrying its character, and without this ⌘Z typed a
                // "z" into the box it was meant to undo.
                if primary {
                    return LineEditKey::Ignored;
                }
                let Some(text) = ev.keystroke.key_char.as_deref() else {
                    return LineEditKey::Ignored;
                };
                if text.chars().any(char::is_control) {
                    return LineEditKey::Ignored;
                }
                // Typing over a selection replaces it, as anywhere.
                self.insert(text);
                LineEditKey::Changed
            }
        }
    }
}

/// The nearest char boundary in `s` at or before byte position `at`.
/// Offsets that come from a hit test are already on one; this is for the
/// ones that come from a buffer that changed underneath the caret.
fn snap(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    if s.is_char_boundary(at) {
        at
    } else {
        caret_left(s, at)
    }
}

/// The previous char boundary in `s` before byte position `at` -- what
/// a left arrow moves a field's caret by.
pub fn caret_left(s: &str, at: usize) -> usize {
    s[..at.min(s.len())]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i)
}

/// The next char boundary in `s` after byte position `at`.
pub fn caret_right(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    at + s[at..].chars().next().map_or(0, |c| c.len_utf8())
}

/// The same column on the line above, for an up arrow in a paragraph
/// box. Columns are counted in characters, which is what the caret
/// moves by; the ends of a short line clamp.
pub fn caret_up(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    let line_start = s[..at].rfind('\n').map_or(0, |i| i + 1);
    if line_start == 0 {
        return 0;
    }
    let column = s[line_start..at].chars().count();
    let above = &s[..line_start - 1];
    let above_start = above.rfind('\n').map_or(0, |i| i + 1);
    column_offset(s, above_start, line_start - 1, column)
}

/// The same column on the line below.
pub fn caret_down(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    let line_start = s[..at].rfind('\n').map_or(0, |i| i + 1);
    let Some(break_at) = s[at..].find('\n').map(|i| at + i) else {
        return s.len();
    };
    let column = s[line_start..at].chars().count();
    let below_start = break_at + 1;
    let below_end = s[below_start..]
        .find('\n')
        .map_or(s.len(), |i| below_start + i);
    column_offset(s, below_start, below_end, column)
}

/// The byte offset `column` characters into `s[start..end]`, or its end.
fn column_offset(s: &str, start: usize, end: usize, column: usize) -> usize {
    s[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(i, _)| start + i)
}

/// The run a double-click selects around byte position `at`: a word, a
/// run of spaces, or a single mark of punctuation.
pub fn word_at(s: &str, at: usize) -> Range<usize> {
    let at = snap(s, at);
    // On the far side of the last character there is nothing under the
    // pointer; take the word that ends there, as every editor does.
    let start_of = |at: usize| -> Option<(usize, char)> { s[at..].chars().next().map(|c| (at, c)) };
    let Some((_, here)) = start_of(at).or_else(|| {
        let prev = caret_left(s, at);
        (at > 0).then(|| (prev, s[prev..].chars().next().unwrap()))
    }) else {
        return 0..0;
    };
    let class = Class::of(here);
    if class == Class::Mark {
        // Punctuation goes one mark at a time.
        let start = if at == s.len() { caret_left(s, at) } else { at };
        return start..caret_right(s, start);
    }
    let mut start = if at == s.len() { caret_left(s, at) } else { at };
    while start > 0 {
        let prev = caret_left(s, start);
        if Class::of(s[prev..].chars().next().unwrap()) != class {
            break;
        }
        start = prev;
    }
    let mut end = start;
    while end < s.len() {
        let c = s[end..].chars().next().unwrap();
        if Class::of(c) != class {
            break;
        }
        end += c.len_utf8();
    }
    start..end
}

/// What a character counts as when a double-click grows a selection out
/// from it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Mark,
}

impl Class {
    fn of(c: char) -> Class {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Mark
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{caret_down, caret_left, caret_right, caret_up, word_at, LineEdit};
    use crate::TextPress;

    #[test]
    fn the_caret_moves_by_whole_characters_and_stays_in_bounds() {
        // "aé🙂" -- one, two and four byte characters.
        let s = "a\u{e9}\u{1f642}";
        assert_eq!(caret_right(s, 0), 1);
        assert_eq!(caret_right(s, 1), 3);
        assert_eq!(caret_right(s, 3), 7);
        // At (or past) the end there is nowhere further to go.
        assert_eq!(caret_right(s, 7), 7);
        assert_eq!(caret_right(s, 99).min(s.len()), 7);
        assert_eq!(caret_left(s, 7), 3);
        assert_eq!(caret_left(s, 3), 1);
        assert_eq!(caret_left(s, 1), 0);
        assert_eq!(caret_left(s, 0), 0);
        assert_eq!(caret_left(s, 99), 3);
        assert_eq!(caret_left("", 0), 0);
    }

    #[test]
    fn a_double_click_takes_the_word_under_it() {
        let s = "hello wide world";
        assert_eq!(word_at(s, 0), 0..5);
        assert_eq!(word_at(s, 4), 0..5);
        // The space between two words is its own run.
        assert_eq!(word_at(s, 5), 5..6);
        assert_eq!(word_at(s, 6), 6..10);
        // Past the last character: the word that ends there.
        assert_eq!(word_at(s, s.len()), 11..16);
        // Punctuation goes one mark at a time.
        assert_eq!(word_at("a, b", 1), 1..2);
        assert_eq!(word_at("", 0), 0..0);
    }

    #[test]
    fn a_selection_is_what_typing_and_backspace_replace() {
        let mut edit = LineEdit {
            text: "hello world".into(),
            active: true,
            ..Default::default()
        };
        edit.select_word_at(0);
        assert_eq!(edit.selection(), 0..5);
        assert_eq!(edit.selected_text(), "hello");
        edit.insert("bye");
        assert_eq!(edit.text, "bye world");
        assert_eq!(edit.cursor, 3);
        assert!(!edit.has_selection());
        // An anchor left behind by a drag selects both ways.
        edit.anchor = 9;
        edit.cursor = 4;
        assert_eq!(edit.selected_text(), "world");
        assert!(edit.delete_selection());
        assert_eq!(edit.text, "bye ");
        assert_eq!(edit.cursor, 4);
    }

    #[test]
    fn a_press_drops_a_caret_and_a_second_or_third_selects() {
        let mut edit = LineEdit::focused("hello wide world");
        let press = |offset, shift, clicks| TextPress {
            offset,
            shift,
            clicks,
        };
        edit.press(&press(6, false, 1));
        assert_eq!(edit.cursor, 6);
        assert!(!edit.has_selection());
        // Shift-clicking elsewhere keeps the anchor, so the two make a
        // range; a drag to the same place would do likewise.
        edit.press(&press(10, true, 1));
        assert_eq!(edit.selection(), 6..10);
        edit.extend_to(16);
        assert_eq!(edit.selection(), 6..16);
        // Double for the word under it, triple for the line.
        edit.press(&press(13, false, 2));
        assert_eq!(edit.selected_text(), "world");
        edit.press(&press(13, false, 3));
        assert_eq!(edit.selection(), 0..16);
        // An unshifted arrow with a selection up lands on that side of
        // it rather than moving a character.
        edit.arrow(false, false);
        assert_eq!(edit.cursor, 0);
        assert!(!edit.has_selection());
        // And a shifted one takes the anchor along.
        edit.arrow(true, true);
        edit.arrow(true, true);
        assert_eq!(edit.selection(), 0..2);
    }

    #[test]
    fn a_paragraph_caret_keeps_its_column_between_lines() {
        let s = "alpha\nbe\ngamma";
        // Down from column 4 of "alpha" clamps to the end of "be".
        assert_eq!(caret_down(s, 4), 8);
        // And on down to that same column of "gamma", which has one.
        assert_eq!(caret_down(s, 8), 11);
        assert_eq!(caret_up(s, 11), 8);
        assert_eq!(caret_up(s, 8), 2);
        // The first line has nothing above it; the last nothing below.
        assert_eq!(caret_up(s, 3), 0);
        assert_eq!(caret_down(s, 12), s.len());
    }
}
