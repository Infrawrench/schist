//! Reader for InDesign's native `.indd` documents, for the subset the
//! Phase 0 research recovered.
//!
//! INDD has no published specification. Everything here was established
//! from the bytes of public specimen documents and the IDML InDesign
//! exported beside each of them, without Adobe headers, SDKs or
//! decompiled code. `docs/indd-format.md` records the evidence and its
//! limits. A read goes through these layers:
//!
//! 1. [`database`]: the two master pages, the logical page map, the two
//!    B+trees indexing objects by UID, and the records holding the bytes.
//! 2. [`chunk`]: an object's bytes as tagged chunks.
//! 3. [`model`]: the objects the subset understands, which are spreads,
//!    pages, layers, swatches, page items and story text.
//! 4. [`import`]: those objects written as IDML parts and read by
//!    `schist-codec-idml`, so geometry, masters, threads and inks take
//!    the same path into a `LayoutDocument` as an IDML document does.
//!
//! The reader does not write INDD. Page checksums are only half
//! understood and the object payloads are version-specific, so IDML stays
//! the write target. Whatever the subset leaves out is reported, never
//! silently dropped.

pub mod chunk;
pub mod database;
pub mod error;
pub mod import;
pub mod model;
pub mod plugin;
mod synthesis;

pub use error::Error;
pub use plugin::InddCodec;
