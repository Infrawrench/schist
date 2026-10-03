#[path = "../examples/support/projected_initials.rs"]
mod proof;
use schist_separation::{
    separate_page, separate_page_built, NaiveBuild, NoGraphics, OutputSettings, Severity,
};

#[test]
fn generated_initial_prefixes_match_independently_authored_text_on_every_plate() {
    proof::register_font();
    for spot in [false, true] {
        for middle in [false, true] {
            let actual = proof::document(false, spot, middle);
            let control = proof::document(true, spot, middle);
            let source = actual.clone();
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
                                .any(|finding| finding.severity == Severity::Error),
                            "{report:?}"
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
                        let count = a.data.iter().zip(&b.data).filter(|(a, b)| a != b).count();
                        assert_eq!(count, 0, "{spot}/{middle}/{dpi}: differing plate samples");
                    }
                }
            }
            assert_eq!(actual, source);
        }
    }
}
