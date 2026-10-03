#[path = "../../separation/examples/support/nested_delimiters.rs"]
mod proof;
use schist_codec_idml::{container, export, import};
use schist_layout::{compose, StoryId};

#[test]
fn native_saves_preserve_delimiter_rules_and_source_without_projected_aliases() {
    proof::register_font();
    for case in 0..proof::CASES {
        let mut doc = proof::document(false, case);
        let source = doc.stories[0].clone();
        let rules = doc
            .styles
            .paragraph("Source")
            .unwrap()
            .nested_styles
            .clone();
        for _ in 0..3 {
            let before = doc.clone();
            let rendered: Vec<_> = compose::compose_story(&doc, StoryId(0))
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            let written = export::write(&doc);
            assert_eq!(doc, before);
            assert!(!written
                .warnings
                .iter()
                .any(|w| w.contains("AllNestedStyles")));
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("Schist generated"));
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.styles.paragraph("Source").unwrap().nested_styles, rules);
            assert_eq!(doc.stories[0].text(), source.text());
            assert_eq!(doc.stories[0].ranges, source.ranges);
            let restored: Vec<_> = compose::compose_story(&doc, StoryId(0))
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            assert_eq!(restored, rendered, "case {case}");
        }
    }
}
