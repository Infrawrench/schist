#[path = "../examples/support/striped_decorations.rs"]
mod striped_decorations;
use schist_separation::{separate_page_without_graphics, OutputSettings};

#[test]
fn stripe_and_gap_plates_match_independent_solid_bands_in_every_writing_mode() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let doc = striped_decorations::document();
    let mut errors = Vec::new();
    for dpi in [72.0, 144.0, 216.0] {
        for page in (0..12).step_by(2) {
            let actual =
                separate_page_without_graphics(&doc, page, OutputSettings::at(dpi)).unwrap();
            let expected =
                separate_page_without_graphics(&doc, page + 1, OutputSettings::at(dpi)).unwrap();
            for (index, (a, b)) in actual
                .separation
                .plates()
                .iter()
                .zip(expected.separation.plates())
                .enumerate()
            {
                let first = a.data.iter().zip(&b.data).position(|(a, b)| a != b);
                if let Some(first) = first {
                    let count = a.data.iter().zip(&b.data).filter(|(a, b)| a != b).count();
                    let maximum = a
                        .data
                        .iter()
                        .zip(&b.data)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0_f32, f32::max);
                    errors.push(format!("dpi={dpi}, page={page}, plate={index}, first={first}, actual={}, expected={}, count={count}, max={maximum}", a.data[first], b.data[first]));
                }
            }
            assert!(
                actual
                    .separation
                    .plates()
                    .iter()
                    .filter(|p| p.data.iter().any(|v| *v > 0.1))
                    .count()
                    >= 3
            );
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
