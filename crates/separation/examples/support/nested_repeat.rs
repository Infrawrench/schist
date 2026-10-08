//! Repeated source rules compared with independently enumerated ranges.
#[path = "nested_delimiters.rs"]
mod delimiters;
use schist_layout::{
    nested_styles::{CharacterStyle, Delimiter, NestedStyle},
    LayoutDocument, Story, StoryPoint,
};
pub const CASES: usize = 8;
pub fn register_font() {
    delimiters::register_font();
}
fn rule(style: bool, token: &str, count: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: if style {
            CharacterStyle::Named("Initial".into())
        } else {
            CharacterStyle::None
        },
        delimiter: Delimiter::Enumeration(token.into()),
        repetition: count,
        inclusive,
    }
}
fn explicit(story: &mut Story, case: usize) {
    let mut spans = Vec::new();
    for (point, offset) in story.points.iter().zip(story.point_offsets()) {
        let StoryPoint::Paragraph { text, .. } = point else {
            continue;
        };
        match case {
            2 => {
                let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
                for (index, pair) in boundaries.windows(2).enumerate() {
                    if index % 3 != 1 {
                        spans.push(offset + pair[0]..offset + pair[1]);
                    }
                }
            }
            7 => spans.push(offset..offset + text.len()),
            _ => {
                if case == 5 {
                    spans.push(offset..offset + 4);
                }
                // Independently enumerate our explicit fixture words. Their
                // separators are ordinary space and the authored soft break.
                let mut start = 0;
                for (index, word) in text.split_inclusive([' ', '\u{2028}']).enumerate() {
                    let end = start + word.len();
                    let selected = match case {
                        0 | 3 | 6 => index % 2 == 0,
                        1 | 5 => index % 2 == 1,
                        4 => index >= 2,
                        _ => unreachable!(),
                    };
                    if selected {
                        let limit = if case == 3 {
                            start + word.trim_end_matches([' ', '\u{2028}']).len()
                        } else {
                            end
                        };
                        spans.push(offset + start..offset + limit);
                    }
                    start = end;
                }
            }
        }
    }
    delimiters::explicit(story, &spans);
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mut doc = delimiters::document(false, case);
    let word = |style, inclusive| rule(style, "AnyWord", 1, inclusive);
    let repeat = |count| rule(false, "Repeat", count, true);
    let rules = match case {
        0 => vec![
            word(true, true),
            word(false, true),
            repeat(2),
            rule(true, "Sentence", 1, true),
        ],
        1 => vec![
            word(false, true),
            word(true, true),
            word(false, true),
            repeat(2),
        ],
        2 => vec![
            rule(true, "AnyCharacter", 1, true),
            rule(false, "AnyCharacter", 1, true),
            rule(true, "AnyCharacter", 1, true),
            repeat(3),
        ],
        3 => vec![word(true, false), word(false, true), repeat(2)],
        4 => vec![rule(false, "AnyWord", 2, true), word(true, true), repeat(1)],
        5 => vec![
            rule(true, "Dropcap", 1, true),
            word(false, true),
            word(true, true),
            repeat(2),
        ],
        6 => vec![word(true, true), word(false, true), repeat(2)],
        7 => vec![
            rule(false, "AnyCharacter", 1, false),
            rule(true, "AnyCharacter", 1, true),
            repeat(2),
        ],
        _ => unreachable!(),
    };
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap()
        .nested_styles = Some(if reference { Vec::new() } else { rules });
    if case >= 4 {
        doc.footnotes.prefix = Some("Loop ".into());
        doc.footnotes.suffix = Some(":".into());
    }
    for story in &mut doc.stories {
        if story
            .points
            .iter()
            .any(|p| matches!(p, StoryPoint::Paragraph {style,..} if style=="Source"))
        {
            if reference {
                explicit(story, case);
            }
            for note in story
                .structures
                .iter_mut()
                .filter_map(|s| s.footnote.as_mut())
            {
                if reference {
                    explicit(&mut note.story, case);
                }
            }
        } else if story.text().starts_with("Nested delimiters / case") {
            *story = Story::from_text(
                format!("Repeated nested styles / case {}", case + 1),
                "Heading",
            );
        }
    }
    doc
}
