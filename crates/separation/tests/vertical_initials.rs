#[path = "../examples/support/vertical_initials.rs"]
mod proof;
use schist_layout::WritingMode;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn vertical_initials_match_independently_placed_frames_in_every_process_and_spot_plate() {
    proof::register_font();
    for spot in [false, true] {
        for mode in [
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            for latin in [false, true] {
                let actual = proof::document(false, spot, mode, latin);
                let control = proof::document(true, spot, mode, latin);
                for dpi in [72.0, 144.0, 216.0] {
                    let settings = OutputSettings::at(dpi);
                    let expected = separate_page(&control, 0, settings, &NoGraphics).unwrap();
                    assert!(
                        !expected
                            .report
                            .findings
                            .iter()
                            .any(|f| f.severity == Severity::Error),
                        "reference {spot}/{mode:?}/{latin}/{dpi}: {:?}",
                        expected.report
                    );
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
                            "actual {:?}",
                            output.report
                        );
                        assert_eq!(
                            output.separation.plates().len(),
                            expected.separation.plates().len()
                        );
                        for (a, b) in output
                            .separation
                            .plates()
                            .iter()
                            .zip(expected.separation.plates())
                        {
                            let differences: Vec<_> = a
                                .data
                                .iter()
                                .zip(&b.data)
                                .enumerate()
                                .filter(|(_, (a, b))| a != b)
                                .collect();
                            assert!(
                                differences.is_empty(),
                                "{spot}/{mode:?}/{latin}/{dpi}: {} differing samples, first {:?}",
                                differences.len(),
                                &differences[..differences.len().min(12)]
                            );
                        }
                    }
                }
            }
        }
    }
}
