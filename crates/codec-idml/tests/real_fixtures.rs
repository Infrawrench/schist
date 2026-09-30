//! Reading the real thing.
//!
//! Everything else in this crate is checked against files this crate
//! wrote, which proves only self-consistency. These fixtures are exports
//! Adobe InDesign produced, and they are the only tests in the codec that
//! say anything about whether it can open a file it has never seen.
//!
//! Provenance: the sample IDML documents published by Customer's Canvas
//! Hub for testing their InDesign importer
//! (`customerscanvas.com/docs/hub/designers-manual/adobe/indesign/gallery`),
//! offered publicly for exactly that purpose. IDML is a published
//! specification and these files are plain XML inside a ZIP, so no
//! proprietary binary was read to obtain them — the same clean-room rule
//! `docs/affinity-format.md` was built under.
//!
//! What they cover, which is what makes the set worth having:
//!
//! | Fixture | Covers |
//! | --- | --- |
//! | `text.idml` | placeholders, text on a path, stroke and shadow effects |
//! | `bounded-text.idml` | wrapping, multi-column frames, OpenType |
//! | `shapes.idml` | vector shapes, fills and strokes |
//! | `multipage.idml` | several pages, page geometry |
//! | `themes.idml` | swatches, colour themes, fonts |
//! | `placeholders.idml` | placeholder frames, barcodes |
//! | `images.idml` | linked and embedded images |
//!
//! Three assumptions this file was written with turned out to be wrong,
//! and every one of them was wrong *in the same direction*: about what a
//! conforming document always contains. They are recorded where they
//! bite, because a reader built on any of them would work on six of these
//! files and fail on the seventh.

use schist_codec_idml::container;
use schist_codec_idml::designmap::{DesignPackage, PartKind};

/// The fixtures, by name.
///
/// `include_bytes!` rather than a path at runtime, following
/// `crates/codec-affinity`: the path is resolved at compile time relative
/// to this file, so the test cannot be broken by the working directory
/// `cargo test` happens to choose.
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "bounded-text.idml",
        include_bytes!("../../../fixtures/idml/bounded-text.idml"),
    ),
    (
        "images.idml",
        include_bytes!("../../../fixtures/idml/images.idml"),
    ),
    (
        "multipage.idml",
        include_bytes!("../../../fixtures/idml/multipage.idml"),
    ),
    (
        "placeholders.idml",
        include_bytes!("../../../fixtures/idml/placeholders.idml"),
    ),
    (
        "shapes.idml",
        include_bytes!("../../../fixtures/idml/shapes.idml"),
    ),
    (
        "text.idml",
        include_bytes!("../../../fixtures/idml/text.idml"),
    ),
    (
        "themes.idml",
        include_bytes!("../../../fixtures/idml/themes.idml"),
    ),
];

/// The fixtures that genuinely contain no story.
///
/// Recorded rather than guessed: `images.idml` places images and
/// `placeholders.idml` holds barcodes, and neither has a text flow, so
/// neither has a `Stories/` directory. Two of the seven having no story
/// is not an edge case; a reader that assumed one would report a missing
/// part for a document that is complete.
const NO_STORIES: &[&str] = &["images.idml", "placeholders.idml"];

#[test]
fn every_real_export_opens() {
    for (name, fixture) in FIXTURES {
        let package =
            container::open(fixture).unwrap_or_else(|error| panic!("{name} did not open: {error}"));
        assert!(
            package.len() > 3,
            "{name} has {} parts, which is too few to be a document",
            package.len()
        );
    }
}

#[test]
fn every_real_export_declares_itself_idml() {
    for (name, fixture) in FIXTURES {
        let package = container::open(fixture).expect("opens");
        // The assertion the hand-rolled container exists to make
        // possible: a reader finds this from the first bytes of the file,
        // before it has a central directory.
        assert_eq!(
            package.text("mimetype").map(str::trim),
            Some(container::MIMETYPE),
            "{name} declares the wrong media type"
        );
    }
}

#[test]
fn the_mimetype_part_is_first_and_stored_in_a_real_export() {
    // The container writer's rule, checked against a producer we did not
    // write for.
    for (name, fixture) in FIXTURES {
        assert_eq!(
            container::method_of_first(fixture),
            Some((
                container::MIMETYPE_PART.to_string(),
                container::METHOD_STORE
            )),
            "{name} does not lead with a stored mimetype"
        );
    }
}

#[test]
fn a_real_export_names_its_root_part() {
    for (name, fixture) in FIXTURES {
        let package = container::open(fixture).expect("opens");
        let opened = DesignPackage::open(&package)
            .unwrap_or_else(|error| panic!("{name} is not a usable package: {error}"));
        assert!(
            opened.root.ends_with("designmap.xml"),
            "{name} names {} as its root",
            opened.root
        );
        assert!(
            !opened.root_bytes().expect("root part").is_empty(),
            "{name} has an empty root part"
        );
    }
}

#[test]
fn a_real_export_lays_its_objects_out_in_the_expected_directories() {
    for (name, fixture) in FIXTURES {
        let package = container::open(fixture).expect("opens");
        let opened = DesignPackage::open(&package).expect("a usable package");
        // Every real document has at least one spread and a resource
        // collection, whatever it is about.
        assert!(
            !opened.parts.of(PartKind::Spread).is_empty(),
            "{name} has no spread parts"
        );
        assert!(
            !opened.parts.of(PartKind::Resource).is_empty(),
            "{name} has no resource parts"
        );
        let has_stories = !opened.parts.of(PartKind::Story).is_empty();
        assert_eq!(
            has_stories,
            !NO_STORIES.contains(name),
            "{name}: story parts disagree with the recorded set"
        );
    }
}

