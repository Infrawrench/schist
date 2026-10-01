//! IME drafts stay outside the document until the composition commits.
use super::{DesignState, Typing};
use schist_layout::{authoring, Story};
use std::ops::Range;

pub struct Composition {
    pub base: Story,
    pub range: Range<usize>,
    pub original: Typing,
}

pub fn byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return at;
        }
        units += ch.len_utf16();
    }
    text.len()
}
fn range(text: &str, r: Range<usize>) -> Range<usize> {
    byte(text, r.start.min(r.end))..byte(text, r.end)
}

pub fn replace(
    state: &mut DesignState,
    native: Option<Range<usize>>,
    text: &str,
    selected: Option<Range<usize>>,
    marked: bool,
) -> bool {
    let Some(typing) = state.typing else {
        return false;
    };
    let Some(base) = state.document.story(typing.story) else {
        return false;
    };
    if state
        .composition
        .as_ref()
        .is_some_and(|draft| draft.base != *base)
    {
        cancel(state);
        return false;
    }
    let selection = native
        .map(|r| range(&state.text_buffer, r))
        .or_else(|| state.composition.as_ref().map(|c| c.range.clone()))
        .unwrap_or(typing.at.min(typing.anchor)..typing.at.max(typing.anchor));
    let mut edit = schist_ui::LineEdit {
        text: state.text_buffer.clone(),
        anchor: selection.start,
        cursor: selection.end,
        ..Default::default()
    };
    edit.insert(text);
    if marked {
        if state.composition.is_none() {
            state.composition = Some(Composition {
                base: base.clone(),
                range: selection.clone(),
                original: typing,
            });
        }
        state.composition.as_mut().unwrap().range = selection.start..selection.start + text.len();
        if let Some(r) = selected {
            let r = range(text, r);
            edit.anchor = selection.start + r.start;
            edit.cursor = selection.start + r.end;
        }
        state.text_buffer = edit.text;
        state.typing = Some(Typing {
            at: edit.cursor,
            anchor: edit.anchor,
            ..typing
        });
        true
    } else {
        let unchanged = edit.text == base.text();
        let accepted = unchanged
            || authoring::set_text(
                &mut state.document,
                &mut state.history,
                typing.story,
                &edit.text,
            );
        if !accepted {
            cancel(state);
            return false;
        }
        state.composition = None;
        state.text_buffer = edit.text;
        state.typing = Some(Typing {
            at: edit.cursor,
            anchor: edit.anchor,
            ..typing
        });
        true
    }
}

pub fn commit(state: &mut DesignState) -> bool {
    let Some(draft) = state.composition.as_ref() else {
        return false;
    };
    let text = state.text_buffer[draft.range.clone()].to_owned();
    replace(state, None, &text, None, false)
}

pub fn cancel(state: &mut DesignState) {
    if let Some(draft) = state.composition.take() {
        state.text_buffer = authoring::text_of(&state.document, draft.original.story);
        state.typing = Some(draft.original);
        super::tools::select_to(state, draft.original.at, false);
    }
}

pub fn preview(state: &DesignState) -> Option<schist_layout::LayoutDocument> {
    let draft = state.composition.as_ref()?;
    let story = draft.original.story;
    if state.document.story(story) != Some(&draft.base) {
        return None;
    }
    let mut document = state.document.clone();
    authoring::set_text(
        &mut document,
        &mut Default::default(),
        story,
        &state.text_buffer,
    );
    Some(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_updates_are_a_draft_and_commit_is_one_unicode_edit() {
        for steps in [1, 2, 10] {
            let mut state = DesignState::new();
            let frame = authoring::text_frame(
                &mut state.document,
                &mut state.history,
                0,
                schist_layout::Rect::new(0.0, 0.0, 100.0, 40.0),
            )
            .unwrap();
            authoring::set_text(&mut state.document, &mut state.history, frame.story, "a😀z");
            super::super::tools::begin_typing(&mut state, frame.object, 1);
            let before = state.document.clone();
            let depth = state.history.undo_depth();
            assert!(replace(&mut state, Some(1..3), "候", Some(0..1), true));
            for _ in 0..steps {
                assert!(replace(&mut state, None, "候補", Some(2..2), true));
            }
            assert_eq!(state.document, before);
            assert_eq!(state.history.undo_depth(), depth);
            assert_eq!(state.text_buffer, "a候補z");
            assert!(replace(&mut state, None, "確定", None, false));
            assert_eq!(state.text_buffer, "a確定z");
            assert_eq!(state.history.undo_depth(), depth + 1);
            state.history.undo(&mut state.document);
            assert_eq!(state.document, before);
            super::super::tools::begin_typing(&mut state, frame.object, 1);
            replace(&mut state, None, "取消", None, true);
            cancel(&mut state);
            assert_eq!(state.document, before);
            assert_eq!(state.text_buffer, "a😀z");
        }
    }
}
