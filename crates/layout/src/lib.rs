//! Schist page layout: the document model behind Design Mode.
//!
//! This crate is the counterpart to [`schist_core::Document`]. That one
//! describes a raster image and its layers; this one describes pages,
//! spreads, frames, text and the rules that print production needs. They
//! are deliberately separate. A placed photograph is a *link* from a
//! graphic frame here to a raster document, not a copy of its pixels, so
//! editing the photo never rewrites the layout that references it.
//!
//! Like the rest of the kernel, this crate holds no user-facing features
//! and no UI types. Tools live in `crates/editor/src/design/`; the editor
//! renders it, and `schist-codec-idml` reads and writes it. Production INDD
//! support remains gated on the separate format research spike.
//!
//! # Units
//!
//! Points, 1/72 inch, everywhere. See [`geometry`].

pub mod affine;
pub mod anchored;
pub mod authoring;
pub mod compose;
mod curves;
pub mod decorations;
pub mod directional_features;
pub mod drop_caps;
pub mod edit;
pub mod footnote_composition;
pub mod footnotes;
pub mod frame_text;
pub mod geometry;
pub mod graphics;
pub mod grid;
pub mod history;
pub mod hyphenation;
pub mod ink;
mod inline_controls;
pub mod inline_text;
pub mod language;
pub mod list_composition;
pub mod list_counters;
pub mod list_numbering;
pub mod lists;
pub mod model;
pub mod nested_styles;
pub mod numbering;
pub mod object_styles;
pub mod paragraph_keeps;
pub mod parents;
pub mod pasteboard;
pub mod properties;
pub(crate) mod running_headers;
pub mod story;
pub mod structure;
pub mod styles;
pub mod swatches;
pub mod tabs;
pub mod text_path;
pub mod text_shape;
pub mod text_variables;
pub mod text_wrap;
pub mod threading;

pub use compose::{compose_object, compose_thread, ComposedFrame, ComposedLine, ComposedThread};
pub use edit::{
    snapshot_character_style, snapshot_ink, snapshot_object, snapshot_page, snapshot_settings,
    snapshot_spread, snapshot_story,
};
pub use geometry::{
    inch, mm, pt_from_mm, to_inch, to_mm, BezierHandles, Insets, NumberStyle, Orientation, Page,
    PageBinding, Point, Pt, Rect, ShapePath, Spread, SubPath, MM_PER_INCH, POINTS_PER_INCH,
    PT_PER_MM,
};
pub use grid::{GridMode, GridSet, GridSettings};
pub use history::{
    History, InkSnapshot, LayoutEdit, ObjectSnapshot, PageSnapshot, SettingsSnapshot,
    SpreadSnapshot, StoryPointSnapshot, StorySnapshot, StyleSnapshot,
};
pub use ink::{Ink, InkAlias, InkManager, PaintTints, PlatedInk};
pub use model::{
    blank_a4, FrameOverflow, GraphicFit, GraphicInfo, LayerId, LayoutDocument, LayoutLayer,
    LayoutObject, Link, ObjectId, ParentObject, ParentPage, PlacedObject, StoryId,
};
pub use numbering::Section;
pub use object_styles::{ObjectAppearance, ObjectPaint, ObjectStyle, Paint};
pub use pasteboard::{pasteboard, Display, Guide, PageBox, PagePlan, Pasteboard, PasteboardView};
pub use story::{
    Point as StoryPoint, Story, StoryDirection, StoryOrientation, StoryPreferences, StoryStructure,
    StyleRange,
};
pub use styles::{
    CharacterStyle, ParagraphDirection, ParagraphStyle, ResolvedCharacter, ResolvedParagraph,
    StyleSet, WritingMode,
};
