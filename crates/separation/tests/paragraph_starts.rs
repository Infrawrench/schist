#[path = "../examples/support/paragraph_starts.rs"]
mod proof;

#[test]
fn paragraph_destinations_match_independent_frames_in_every_plate() {
    compare(proof::document);
}

#[test]
fn explicit_break_destinations_match_independent_frames_in_every_plate() {
    compare(proof::forced_document);
}

fn compare(document: fn(bool, u16, bool) -> schist_layout::LayoutDocument) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for columns in [1, 2] {
        for reverse in [false, true] {
            let actual = document(false, columns, reverse);
            let expected = document(true, columns, reverse);
            for dpi in [72.0, 144.0, 216.0] {
                for page in 0..3 {
                    let settings = schist_separation::OutputSettings::at(dpi);
                    let a =
                        schist_separation::separate_page_without_graphics(&actual, page, settings)
                            .unwrap();
                    let b = schist_separation::separate_page_without_graphics(
                        &expected, page, settings,
                    )
                    .unwrap();
                    assert!(
                        !a.report
                            .findings
                            .iter()
                            .any(|f| f.severity == schist_separation::Severity::Error),
                        "{:?}",
                        a.report
                    );
                    assert_eq!(a.separation.plates().len(), b.separation.plates().len());
                    for (index, (a, b)) in a
                        .separation
                        .plates()
                        .iter()
                        .zip(b.separation.plates())
                        .enumerate()
                    {
                        let difference = a
                            .data
                            .iter()
                            .zip(&b.data)
                            .enumerate()
                            .find(|(_, (a, b))| a != b);
                        assert!(difference.is_none(), "{columns}/{reverse}, page={page}, dpi={dpi}, plate={index}, difference={difference:?}");
                    }
                }
            }
        }
    }
}
