//! The software keyboard on iOS and iPadOS.
//!
//! Schist's text entry is its own: the dialog fields, the gallery's
//! search boxes, a layer being renamed, a note, the type tool, all take
//! `KeyDown` events and keep their own buffer and caret. gpui raises the
//! system keyboard only while a window has an input handler registered
//! for the focused element, which those never do, so on iOS nothing
//! showed for a tap in a field. This registers one whenever something is
//! taking typing. What the keyboard types arrives through the handler and
//! is replayed as the key events a hardware keyboard would have sent, so
//! every field behaves the same from either; Return and Backspace already
//! come as keys. The handler reports the focused text and caret so the
//! keyboard's autocorrect and predictions can read the word being typed;
//! an autocorrect replacement of the word at the caret is backspaced and
//! retyped. Inline composition (marked text) is not shown: a Japanese or
//! Chinese keyboard's preview stays in the keyboard until it commits.

use super::*;
use gpui::{Bounds, ElementInputHandler, EntityInputHandler, Keystroke, Modifiers, UTF16Selection};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set while a replayed key is being dispatched: a key no field takes
/// falls back to the input handler (that is how gpui types unhandled
/// characters into a field), which must not replay it again.
static REPLAYING: AtomicBool = AtomicBool::new(false);

impl Workspace {
    /// Whether something is taking typing right now: what should have
    /// the keyboard up.
    fn wants_text_input(&mut self) -> bool {
        self.focused_field.is_some()
            || self.gallery_typing()
            || self.layer_rename.is_some()
            || self.note_edit.is_some()
            || self.tool_captures_keys()
    }

    /// A zero-sized element that, while something is taking typing,
    /// registers the workspace as the window's input handler in its
    /// paint, which is where gpui accepts one.
    pub(super) fn text_input_bridge(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.wants_text_input() {
            return None;
        }
        let focus = self.focus.clone();
        let entity = cx.entity();
        Some(
            gpui::canvas(
                |_, _, _| (),
                move |bounds, (), window, cx| {
                    window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                },
            )
            .absolute()
            .size_0()
            .into_any_element(),
        )
    }

    /// The focused text and its caret (a byte offset), for the sources
    /// that keep a plain buffer; the type tool's text stays with the
    /// tool.
    fn focused_text(&self) -> Option<(&str, Range<usize>)> {
        if self.focused_field.is_some() {
            let caret = self.field_cursor.min(self.field_buffer.len());
            return Some((&self.field_buffer, caret..caret));
        }
        if self.gallery_open() {
            let edit = if self.cloud.show {
                &self.cloud.search
            } else {
                &self.library.search
            };
            if edit.active {
                let range = if edit.selected {
                    0..edit.text.len()
                } else {
                    let caret = edit.cursor.min(edit.text.len());
                    caret..caret
                };
                return Some((&edit.text, range));
            }
        }
        if let Some((_, name)) = &self.layer_rename {
            return Some((name, name.len()..name.len()));
        }
        if let Some((_, text)) = &self.note_edit {
            return Some((text, text.len()..text.len()));
        }
        None
    }

    /// Replays `text` as the key events a keyboard would have sent.
    fn type_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        for ch in text.chars() {
            let key = match ch {
                ' ' => "space".to_string(),
                '\n' | '\r' => "enter".to_string(),
                '\t' => "tab".to_string(),
                c => c.to_lowercase().to_string(),
            };
            let key_char = (!ch.is_control()).then(|| ch.to_string());
            self.send_key(key, key_char, window, cx);
        }
    }

    fn send_key(
        &mut self,
        key: String,
        key_char: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = Keystroke {
            modifiers: Modifiers::default(),
            key,
            key_char,
        };
        // The keystroke goes through the window's dispatch, which lands
        // in the same listeners a hardware key reaches. Deferred, so the
        // workspace is not borrowed while they run.
        window.defer(cx, move |window, cx| {
            REPLAYING.store(true, Ordering::SeqCst);
            window.dispatch_keystroke(keystroke, cx);
            REPLAYING.store(false, Ordering::SeqCst);
        });
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The byte offset of a UTF-16 offset into `s`, clamped to the text.
fn byte_offset(s: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, ch) in s.char_indices() {
        if units >= utf16 {
            return at;
        }
        units += ch.len_utf16();
    }
    s.len()
}

impl EntityInputHandler for Workspace {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, _) = self.focused_text()?;
        let start = byte_offset(text, range.start);
        let end = byte_offset(text, range.end.max(range.start));
        let slice = &text[start..end];
        *adjusted_range = Some(utf16_len(&text[..start])..utf16_len(&text[..end]));
        Some(slice.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (text, range) = self.focused_text()?;
        Some(UTF16Selection {
            range: utf16_len(&text[..range.start])..utf16_len(&text[..range.end]),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if REPLAYING.load(Ordering::SeqCst) {
            return;
        }
        // A range other than the caret is autocorrect rewriting the word
        // just typed, which ends at the caret: take it back first.
        let taken = match (range, self.focused_text()) {
            (Some(range), Some((current, caret))) => {
                let caret16 = utf16_len(&current[..caret.end]);
                if range.end == caret16 && range.start < range.end {
                    let start = byte_offset(current, range.start);
                    current[start..caret.end].chars().count()
                } else {
                    0
                }
            }
            _ => 0,
        };
        for _ in 0..taken {
            self.send_key("backspace".to_string(), None, window, cx);
        }
        self.type_text(text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        _new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Composition previews are not drawn; the committed text arrives
        // through `replace_text_in_range`.
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
