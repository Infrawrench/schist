//! Source-derived character formatting, shared by geometry and typed paint.
use super::{CharacterStyle, NestedStyle};
use crate::{ResolvedCharacter, ResolvedParagraph, Story, StoryPoint, StyleRange, StyleSet};
use std::ops::Range;

pub(super) fn canonical_initial(rule: &NestedStyle) -> bool {
    rule.is_initial()
        && matches!(
            rule.character_style,
            CharacterStyle::Named(_) | CharacterStyle::None
        )
}

#[derive(Clone)]
pub(crate) struct Run {
    pub start: usize,
    pub end: usize,
    pub character: ResolvedCharacter,
}

/// The named style of an active canonical source initial. One-line initials
/// receive nominal character formatting without multi-line enlargement.
pub fn initial_style(paragraph: &ResolvedParagraph) -> Option<&str> {
    let rule = paragraph.nested_styles.as_ref()?.first()?;
    if !canonical_initial(rule)
        || paragraph.drop_caps_lines.unwrap_or(0) == 0
        || paragraph.drop_caps_characters.unwrap_or(1) == 0
    {
        return None;
    }
    let CharacterStyle::Named(name) = &rule.character_style else {
        return None;
    };
    Some(name)
}

fn derived_runs(
    story: &Story,
    text: &str,
    start: usize,
    paragraph: &ResolvedParagraph,
    styles: &StyleSet,
) -> Vec<Run> {
    let rules = paragraph.nested_styles.as_deref().unwrap_or_default();
    let plan = super::boundaries::plan(rules);
    let rules = &rules[..plan.prefix];
    let mut out = Vec::new();
    if !rules
        .iter()
        .any(|rule| matches!(rule.character_style, CharacterStyle::Named(_)))
    {
        return out;
    }
    let mut cursor = 0;
    let markers = crate::inline_controls::end_markers(story, start, text, paragraph);
    let mut marker = 0;
    let mut index = 0;
    let mut cycle_cursor = (0, 0);
    while index < rules.len() {
        if plan.repeat_from == Some(index) {
            cycle_cursor = (cursor, marker);
        }
        let rule = &rules[index];
        let end = if index == 0 && rule.is_initial() {
            if paragraph.drop_caps_lines.unwrap_or(0) == 0 {
                0
            } else {
                schist_text_engine::grapheme_boundaries(text)
                    .nth(paragraph.drop_caps_characters.unwrap_or(1))
                    .unwrap_or(text.len())
            }
        } else {
            let boundary = super::boundaries::end(&text[cursor..], rule);
            let end = cursor + boundary.at;
            if let Some(at) = markers
                .get(marker)
                .copied()
                .filter(|at| *at < end || (*at == end && (!boundary.found || !rule.inclusive)))
            {
                // Both through/up-to consume the invisible control. Its own
                // formatting has no painted bytes; following rules move on.
                marker += 1;
                at
            } else {
                end
            }
        };
        if let CharacterStyle::Named(name) = &rule.character_style {
            if cursor < end && styles.character(name).is_some() {
                out.push(Run {
                    start: start + cursor,
                    end: start + end,
                    character: styles.resolve_character(name),
                });
            }
        }
        cursor = end;
        index += 1;
        if index == rules.len() {
            if let Some(repeat_from) = plan.repeat_from {
                // Individual excluded delimiters may be zero-width. Only a
                // whole cycle without text or control progress stops the loop.
                if (cursor, marker) == cycle_cursor || cursor == text.len() {
                    break;
                }
                index = repeat_from;
            }
        }
    }
    out
}

