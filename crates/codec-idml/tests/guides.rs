use schist_codec_idml::{container, export, import};
use schist_layout::{blank_a4, geometry::RulerGuide};

#[test]
fn page_guides_keep_coordinates_orientation_and_lock_state() {
    let mut doc = blank_a4();
    for horizontal in [false, true] {
        for locked in [false, true] {
            for position in [-4.25, 0.0, 72.0, 543.125] {
                doc.pages[0].guides.push(RulerGuide {
                    horizontal,
                    position,
                    locked,
                });
            }
        }
    }
    let expected = doc.pages[0].guides.clone();
    for _ in 0..4 {
        let encoded = export::write(&doc);
        let package = container::read(&encoded.bytes).unwrap();
        let name = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Spreads/"))
            .unwrap();
        let xml = package.text(name).unwrap();
        assert_eq!(xml.matches("<Guide ").count(), expected.len());
        assert!(xml.contains("FitToPage=\"true\""));
        doc = import::read(&encoded.bytes).unwrap().document;
        assert_eq!(doc.pages[0].guides, expected);
    }
}
