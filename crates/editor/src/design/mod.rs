//! Design Mode: the page layout editor.
//!
//! Design Mode is a second body in the same shell as the photo editor,
//! side by side with the gallery. The raster `Document` stays exactly as
//! it is; a [`schist_layout::LayoutDocument`] sits beside it and the
//! pasteboard paints from that. Nothing in the photo editor's path
//! changes when this is switched on, which is the point: a half-finished
//! page layout feature must not be able to break a working image editor.
//!
//! The separation of concerns is deliberate and strict:
//!
//! * `schist-layout` decides *what* is where -- the pasteboard plan,
//!   composition, styles. It is pure and tested without a window.
//! * `schist-separation` decides *which inks*, and writes prepress files.
//! * this module decides only *how to draw it* and *what the pointer
//!   means*.
//!
//! So the pasteboard renderer takes a plan and paints it, and the
//! selection code takes page-space points and answers questions about the
//! document. Neither of them computes layout, which is why a layout bug
//! can be reproduced in a unit test and a paint bug cannot hide one.

pub mod composition;
pub mod controls;
pub mod dragging;
pub mod graphics;
pub mod guides;
pub mod lifecycle;
pub mod paint;
pub mod pen;
mod plan_cache;
pub mod preflight;
pub mod rulers;
pub mod select;
pub mod story_editor;
pub mod text;
pub mod tools;
mod view;

use schist_layout::{LayoutDocument, ObjectId, PasteboardView};

pub use paint::paint_pasteboard;
pub use select::hit_test;

/// How the pasteboard is laid out.
///
/// InDesign has two, and the plan called it three; the third is not a view
/// mode but the pasteboard itself, which is the surface both modes draw
/// on. Naming it as a third mode would give a menu entry that does
/// nothing.
///
/// The two are not a presentation detail. In spread view a two-page
/// spread is one object a reader reasons about, and its gutter is
/// meaningful. In single-page view the facing page is not visible at all,
/// so a spread's gutter stops being something you can see — which is why
/// the mode is a document-level choice rather than a zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasteboardMode {
    /// Facing pages side by side, every spread on the pasteboard.
    #[default]
    Spread,
    /// One page at a time, filling the canvas.
    SinglePage,
}

impl PasteboardMode {
    pub fn toggled(self) -> PasteboardMode {
        match self {
            PasteboardMode::Spread => PasteboardMode::SinglePage,
            PasteboardMode::SinglePage => PasteboardMode::Spread,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PasteboardMode::Spread => "spread",
            PasteboardMode::SinglePage => "single_page",
        }
    }
}

/// Which body the workspace is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceMode {
    /// The image editor.
    #[default]
    Photo,
    /// The page layout editor.
    Design,
}

impl WorkspaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceMode::Photo => "photo",
            WorkspaceMode::Design => "design",
        }
    }

    /// The other mode, for a toggle.
    pub fn toggled(self) -> WorkspaceMode {
        match self {
            WorkspaceMode::Photo => WorkspaceMode::Design,
            WorkspaceMode::Design => WorkspaceMode::Photo,
        }
    }
}

/// Whether Design Mode can be entered at all.
///
/// The feature ships dark, and the gate lives here rather than at each
/// call site so a command that switches mode cannot be reached another
/// way.
pub fn available() -> bool {
    schist_app_settings::feature_enabled("design-mode")
}

/// A blank A4 layout document.
pub fn blank_document() -> LayoutDocument {
    schist_layout::blank_a4()
}

/// The text of a story, for a caller that is not a tool.
///
/// A thin door into `authoring`, so a panel that shows a frame's text
/// reads it the same way the text tool writes it.
pub fn authoring_text(state: &DesignState, story: schist_layout::StoryId) -> String {
    schist_layout::authoring::text_of(&state.document, story)
}

