//! Unicode letter counts compared with independently authored source ranges.
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
fn letters(count: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: CharacterStyle::Named("Initial".into()),
        delimiter: Delimiter::Enumeration("Letters".into()),
        repetition: count,
        inclusive,
    }
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mut doc = delimiters::document(false, case);
    let (rules, span) = match case {
        0 => (vec![letters(2, true)], 0..5),
        1 => (
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..letters(2, true)
                },
                letters(3, false),
            ],
            4..6,
        ),
        2 => (vec![letters(2, true)], 0..4),
        3 => (
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..letters(1, true)
                },
                letters(2, true),
            ],
            3..5,
        ),
        4 => (vec![letters(1, false)], 0..0),
        5 => (
            vec![
                NestedStyle {
                    delimiter: Delimiter::Enumeration("Dropcap".into()),
                    ..letters(1, true)
                },
                letters(2, true),
            ],
            0..6,
        ),
        6 => (vec![letters(5, true)], 0..7),
        7 => (vec![letters(i32::MAX, true)], 0..usize::MAX),
        _ => unreachable!(),
    };
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap();
    paragraph.nested_styles = Some(if reference { Vec::new() } else { rules });
    if case == 0 {
        paragraph.drop_caps_lines = Some(1);
    }
    if case >= 4 {
        doc.footnotes.prefix = Some("Note ".into());
        doc.footnotes.suffix = Some(":".into());
    }
    for story in &mut doc.stories {
        if story
            .points
            .iter()
            .any(|p| matches!(p,StoryPoint::Paragraph {style,..} if style=="Source"))
        {
            if case == 0 {
                assert!(story.ranges.is_empty() && story.structures.is_empty());
                delimiters::replace_first_line(story, "9-Éabcdef first row");
            }
            if reference {
                delimiters::explicit(story, std::slice::from_ref(&span));
            }
            for note in story
                .structures
                .iter_mut()
                .filter_map(|s| s.footnote.as_mut())
            {
                if reference {
                    delimiters::explicit(&mut note.story, std::slice::from_ref(&span));
                }
            }
        } else if story.text().starts_with("Nested delimiters / case") {
            *story = Story::from_text(format!("Nested letters / case {}", case + 1), "Heading");
        }
    }
    doc
}
