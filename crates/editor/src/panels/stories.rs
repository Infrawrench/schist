//! The Stories panel: every story in the document, whatever it is attached
//! to.
//!
//! This exists because of overflow. A page layout editor's stories are not
//! one-per-frame: text that overflows a frame threads to the next, and text
//! on a master page belongs to no frame the user can click on the page at
//! all. Once a document has any of that, the pasteboard is no longer a
//! complete view of its text, and a user who cannot find their words has
//! nowhere to look.
//!
//! So this lists the document's stories rather than the page's frames. It
//! is the one panel that can show text nobody can currently see, which is
//! exactly why it is not a page list with a second column.

use gpui::{div, px, rgb, Context, IntoElement};

use schist_layout::StoryId;

use super::*;

/// How much of a story's text to show.
///
/// A story can be a book's worth of text and the panel is a few hundred
/// pixels tall, so the tail is what is shown: a user looking for the end of
/// a threaded story is looking for the end. An empty story shows a marker
/// instead of nothing at all, because a blank row and a row whose text is
/// white are indistinguishable.
const PREVIEW_CHARS: usize = 160;

/// The stories panel.
///
/// `None` when there is no document to list, or when it has no stories —
/// a document with no text frames has nothing here, and an empty dock
/// section is worse than no dock section.
pub(super) fn stories_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let document = &ws.design.document;
    if document.stories.is_empty() {
        return None;
    }
    let editing = ws.design.typing.map(|typing| typing.story);
    let rows = story_rows(ws, editing, cx);
    Some(
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.0))
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(rgb(palette().panel_edge))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(schist_i18n::tn!("design.story_count", document.stories.len() as u64))
                    .child(if editing.is_some() {
                        t("design.editing_story").to_string()
                    } else {
                        String::new()
                    }),
            )
            .child(Button::new("story-editor-open",t("design.story_editor"))
                .on_click(cx.listener(|ws,_,_,cx| {
                    let story=ws.design.typing.map(|t|t.story).or_else(||ws.design.selection.first().and_then(|id|schist_layout::threading::story_of(&ws.design.document,*id))).unwrap_or(StoryId(0));
                    ws.open_story_editor(story,cx);
                })))
            .child(Button::new("thread-start",t("design.thread_to_frame"))
                .disabled(ws.design.selection.first().and_then(|id|schist_layout::threading::story_of(&ws.design.document,*id)).is_none())
                .on_click(cx.listener(|ws,_,_,cx| {
                    ws.design.thread_source=ws.design.selection.first().copied();
                    ws.design.typing=None; ws.status=t("design.choose_empty_frame").into();cx.notify();
                })))
            .child(Button::new("thread-detach",t("design.detach_frame"))
                .disabled(ws.design.selection.first().and_then(|id|schist_layout::threading::story_of(&ws.design.document,*id)).is_none())
                .on_click(cx.listener(|ws,_,_,cx| {
                    if let Some(id)=ws.design.selection.first().copied() {schist_layout::threading::detach(&mut ws.design.document,&mut ws.design.history,id);}
                    ws.design.typing=None;cx.notify();
                })))
            .child(div().flex().flex_col().gap_1().children(rows))
            .into_any_element(),
    )
}

/// One row per story, with its text.
fn story_rows(ws: &Workspace, editing: Option<StoryId>, cx: &mut Context<Workspace>) -> Vec<gpui::AnyElement> {
    let document = &ws.design.document;
    let frames: Vec<StoryId> = document
        .objects
        .iter()
        .filter_map(|placed| match &placed.object {
            schist_layout::LayoutObject::TextFrame { story, .. } => Some(*story),
            _ => None,
        })
        .collect();
    document
        .stories
        .iter()
        .enumerate()
        .map(|(index, _story)| {
            let id = StoryId(index as u32);
            // Unplaced stories and overset attached stories are distinct:
            // both remain accessible in the Story Editor.
            let attached = frames.contains(&id);
            let overset=schist_layout::compose::compose_story(document,id).has_overflow();
            let text = schist_layout::authoring::text_of(document, id);
            let preview = preview_of(&text);
            let selected = editing == Some(id);
            let mut row = div()
                .id(("story-row", index))
                .flex()
                .flex_col()
                .p_1()
                .rounded_sm()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_xs()
                        .text_color(rgb(palette().text_dim))
                        .child(schist_i18n::tn!("design.story_number", (index + 1) as u64))
                        .child(if overset {t("design.overflowed").to_string()}
                            else if attached {t("design.on_a_frame").to_string()}
                            else {t("design.unplaced_story").to_string()}),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(palette().text))
                        .child(preview),
                );
            if selected {
                row = row.bg(rgb(palette().selection_bg));
            }
            row.hover(move |s| {
                if selected {
                    s
                } else {
                    s.bg(rgb(palette().hover))
                }
            })
            .on_click(cx.listener(move |ws, _ev, _window, cx| {
                ws.edit_story(id, cx);
            }))
            .into_any_element()
        })
        .collect()
}

/// The end of a story's text, on one line.
fn preview_of(text: &str) -> String {
    let flattened: String = text
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if flattened.trim().is_empty() {
        return t("design.empty_story").to_string();
    }
    let chars: Vec<char> = flattened.chars().collect();
    if chars.len() <= PREVIEW_CHARS {
        return flattened;
    }
    let mut out: String = chars[chars.len() - PREVIEW_CHARS..].iter().collect();
    out.insert(0, '\u{2026}');
    out
}

impl Workspace {
    /// Bring the first threaded frame into view for canvas editing.
    /// Unplaced stories open directly in the Story Editor.
    pub fn edit_story(&mut self, story: StoryId, cx: &mut Context<Self>) {
        let frame = self.design.document.story_frames(story).first().map(|placed| (placed.id,placed.page));
        if let Some((object, page)) = frame {
            self.design.selection = vec![object];
            crate::design::tools::begin_typing(&mut self.design,object,0);
            if self.design.current_page() != page {
                self.design.page = Some(page);
                self.design.view.page = Some(page);
                self.refit_design = true;
            }
        } else {
            self.open_story_editor(story,cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_story_says_so_rather_than_looking_blank() {
        // A blank row and a row whose text is white are the same to a
        // reader, so an empty story is labelled.
        assert_eq!(preview_of(""), t("design.empty_story"));
        assert_eq!(preview_of("   \n  \n"), t("design.empty_story"));
    }

    #[test]
    fn a_story_is_previewed_on_one_line() {
        assert_eq!(preview_of("one\ntwo"), "one two");
    }

    #[test]
    fn a_long_story_is_previewed_from_the_end() {
        // The end is what a user threading text is looking for.
        let long = "x".repeat(PREVIEW_CHARS + 50);
        let preview = preview_of(&long);
        assert!(preview.starts_with('\u{2026}'), "marked as cut off");
        assert_eq!(preview.chars().count(), PREVIEW_CHARS + 1);
        assert!(preview.ends_with(&"x".repeat(4)));
    }

    #[test]
    fn a_short_story_is_not_cut_off() {
        assert_eq!(preview_of("short"), "short");
    }

    #[test]
    fn multibyte_text_is_previewed_by_character_and_not_cut_in_half() {
        // Slicing by bytes here would end the preview on a byte boundary
        // that is not a character, and the panel would fail to render.
        let long = "é".repeat(PREVIEW_CHARS + 10);
        let preview = preview_of(&long);
        assert_eq!(preview.chars().count(), PREVIEW_CHARS + 1);
        assert!(preview.is_char_boundary(preview.len()));
    }
}