/// Which tool a click acts with, in Design Mode.
///
/// A page layout editor's tools are not the photo editor's: there is
/// nothing to brush here, and a tool that draws a box has to know whether
/// that box becomes text, a shape or a placed graphic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesignTool {
    /// Click to select, drag to move.
    #[default]
    Select,
    /// Click a shape's own anchor points and drag them.
    DirectSelect,
    /// Click anchors; drag to set cubic handles.
    Pen,
    /// Drag a box to make a text frame.
    TextFrame,
    /// Drag a box to make a rectangle.
    Rectangle,
    /// Drag a box to make an ellipse.
    Ellipse,
    /// Drag a box to make a straight line across it.
    Line,
    /// Drag a box to make a five-sided polygon.
    Polygon,
    /// Click to delete what is under the pointer.
    Delete,
    Hand,
    Zoom,
    Eyedropper,
}

impl DesignTool {
    pub fn as_str(self) -> &'static str {
        match self {
            DesignTool::Select => "select",
            DesignTool::DirectSelect => "direct_select",
            DesignTool::Pen => "pen",
            DesignTool::TextFrame => "text_frame",
            DesignTool::Rectangle => "rectangle",
            DesignTool::Ellipse => "ellipse",
            DesignTool::Line => "line",
            DesignTool::Polygon => "polygon",
            DesignTool::Delete => "delete",
            DesignTool::Hand => "hand",
            DesignTool::Zoom => "zoom",
            DesignTool::Eyedropper => "eyedropper",
        }
    }

    /// Whether this tool makes something on a drag.
    pub fn draws(self) -> bool {
        matches!(
            self,
            DesignTool::TextFrame
                | DesignTool::Rectangle
                | DesignTool::Ellipse
                | DesignTool::Line
                | DesignTool::Polygon
        )
    }

    /// The shape this tool makes, if it makes one.
    ///
    /// The tool knows its own kind; the editor does not have to hold a
    /// second table that could drift from the first.
    pub fn shape_kind(self) -> Option<schist_layout::authoring::ShapeKind> {
        use schist_layout::authoring::ShapeKind;
        match self {
            DesignTool::Rectangle => Some(ShapeKind::Rectangle),
            DesignTool::Ellipse => Some(ShapeKind::Ellipse),
            DesignTool::Line => Some(ShapeKind::Line),
            DesignTool::Polygon => Some(ShapeKind::Polygon { sides: 5 }),
            _ => None,
        }
    }

    /// Whether this tool removes something on a click.
    pub fn removes(self) -> bool {
        matches!(self, DesignTool::Delete)
    }
}

/// A drag that is making something rather than moving it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawing {
    /// Where the drag began, in page space.
    pub from: schist_layout::Point,
    /// Where the pointer is now, in page space.
    pub to: schist_layout::Point,
}

impl Drawing {
    /// The box the drag has drawn so far, always with a positive extent.
    ///
    /// A drag up and to the left has negative extents, and a frame with a
    /// negative width is a frame that cannot be drawn.
    pub fn bounds(&self) -> schist_layout::Rect {
        schist_layout::Rect::new(
            self.from.x.min(self.to.x),
            self.from.y.min(self.to.y),
            (self.to.x - self.from.x).abs(),
            (self.to.y - self.from.y).abs(),
        )
    }
}

/// An anchor point being dragged.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub object: ObjectId,
    /// Which point of the outline this is.
    pub at: schist_layout::authoring::PointRef,
    /// Where the drag began, in page space.
    pub from: schist_layout::Point,
    /// Where the pointer is now, in page space.
    pub to: schist_layout::Point,
    /// The whole object as it was before the drag.
    ///
    /// Recorded up front rather than reconstructed at the end, because a
    /// drag that ends where it began has to record nothing at all and
    /// cannot be recognised from the object alone.
    pub before: schist_layout::ObjectSnapshot,
}

/// A frame being typed into, and where the caret is in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typing {
    pub object: ObjectId,
    pub story: schist_layout::StoryId,
    /// The caret's byte offset into the frame's text.
    pub at: usize,
    pub anchor: usize,
}