/// Keep authored range precedence. Within a derived prefix, explicit character
/// properties override the paragraph's nested style. Outside it, resolution is
/// unchanged. Derived runs fill only source bytes without an authored range.
fn merge(story: &Story, styles: &StyleSet, query: Range<usize>, derived: &[Run]) -> Vec<Run> {
    if derived.is_empty() {
        return story
            .ranges
            .iter()
            .filter(|range| range.start < query.end && range.end > query.start)
            .map(|range| Run {
                start: range.start.max(query.start),
                end: range.end.min(query.end),
                character: styles.resolve_character(&range.style),
            })
            .collect();
    }
    let mut out = Vec::new();
    for range in story
        .ranges
        .iter()
        .filter(|r| r.start < query.end && r.end > query.start)
    {
        let start = range.start.max(query.start);
        let end = range.end.min(query.end);
        let mut cuts = vec![start, end];
        for run in derived.iter().filter(|r| r.start < end && r.end > start) {
            cuts.extend([run.start.max(start), run.end.min(end)]);
        }
        cuts.sort_unstable();
        cuts.dedup();
        let character = styles.resolve_character(&range.style);
        for pair in cuts.windows(2) {
            let character = derived
                .iter()
                .find(|r| r.start <= pair[0] && pair[0] < r.end)
                .map_or_else(
                    || character.clone(),
                    |r| character.clone().over(&r.character),
                );
            out.push(Run {
                start: pair[0],
                end: pair[1],
                character,
            });
        }
    }
    for run in derived
        .iter()
        .filter(|r| r.start < query.end && r.end > query.start)
    {
        let start = run.start.max(query.start);
        let end = run.end.min(query.end);
        let mut cuts = vec![start, end];
        for range in story
            .ranges
            .iter()
            .filter(|r| r.start < end && r.end > start)
        {
            cuts.extend([range.start.max(start), range.end.min(end)]);
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            if !story
                .ranges
                .iter()
                .any(|r| r.start <= pair[0] && pair[0] < r.end)
            {
                out.push(Run {
                    start: pair[0],
                    end: pair[1],
                    character: run.character.clone(),
                });
            }
        }
    }
    out
}

/// Effective runs for one paragraph slice. Derive boundaries from the complete
/// paragraph so a continuation never restarts the rule sequence.
pub(crate) fn runs(
    story: &Story,
    styles: &StyleSet,
    query: Range<usize>,
    paragraph: &ResolvedParagraph,
) -> Vec<Run> {
    if paragraph.nested_styles.as_ref().is_none_or(Vec::is_empty) {
        return merge(story, styles, query, &[]);
    }
    let derived = story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, start)| {
            let StoryPoint::Paragraph { text, .. } = point else {
                return None;
            };
            (query.start >= start && query.start <= start + text.len())
                .then(|| derived_runs(story, text, start, paragraph, styles))
        })
        .unwrap_or_default();
    merge(story, styles, query, &derived)
}

/// Resolve source prefixes before generated references enter the text. Only the
/// cloned story/style set receives these ranges and aliases. Suppress the
/// consumed prefix there; source diagnostics travel separately from projection.
pub(crate) fn materialize<'a>(
    source: &'a Story,
    styles: &mut StyleSet,
) -> std::borrow::Cow<'a, Story> {
    let derived: Vec<_> = source
        .points
        .iter()
        .zip(source.point_offsets())
        .flat_map(|(point, start)| {
            let StoryPoint::Paragraph { text, style } = point else {
                return Vec::new();
            };
            derived_runs(
                source,
                text,
                start,
                &styles.resolve_paragraph(style),
                styles,
            )
        })
        .collect();
    // An empty source span is still consumed: generated labels must not make
    // that rule acquire characters after projection.
    if derived.is_empty()
        && !source.points.iter().any(|point| {
            let StoryPoint::Paragraph { style, .. } = point else {
                return false;
            };
            let paragraph = styles.resolve_paragraph(style);
            let rules = paragraph.nested_styles.as_deref().unwrap_or_default();
            rules[..super::boundaries::prefix(rules)]
                .iter()
                .any(|rule| matches!(rule.character_style, CharacterStyle::Named(_)))
        })
    {
        return std::borrow::Cow::Borrowed(source);
    }
    let mut story = source.clone();
    let runs = if derived.is_empty() {
        Vec::new()
    } else {
        story.ranges.clear();
        merge(source, styles, 0..source.text_len(), &derived)
    };
    for run in runs {
        let mut index = styles.characters.len();
        let name = loop {
            let name = format!("Schist generated initial character {index}");
            if styles.character(&name).is_none() {
                break name;
            }
            index += 1;
        };
        styles.characters.push(run.character.into_style(&name));
        story.ranges.push(StyleRange::new(run.start, run.end, name));
    }
    for point in &mut story.points {
        let StoryPoint::Paragraph { style, .. } = point else {
            continue;
        };
        let paragraph = styles.resolve_paragraph(style);
        let Some(mut rules) = paragraph.nested_styles else {
            continue;
        };
        let count = super::boundaries::prefix(&rules);
        if count == 0 {
            continue;
        }
        for rule in &mut rules[..count] {
            rule.character_style = CharacterStyle::None;
        }
        let mut index = styles.paragraphs.len();
        let name = loop {
            let name = format!("Schist generated initial paragraph {index}");
            if styles.paragraph(&name).is_none() {
                break name;
            }
            index += 1;
        };
        styles.paragraphs.push(crate::ParagraphStyle {
            name: name.clone(),
            based_on: Some(style.clone()),
            nested_styles: Some(rules),
            ..Default::default()
        });
        *style = name;
    }
    std::borrow::Cow::Owned(story)
}
