//! What can go wrong reading an INDD document.
//!
//! The messages reach the user when a file fails to open, so they go
//! through the catalogs like every other string the app shows.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Neither master page carries the document signature.
    NotIndd,
    /// The master page declares big-endian streams, which no specimen
    /// has, so their layout is unverified.
    BigEndian,
    /// The file ends before the database extent the master page declares.
    Truncated,
    /// A database page is not what the page that points at it expects.
    DamagedPage { page: usize },
    /// An object's bytes could not be assembled from its records.
    DamagedObject { uid: u32 },
    /// The recovered objects could not be read as IDML.
    Idml(schist_codec_idml::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotIndd => f.write_str(schist_i18n::t("design.indd_not_indd")),
            Error::BigEndian => f.write_str(schist_i18n::t("design.indd_big_endian")),
            Error::Truncated => f.write_str(schist_i18n::t("design.indd_truncated")),
            Error::DamagedPage { page } => {
                f.write_str(&schist_i18n::tf!("design.indd_damaged_page", page = page))
            }
            Error::DamagedObject { uid } => {
                f.write_str(&schist_i18n::tf!("design.indd_damaged_object", uid = uid))
            }
            Error::Idml(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<schist_codec_idml::Error> for Error {
    fn from(error: schist_codec_idml::Error) -> Error {
        Error::Idml(error)
    }
}
