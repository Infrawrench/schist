use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};
use schist_layout::{authoring, History, ParagraphStyle, Rect, Story, WritingMode};
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn preflight_reports_only_used_inherited_nested_rules_in_both_separation_paths() {
    for writing in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for used in [false, true] {
            for reset in [false, true] {
                let mut doc = schist_layout::blank_a4();
                let made = authoring::text_frame(
                    &mut doc,
                    &mut History::default(),
                    0,
                    Rect::new(20.0, 20.0, 200.0, 200.0),
                )
                .unwrap();
                doc.styles.add_character(schist_layout::CharacterStyle {
                    name: "Initial".into(),
                    bold: Some(true),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Nested".into(),
                    writing_mode: Some(writing),
                    nested_styles: Some(vec![NestedStyle {
                        character_style: CharacterStyle::Named("Initial".into()),
                        delimiter: Delimiter::Enumeration("Dropcap".into()),
                        repetition: 1,
                        inclusive: true,
                    }]),
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Child".into(),
                    based_on: Some("Nested".into()),
                    nested_styles: reset.then(Vec::new),
                    ..Default::default()
                });
                doc.stories[made.story.0 as usize] =
                    Story::from_text("Alpha beta gamma.", if used { "Child" } else { "Body" });
                let before = doc.clone();
                for page in [
                    separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
                    separate_page_built(
                        &doc,
                        0,
                        OutputSettings::at(72.0),
                        &NoGraphics,
                        &NaiveBuild,
                    ),
                ] {
                    let page = page.unwrap();
                    let issues: Vec<_> = page
                        .report
                        .findings
                        .iter()
                        .filter(|finding| finding.message.contains("AllNestedStyles"))
                        .collect();
                    assert_eq!(issues.len(), usize::from(used && !reset));
                    assert!(issues
                        .iter()
                        .all(|finding| finding.severity == Severity::Error));
                    assert_eq!(doc, before);
                }
            }
        }
    }
}
