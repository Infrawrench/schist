//! The Styles panel: the document's paragraph and character styles.
//!
//! This lists the *document's* styles, not the ones a user has defined for
//! themselves in a preferences file. A layout document carries its styles
//! with it, and that is the point: opening someone else's file gives you the
//! styles their text is written in, which is the only way the text can
//! look the way they meant it to.
//!
//! The two kinds are kept apart because they are applied differently, and
//! conflating them is a real category error rather than a small one. A
//! paragraph style applies to whole frames. A character style applies to a
//! range of text inside one. Clicking a paragraph style with three frames
//! selected restyles three frames; clicking a character style with a caret
//! in one of them styles the word under the caret.

use gpui::{div, px, rgb, Context, IntoElement};

use super::*;

/// The styles panel.
///
/// `None` when there is no document, or when it has no styles at all. A
/// document always has at least a `Default`, so an empty list means there
/// is genuinely no document to describe.
pub(super) fn styles_panel(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    if !ws.design_mode() {
        return None;
    }
    let styles = ws.design.document.styles.clone();
    if styles.paragraphs.is_empty() && styles.characters.is_empty() && styles.objects.is_empty() {
        return None;
    }
    // What the selection already uses, so the applied style is visible
    // rather than something the user has to remember.
    let (paragraph_in_use, character_in_use) = styles_in_use(ws);
    Some(
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.0))
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(rgb(palette().panel_edge))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(t("design.paragraph_styles")),
            )
            .child(div().flex().flex_col().gap_1().children(paragraph_rows(
                &styles.paragraphs,
                paragraph_in_use.as_deref(),
                cx,
            )))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette().text_dim))
                    .child(t("design.character_styles")),
            )
            .child(div().flex().flex_col().gap_1().children(character_rows(
                &styles.characters,
                character_in_use.as_deref(),
                cx,
            )))
            .child(super::object_styles::style_controls(ws, cx))
            .into_any_element(),
    )
}

/// One row per paragraph style.
fn paragraph_rows(
    styles: &[schist_layout::ParagraphStyle],
    in_use: Option<&str>,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    styles
        .iter()
        .enumerate()
        .map(|(index, style)| {
            style_row(
                style.name.clone(),
                style.point_size,
                in_use == Some(style.name.as_str()),
                index,
                cx,
                |ws, name, cx| {
                    let selection = ws.design.selection.clone();
                    if schist_layout::authoring::set_paragraph_style(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        &selection,
                        name,
                    ) {
                        cx.notify();
                        return true;
                    }
                    false
                },
            )
        })
        .collect()
}

/// One row per character style.
fn character_rows(
    styles: &[schist_layout::CharacterStyle],
    in_use: Option<&str>,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    styles
        .iter()
        .enumerate()
        .map(|(index, style)| {
            style_row(
                style.name.clone(),
                style.point_size,
                in_use == Some(style.name.as_str()),
                index,
                cx,
                |ws, name, cx| {
                    // A character style needs a range, and the only range a
                    // user has expressed is where the caret is: the word
                    // around it. Applying one to a whole frame would be the
                    // paragraph style's job done badly.
                    let Some(typing) = ws.design.typing else {
                        return false;
                    };
                    let story = typing.story;
                    let text = schist_layout::authoring::text_of(&ws.design.document, story);
                    let (start, end) = if typing.at != typing.anchor {
                        (typing.at.min(typing.anchor), typing.at.max(typing.anchor))
                    } else {
                        word_around(&text, typing.at)
                    };
                    if schist_layout::authoring::set_character_style(
                        &mut ws.design.document,
                        &mut ws.design.history,
                        story,
                        start..end,
                        name,
                    ) {
                        cx.notify();
                        return true;
                    }
                    false
                },
            )
        })
        .collect()
}

/// The styles the current selection already uses.
///
/// Only when the whole selection agrees: a selection of three frames where
/// two are Body and one is Default has no single answer, and picking one
/// would be a lie.
fn styles_in_use(ws: &Workspace) -> (Option<String>, Option<String>) {
    let document = &ws.design.document;
    let mut paragraphs: Option<Option<String>> = None;
    let mut characters: Option<Option<String>> = None;
    for id in &ws.design.selection {
        let Some(placed) = document.object(*id) else {
            continue;
        };
        let schist_layout::LayoutObject::TextFrame { story, .. } = &placed.object else {
            continue;
        };
        let Some(story) = document.story(*story) else {
            continue;
        };
        // The first paragraph's style, and the first range's: both are
        // "what this frame starts as", which is what a panel can show
        // without summarising a whole frame's formatting.
        let paragraph = story.points.iter().find_map(|point| match point {
            schist_layout::StoryPoint::Paragraph { style, .. } => Some(style.clone()),
            _ => None,
        });
        let character = story.ranges.first().map(|range| range.style.clone());
        paragraphs = Some(match paragraphs {
            None => paragraph,
            Some(seen) if seen == paragraph => seen,
            // Two different answers: there is no one answer.
            Some(_) => return (None, None),
        });
        characters = Some(match characters {
            None => character,
            Some(seen) if seen == character => seen,
            Some(_) => return (None, None),
        });
    }
    (paragraphs.flatten(), characters.flatten())
}

