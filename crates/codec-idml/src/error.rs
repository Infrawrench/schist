//! What can go wrong reading or writing an IDML package.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("package container: {0}")]
    Container(#[from] crate::container::ContainerError),
    #[error("{part} is missing from the package")]
    MissingPart { part: String },
    #[error("{part} is not valid XML: {message}")]
    Xml { part: String, message: String },
    #[error("unsupported IDML version {version}")]
    UnsupportedVersion { version: String },
    #[error("{0}")]
    Unsupported(String),
}
