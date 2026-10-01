#[path = "../examples/support/language.rs"]
mod language;
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn language_inheritance_and_resets_match_independent_glyphs_through_affines_and_resolution() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = language::document();
    for transformed in [true, false] {
        if !transformed {
            for object in &mut doc.objects {
                object.transform = schist_core::Affine::IDENTITY;
            }
        }
        for dpi in [72.0, 144.0, 216.0] {
            for page in (0..24).step_by(2) {
                let actual =
                    separate_page_without_graphics(&doc, page, OutputSettings::at(dpi)).unwrap();
                let expected =
                    separate_page_without_graphics(&doc, page + 1, OutputSettings::at(dpi))
                        .unwrap();
                assert_eq!(
                    actual.separation.plates().len(),
                    expected.separation.plates().len()
                );
                for (a, b) in actual
                    .separation
                    .plates()
                    .iter()
                    .zip(expected.separation.plates())
                {
                    let first = a.data.iter().zip(&b.data).position(|(a, b)| a != b);
                    assert!(
                        first.is_none(),
                        "page={page},dpi={dpi},transformed={transformed},first={first:?}"
                    );
                }
                assert!(
                    actual
                        .separation
                        .plates()
                        .iter()
                        .filter(|p| p.data.iter().any(|v| *v > 0.1))
                        .count()
                        >= 2
                );
            }
        }
    }
}
