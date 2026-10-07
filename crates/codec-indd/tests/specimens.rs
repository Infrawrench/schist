//! The INDD reader against the IDML InDesign exported beside the same
//! document. Every object the subset recovers must agree with the IDML
//! importer's reading of its twin; what the subset leaves out must be
//! reported.

use schist_codec_indd::database;
use schist_layout::object_styles::Paint;
use schist_layout::{Ink, LayoutDocument, LayoutObject, PlacedObject};

struct Pair {
    name: &'static str,
    indd: &'static [u8],
    idml: &'static [u8],
}

const PAIRS: [Pair; 3] = [
    Pair {
        name: "proof",
        indd: include_bytes!("../../../fixtures/indd/proof/proof.indd"),
        idml: include_bytes!("../../../fixtures/indd/proof/proof.idml"),
    },
    Pair {
        name: "psu-academic-2",
        indd: include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.indd"),
        idml: include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"),
    },
    Pair {
        name: "psu-literary",
        indd: include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.indd"),
        idml: include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml"),
    },
];

fn read(pair: &Pair) -> (LayoutDocument, Vec<String>, LayoutDocument) {
    let indd =
        schist_codec_indd::import::read(pair.indd).unwrap_or_else(|e| panic!("{}: {e}", pair.name));
    let idml = schist_codec_idml::import::read(pair.idml).unwrap().document;
    (indd.document, indd.skipped, idml)
}

