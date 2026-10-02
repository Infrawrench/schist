//! Saving a layout document, and opening it again.
//!
//! The round trip the codec's own tests check is in memory. This one goes
//! through a file, because that is where a save actually fails: a
//! directory that does not exist, a write that truncates, an extension
//! nothing can write, or a file that lands but cannot be reopened.
//!
//! It is also the test that says whether Design Mode can be lit. A
//! document that opens and cannot be saved is a document a user cannot
//! keep.

use std::path::PathBuf;

use schist_codec_idml::{export, import};
use schist_plugin_api::PluginRegistry;

fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    registry.register_layout_codec(Box::new(schist_codec_idml::IdmlCodec));
    registry
}

/// A scratch directory for one test, emptied first.
///
/// Keyed by the test's own name because these run concurrently: a
/// directory shared between them would have one test's file deleted by
/// another's setup, which is a confusing failure that looks like the
/// writer losing a save.
fn scratch_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("schist-design-save-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Write exactly as the editor does: temp file, then rename.
fn save(path: &PathBuf, document: &schist_layout::LayoutDocument) -> std::io::Result<()> {
    let bytes = export::write(document).bytes;
    let temporary = path.with_extension("schist-tmp");
    std::fs::write(&temporary, &bytes)?;
    std::fs::rename(&temporary, path)
}

fn reopened(path: &PathBuf) -> schist_layout::LayoutDocument {
    let registry = registry();
    let bytes = std::fs::read(path).expect("the saved file is readable");
    let codec = registry
        .layout_codec_for(&bytes, Some("idml"))
        .expect("what we saved is recognised on the way back in");
    codec
        .read_layout(&bytes)
        .expect("what we saved can be read back")
        .0
}

#[test]
fn a_document_survives_a_save_and_an_open() {
    let path = scratch_dir("document").join("document.idml");
    let original = schist_layout::blank_a4();
    save(&path, &original).expect("saving");
    let back = reopened(&path);

    assert_eq!(back.pages.len(), original.pages.len());
    assert_eq!(back.pages[0].width, original.pages[0].width);
    assert_eq!(back.pages[0].height, original.pages[0].height);
    // And it has a pasteboard, which is the point of a document.
    assert!(
        schist_layout::pasteboard::pasteboard(
            &back,
            &schist_layout::pasteboard::PasteboardView::default()
        )
        .is_some(),
        "a saved document cannot be drawn"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_save_leaves_no_temporary_file_behind() {
    // The temp-and-rename is there so an interrupted save cannot
    // truncate the user's file. A temporary file left behind after a
    // successful save would show up in the folder and in every next
    // Save As dialog.
    let path = scratch_dir("no-temp").join("no-temp.idml");
    save(&path, &schist_layout::blank_a4()).expect("saving");
    let leftovers: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .expect("listing the directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("schist-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_save_over_an_existing_file_replaces_it() {
    // The rename is what makes a save atomic; without it a second save
    // over a first is where a truncated file comes from.
    let path = scratch_dir("twice").join("twice.idml");
    let mut first = schist_layout::blank_a4();
    first.pages[0].name = "First".into();
    save(&path, &first).expect("the first save");
    let mut second = schist_layout::blank_a4();
    second.pages[0].name = "Second".into();
    save(&path, &second).expect("the second save");
    assert_eq!(
        reopened(&path).pages[0].name,
        "Second",
        "the second save did not replace the first"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_failed_save_leaves_the_previous_file_intact() {
    // The reason for the temp file: an interrupted save must not cost the
    // user the document they already had.
    let path = scratch_dir("kept").join("kept.idml");
    let mut document = schist_layout::blank_a4();
    document.pages[0].name = "Original".into();
    save(&path, &document).expect("the first save");

    // A directory that is not there is the ordinary failure.
    let missing = path.parent().unwrap().join("gone").join("deep.idml");
    let failed = save(&missing, &document);
    assert!(
        failed.is_err(),
        "saving into a missing directory should fail"
    );
    assert_eq!(
        reopened(&path).pages[0].name,
        "Original",
        "a failed save disturbed the file that was already there"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_saved_document_keeps_its_pages_and_its_frames() {
    // The substance of a save, on a document with something on it.
    let path = scratch_dir("frames").join("with-frames.idml");
    let mut document = schist_layout::blank_a4();
    document.stories.push(schist_layout::Story {
        prefs: Default::default(),
        structures: Vec::new(),
        points: vec![schist_layout::StoryPoint::Paragraph {
            text: "Headline".into(),
            style: "Body".into(),
        }],
        ranges: Vec::new(),
    });
    let id = document.add_object(schist_layout::PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: schist_layout::ObjectId::next(),
        page: 0,
        bounds: schist_layout::Rect::new(10.0, 20.0, 300.0, 40.0),
        object: schist_layout::LayoutObject::TextFrame {
            footnotes: Default::default(),
            text_path: None,
            story: schist_layout::StoryId(0),
            columns: 1,
            gutter: 0.0,
            insets: Default::default(),
            overflow: Default::default(),
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Headline".into(),
        locked: false,
        overprint: false,
        transparency: 0.0,
    });
    document.object_layers.push((id, document.layers[0]));

    save(&path, &document).expect("saving");
    let back = reopened(&path);
    assert_eq!(back.stories.len(), 1);
    let frame = back
        .objects
        .iter()
        .find(|object| matches!(object.object, schist_layout::LayoutObject::TextFrame { .. }))
        .expect("the frame came back");
    assert_eq!(
        frame.bounds,
        schist_layout::Rect::new(10.0, 20.0, 300.0, 40.0)
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_codec_reports_what_a_save_could_not_carry() {
    // A writer that drops content silently is the failure this exists to
    // prevent, so the report is part of the contract rather than a debug
    // aid.
    let mut document = schist_layout::blank_a4();
    document.add_object(schist_layout::PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: schist_layout::ObjectId::next(),
        page: 0,
        bounds: schist_layout::Rect::new(0.0, 0.0, 50.0, 50.0),
        // A group has no encoding yet, and a save that dropped it
        // without saying so would lose a frame silently.
        object: schist_layout::LayoutObject::Group {
            children: vec![schist_layout::ObjectId::next()],
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Group".into(),
        locked: false,
        overprint: false,
        transparency: 0.0,
    });
    let written = export::write(&document);
    assert!(
        !written.warnings.is_empty(),
        "a document with something unwritable should say so"
    );
    assert!(
        written
            .warnings
            .iter()
            .any(|w| w
                == &schist_i18n::tf!("design.idml_group_unwritten", name = "Group", count = 1)),
        "{:?}",
        written.warnings
    );
    // And what it warned about is still gone, rather than half written.
    let back = import::read(&written.bytes).expect("reading it back");
    assert!(!back
        .document
        .objects
        .iter()
        .any(|object| matches!(object.object, schist_layout::LayoutObject::Group { .. })));
}

#[test]
fn a_file_nothing_can_write_is_refused_rather_than_silently_written() {
    // Saving a layout document as a PSD would produce a file that is not
    // a layout document, so the exporter lookup has to fail.
    let registry = registry();
    let codec = registry
        .layout_codecs()
        .find(|codec| codec.can_export())
        .expect("a writable layout codec is registered");
    assert_eq!(codec.extensions(), &["idml"]);
    // Nothing in the layout registry claims a raster extension.
    for raster in ["psd", "psb", "png", "jpg"] {
        assert!(
            !registry
                .layout_codecs()
                .any(|codec| codec.extensions().contains(&raster)),
            "a layout codec claims {raster}"
        );
    }
}