/// The workspace state Design Mode keeps.
///
/// Document data stays in LayoutDocument. The view, interaction and undo
/// history belong to this editing session and are reset on opening a file.
pub struct DesignState {
    pub document: LayoutDocument,
    pub lifecycle: lifecycle::Lifecycle,
    pub view: PasteboardView,
    /// Whether the pasteboard shows spreads or one page.
    pub mode: PasteboardMode,
    /// What is selected, front to back.
    pub selection: Vec<ObjectId>,
    /// The page being looked at, or `None` for a spread.
    pub page: Option<usize>,
    pub history: schist_layout::History,
    /// Read-only reports are transient UI state, never undoable edits.
    pub preflight: preflight::PreflightState,
    pub ruler_unit: rulers::RulerUnit,
    pub controls: controls::Controls,
    pub session: std::sync::Arc<()>,
    pub graphics: std::sync::Arc<graphics::Graphics>,
    pub graphics_busy: bool,
    /// The object a drag is moving, and where the drag started.
    pub drag: Option<Drag>,
    pub drag_objects: Vec<(ObjectId, schist_layout::Rect)>,
    pub band: Option<Drawing>,
    pub pan: Option<(schist_layout::Point, schist_layout::Point)>,
    pub guide_drag: Option<guides::GuideDrag>,
    pub show_guides: bool,
    pub snap_guides: bool,
    /// The tool a click acts with.
    pub tool: DesignTool,
    /// Where a drawing drag began, for the tools that make something.
    ///
    /// Separate from `drag`, which is a move: a drawing drag is not yet
    /// an object, and treating it as one would make an abandoned drag
    /// leave a zero-size frame behind.
    pub drawing: Option<Drawing>,
    /// The frame being typed into, and where the caret is.
    pub typing: Option<Typing>,
    pub text_buffer: String,
    pub composition: Option<composition::Composition>,
    pub text_selecting: bool,
    pub thread_source: Option<ObjectId>,
    /// An anchor point being dragged, rather than a whole frame.
    pub anchor: Option<Anchor>,
    pub pen: Option<pen::Pen>,
    /// Set when the pasteboard has to be fitted again, which is after a
    /// mode change: the two modes need different zooms.
    pub needs_refit: bool,
    plan_cache: std::cell::RefCell<plan_cache::PlanCache>,
}

/// An in-progress drag on the pasteboard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub object: ObjectId,
    /// Where the pointer went down, in page space.
    pub from: schist_layout::Point,
    /// Where it is now, in page space.
    pub to: schist_layout::Point,
    /// The object's bounds when the drag began, in page space.
    ///
    /// Recorded rather than reconstructed from the pointer, because the
    /// pointer's last position says nothing about where the object
    /// started: a drag that ends where it began, or that the threshold
    /// swallowed, would otherwise be recorded as a move.
    pub bounds: schist_layout::Rect,
}

impl Default for DesignState {
    fn default() -> Self {
        let document = blank_document();
        let lifecycle = lifecycle::Lifecycle::new(&document);
        DesignState {
            document,
            lifecycle,
            view: PasteboardView::default(),
            mode: PasteboardMode::default(),
            selection: Vec::new(),
            page: None,
            history: schist_layout::History::default(),
            preflight: preflight::PreflightState::default(),
            ruler_unit: rulers::RulerUnit::default(),
            drag: None,
            drag_objects: Vec::new(),
            band: None,
            pan: None,
            guide_drag: None,
            show_guides: true,
            snap_guides: true,
            tool: DesignTool::default(),
            drawing: None,
            typing: None,
            text_buffer: String::new(),
            composition: None,
            text_selecting: false,
            thread_source: None,
            anchor: None,
            pen: None,
            needs_refit: true,
            plan_cache: Default::default(),
            controls: controls::Controls::default(),
            session: std::sync::Arc::new(()),
            graphics: Default::default(),
            graphics_busy: false,
        }
    }
}

impl DesignState {
    pub fn new() -> DesignState {
        DesignState::default()
    }

    /// Whether Design Mode has anything to show.
    ///
    /// A document mid-creation has no pages, and painting a blank
    /// pasteboard would be a lie rather than a placeholder.
    pub fn ready(&self) -> bool {
        !self.document.pages.is_empty()
    }