/// A story threaded from a spread resolves to its part.
///
/// This is the reference the part index exists for, and the one that
/// matters: a text frame names its story, and a reader has to follow it to
/// the file holding the text.
#[test]
fn a_spreads_story_reference_resolves_to_a_story_part() {
    for (name, fixture) in FIXTURES {
        if NO_STORIES.contains(name) {
            continue;
        }
        let package = container::open(fixture).expect("opens");
        let opened = DesignPackage::open(&package).expect("a usable package");
        let mut threads = 0;
        let mut unresolved = Vec::new();
        for spread in opened.listed_of(PartKind::Spread) {
            let Ok(text) = opened.text_of(&spread.name) else {
                continue;
            };
            for story in attribute_values(text, "ParentStory") {
                // `n` is InDesign's null reference, and appears on a
                // frame with no text flow at all.
                if story == "n" {
                    continue;
                }
                threads += 1;
                if opened.parts.file_for(&story).is_none() {
                    unresolved.push((spread.name.clone(), story));
                }
            }
        }
        assert!(threads > 0, "{name} threaded no stories at all");
        assert!(
            unresolved.is_empty(),
            "{name} threaded to {} missing stories, e.g. {unresolved:?}",
            unresolved.len()
        );
    }
}

/// Most object ids have no part of their own.
///
/// The subtlety this records, and the reason the fixtures were worth
/// finding. A style, a font, a layer, a swatch group and every frame
/// inside a spread are declared *inline*, in whichever part holds them.
/// Only stories, spreads and master spreads are promoted to a file each.
/// So an id that does not resolve to a part is usually an inline object
/// rather than a missing part, and a reader that treated every
/// unresolved id as an error would reject every one of these files.
#[test]
fn an_unresolved_object_id_is_usually_an_inline_object() {
    let fixture = include_bytes!("../../../fixtures/idml/bounded-text.idml");
    let package = container::open(fixture).expect("opens");
    let opened = DesignPackage::open(&package).expect("a usable package");

    // The ids that *do* have parts: one per story, spread and master
    // spread, and each distinct.
    let part_ids: Vec<&str> = opened
        .parts
        .all()
        .iter()
        .filter(|part| !part.id.is_empty())
        .map(|part| part.id.as_str())
        .collect();
    assert!(part_ids.len() > 5, "expected several part-level objects");
    let mut unique = part_ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), part_ids.len(), "two parts claim the same id");

    // The ids that do not: styles, fonts, layers, and frames inside a
    // spread, all declared where they are used.
    let mut inline = 0;
    for part in opened.parts.all() {
        let Some(text) = package.text(&part.name) else {
            continue;
        };
        for id in object_references(text) {
            if opened.parts.file_for(&id).is_none() {
                inline += 1;
            }
        }
    }
    assert!(
        inline > part_ids.len(),
        "expected more inline objects than part-level ones: {inline} inline against {} parts",
        part_ids.len()
    );
}

#[test]
fn a_real_export_round_trips_through_our_writer() {
    for (name, fixture) in FIXTURES {
        let package = container::open(fixture).expect("opens");
        let written = container::write(&package.clone().into_parts());
        let reread = container::open(&written)
            .unwrap_or_else(|error| panic!("{name} did not survive our writer: {error}"));
        assert_eq!(
            reread.len(),
            package.len(),
            "{name} lost parts through our writer"
        );
        for part in package.names() {
            assert_eq!(
                reread
                    .get(part)
                    .unwrap_or_else(|| panic!("{name} lost {part}")),
                package.get(part).unwrap(),
                "{name} changed {part}"
            );
        }
    }
}

/// Every value of an attribute in a part.
fn attribute_values(text: &str, attribute: &str) -> Vec<String> {
    let needle = format!("{attribute}=\"");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let Some(end) = rest.find('"') else { break };
        out.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    out
}

/// The `Self=` values in a part that are object ids.
///
/// A real export sets `Self` on three different things: the object
/// itself, a named resource (`Color/Black`, `Ink/$ID/Process Cyan`) and a
/// style (`ParagraphStyle/$ID/NormalParagraphStyle`). Only the first is an
/// object id, and in these files they are recognisable by shape: a `Self_`
/// token for a part's own document object, or a short prefix and a
/// hexadecimal tail.
fn object_references(text: &str) -> Vec<String> {
    attribute_values(text, "Self")
        .into_iter()
        .filter(|value| is_object_id(value))
        .collect()
}

/// Whether a `Self=` value is an object id rather than a resource name.
///
/// Resource names always contain a `/` or a `$`; object ids never do. The
/// document itself is `d`, and InDesign's null reference is `n`.
fn is_object_id(value: &str) -> bool {
    if value.is_empty() || value.contains('/') || value.contains('$') {
        return false;
    }
    if value.starts_with("Self_") {
        return true;
    }
    let Some(tail) = value.strip_prefix('u') else {
        return value == "d";
    };
    !tail.is_empty() && tail.chars().all(|c| c.is_ascii_hexdigit())
}
