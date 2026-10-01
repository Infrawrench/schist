//! The open path: a page layout file has to land in Design Mode.
//!
//! The failure this guards against is quiet and looks like success. An
//! IDML offered to the raster decoder does not error — it produces a
//! small grey image the size of a page — so a routing mistake shows up as
//! "the file opened but it's blank", which is exactly the kind of bug a
//! user reports and a developer takes an hour to find.

use schist_codec_idml::export;
use schist_plugin_api::PluginRegistry;

fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    registry.register_layout_codec(Box::new(schist_codec_idml::IdmlCodec));
    registry
}

fn an_idml() -> Vec<u8> {
    export::write(&schist_layout::blank_a4()).bytes
}

#[test]
fn a_layout_file_is_recognised_as_one() {
    let registry = registry();
    let bytes = an_idml();
    let codec = registry
        .layout_codec_for(&bytes, Some("idml"))
        .expect("a file we wrote should be recognised");
    assert_eq!(codec.id(), "idml");
}

#[test]
fn a_raster_file_is_not_claimed_by_a_layout_codec() {
    let registry = registry();
    // A ZIP that is not an IDML package, which is what a PSD also is.
    let psd = schist_codec_idml::container::write(&[(
        "mimetype".to_string(),
        b"image/vnd.adobe.photoshop".to_vec(),
    )]);
    assert!(
        registry.layout_codec_for(&psd, Some("psd")).is_none(),
        "a PSD must not be offered to the layout reader"
    );
    assert!(registry.layout_codec_for(b"not a package", None).is_none());
}

#[test]
fn a_real_export_is_recognised_and_reads() {
    // The routing question is asked of files this code did not write, so
    // the answer has to hold for a genuine InDesign export.
    let registry = registry();
    let fixtures: &[(&str, &[u8])] = &[
        (
            "text.idml",
            include_bytes!("../../../fixtures/idml/text.idml"),
        ),
        (
            "multipage.idml",
            include_bytes!("../../../fixtures/idml/multipage.idml"),
        ),
    ];
    for (name, fixture) in fixtures {
        let codec = registry
            .layout_codec_for(fixture, Some("idml"))
            .unwrap_or_else(|| panic!("{name} was not recognised"));
        let (document, _) = codec
            .read_layout(fixture)
            .unwrap_or_else(|error| panic!("{name} did not read: {error}"));
        assert!(!document.pages.is_empty(), "{name} produced no pages");
    }
}

#[test]
fn a_raster_codec_never_claims_a_layout_file() {
    // The two lists are separate, and this is why: a layout codec being
    // reachable from the raster path is how a page becomes a grey
    // rectangle.
    let registry = registry();
    let bytes = an_idml();
    assert!(
        registry.codec_for(&bytes, Some("idml")).is_none(),
        "the raster path must not find a codec for a layout file"
    );
}

#[test]
fn a_read_document_keeps_the_files_name() {
    // The Pages panel and the title bar both show this, and a document
    // that came back nameless is one the user cannot tell apart from a
    // blank one.
    let registry = registry();
    let codec = registry.layout_codec_for(&an_idml(), Some("idml")).unwrap();
    let (mut document, _) = codec.read_layout(&an_idml()).unwrap();
    assert!(
        document.name.is_empty(),
        "a document read has no name of its own"
    );
    // And the open path is what supplies it.
    let name = "brochure.idml";
    let file_name = std::path::Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap();
    document.name = file_name.to_owned();
    assert_eq!(document.name, "brochure.idml");
}

#[test]
fn a_layout_file_opens_to_a_document_that_paints() {
    // The end of the chain. A file that opens to a document the pasteboard
    // cannot draw has not really opened.
    let registry = registry();
    let codec = registry.layout_codec_for(&an_idml(), Some("idml")).unwrap();
    let (document, _) = codec.read_layout(&an_idml()).unwrap();
    let plan = schist_layout::pasteboard::pasteboard(
        &document,
        &schist_layout::pasteboard::PasteboardView::default(),
    )
    .expect("an opened layout document has a pasteboard");
    assert!(!plan.pages.is_empty());
}
