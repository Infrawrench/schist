//! The keyboard model behind a one-line text box.
//!
//! GPUI ships no text editor, and a full one needs an IME handler, so
//! Schist's fields are plain state plus a keystroke function: the view
//! that owns a [`LineEdit`] hands it each [`gpui::KeyDownEvent`] while
//! the box is active, and draws it with [`crate::TextInput`]. Every box
//! built this way types, selects, pastes and moves its caret the same
//! way, which is the point.

/// A one-line text box's state: the text, a caret on a char boundary,
/// a whole-line selection, and whether it is taking keystrokes.
#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    pub text: String,
    /// Byte position, always on a char boundary -- arrows move it,
    /// typing inserts at it.
    pub cursor: usize,
    /// ⌘A selected the whole line: the next keystroke replaces it,
    /// backspace clears it, ⌘C/⌘X take it -- the minimal selection a
    /// one-line box owes the keyboard.
    pub selected: bool,
    pub active: bool,
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
    /// A click lands a caret at the end, not a selection.
    pub fn focus(&mut self) {
        self.active = true;
        self.selected = false;
        self.cursor = self.text.len();
    }

    /// Empty and inactive.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selected = false;
        self.active = false;
    }

    pub fn set_text(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
        self.selected = false;
    }

    fn replace_selection(&mut self) {
        if self.selected {
            self.text.clear();
            self.cursor = 0;
            self.selected = false;
        }
    }

    /// A keystroke while the box is active. `cx` is for the clipboard.
    pub fn key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut gpui::App) -> LineEditKey {
        if !self.active {
            return LineEditKey::Ignored;
        }
        let primary = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
        // Keep the caret on the rails whatever changed the text.
        self.cursor = self.cursor.min(self.text.len());
        match ev.keystroke.key.as_str() {
            "a" if primary => {
                self.selected = !self.text.is_empty();
                self.cursor = self.text.len();
                LineEditKey::Moved
            }
            "c" if primary && self.selected => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.text.clone()));
                LineEditKey::Moved
            }
            "x" if primary && self.selected => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.text.clone()));
                self.replace_selection();
                LineEditKey::Changed
            }
            "v" if primary => {
                let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return LineEditKey::Moved;
                };
                // One line: a pasted paragraph flattens rather than
                // breaking the box.
                let pasted: String = pasted
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                self.replace_selection();
                let at = self.cursor;
                self.text.insert_str(at, &pasted);
                self.cursor = at + pasted.len();
                LineEditKey::Changed
            }
            "left" | "right" if primary => {
                // ⌘←/⌘→: the ends of the line.
                self.selected = false;
                self.cursor = if ev.keystroke.key == "left" {
                    0
                } else {
                    self.text.len()
                };
                LineEditKey::Moved
            }
            "left" => {
                self.cursor = if self.selected {
                    0
                } else {
                    caret_left(&self.text, self.cursor)
                };
                self.selected = false;
                LineEditKey::Moved
            }
            "right" => {
                self.cursor = if self.selected {
                    self.text.len()
                } else {
                    caret_right(&self.text, self.cursor).min(self.text.len())
                };
                self.selected = false;
                LineEditKey::Moved
            }
            "home" | "up" => {
                self.cursor = 0;
                self.selected = false;
                LineEditKey::Moved
            }
            "end" | "down" => {
                self.cursor = self.text.len();
                self.selected = false;
                LineEditKey::Moved
            }
            "backspace" => {
                if self.selected {
                    self.replace_selection();
                } else if self.cursor > 0 {
                    let from = caret_left(&self.text, self.cursor);
                    self.text.replace_range(from..self.cursor, "");
                    self.cursor = from;
                }
                LineEditKey::Changed
            }
            "delete" => {
                if self.selected {
                    self.replace_selection();
                } else if self.cursor < self.text.len() {
                    let to = caret_right(&self.text, self.cursor);
                    self.text.replace_range(self.cursor..to, "");
                }
                LineEditKey::Changed
            }
            "enter" => {
                self.active = false;
                self.selected = false;
                LineEditKey::Submitted
            }
            _ => {
                let Some(text) = ev.keystroke.key_char.as_deref() else {
                    return LineEditKey::Ignored;
                };
                if text.chars().any(char::is_control) {
                    return LineEditKey::Ignored;
                }
                // Typing over a selection replaces it, as anywhere.
                self.replace_selection();
                let at = self.cursor;
                self.text.insert_str(at, text);
                self.cursor = at + text.len();
                LineEditKey::Changed
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{caret_left, caret_right};

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
}
