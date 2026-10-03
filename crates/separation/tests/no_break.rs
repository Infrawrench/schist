#[path = "../examples/support/no_break.rs"]
mod proof;
use schist_layout::WritingMode;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn protected_thread_destinations_match_independent_frames_in_every_plate() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for character in [false, true] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            for reverse in [false, true] {
                let actual = proof::document(false, character, axis, reverse);
                let expected = proof::document(true, character, axis, reverse);
                for dpi in [72.0, 144.0, 216.0] {
                    let settings = OutputSettings::at(dpi);
                    let control = separate_page(&expected, 0, settings, &NoGraphics).unwrap();
                    for output in [
                        separate_page(&actual, 0, settings, &NoGraphics),
                        separate_page_built(&actual, 0, settings, &NoGraphics, &NaiveBuild),
                    ] {
                        let output = output.unwrap();
                        assert!(
                            !output
                                .report
                                .findings
                                .iter()
                                .any(|f| f.severity == Severity::Error),
                            "{:?}",
                            output.report
                        );
                        assert_eq!(
                            output.separation.plates().len(),
                            control.separation.plates().len()
                        );
                        for (a, b) in output
                            .separation
                            .plates()
                            .iter()
                            .zip(control.separation.plates())
                        {
                            assert_eq!(a.data, b.data, "{character}/{axis:?}/{reverse}/{dpi}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unavailable_protected_destinations_report_terminal_overset_in_both_paths() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for character in [false, true] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            let mut doc = proof::document(false, character, axis, false);
            doc.objects[1].bounds.width = 20.0;
            doc.objects[1].bounds.height = 20.0;
            let expected =
                schist_i18n::tf!("design.preflight_overset", name = "Protected destination");
            for output in [
                separate_page(&doc, 0, OutputSettings::at(72.0), &NoGraphics),
                separate_page_built(&doc, 0, OutputSettings::at(72.0), &NoGraphics, &NaiveBuild),
            ] {
                assert!(output
                    .unwrap()
                    .report
                    .findings
                    .iter()
                    .any(|f| f.severity == Severity::Error && f.message == expected));
            }
        }
    }
}
