//! Print separation: turning a page layout into printing plates.
//!
//! This is the second half of Design Mode's kernel. A layout says "this
//! shape is PANTONE 032 C at 60%, that paragraph is black body copy",
//! and separation answers the question a press operator asks: which
//! inks, at what coverage, on which plates.
//!
//! # Why not the existing spot ink system
//!
//! `schist_core::ink` holds a *painted plate*: a scalar coverage buffer
//! you brush ink into, registered to a raster, round-tripping through
//! PSD as a DisplayInfo alpha channel. That is the right model for
//! retouching a channel.
//!
//! Page layout works on a different axis. An object names an ink,
//! overprint is a per-object property, and the plate is *derived* at
//! output time. So the two coexist: this crate derives plates, then emits
//! them as `schist_core::InkChannel`s, which inherit the existing
//! registration, undo, CRDT and PSD write.
//!
//! # The stages
//!
//! 1. [`plan`] resolves the document's inks and the manager's rules into
//!    an ordered set of plates.
//! 2. [`raster`] turns each object into a coverage mask: shapes from the
//!    vector rasteriser, text from the text engine, placed graphics from
//!    a caller-supplied [`GraphicSource`].
//! 3. [`coverage`] accumulates those masks onto plates, which is where
//!    knockout and overprint differ.
//! 4. The ink manager's under-colour removal, black generation and ink
//!    limit are applied to the process plates.
//! 5. [`SeparatedPage::to_ink_channels`] emits the result.
//!
//! # Knockout and overprint
//!
//! These are properties of the *object*, not of the ink or the plate, and
//! they are the whole of [`InkMode`]. Knockout removes what is beneath
//! and prints; overprint prints on top. A hairline of colour inside a
//! black box knocks a hole in the black, which is why fine detail is set
//! to overprint and body copy is not. Nothing about this is visible on
//! screen, and getting it wrong is the difference between a reprint and
//! a delivery.

pub mod build;
pub mod coverage;
pub mod geometry;
pub mod plan;
pub mod raster;
pub mod report;
pub mod separate;

pub use build::{Build, CmykSource, NaiveBuild, NamedBuilds};
pub use coverage::{Coat, CompositeCoverage, Coverage, InkMode, PlateCoverage, Separation};
pub use geometry::{OutputSettings, PagePixel, DEFAULT_RESOLUTION_DPI};
pub use plan::{Halftone, Plate, PlateKind, PlatePlan, PROCESS};
pub use raster::{
    coats_for, frame_coverage, graphic_coverage, line_coverage, shape_coverage, GraphicPlacement,
    GraphicSource, NoGraphics, PlacedGraphic,
};
pub use report::{Finding, PreflightReport, Severity};
pub mod halftone;
pub mod pdf;
pub mod preview;
pub use halftone::{halftone, halftone_all, trap, trap_all, trap_order, Screen, Trapping};
pub use pdf::{drawn_plates, page_content, Imposition, Marks, PageOutput, Pdf};
pub use preview::{cmyk_to_rgb, PlateSummary, PlateView, SeparationsPreview};
pub use raster::scale_spec;
pub use separate::{
    apply_ink_manager, separate_page, separate_page_built, separate_page_with,
    separate_page_without_graphics, SeparatedPage,
};
