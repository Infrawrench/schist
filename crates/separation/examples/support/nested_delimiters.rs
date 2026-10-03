//! Delimiter-derived formatting compared with independently authored ranges.
#[path = "named_initials.rs"]
mod base;
use schist_layout::{
    nested_styles::{CharacterStyle, Delimiter, NestedStyle},
    LayoutDocument, Story, StyleRange,
};
pub const CASES: usize = base::CASES + 4;
pub fn register_font() {
    base::register_font();
}
fn rule(delimiter: Delimiter, repetition: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: CharacterStyle::Named("Initial".into()),
        delimiter,
        repetition,
        inclusive,
    }
}
fn explicit(story: &mut Story, spans: &[std::ops::Range<usize>]) {
    let text = story.text();
    let direct = story.ranges.clone();
    story.ranges.clear();
    let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(&text).collect();
    for pair in boundaries.windows(2) {
        let nested = spans.iter().any(|span| span.contains(&pair[0]));
        let direct = direct
            .iter()
            .any(|range| range.start <= pair[0] && pair[0] < range.end);
        let style = match (nested, direct) {
            (true, true) => Some("Combined"),
            (true, false) => Some("Initial"),
            (false, true) => Some("Direct"),
            _ => None,
        };
        if let Some(style) = style {
            story.ranges.push(StyleRange::new(pair[0], pair[1], style));
        }
    }
}
fn replace_first_line(story: &mut Story, text: &str) {
    if let Some(schist_layout::StoryPoint::Paragraph { text: source, .. }) =
        story.points.first_mut()
    {
        let end = source.find('\u{2028}').unwrap_or(source.len());
        source.replace_range(..end, text);
    }
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    let base_case = match case {
        8 => 1,
        9 => 0,
        10 => 4,
        11 => 6,
        _ => case,
    };
    let mut doc = base::document(false, base_case);
    let character = |count, inclusive| {
        rule(
            Delimiter::Enumeration("AnyCharacter".into()),
            count,
            inclusive,
        )
    };
    let space = |count, inclusive| rule(Delimiter::Text(" ".into()), count, inclusive);
    let (rules, spans) = match case {
        0 | 6 => (vec![space(1, false)], std::iter::once(0..9).collect()),
        1 => (vec![space(2, true)], std::iter::once(0..16).collect()),
        2 => (vec![character(2, true)], std::iter::once(0..4).collect()),
        3 => (
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..space(1, true)
                },
                character(5, true),
            ],
            std::iter::once(10..15).collect(),
        ),
        4 => (
            vec![rule(
                Delimiter::Enumeration("ForcedLineBreak".into()),
                1,
                false,
            )],
            std::iter::once(0..19).collect(),
        ),
        5 => (
            vec![
                rule(Delimiter::Enumeration("Dropcap".into()), 1, true),
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..space(1, true)
                },
                character(5, true),
            ],
            vec![0..4, 10..15],
        ),
        7 => (vec![character(2, false)], std::iter::once(0..3).collect()),
        8 | 10 => (
            vec![rule(
                Delimiter::Text("-:?".into()),
                if case == 8 { 2 } else { 1 },
                false,
            )],
            std::iter::once(0..if case == 8 { 10 } else { 6 }).collect(),
        ),
        9 | 11 => (
            vec![rule(Delimiter::Enumeration("Digits".into()), 2, case == 9)],
            std::iter::once(0..if case == 9 { 14 } else { 13 }).collect(),
        ),
        _ => unreachable!(),
    };
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap()
        .nested_styles = Some(if reference { Vec::new() } else { rules });
    for story in &mut doc.stories {
        if story.points.iter().any(|point| matches!(point, schist_layout::StoryPoint::Paragraph { style, .. } if style == "Source")) {
            let replacement = match case {
                8 | 10 => Some("Éhead-foo:bar?tail"),
                9 | 11 => Some("first7 middle9 tail"),
                _ => None,
            };
            if let Some(text) = replacement { replace_first_line(story, text); }
            if reference { explicit(story, &spans); }
            for note in story.structures.iter_mut().filter_map(|s| s.footnote.as_mut()) {
                if let Some(text) = replacement { replace_first_line(&mut note.story, text); }
                if reference { explicit(&mut note.story, &spans); }
            }
        } else if story.text().starts_with("Named initials / case") {
            *story = Story::from_text(format!("Nested delimiters / case {}", case + 1), "Heading");
        }
    }
    doc
}
