#[path = "../examples/support/dotted_decorations.rs"]
mod dotted_decorations;
use schist_separation::{separate_page_without_graphics, OutputSettings};
#[test]
fn dotted_inks_match_independent_circles_through_resolution_and_affine_changes() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = dotted_decorations::document();
    for transformed in [false, true] {
        for object in &mut doc.objects {
            object.transform = if transformed {
                schist_core::Affine {
                    a: 0.9,
                    b: 0.12,
                    c: 0.15,
                    d: 0.9,
                    tx: 0.0,
                    ty: 0.0,
                }
            } else {
                schist_core::Affine::IDENTITY
            };
        }
        for dpi in [72.0, 144.0, 216.0] {
            let settings = OutputSettings::at(dpi);
            for page in 0..doc.pages.len() {
                let actual = separate_page_without_graphics(&doc, page, settings).unwrap();
                let reference = dotted_decorations::reference(&doc, page, settings);
                for (index, (a, b)) in actual
                    .separation
                    .plates()
                    .iter()
                    .zip(reference.separation.plates())
                    .enumerate()
                {
                    let first = a.data.iter().zip(&b.data).position(|(a, b)| a != b);
                    assert!(
                    first.is_none(),
                    "page={page},dpi={dpi},transformed={transformed},plate={index},first={:?},count={}",
                    first.map(|i| (i, a.data[i], b.data[i])),
                    a.data.iter().zip(&b.data).filter(|(a,b)| a != b).count()
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
