#[path = "../examples/support/nested_delimiters.rs"]
mod proof;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn nested_delimiters_match_explicit_source_ranges_in_every_plate_and_separation_path() {
    proof::register_font();
    for case in 0..proof::CASES {
        let actual = proof::document(false, case);
        let control = proof::document(true, case);
        let source = actual.clone();
        if case == 6 || case == 11 {
            let thread = schist_layout::compose::compose_story(&actual, schist_layout::StoryId(0));
            assert_eq!(
                thread
                    .frames
                    .iter()
                    .filter(|frame| !frame.footnotes.is_empty())
                    .count(),
                2
            );
        }
        for dpi in [72.0, 144.0, 216.0] {
            let settings = OutputSettings::at(dpi);
            let expected = separate_page(&control, 0, settings, &NoGraphics).unwrap();
            for page in [
                separate_page(&actual, 0, settings, &NoGraphics),
                separate_page_built(&actual, 0, settings, &NoGraphics, &NaiveBuild),
            ] {
                let page = page.unwrap();
                for report in [&page.report, &expected.report] {
                    assert!(
                        !report
                            .findings
                            .iter()
                            .any(|f| f.severity == Severity::Error),
                        "{case}: {report:?}"
                    );
                }
                assert_eq!(
                    page.separation.plates().len(),
                    expected.separation.plates().len()
                );
                for (a, b) in page
                    .separation
                    .plates()
                    .iter()
                    .zip(expected.separation.plates())
                {
                    assert_eq!(
                        a.data.iter().zip(&b.data).filter(|(a, b)| a != b).count(),
                        0,
                        "case {case}/{dpi}: plate samples differ"
                    );
                }
            }
        }
        assert_eq!(actual, source);
    }
}

#[test]
fn source_nested_diagnostics_survive_materialized_main_whole_and_split_notes() {
    use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};
    proof::register_font();
    for case in [0, 4, 6] {
        for named in [false, true] {
            let mut doc = proof::document(false, case);
            let rules = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Source")
                .unwrap()
                .nested_styles
                .as_mut()
                .unwrap();
            if !named {
                for rule in rules.iter_mut() {
                    rule.character_style = CharacterStyle::None;
                }
            }
            // An uncomposed repeat can change prior named formatting, even
            // though its own character-style slot is None.
            rules.push(NestedStyle {
                character_style: CharacterStyle::None,
                delimiter: Delimiter::Enumeration("Repeat".into()),
                repetition: 1,
                inclusive: true,
            });
            for page in [
                separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
                separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
            ] {
                let page = page.unwrap();
                let issues: Vec<_> = page
                    .report
                    .findings
                    .iter()
                    .filter(|f| f.message.contains("AllNestedStyles"))
                    .collect();
                assert_eq!(!issues.is_empty(), named, "case {case}");
                assert!(issues.iter().all(|f| f.severity == Severity::Error));
            }
        }
    }
}
