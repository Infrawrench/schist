//! An INDD document as a [`LayoutDocument`].

use schist_codec_idml::container::Package;
use schist_codec_idml::designmap::DesignPackage;
use schist_layout::LayoutDocument;

use crate::error::Error;

/// A read document and what the reader could not carry into it.
pub struct Imported {
    pub document: LayoutDocument,
    pub skipped: Vec<String>,
}

/// Read an INDD document's recovered subset.
pub fn read(bytes: &[u8]) -> Result<Imported, Error> {
    let database = crate::database::read(bytes)?;
    let model = crate::model::read(&database)?;
    let (parts, written) = crate::synthesis::parts(&model);
    let package = Package::from_parts(parts);
    let opened = DesignPackage::open(&package)?;
    let imported = schist_codec_idml::import::read_package(&opened)?;
    let mut skipped = model.skipped;
    skipped.extend(written);
    skipped.extend(imported.report.skipped);
    Ok(Imported {
        document: imported.document,
        skipped,
    })
}
