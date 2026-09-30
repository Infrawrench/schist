//! Public-domain Penn State templates exported by InDesign 20.2.
//! Source, dedication and hashes are beside the paired documents in fixtures/indd.
use schist_codec_idml::{container, export, import, xml};
use schist_layout::LayoutDocument;

const TEMPLATES: &[(&str, &[u8], usize)] = &[
    (
        "academic",
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"),
        7,
    ),
    (
        "literary",
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml"),
        8,
    ),
];

fn texts(doc: &LayoutDocument) -> Vec<String> {
    let mut text: Vec<_> = doc.stories.iter().map(|story| story.text()).collect();
    text.sort();
    text
}

#[test]
fn published_facing_templates_preserve_page_geometry_parent_artwork_and_text_on_resave() {
    for (name, bytes, count) in TEMPLATES {
        let original = import::read(bytes).unwrap().document;
        assert_eq!(original.pages.len(), *count, "{name}");
        assert!(original.spreads.iter().any(|s| s.pages.len() == 2));
        assert_eq!(original.parents.len(), 2);
        assert!(original
            .parents
            .iter()
            .all(|p| p.sheets.len() == 2 && !p.objects.is_empty()));
        assert!(texts(&original).iter().any(|text| !text.is_empty()));
        let mut doc = original.clone();
        for _ in 0..4 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(texts(&doc), texts(&original), "{name}: story text changed");
            assert_eq!(doc.pages.len(), original.pages.len());
            assert_eq!(doc.objects.len(), original.objects.len());
            assert_eq!(doc.page_binding, original.page_binding);
            assert_eq!(
                doc.spreads.iter().map(|s| &s.pages).collect::<Vec<_>>(),
                original
                    .spreads
                    .iter()
                    .map(|s| &s.pages)
                    .collect::<Vec<_>>()
            );
            for (a, b) in doc.pages.iter().zip(&original.pages) {
                assert_eq!(a.name, b.name);
                for (a, b) in [
                    (a.width, b.width),
                    (a.height, b.height),
                    (a.margins.left, b.margins.left),
                    (a.margins.right, b.margins.right),
                    (a.margins.top, b.margins.top),
                    (a.margins.bottom, b.margins.bottom),
                ] {
                    assert!((a - b).abs() < 0.001, "{name}: {a} != {b}");
                }
            }
            for (a, b) in doc.parents.iter().zip(&original.parents) {
                assert_eq!(a.applied_to, b.applied_to);
                assert_eq!(a.sheets.len(), b.sheets.len());
                assert_eq!(a.objects.len(), b.objects.len());
                for (a, b) in a.objects.iter().zip(&b.objects) {
                    assert_eq!(a.object.page, b.object.page);
                    let a = a.object.visual_bounds();
                    let b = b.object.visual_bounds();
                    for (a, b) in [
                        (a.x, b.x),
                        (a.y, b.y),
                        (a.width, b.width),
                        (a.height, b.height),
                    ] {
                        assert!((a - b).abs() < 0.001, "{name}: parent geometry {a} != {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn real_spot_resource_stays_a_spot_with_its_native_definition() {
    let bytes = TEMPLATES[0].1;
    let package = container::read(bytes).unwrap();
    let graphic = xml::parse(package.text("Resources/Graphic.xml").unwrap()).unwrap();
    let native = graphic
        .find_all("Color")
        .into_iter()
        .filter(|color| color.attr("Model") == Some("Spot"))
        .collect::<Vec<_>>();
    assert!(!native.is_empty());
    let mut doc = import::read(bytes).unwrap().document;
    for _ in 0..4 {
        for color in &native {
            let name = color.attr("Name").unwrap();
            let ink = doc.ink(name).unwrap();
            assert!(ink.spot);
            let values = xml::numbers(color.attr("ColorValue").unwrap());
            match color.attr("Space") {
                Some("Lab") => assert_eq!(ink.lab.as_slice(), values),
                Some("CMYK") => assert_eq!(
                    ink.source_cmyk.unwrap().as_slice(),
                    values.iter().map(|v| v / 100.0).collect::<Vec<_>>()
                ),
                Some("RGB") => assert_eq!(
                    ink.preview_rgb.as_slice(),
                    values.iter().map(|v| v / 255.0).collect::<Vec<_>>()
                ),
                other => panic!("unexpected spot colour space {other:?}"),
            }
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}
