//! Word-derived rules compared with independently authored character ranges.
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
fn word(count: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: CharacterStyle::Named("Initial".into()),
        delimiter: Delimiter::Enumeration("AnyWord".into()),
        repetition: count,
        inclusive,
    }
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mut doc = delimiters::document(false, case);
    let (rules, span) = match case {
        0 => (vec![word(2, false)], 0..15),
        1 | 3 => (
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..word(1, true)
                },
                word(1, true),
            ],
            if case == 1 { 10..16 } else { 7..15 },
        ),
        2 => (vec![word(1, true)], 0..10),
        4 => (vec![word(1, false)], 0..9),
        5 => (
            vec![
                NestedStyle {
                    delimiter: Delimiter::Enumeration("Dropcap".into()),
                    ..word(1, true)
                },
                word(1, false),
            ],
            0..9,
        ),
        6 => (vec![word(2, true)], 0..16),
        7 => (vec![word(i32::MAX, true)], 0..usize::MAX),
        _ => unreachable!(),
    };
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap();
    paragraph.nested_styles = Some(if reference { Vec::new() } else { rules });
    if case == 3 {
        paragraph.drop_caps_lines = Some(1);
    }
    if case >= 4 {
        // These generated labels contain a space, but own no source words.
        doc.footnotes.prefix = Some("Note ".into());
        doc.footnotes.suffix = Some(":".into());
    }
    let replacement = match case {
        2 => Some("E\u{301}ab\u{a0}cd first row"),
        3 => Some("  Éab  second tail"),
        _ => None,
    };
    for story in &mut doc.stories {
        if story
            .points
            .iter()
            .any(|p| matches!(p,StoryPoint::Paragraph {style,..} if style=="Source"))
        {
            if let Some(text) = replacement {
                replace_first_line(story, text, case);
            }
            if reference {
                delimiters::explicit(story, std::slice::from_ref(&span));
            }
            for note in story
                .structures
                .iter_mut()
                .filter_map(|s| s.footnote.as_mut())
            {
                if let Some(text) = replacement {
                    replace_first_line(&mut note.story, text, case);
                }
                if reference {
                    delimiters::explicit(&mut note.story, std::slice::from_ref(&span));
                }
            }
        } else if story.text().starts_with("Nested delimiters / case") {
            *story = Story::from_text(format!("Nested words / case {}", case + 1), "Heading");
        }
    }
    doc
}

fn replace_first_line(story: &mut Story, text: &str, case: usize) {
    delimiters::replace_first_line(story, text);
    if matches!(case, 2 | 3) {
        // Replacing the first line moved an endpoint inside a multibyte scalar.
        for range in &mut story.ranges {
            assert_eq!((range.start, range.end), (3, 6));
            (range.start, range.end) = if case == 2 { (3, 7) } else { (4, 6) };
        }
    }
    assert!(story.ranges.iter().all(|range| {
        let text = story.text();
        text.is_char_boundary(range.start) && text.is_char_boundary(range.end)
    }));
}
