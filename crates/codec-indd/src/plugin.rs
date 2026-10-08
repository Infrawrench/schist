//! INDD as a plugin, so the shell can find and call it.
//!
//! Registered beside the IDML codec, behind the same `design-mode`
//! feature. It only reads: an INDD document saved from Schist is saved
//! as IDML.

use schist_layout::LayoutDocument;
use schist_plugin_api::LayoutCodecPlugin;

/// The INDD reader.
pub struct InddCodec;

impl LayoutCodecPlugin for InddCodec {
    fn id(&self) -> &'static str {
        "indd"
    }

    fn name(&self) -> &'static str {
        "InDesign Document"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["indd"]
    }

    fn probe(&self, bytes: &[u8]) -> bool {
        crate::database::probe(bytes)
    }

    fn read_layout(&self, bytes: &[u8]) -> anyhow::Result<(LayoutDocument, Vec<String>)> {
        let imported = crate::import::read(bytes)?;
        Ok((imported.document, imported.skipped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_indd_and_does_not_offer_to_write_it() {
        let codec = InddCodec;
        assert_eq!(codec.extensions(), &["indd"]);
        assert!(!codec.can_export());
        assert!(!codec.probe(b"PK\x03\x04 an IDML package"));
    }
}
