#[path = "../../separation/examples/support/named_initials.rs"]
mod proof;
use schist_codec_idml::{container, export, import};
use schist_layout::{compose, StoryId};
#[test]
fn native_saves_preserve_named_initial_rules_without_projected_ranges_or_aliases() {
    proof::register_font();
    for case in 0..proof::CASES {
        let mut doc = proof::document(false, case);
        let source = doc.stories[0].clone();
        let rule = doc
            .styles
            .paragraph("Source")
            .unwrap()
            .nested_styles
            .clone();
        for _ in 0..4 {
            let before = doc.clone();
            let thread = compose::compose_story(&doc, StoryId(0));
            assert!(!thread.has_overflow(), "case {case}");
            let rendered: Vec<_> = thread
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
            assert_eq!(doc, before);
            let written = export::write(&doc);
            assert!(!written
                .warnings
                .iter()
                .any(|s| s.contains("AllNestedStyles")));
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("Schist generated initial"));
            let fonts = package.text("Resources/Fonts.xml").unwrap();
            assert!(fonts.contains("IBM Plex Sans") && fonts.contains("Light"));
            let imported = import::read(&written.bytes).unwrap();
            assert!(!imported
                .report
                .skipped
                .iter()
                .any(|s| s.contains("AllNestedStyles")));
            doc = imported.document;
            assert_eq!(doc.styles.paragraph("Source").unwrap().nested_styles, rule);
            assert_eq!(doc.stories[0].text(), source.text());
            assert_eq!(doc.stories[0].ranges, source.ranges);
            for (a, b) in doc.stories[0].structures.iter().zip(&source.structures) {
                let (Some(a), Some(b)) = (&a.footnote, &b.footnote) else {
                    continue;
                };
                assert_eq!(a.story.text(), b.story.text());
                assert_eq!(a.story.ranges, b.story.ranges);
            }
            let restored = compose::compose_story(&doc, StoryId(0));
            let restored: Vec<_> = restored
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

#[test]
fn native_counts_enable_one_line_named_initials_without_changing_explicit_or_dormant_counts() {
    use schist_codec_idml::xml;
    proof::register_font();
    for named in [false, true] {
        for lines in [0, 1, 3] {
            for count in [None, Some(0), Some(2)] {
                let mut doc = proof::document(false, 7);
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Source")
                    .unwrap();
                style.drop_caps_lines = Some(lines);
                style.drop_caps_characters = count;
                if !named {
                    style.nested_styles = None;
                }
                let before = doc.clone();
                let written = export::write(&doc);
                assert_eq!(doc, before);
                let package = container::read(&written.bytes).unwrap();
                let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                let styles = root.find_all("ParagraphStyle");
                let native = styles
                    .iter()
                    .find(|s| s.attr("Name") == Some("Source"))
                    .unwrap();
                let expected = count
                    .or_else(|| (lines > 1 || (lines == 1 && named)).then_some(1))
                    .map(|v| v.to_string());
                assert_eq!(
                    native.attr("DropCapCharacters"),
                    expected.as_deref(),
                    "{named}/{lines}/{count:?}"
                );
            }
        }
    }
}