/// A row for a style, with its size on the right.
fn style_row(
    name: String,
    size: Option<f32>,
    in_use: bool,
    index: usize,
    cx: &mut Context<Workspace>,
    apply: impl Fn(&mut Workspace, &str, &mut Context<Workspace>) -> bool + 'static,
) -> gpui::AnyElement {
    let mut row = div()
        // Keyed by position, because a paragraph style and a character
        // style can share a name and two rows with one id is a row that
        // stops responding to clicks.
        .id(("style-row", index))
        .flex()
        .justify_between()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .child(
            div()
                .flex_1()
                .text_sm()
                .overflow_hidden()
                .text_ellipsis()
                .child(name.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(palette().text_dim))
                .child(match size {
                    Some(points) => format!("{points:.0} pt"),
                    None => t("design.inherited").to_string(),
                }),
        );
    if in_use {
        row = row.bg(rgb(palette().selection_bg));
    }
    row.hover(move |s| {
        if in_use {
            s
        } else {
            s.bg(rgb(palette().hover))
        }
    })
    .on_click(cx.listener(move |ws, _ev, _window, cx| {
        apply(ws, &name, cx);
    }))
    .into_any_element()
}

/// The word around a byte offset, as a byte range.
///
/// Whitespace is the delimiter, which is what makes this usable for a
/// one-line label and wrong for a language that does not put spaces between
/// words. It is used for *applying* a style the user just chose, so a
/// slightly wrong selection is recoverable by clicking again, and a
/// full word-segmentation pass would be a dependency for that.
pub(super) fn word_around(text: &str, at: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let is_space = |index: usize| bytes.get(index).is_some_and(u8::is_ascii_whitespace);

    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }

    // On whitespace, the word *after* it is the one meant: a caret in the
    // gap between two words is more likely in the next one, since that is
    // the one being typed into.
    //
    // At the end of the text there is no word after -- and that is exactly
    // where the caret sits after typing one -- so the word *before* is taken
    // instead. Without that fallback a click at the end of a frame would
    // style nothing, and look broken.
    let after = skip_spaces_forward(text, &is_space, at);
    at = match after {
        Some(ahead) => ahead,
        None => skip_spaces_backward(&is_space, at),
    };

    let mut start = at;
    while start > 0 && !is_space(start - 1) {
        start -= 1;
    }
    let mut end = at;
    while end < text.len() && !is_space(end) {
        end += 1;
    }
    (start, end)
}

/// The first word byte at or after `from`, or `None` if the text runs out.
fn skip_spaces_forward(
    text: &str,
    is_space: &impl Fn(usize) -> bool,
    from: usize,
) -> Option<usize> {
    let mut at = from;
    while at < text.len() {
        while at < text.len() && is_space(at) {
            at += 1;
            while at < text.len() && !text.is_char_boundary(at) {
                at += 1;
            }
        }
        if at < text.len() {
            return Some(at);
        }
    }
    None
}

/// The last word byte before the caret.
///
/// The caret at `from` sits *between* the bytes `from - 1` and `from`, so
/// the whitespace to skip is the one *before* it. A caret one past the end
/// of the text has no byte at `from` at all, and reading it as "not
/// whitespace" would stop the walk on a byte that is not there.
fn skip_spaces_backward(is_space: &impl Fn(usize) -> bool, from: usize) -> usize {
    let mut at = from;
    if at > 0 && is_space(at - 1) {
        at -= 1;
        while at > 0 && is_space(at - 1) {
            at -= 1;
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_the_run_of_non_whitespace_around_the_caret() {
        assert_eq!(word_around("hello world", 2), (0, 5));
        assert_eq!(word_around("hello world", 7), (6, 11));
    }

    #[test]
    fn a_caret_in_the_gap_takes_the_word_after_it() {
        // A caret between two words is more likely to mean the one being
        // typed into than the one just left.
        assert_eq!(word_around("hello world", 5), (6, 11));
        assert_eq!(word_around("hello world", 0), (0, 5));
    }

    #[test]
    fn a_word_at_the_very_end_is_still_found() {
        // A caret after the last word is where it sits after typing one,
        // and there is no word after it, so the one before is taken.
        assert_eq!(word_around("hello ", 6), (0, 5));
        assert_eq!(word_around("hello", 5), (0, 5));
        assert_eq!(word_around("hello world", 11), (6, 11));
    }

    #[test]
    fn whitespace_only_text_has_no_word() {
        // An empty range is refused by the style operation, so nothing is
        // recorded and clicking a style on a blank frame does nothing.
        // The position it collapses to is not important, only that it is
        // empty.
        let (start, end) = word_around("   ", 1);
        assert_eq!(start, end, "no word in whitespace");
    }

    #[test]
    fn a_caret_inside_a_multibyte_character_lands_on_its_boundary() {
        // "héllo": the é is bytes 1..3, so an offset of 2 is inside it.
        let (start, end) = word_around("héllo", 2);
        assert!((start..=end).contains(&0));
        assert_eq!(
            &"héllo"[start..end],
            "héllo",
            "the whole word, not half a character"
        );
    }
}
