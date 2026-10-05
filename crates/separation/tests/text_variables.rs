#[path = "../examples/support/text_variables.rs"]
mod proof;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn variables_match_independent_ordinary_text_in_every_plate_and_separation_path() {
    proof::register_font();
    for case in 0..proof::CASES {
        let actual = proof::document(false, case);
        let control = proof::document(true, case);
        let source = actual.clone();
        for dpi in [72.0, 144.0, 216.0] {
            let settings = OutputSettings::at(dpi);
            let expected =
                separate_page(&control, proof::page(case), settings, &NoGraphics).unwrap();
            for page in [
                separate_page(&actual, proof::page(case), settings, &NoGraphics),
                separate_page_built(
                    &actual,
                    proof::page(case),
                    settings,
                    &NoGraphics,
                    &NaiveBuild,
                ),
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
fn section_values_without_one_section_are_preflight_errors_not_guesses() {
    use schist_layout::{
        authoring, story::InlineControl, text_variables::*, threading, History, Rect, Section,
        Story, StoryStructure,
    };
    proof::register_font();
    for (second_page, expected_error) in [(1, true), (0, false)] {
        let mut doc = schist_layout::blank_a4();
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Body")
            .unwrap()
            .family = Some("IBM Plex Sans".into());
        doc.add_page(doc.pages[0].clone());
        doc.pages[1].section = Some(Section {
            start: 9,
            continue_numbering: false,
            ..Default::default()
        });
        let mut history = History::default();
        let first = authoring::text_frame(
            &mut doc,
            &mut history,
            0,
            Rect::new(30.0, 30.0, 200.0, 80.0),
        )
        .unwrap();
        let second = authoring::text_frame(
            &mut doc,
            &mut history,
            second_page,
            Rect::new(30.0, 300.0, 200.0, 80.0),
        )
        .unwrap();
        assert!(threading::link(
            &mut doc,
            &mut history,
            first.object,
            second.object
        ));
        let story = threading::story_of(&doc, first.object).unwrap();
        let mut text = Story::from_text("Last ", "Body");
        text.structures.push(StoryStructure {
            at: Some(5),
            kind: "TextVariableInstance".into(),
            payload: String::new(),
            footnote: None,
            control: Some(InlineControl::TextVariable {
                variable: "last".into(),
                character_style: String::new(),
                name: String::new(),
            }),
            anchored: None,
        });
        doc.stories[story.0 as usize] = text;
        doc.text_variables.push(TextVariable::new(
            "last",
            "Last",
            VariableKind::LastPage(LastPageNumber::default()),
        ));
        for page in [
            separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
            separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
        ] {
            let errors = page
                .unwrap()
                .report
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count();
            assert_eq!(
                errors > 0,
                expected_error,
                "second frame on page {second_page}"
            );
        }
    }
}