    /// The view the current mode asks for, without changing what is
    /// stored.
    ///
    /// The mode is applied to a copy rather than written back, because
    /// `view.page` is also where the Pages panel records which page is
    /// current, and overwriting it here would make the two disagree.
    pub fn view_for_mode(&self) -> PasteboardView {
        let mut view = self.view.clone();
        match self.mode {
            // Every spread, so the plan lays the whole document out.
            PasteboardMode::Spread => {
                view.page = None;
                view.spread = None;
            }
            // Just the current page, filled to the canvas.
            PasteboardMode::SinglePage => {
                view.page = Some(self.current_page());
                view.spread = None;
            }
        }
        view
    }

    /// Change mode, keeping the page being looked at.
    pub fn set_mode(&mut self, mode: PasteboardMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // Single page fills the canvas and spread view does not, so the
        // zoom has to be recomputed rather than carried over.
        self.needs_refit = true;
    }

    pub fn cancel_gesture(&mut self) {
        composition::cancel(self);
        dragging::cancel(self);
        self.band = None;
        self.pan = None;
        self.guide_drag = None;
        self.drawing = None;
        self.pen = None;
        if let Some(anchor) = self.anchor.take() {
            let edit = schist_layout::LayoutEdit::ObjectChanged {
                id: anchor.object.0,
                before: anchor.before.clone(),
                after: anchor.before,
            };
            schist_layout::edit::reverse(&mut self.document, &edit);
        }
        self.typing = None;
        self.text_selecting = false;
        self.thread_source = None;
    }

    pub fn undo_or_redo(&mut self, redo: bool) -> bool {
        self.cancel_gesture();
        let pages = self.document.pages.clone();
        let spreads = self.document.spreads.clone();
        let changed = if redo {
            self.history.redo(&mut self.document)
        } else {
            self.history.undo(&mut self.document)
        };
        self.typing = None;
        self.selection
            .retain(|id| self.document.object(*id).is_some());
        let page = self
            .current_page()
            .min(self.document.pages.len().saturating_sub(1));
        self.page = Some(page);
        self.view.page = Some(page);
        self.needs_refit |= pages != self.document.pages || spreads != self.document.spreads;
        changed
    }

    /// The pasteboard plan for the current view.
    ///
    /// `None` while a document is mid-creation, which is the same answer
    /// a paint gives, so a caller can treat "no plan" as "nothing to
    /// show" rather than as an error.
    pub fn plan(&self) -> Option<schist_layout::pasteboard::Pasteboard> {
        if !self.ready() {
            self.plan_cache.borrow_mut().clear();
            return None;
        }
        let mut preview = composition::preview(self);
        if let Some(drag) = &self.guide_drag {
            let document = preview.get_or_insert_with(|| self.document.clone());
            if let Some(page) = document.pages.get_mut(drag.page) {
                if let Some(index) = drag.index {
                    if let Some(guide) = page.guides.get_mut(index) {
                        *guide = drag.guide;
                    }
                } else {
                    page.guides.push(drag.guide);
                }
            }
        }
        let mut plan = self.plan_cache.borrow_mut().get(
            preview.as_ref().unwrap_or(&self.document),
            &self.view_for_mode(),
            schist_text_engine::font_revision(),
        )?;
        if !self.show_guides {
            for page in &mut plan.pages {
                page.guides
                    .retain(|g| g.kind() != schist_layout::pasteboard::GuideKind::Ruler);
            }
        }
        Some(plan)
    }

