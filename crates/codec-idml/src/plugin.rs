//! IDML as a plugin, so the shell can find and call it.
//!
//! The codec itself is a plain library over
//! [`schist_layout::LayoutDocument`] and needs nothing from the app. This
//! is the adapter that makes it reachable: it implements
//! [`LayoutCodecPlugin`], so a file the shell is asked to open is
//! recognised as a layout document and handed to the right reader.
//!
//! Registration is the app shell's job, and is gated on the same
//! `design-mode` feature as the editor: a build with the codec compiled in
//! but Design Mode off should not offer to open a document it has no way
//! to show.

use crate::{container, designmap, export, import};
use schist_layout::LayoutDocument;
use schist_plugin_api::LayoutCodecPlugin;

/// The IDML codec.
pub struct IdmlCodec;

impl LayoutCodecPlugin for IdmlCodec {
    fn id(&self) -> &'static str {
        "idml"
    }

    fn name(&self) -> &'static str {
        "InDesign Markup Language"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["idml"]
    }

    fn probe(&self, bytes: &[u8]) -> bool {
        // An IDML package is a ZIP, so a file that is not one is not this
        // format. The media type inside is the real test, and it is the
        // first part of the package, so this is cheap.
        container::read(bytes)
            .ok()
            .and_then(|package| package.text("mimetype").map(str::trim).map(str::to_owned))
            .is_some_and(|declared| declared == designmap::MIMETYPE)
    }

    fn read_layout(&self, bytes: &[u8]) -> anyhow::Result<(LayoutDocument, Vec<String>)> {
        let imported = import::read(bytes)?;
        Ok((imported.document, imported.report.skipped))
    }

    fn can_export(&self) -> bool {
        true
    }

    fn export_layout(&self, document: &LayoutDocument) -> anyhow::Result<(Vec<u8>, Vec<String>)> {
        let written = export::write(document);
        Ok((written.bytes, written.warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container;

    fn codec() -> IdmlCodec {
        IdmlCodec
    }

    fn written() -> Vec<u8> {
        export::write(&schist_layout::blank_a4()).bytes
    }

    #[test]
    fn it_is_registered_under_the_extension_indesign_uses() {
        let codec = codec();
        assert_eq!(codec.id(), "idml");
        assert_eq!(codec.extensions(), &["idml"]);
        assert!(codec.can_export());
    }

    #[test]
    fn it_recognises_a_real_export_by_its_media_type() {
        // Sniffing is what a File dialog's preview and the open path both
        // use, so it has to work on a file this code did not write.
        let fixtures: &[&[u8]] = &[
            include_bytes!("../../../fixtures/idml/text.idml"),
            include_bytes!("../../../fixtures/idml/multipage.idml"),
        ];
        for fixture in fixtures {
            assert!(codec().probe(fixture), "a real export was not recognised");
        }
    }

    #[test]
    fn it_does_not_recognise_something_else() {
        assert!(!codec().probe(b"not a zip file at all"));
        assert!(!codec().probe(b""));
        // A ZIP with a different declared media type is not IDML.
        let other = container::write(&[(
            "mimetype".to_string(),
            b"image/vnd.adobe.photoshop".to_vec(),
        )]);
        assert!(!codec().probe(&other));
    }

    #[test]
    fn it_reads_and_writes_through_the_trait() {
        let codec = codec();
        let bytes = written();
        assert!(codec.probe(&bytes), "our own output should be recognised");
        let (document, skipped) = codec.read_layout(&bytes).expect("reading our own output");
        assert_eq!(document.pages.len(), 1);
        assert!(
            skipped.is_empty(),
            "a supported blank document drops nothing: {skipped:?}"
        );

        let (again, _) = codec.export_layout(&document).expect("writing");
        let (back, _) = codec.read_layout(&again).expect("reading again");
        assert_eq!(back.pages.len(), document.pages.len());
    }
}
