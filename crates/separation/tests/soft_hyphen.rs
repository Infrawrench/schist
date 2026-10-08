#[path = "../examples/support/soft_hyphen.rs"]
mod proof;
use schist_layout::{compose, StoryId, WritingMode};
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn discretionary_ink_matches_independent_source_in_every_process_and_spot_plate() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for spot in [false, true] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ] {
            for reverse in [false, true] {
                let actual = proof::document(false, spot, axis, reverse);
                let expected = proof::document(true, spot, axis, reverse);
                let flow = compose::compose_story(&actual, StoryId(0));
                assert_eq!(flow.frames[0].lines.len(), 1);
                assert_eq!(flow.frames[0].lines[0].end, 4);
                assert!(flow.frames[0].passed_on);
                assert!(!flow.frames[1].lost);
                let forced = compose::compose_story(&actual, StoryId(2));
                assert_eq!(forced.frames[0].lines.len(), 2);
                assert!(forced.frames[0]
                    .lines
                    .iter()
                    .all(|l| !l.discretionary_hyphen));
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
                            assert_eq!(a.data, b.data, "{spot}/{axis:?}/{reverse}/{dpi}");
                        }
                    }
                }
            }
        }
    }
}
