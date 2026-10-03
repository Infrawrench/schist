//! Reader and writer for IDML, InDesign's published interchange format.
//!
//! An IDML package is an OPC/UCF archive: a ZIP holding one XML part per
//! object, with a `designmap.xml` naming the document and the other parts
//! holding the objects it refers to. Unlike INDD, IDML has a published
//! specification, so this is ordinary engineering against a document
//! rather than reverse engineering a binary.
//!
//! The layers, in the order a read goes through them:
//!
//! 1. [`container`] — the ZIP/OPC package, with `mimetype` stored first.
//! 2. [`designmap`] — `designmap.xml` and the parts it names.
//! 3. [`import`] — interpretation into a [`schist_layout::LayoutDocument`].
//!
//! and the write direction mirrors them, with [`export`] producing a
//! package from a `LayoutDocument`.
//!
//! ## Scope
//!
//! This implements the subset the real exports in `fixtures/idml/`
//! exercise: pages and spreads, master pages, text frames and their
//! stories, character styles over story text, placed graphics as links,
//! inks, and the document settings. Anything outside that is reported
//! rather than silently dropped, in both directions.
//! `docs/idml-format.md` records what is verified, and what is only read
//! from the specification.

mod auto_direction;
mod capitalization_codec;
pub mod container;
mod creation_codec;
pub mod designmap;
pub mod error;
pub mod export;
mod footnote_codec;
mod footnote_writer;
mod hyphenation_codec;
pub mod import;
mod keep_codec;
mod language_codec;
mod list_codec;
pub mod plugin;
mod story_codec;
mod stroke_style_codec;
mod structured_story;
mod style_codec;
pub mod xml;

pub use error::Error;
pub use plugin::IdmlCodec;

mod graphic_codec;
mod text_path_codec;
mod thread_codec;

mod color_codec;
mod decoration_codec;
mod object_style_codec;
mod opentype_codec;
mod preferences_codec;

pub mod package;