    /// The objects on the page being looked at, for a hit test.
    pub fn objects(&self) -> Vec<std::borrow::Cow<'_, schist_layout::PlacedObject>> {
        let page = self.current_page();
        self.document.page_objects(page)
    }

    /// The page the pasteboard is showing: the requested one, or the
    /// first.
    pub fn current_page(&self) -> usize {
        self.page.or(self.view.page).unwrap_or(0)
    }

    /// Turn a page-space point into a pasteboard point.
    pub fn to_pasteboard(&self, point: schist_layout::Point) -> schist_layout::Point {
        let origin = self
            .document
            .page_origin(self.current_page())
            .unwrap_or_default();
        self.view.to_pasteboard(schist_layout::Point::new(
            point.x + origin.x,
            point.y + origin.y,
        ))
    }

    /// Turn a pasteboard point into page space.
    pub fn to_page(&self, point: schist_layout::Point) -> schist_layout::Point {
        let origin = self
            .document
            .page_origin(self.current_page())
            .unwrap_or_default();
        let document = self.view.to_page(point);
        schist_layout::Point::new(document.x - origin.x, document.y - origin.y)
    }

    /// The text a story editor would show, for the frame the pointer is
    /// over.
    pub fn story_of(&self, object: ObjectId) -> Option<&schist_layout::Story> {
        let placed = self.document.object(object)?;
        let story = match &placed.object {
            schist_layout::LayoutObject::TextFrame { story, .. } => *story,
            _ => return None,
        };
        self.document.story(story)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_toggles_between_photo_and_design() {
        assert_eq!(WorkspaceMode::default(), WorkspaceMode::Photo);
        assert_eq!(WorkspaceMode::Photo.toggled(), WorkspaceMode::Design);
        assert_eq!(WorkspaceMode::Design.toggled(), WorkspaceMode::Photo);
        assert_eq!(WorkspaceMode::Photo.as_str(), "photo");
        assert_eq!(WorkspaceMode::Design.as_str(), "design");
    }

    #[test]
    fn design_mode_ships_dark() {
        // Authoring and output are still incomplete; availability remains
        // an explicit feature opt-in.
        assert!(!available());
    }

    #[test]
    fn a_new_design_state_has_one_blank_page() {
        let state = DesignState::new();
        assert!(state.ready());
        assert_eq!(state.document.pages.len(), 1);
        assert!(state.selection.is_empty());
        assert!(!state.history.can_undo());
    }

    #[test]
    fn a_document_with_no_pages_is_not_ready() {
        let mut state = DesignState::new();
        state.document.pages.clear();
        assert!(!state.ready());
    }

    #[test]
    fn the_current_page_falls_back_to_the_first() {
        let mut state = DesignState::new();
        state.page = None;
        state.view.page = None;
        assert_eq!(state.current_page(), 0);
        state.page = Some(0);
        assert_eq!(state.current_page(), 0);
    }

    #[test]
    fn spread_view_shows_every_spread() {
        let mut state = DesignState::new();
        for _ in 0..3 {
            state.document.add_page(schist_layout::Page::a4());
        }
        state.mode = PasteboardMode::Spread;
        let plan = state.plan().expect("a plan");
        assert_eq!(
            plan.pages.len(),
            state.document.pages.len(),
            "spread view shows the whole document"
        );
    }

    #[test]
    fn single_page_view_shows_one_page() {
        let mut state = DesignState::new();
        for _ in 0..3 {
            state.document.add_page(schist_layout::Page::a4());
        }
        state.page = Some(2);
        state.mode = PasteboardMode::SinglePage;
        let plan = state.plan().expect("a plan");
        assert_eq!(plan.pages.len(), 1);
        assert_eq!(plan.pages[0].page.page, 2);
    }

    #[test]
    fn the_mode_does_not_overwrite_the_current_page() {
        // `view.page` is where the Pages panel records the current page,
        // so a mode change must not write it or the two disagree.
        let mut state = DesignState::new();
        state.page = Some(0);
        state.set_mode(PasteboardMode::SinglePage);
        assert_eq!(state.page, Some(0));
        assert_eq!(state.view_for_mode().page, Some(0));
        state.set_mode(PasteboardMode::Spread);
        assert_eq!(state.page, Some(0), "the page is still remembered");
        assert_eq!(
            state.view_for_mode().page,
            None,
            "spread view shows them all"
        );
    }

    #[test]
    fn changing_mode_asks_for_a_refit() {
        // The two modes need different zooms, so carrying one over would
        // leave a single page at a spread's zoom.
        let mut state = DesignState::new();
        state.needs_refit = false;
        state.set_mode(PasteboardMode::SinglePage);
        assert!(state.needs_refit);
        // Setting the same mode again is not a change, so no refit.
        state.needs_refit = false;
        state.set_mode(PasteboardMode::SinglePage);
        assert!(!state.needs_refit);
    }

    #[test]
    fn the_pasteboard_mode_toggles() {
        assert_eq!(PasteboardMode::default(), PasteboardMode::Spread);
        assert_eq!(PasteboardMode::Spread.toggled(), PasteboardMode::SinglePage);
        assert_eq!(PasteboardMode::SinglePage.toggled(), PasteboardMode::Spread);
        assert_eq!(PasteboardMode::Spread.as_str(), "spread");
        assert_eq!(PasteboardMode::SinglePage.as_str(), "single_page");
    }

    #[test]
    fn a_state_with_no_pages_has_no_plan() {
        let mut state = DesignState::new();
        assert!(state.plan().is_some());
        state.document.pages.clear();
        assert!(state.plan().is_none());
    }

    #[test]
    fn the_plan_is_in_canvas_points() {
        // The layout crate scales into the canvas, so a caller does not
        // apply a transform of its own and get it wrong.
        let state = DesignState::new();
        let plan = state.plan().expect("a blank page has a plan");
        let trim = plan.pages[0].page.trim;
        let page = &state.document.pages[0];
        assert!(trim.width <= page.width + 1.0);
        assert!((trim.x - state.view.origin.x).abs() < 1e-3);
    }

    #[test]
    fn a_fitted_pasteboard_sits_inside_the_canvas() {
        let doc = blank_document();
        let page = doc.pages[0].clone();
        let view = PasteboardView::fit_page(&page, 800.0, 24.0);
        let plan = schist_layout::pasteboard::pasteboard(&doc, &view).unwrap();
        let trim = plan.pages[0].page.trim;
        // The margin is on both sides, and the page is centred in what is
        // left, so the slack is split evenly.
        assert!(trim.x >= 24.0, "{:?}", trim);
        assert!(trim.right() <= 776.0, "{:?}", trim);
    }

    #[test]
    fn a_story_is_found_through_its_frame() {
        let mut state = DesignState::new();
        let story = state
            .document
            .add_story(schist_layout::Story::from_text("hi", "Body"));
        let id = state.document.add_object(schist_layout::PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: schist_layout::Rect::new(0.0, 0.0, 100.0, 50.0),
            object: schist_layout::LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Body".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        // A text frame has a story and a shape does not.
        assert!(state.story_of(id).is_some());
        let shape = state.document.add_object(schist_layout::PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: schist_layout::Rect::new(0.0, 0.0, 10.0, 10.0),
            object: schist_layout::LayoutObject::Note {
                text: "a note".into(),
                author: "me".into(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Rect".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        assert!(state.story_of(shape).is_none());
        assert!(state.story_of(ObjectId::next()).is_none());
    }

    #[test]
    fn the_objects_are_the_current_pages_own() {
        let mut state = DesignState::new();
        let story = state
            .document
            .add_story(schist_layout::Story::from_text("hi", "Body"));
        state.document.add_object(schist_layout::PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: schist_layout::Rect::new(0.0, 0.0, 100.0, 50.0),
            object: schist_layout::LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Body".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        assert_eq!(state.objects().len(), 1);
        state.page = Some(1);
        assert_eq!(state.objects().len(), 0, "another page has none of them");
    }

    #[test]
    fn the_page_transform_round_trips() {
        let mut state = DesignState::new();
        state.view.origin = schist_layout::Point::new(12.0, 34.0);
        state.view.scale = 1.75;
        let page_space = schist_layout::Point::new(100.0, 200.0);
        let back = state.to_page(state.to_pasteboard(page_space));
        assert!((back.x - page_space.x).abs() < 1e-3);
        assert!((back.y - page_space.y).abs() < 1e-3);
    }
}

pub mod output;
mod output_window;