/// Every `Self="u…"` id in an IDML package, with the element it is on.
fn idml_ids(idml: &[u8]) -> Vec<(String, u32)> {
    let package = schist_codec_idml::container::read(idml).unwrap();
    let mut out = Vec::new();
    for name in package.names() {
        let Some(text) = package.text(name).filter(|_| name.ends_with(".xml")) else {
            continue;
        };
        for (at, _) in text.match_indices(" Self=\"u") {
            let digits: String = text[at + 8..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if !text[at + 8 + digits.len()..].starts_with('"') {
                continue;
            }
            let open = text[..at].rfind('<').unwrap();
            let element: String = text[open + 1..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == ':')
                .collect();
            out.push((element, u32::from_str_radix(&digits, 16).unwrap()));
        }
    }
    out
}

#[test]
fn every_idml_object_is_a_database_object_of_the_matching_class() {
    for pair in &PAIRS {
        let db = database::read(pair.indd).unwrap();
        let ids = idml_ids(pair.idml);
        assert!(ids.len() > 60, "{}: {} ids", pair.name, ids.len());
        for (element, uid) in ids {
            let object = db
                .get(uid)
                .unwrap_or_else(|| panic!("{}: {element} u{uid:x} missing", pair.name));
            let expected = match element.as_str() {
                "Page" => Some(0x50f),
                "Spread" => Some(0x501),
                "MasterSpread" => Some(0x1401),
                "Story" | "XmlStory" => Some(0x201),
                "TextFrame" | "Rectangle" => Some(0x6201),
                "Group" => Some(0x401),
                "Layer" => Some(0x302),
                "Guide" => Some(0x3301),
                "Section" => Some(0x4c01),
                _ => None,
            };
            if let Some(class) = expected {
                assert_eq!(
                    object.class, class,
                    "{}: {element} u{uid:x} has class {:#x}",
                    pair.name, object.class
                );
            }
        }
    }
}

#[test]
fn pages_spreads_and_parents_agree_with_idml() {
    for pair in &PAIRS {
        let (indd, _, idml) = read(pair);
        assert_eq!(indd.pages.len(), idml.pages.len(), "{}", pair.name);
        for (i, (a, b)) in indd.pages.iter().zip(&idml.pages).enumerate() {
            assert_eq!(
                (a.width, a.height),
                (b.width, b.height),
                "{} page {i}",
                pair.name
            );
            assert_eq!(a.margins, b.margins, "{} page {i}", pair.name);
            assert_eq!(a.master, b.master, "{} page {i}", pair.name);
            // Named by number within their sections, as InDesign names them.
            assert_eq!(a.name, b.name, "{} page {i}", pair.name);
            assert_eq!(a.bleed, b.bleed, "{} page {i}", pair.name);
        }
        assert_eq!(indd.facing_pages, idml.facing_pages, "{}", pair.name);
        let spreads = |d: &LayoutDocument| {
            d.spreads
                .iter()
                .map(|s| s.pages.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(spreads(&indd), spreads(&idml), "{}", pair.name);
        assert_eq!(indd.parents.len(), idml.parents.len(), "{}", pair.name);
        for (a, b) in indd.parents.iter().zip(&idml.parents) {
            assert_eq!(a.name, b.name, "{}", pair.name);
            assert_eq!(a.objects.len(), b.objects.len(), "{} {}", pair.name, a.name);
        }
        let layers = |d: &LayoutDocument| {
            d.layer_properties
                .iter()
                .map(|l| (l.name.clone(), l.visible))
                .collect::<Vec<_>>()
        };
        assert_eq!(layers(&indd), layers(&idml), "{}", pair.name);
    }
}

/// A shape's fill, wherever the importer keeps it: a styled shape's paint
/// is its appearance, an unstyled one's is the shape's own. The subset
/// reads no object styles, so its shapes are unstyled.
fn fill(object: &PlacedObject) -> Option<Ink> {
    if object.appearance.style.is_some() {
        return match &object.appearance.paint.fill {
            Some(Paint::Ink(ink)) => Some(ink.clone()),
            _ => None,
        };
    }
    match &object.object {
        LayoutObject::Shape { fill, .. } => fill.clone(),
        _ => None,
    }
}

/// Two placed objects agree on page, geometry and kind. A frame the
/// subset reads as an empty shape because its contents went unread is
/// allowed where the IDML has a graphic.
fn same_object(name: &str, a: &PlacedObject, b: &PlacedObject, contents: &mut usize) {
    assert_eq!(a.page, b.page, "{name}");
    assert_eq!(a.bounds, b.bounds, "{name}");
    assert_eq!(a.transform, b.transform, "{name}");
    assert_eq!(a.rotation, b.rotation, "{name}");
    assert_eq!(a.hidden, b.hidden, "{name}");
    match (&a.object, &b.object) {
        (
            LayoutObject::TextFrame { overflow: x, .. },
            LayoutObject::TextFrame { overflow: y, .. },
        ) => assert_eq!(x, y, "{name}"),
        (LayoutObject::Shape { path: pa, .. }, LayoutObject::Shape { path: pb, .. }) => {
            assert_eq!(pa, pb, "{name}");
            assert_eq!(fill(a), fill(b), "{name}");
        }
        (LayoutObject::Shape { fill: None, .. }, LayoutObject::GraphicFrame { .. }) => {
            *contents += 1;
        }
        (x, y) => panic!("{name}: {x:?} read as {y:?}"),
    }
}

#[test]
fn objects_and_parent_objects_agree_with_idml() {
    for pair in &PAIRS {
        let (indd, skipped, idml) = read(pair);
        let mut contents = 0;
        assert_eq!(indd.objects.len(), idml.objects.len(), "{}", pair.name);
        for (a, b) in indd.objects.iter().zip(&idml.objects) {
            same_object(pair.name, a, b, &mut contents);
        }
        for (pa, pb) in indd.parents.iter().zip(&idml.parents) {
            for (a, b) in pa.objects.iter().zip(&pb.objects) {
                same_object(pair.name, &a.object, &b.object, &mut contents);
            }
        }
        if contents > 0 {
            let message = schist_i18n::tf!("design.indd_contents", count = contents);
            assert!(skipped.contains(&message), "{}: {skipped:?}", pair.name);
        }
    }
}

#[test]
fn story_text_agrees_with_idml_but_for_break_kinds() {
    // A page or column break is a paragraph return whose break kind is a
    // text attribute, which the subset does not read; it arrives as an
    // empty paragraph. Nothing else in the specimens' stories differs.
    let collapse = |text: String| {
        let mut text = text;
        while text.contains("\n\n") {
            text = text.replace("\n\n", "\n");
        }
        text
    };
    for pair in &PAIRS {
        let (indd, _, idml) = read(pair);
        let mut frames = 0;
        for (a, b) in indd.objects.iter().zip(&idml.objects) {
            let (
                LayoutObject::TextFrame { story: x, .. },
                LayoutObject::TextFrame { story: y, .. },
            ) = (&a.object, &b.object)
            else {
                continue;
            };
            let (x, y) = (
                indd.stories[x.0 as usize].text(),
                idml.stories[y.0 as usize].text(),
            );
            assert_eq!(collapse(x), collapse(y), "{}", pair.name);
            frames += 1;
        }
        assert!(frames >= 10, "{}: {frames} frames", pair.name);
    }
}

#[test]
fn the_report_names_what_the_subset_leaves_out() {
    let (_, skipped, _) = read(&PAIRS[1]);
    for expected in [
        schist_i18n::tf!("design.indd_story_tables", name = "u129a6", count = 1),
        schist_i18n::tf!("design.indd_story_footnotes", name = "u12666", count = 1),
        schist_i18n::tf!("design.indd_guides", count = 35),
        schist_i18n::t("design.indd_formatting").to_owned(),
    ] {
        assert!(skipped.contains(&expected), "{expected} not in {skipped:?}");
    }
    // Sections, bleed and setup are read in every specimen.
    for pair in &PAIRS {
        let (_, skipped, _) = read(pair);
        for unexpected in [
            schist_i18n::t("design.indd_document_setup").to_owned(),
            schist_i18n::tf!("design.indd_sections", count = 1),
        ] {
            assert!(
                !skipped.contains(&unexpected),
                "{}: {unexpected}",
                pair.name
            );
        }
    }
}

#[test]
fn damaged_files_are_errors_not_panics() {
    let original = PAIRS[0].indd;
    // Truncations at page boundaries and inside pages.
    for length in [
        0,
        100,
        4096,
        8192,
        8200,
        40_000,
        400_000,
        original.len() - 1,
    ] {
        let _ = schist_codec_indd::import::read(&original[..length]);
    }
    // Deterministic byte damage across the database.
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..200 {
        let mut bytes = original.to_vec();
        for _ in 0..16 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = (state % bytes.len() as u64) as usize;
            bytes[at] = (state >> 32) as u8;
        }
        let _ = schist_codec_indd::import::read(&bytes);
    }
    // A whole page zeroed, for every page of the database.
    for page in 0..original.len() / database::PAGE {
        let mut bytes = original.to_vec();
        bytes[page * database::PAGE..(page + 1) * database::PAGE].fill(0);
        let _ = schist_codec_indd::import::read(&bytes);
    }
}
