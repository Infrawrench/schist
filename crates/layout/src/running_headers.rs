//! Running headers: MatchParagraphStyleType and MatchCharacterStyleType text
//! variables show the first or last text in a style on their page, and carry
//! the previous page's value forward when their page has none. The rules and
//! their evidence (InDesign's PDF of the public paged-media `variables`
//! sample) are in `docs/idml-format.md`.
use crate::text_variables::{ChangeCase, MatchStyle, RunningHeader, VariableKind};
use crate::{LayoutDocument, Story, StoryId};
use std::collections::BTreeMap;

/// The header's text on `page`, before its literal text before and after.
pub(crate) fn value(doc: &LayoutDocument, header: &RunningHeader, page: usize) -> String {
    let found = matches(doc, &header.style);
    let mut current = String::new();
    for page in 0..=page {
        let on_page = found.get(&page);
        let pick = if header.last {
            on_page.and_then(|texts| texts.last())
        } else {
            on_page.and_then(|texts| texts.first())
        };
        if let Some(text) = pick {
            current.clone_from(text);
        }
    }
    let text = if header.delete_end_punctuation {
        current.trim_end_matches(END_PUNCTUATION).to_owned()
    } else {
        current
    };
    change_case(&text, header.case)
}

/// Sentence punctuation DeleteEndPunctuation removes. InDesign's PDF of the
/// public sample drops a final full stop and exclamation mark but keeps a
/// closing parenthesis.
const END_PUNCTUATION: &[char] = &[
    '.', ',', ';', ':', '!', '?', '…', '。', '、', '，', '；', '：', '！', '？',
];

/// ChangeCase as the public sample's PDF shows it: title case capitalizes the
/// first character of each space-separated word when it is a letter and
/// lowers the rest; sentence case capitalizes the first character and lowers
/// the rest.
pub(crate) fn change_case(text: &str, case: ChangeCase) -> String {
    let capitalized = |word: &str| {
        let mut chars = word.chars();
        chars.next().map_or_else(String::new, |first| {
            first
                .to_uppercase()
                .chain(chars.flat_map(char::to_lowercase))
                .collect()
        })
    };
    match case {
        ChangeCase::None => text.to_owned(),
        ChangeCase::Upper => text.to_uppercase(),
        ChangeCase::Lower => text.to_lowercase(),
        ChangeCase::Sentence => capitalized(text),
        ChangeCase::Title => text
            .split_inclusive(char::is_whitespace)
            .map(capitalized)
            .collect(),
    }
}

/// Whether `story` shows a running header. Such stories are not searched, so
/// evaluating a header never composes a story that needs one.
pub(crate) fn shows_headers(doc: &LayoutDocument, story: &Story) -> bool {
    story.structures.iter().any(|structure| {
        let Some(crate::story::InlineControl::TextVariable { variable, .. }) = &structure.control
        else {
            return false;
        };
        doc.text_variables
            .iter()
            .filter(|d| d.id == *variable)
            .any(|d| matches!(d.kind(), Some(VariableKind::RunningHeader(_))))
    })
}

/// Where text sits on its page: frame top and left, frame and line index.
type Order = (f32, f32, usize, usize);

/// Text in `style` by page, in page order: frames top to bottom then left to
/// right, lines in order. A paragraph counts on the page of its first line, a
/// run of the character style on the page where it starts.
fn matches(doc: &LayoutDocument, style: &MatchStyle) -> BTreeMap<usize, Vec<String>> {
    let mut found: BTreeMap<usize, Vec<(Order, String)>> = BTreeMap::new();
    for (index, story) in doc.stories.iter().enumerate() {
        if shows_headers(doc, story) {
            continue;
        }
        let candidates = candidates(story, style);
        if candidates.is_empty() {
            continue;
        }
        let thread = crate::compose::compose_story(doc, StoryId(index as u32));
        for (frame_index, frame) in thread.frames.iter().enumerate() {
            let Some(placed) = doc.object(frame.object) else {
                continue;
            };
            for (line_index, line) in frame.all_lines().enumerate() {
                if line.is_generated() {
                    continue;
                }
                for (at, text) in &candidates {
                    let starts_here =
                        line.start <= *at && (*at < line.end || line.start == line.end);
                    if starts_here {
                        found.entry(placed.page).or_default().push((
                            (placed.bounds.y, placed.bounds.x, frame_index, line_index),
                            text.clone(),
                        ));
                    }
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(page, mut texts)| {
            texts.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            (page, texts.into_iter().map(|(_, text)| text).collect())
        })
        .collect()
}

/// Where each paragraph or styled run in `style` starts, with its text.
fn candidates(story: &Story, style: &MatchStyle) -> Vec<(usize, String)> {
    match style {
        MatchStyle::Paragraph(name) => story
            .points
            .iter()
            .zip(story.point_offsets())
            .filter_map(|(point, start)| match point {
                crate::StoryPoint::Paragraph { text, style }
                    if style == name && !text.is_empty() =>
                {
                    Some((start, text.clone()))
                }
                _ => None,
            })
            .collect(),
        MatchStyle::Character(name) => {
            let text = story.text();
            let mut runs: Vec<(usize, usize)> = Vec::new();
            let mut ranges: Vec<_> = story.ranges.iter().filter(|r| r.style == *name).collect();
            ranges.sort_by_key(|r| r.start);
            for range in ranges {
                match runs.last_mut() {
                    Some(run) if range.start <= run.1 => run.1 = run.1.max(range.end),
                    _ => runs.push((range.start, range.end)),
                }
            }
            runs.into_iter()
                .filter(|(start, end)| start < end && *end <= text.len())
                .filter_map(|(start, end)| Some((start, text.get(start..end)?.to_owned())))
                .collect()
        }
    }
}
