//! End-style controls compared with independently authored source ranges.
#[path = "nested_delimiters.rs"]
mod delimiters;
use schist_layout::{
    nested_styles::{CharacterStyle, Delimiter, NestedStyle},
    story::InlineControl,
    LayoutDocument, Story, StoryPoint, StoryStructure,
};
pub const CASES: usize = 8;
pub fn register_font() {
    delimiters::register_font();
}
fn end(inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: CharacterStyle::Named("Initial".into()),
        delimiter: Delimiter::Enumeration("EndNestedStyle".into()),
        repetition: 1,
        inclusive,
    }
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mut doc = delimiters::document(false, case);
    let (controls, spans, rules) = match case {
        0 => (
            vec![6],
            std::iter::once(0..6).collect(),
            vec![NestedStyle {
                delimiter: Delimiter::Enumeration("AnyWord".into()),
                repetition: 100,
                ..end(true)
            }],
        ),
        1 => (vec![4], std::iter::once(0..4).collect(), vec![end(false)]),
        2 | 6 => (vec![6], std::iter::once(0..6).collect(), vec![end(true)]),
        3 => (
            vec![3, 7],
            std::iter::once(3..7).collect(),
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..end(false)
                },
                end(true),
            ],
        ),
        4 => (vec![0], vec![], vec![end(true)]),
        5 => (vec![4], std::iter::once(0..4).collect(), vec![end(true)]),
        7 => (
            vec![0, 3, 4, 6, 8, 9],
            vec![0..3, 4..6, 8..9],
            vec![
                NestedStyle {
                    character_style: CharacterStyle::None,
                    ..end(true)
                },
                end(false),
                NestedStyle {
                    character_style: CharacterStyle::None,
                    delimiter: Delimiter::Enumeration("Repeat".into()),
                    repetition: 2,
                    inclusive: true,
                },
            ],
        ),
        _ => unreachable!(),
    };
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap();
    paragraph.drop_caps_lines = Some(0);
    paragraph.nested_styles = Some(if reference { Vec::new() } else { rules });
    // Controls inside note bodies remain unsupported. These notes exercise
    // projection of main-story controls around generated reference labels.
    let mut note_style = paragraph.clone();
    note_style.name = "Note".into();
    note_style.nested_styles = Some(Vec::new());
    doc.styles.add_paragraph(note_style);
    for story in &mut doc.stories {
        if story
            .points
            .iter()
            .any(|p| matches!(p, StoryPoint::Paragraph { style, .. } if style == "Source"))
        {
            if reference {
                delimiters::explicit(story, &spans);
            }
            for note in story
                .structures
                .iter_mut()
                .filter_map(|s| s.footnote.as_mut())
            {
                for point in &mut note.story.points {
                    if let StoryPoint::Paragraph { style, text } = point {
                        *style = "Note".into();
                        if case == 6 {
                            text.push_str("\u{2028}ninth row\u{2028}tenth row\u{2028}eleventh row\u{2028}twelfth row");
                        }
                    }
                }
            }
            if !reference {
                story
                    .structures
                    .extend(controls.iter().map(|at| StoryStructure {
                        at: Some(*at),
                        kind: "ProcessingInstruction".into(),
                        payload: "source control".into(),
                        control: Some(InlineControl::EndNestedStyle {
                            character_style: String::new(),
                        }),
                        footnote: None,
                        table: None,
                        anchored: None,
                    }));
            }
        } else if story.text().starts_with("Nested delimiters / case") {
            *story = Story::from_text(format!("End nested style / case {}", case + 1), "Heading");
        }
    }
    doc
}
