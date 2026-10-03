#[path = "../examples/support/named_initials.rs"]
mod proof;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};
#[test]
fn named_initials_match_explicit_source_ranges_in_every_plate_and_separation_path() {
    proof::register_font();
    for case in 0..proof::CASES {
        let actual = proof::document(false, case);
        let control = proof::document(true, case);
        let source = actual.clone();
        if case == 6 {
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
                        "case {case}: {report:?}"
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
fn preflight_checks_initial_faces_only_when_the_source_range_uses_them() {
    proof::register_font();
    for case in [0, 4, 6] {
        for active in [false, true] {
            let mut doc = proof::document(false, case);
            doc.styles
                .characters
                .iter_mut()
                .find(|s| s.name == "Initial")
                .unwrap()
                .font_style = Some("Schist missing initial face".into());
            if !active {
                doc.styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Source")
                    .unwrap()
                    .drop_caps_characters = Some(0);
            }
            for page in [
                separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
                separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
            ] {
                let page = page.unwrap();
                let findings: Vec<_> = page
                    .report
                    .findings
                    .iter()
                    .filter(|f| f.message.contains("Schist missing initial face"))
                    .collect();
                assert_eq!(findings.len(), usize::from(active), "case {case}");
                assert!(findings.iter().all(|f| f.severity == Severity::Error));
            }
        }
    }
}
