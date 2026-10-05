//! The layout document: pages, spreads, objects, and how they relate.
//!
//! # Two models, on purpose
//!
//! [`schist_core::Document`] is a raster image with layers. This is a
//! document with pages and frames. They are not two views of one thing:
//! a placed photograph is a *link* from a [`LayoutObject::GraphicFrame`]
//! to a raster document, so retouching the photo never rewrites the
//! layout that references it, and deleting a frame never touches the
//! photo. That is the whole reason InDesign does not open a PSD when you
//! place one.
//!
//! # Coordinates
//!
//! An object's `bounds` are in **page space**: relative to its own page's
//! top-left corner, not the spread's. That is deliberate, because a
//! paragraph style with an indent must mean the same thing on page 1 and
//! page 200, and because reordering a spread should move content without
//! rewriting any coordinates. Use [`LayoutDocument::object_rect`] to get
//! spread-space or pasteboard-space geometry for drawing and hit-testing.

use serde::{Deserialize, Serialize};

use crate::geometry::{Insets, Page, Point, Pt, Rect, ShapePath, Spread, SPREAD_GAP};
use crate::grid::GridSet;
use crate::ink::{Ink, InkManager};
use crate::story::Story;
use crate::styles::StyleSet;

/// Identifies an object within a layout document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectId(pub u32);

impl ObjectId {
    pub fn next() -> ObjectId {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(1);
        ObjectId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Identifies a story within a layout document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StoryId(pub u32);

impl StoryId {
    pub fn next() -> StoryId {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(1);
        StoryId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Identifies a z-layer within a layout document.
///
/// These are *not* Photoshop layers. A layout layer is a flat
/// pass-through stack -- "Above Column Text", "Above Masthead" -- that
/// controls what a reader clicks first. It carries no blend mode and no
/// pixels, because everything with pixels is a linked graphic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LayerId(pub u32);

/// What a text frame does with text that does not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FrameOverflow {
    /// Clip at the frame edge. The default, and the safe one.
    #[default]
    Clip,
    /// Push the remainder into the next frame of the thread.
    Thread,
}

/// How a placed graphic is fitted to its frame.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum GraphicFit {
    /// Fill the frame, cropping the overflow. The print default.
    #[default]
    Fill,
    /// Fit entirely inside, leaving empty frame area.
    Contain,
    /// Do not scale; place at 100% and crop.
    Original,
    /// Stretch to the frame, ignoring aspect ratio. Rarely what is meant.
    Stretch,
}

/// Intrinsic raster geometry, independent of its frame's placement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GraphicInfo {
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
}

/// A reference to a linked file, resolved by the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// As the user sees it, so relinking can rewrite it.
    pub path: String,
    #[serde(default)]
    pub info: Option<GraphicInfo>,
    /// Last observed modification time, for missing-link detection.
    pub modified: Option<u64>,
    /// Whether the file was found the last time the document opened.
    pub present: bool,
}

impl Link {
    pub fn new(path: impl Into<String>) -> Link {
        Link {
            path: path.into(),
            info: None,
            modified: None,
            present: true,
        }
    }
}

/// A reusable template applied to pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentPage {
    pub name: String,
    /// Pages whose settings came from this parent.
    pub applied_to: Vec<usize>,
    /// The base parent, for a hierarchy of them.
    pub based_on: Option<usize>,
    /// Native master sheets and their spread origins. Empty legacy templates
    /// use one sheet with the geometry of their first application.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<crate::parents::ParentSheet>,
    /// Per-document-page sheet choice and page-local overlay transform.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<crate::parents::ParentPlacement>,
    /// Objects the parent contributes, in page space.
    pub objects: Vec<ParentObject>,
    pub hidden: bool,
}

/// An object a parent page contributes to the pages it is applied to.
///
/// Overridden objects are recorded so that editing a parent can tell
/// "the user changed this on page 3" from "this still tracks the parent".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentObject {
    /// The object the parent contributes. It carries its own page-space
    /// bounds, so a parent page's object is otherwise identical to one
    /// placed directly -- which is what makes inheriting it a copy
    /// rather than a special case.
    pub object: PlacedObject,
    /// Pages where a local edit has detached this from the parent.
    pub overridden_on: Vec<usize>,
}

impl ParentObject {
    /// Whether this object still tracks the parent on `page`.
    pub fn tracks_parent(&self, page: usize) -> bool {
        !self.overridden_on.contains(&page)
    }
}

/// Anything that can sit on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayoutObject {
    /// A box of flowing text, or one bounded baseline when `text_path` is set.
    TextFrame {
        story: StoryId,
        #[serde(
            default,
            skip_serializing_if = "crate::footnotes::FrameFootnotes::is_empty"
        )]
        footnotes: crate::footnotes::FrameFootnotes,
        /// None inherits the enabled object-style category; native default is false.
        #[serde(default = "crate::frame_text::legacy_balance")]
        balance_columns: Option<bool>,
        /// A single path container shares the story/thread model with boxes.
        /// Columns, gutters and insets apply only to rectangular frames.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_path: Option<crate::text_path::PathText>,
        /// Column count, 1 for a single-column frame.
        columns: u16,
        /// Space between columns.
        gutter: Pt,
        /// Margins inside the frame.
        insets: Insets,
        overflow: FrameOverflow,
    },
    /// A placed or embedded image.
    GraphicFrame {
        link: Link,
        /// Content is embedded in the file rather than linked. Preferred
        /// for anything sent to a printer.
        embedded: bool,
        fit: GraphicFit,
        /// Source region used to calculate fitting, in normalized coordinates.
        /// It can extend beyond 0..=1 to include empty space. Only the frame
        /// clips the image; this rectangle is not a second clipping mask.
        crop: Option<Rect>,
        /// Uniform scale applied on top of the frame size.
        scale: Pt,
        /// Applied after fitting, before the frame clips the image. Coordinates
        /// are normalized to the frame, so resizing preserves the placement.
        #[serde(default)]
        image_transform: schist_core::Affine,
        /// Frame outline in normalized frame coordinates. None is rectangular.
        /// The image transform moves pixels inside this outline, never the outline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip_path: Option<ShapePath>,
    },
    /// A filled and/or stroked path.
    ///
    /// `path`'s points are relative to the object's `bounds` origin, the
    /// same convention a group's children use. Resizing rewrites these
    /// points and handles along with the bounds. Stroke width is in points
    /// and is retained by the size controls.
    Shape {
        path: ShapePath,
        fill: Option<Ink>,
        stroke: Option<Ink>,
        stroke_width: Pt,
        fill_overprint: bool,
        stroke_overprint: bool,
        #[serde(default)]
        tints: crate::PaintTints,
    },
    /// Other frames, for grouping. The child bounds are relative to this
    /// frame's origin, so a group can be moved as one.
    Group { children: Vec<ObjectId> },
    /// An anchored note, for review.
    Note { text: String, author: String },
}

impl LayoutObject {
    /// Geometry editable by Design's anchor tools, excluding synthetic frame paint.
    pub fn editable_path(&self) -> Option<&ShapePath> {
        match self {
            Self::Shape { path, .. } => Some(path),
            Self::TextFrame {
                text_path: Some(path),
                ..
            } => Some(&path.path),
            _ => None,
        }
    }

    pub fn editable_path_mut(&mut self) -> Option<&mut ShapePath> {
        match self {
            Self::Shape { path, .. } => Some(path),
            Self::TextFrame {
                text_path: Some(path),
                ..
            } => Some(&mut path.path),
            _ => None,
        }
    }

    /// The inset content area of a text frame, given its bounds.
    ///
    /// Only text frames have margins. Every other kind of frame's content
    /// fills it edge to edge, which is why this returns the bounds
    /// unchanged for them rather than a zero rect.
    pub fn content_bounds(&self, bounds: Rect) -> Rect {
        match self {
            LayoutObject::TextFrame {
                insets,
                text_path: None,
                ..
            } => bounds.inset(*insets),
            _ => bounds,
        }
    }

    /// Whether this kind of frame can hold text.
    pub fn is_text_frame(&self) -> bool {
        matches!(self, LayoutObject::TextFrame { .. })
    }
}

/// A page-local object with its position.
///
/// Kept separate from [`LayoutObject`] so a parent page can carry the
/// same object without duplicating its position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedObject {
    /// Object visibility is independent of its layer and opacity. Hidden
    /// frames still participate in their story's thread, but paint no artwork.
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub appearance: crate::object_styles::ObjectAppearance,
    pub id: ObjectId,
    /// The page this object sits on, 0-based. A parent page's own
    /// objects are not in `LayoutDocument::objects`; they are in
    /// [`ParentPage::objects`] and reach pages through that parent.
    pub page: usize,
    /// Untransformed composition box in page points.
    pub bounds: Rect,
    /// Affine applied about the bounds origin, before `rotation`. Movement
    /// changes bounds only; resizing changes the composition box.
    #[serde(default)]
    pub transform: schist_core::Affine,
    pub object: LayoutObject,
    /// Rotation about the bounds' centre, in degrees.
    pub rotation: Pt,
    pub name: String,
    pub locked: bool,
    /// Printed without knocking out what is beneath it.
    pub overprint: bool,
    /// Fill opacity, 0..=1. Distinct from overprint, which is about ink
    /// rather than coverage.
    pub transparency: Pt,
}

impl PlacedObject {
    /// Page-space map from the composition box into visible artwork.
    pub fn content_transform(&self) -> schist_core::Affine {
        schist_core::Affine::rotate(self.rotation.to_radians())
            .around(
                self.bounds.x + self.bounds.width * 0.5,
                self.bounds.y + self.bounds.height * 0.5,
            )
            .then(&self.transform.around(self.bounds.x, self.bounds.y))
    }
    pub fn visual_bounds(&self) -> Rect {
        crate::affine::bounds(self.content_transform(), self.bounds)
    }

    /// Conservative painted extent, including joins outside a shape's frame.
    /// Selection continues to use the frame bounds rather than this ink extent.
    pub fn paint_bounds(&self) -> Rect {
        let bounds = match &self.object {
            LayoutObject::Shape {
                path,
                stroke,
                stroke_width,
                ..
            } => {
                let bounds = path.bounds().translated(self.bounds.origin());
                let padding = if stroke.is_some() {
                    stroke_width.max(0.0) * 0.5 * schist_vector::StrokeStyle::default().miter_limit
                } else {
                    0.0
                };
                Rect::new(
                    bounds.x - padding,
                    bounds.y - padding,
                    bounds.width + 2.0 * padding,
                    bounds.height + 2.0 * padding,
                )
            }
            _ => self.bounds,
        };
        let bounds = crate::affine::bounds(self.content_transform(), bounds);
        self.frame_paint(true)
            .map_or(bounds, |stroke| bounds.union(stroke.paint_bounds()))
    }

    /// Move rendered artwork to another page coordinate system without changing
    /// ownership, the composition box, or the baseline grid used by its story.
    pub fn translated_artwork(&self, offset: Point) -> Self {
        let mut placed = self.clone();
        placed.transform = crate::affine::Affine::translate(-self.bounds.x, -self.bounds.y)
            .then(&crate::affine::Affine::translate(offset.x, offset.y))
            .then(&self.content_transform())
            .then(&crate::affine::Affine::translate(
                self.bounds.x,
                self.bounds.y,
            ));
        placed.rotation = 0.0;
        placed
    }
    pub fn contains(&self, point: crate::Point) -> bool {
        self.content_transform()
            .invert()
            .is_some_and(|inverse| self.bounds.contains(crate::affine::point(inverse, point)))
    }
}

/// Properties of a document layer. Order is kept separately in `layers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutLayer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Text frames on this layer ignore other items' text wrap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_wrap: bool,
}

/// A document's dates. Creation and modification come from its metadata or
/// its saves; output is when the current output began, or, outside output,
/// when the document was opened, so an output date never guesses.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentDates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<crate::dates::DateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<crate::dates::DateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<crate::dates::DateTime>,
}

impl DocumentDates {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// A complete page layout document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutDocument {
    /// The document's name, as a user sees it: the file's name, or
    /// something a reader chose.
    ///
    /// Carried here rather than left to the file that holds the document,
    /// because a document that has been opened, renamed and saved needs
    /// to keep its name, and because the title bar and a save dialog both
    /// need it without knowing where the bytes came from.
    #[serde(default)]
    pub name: String,
    pub pages: Vec<Page>,
    pub spreads: Vec<Spread>,
    pub parents: Vec<ParentPage>,
    /// Objects, keyed by id. Their page is in [`PlacedObject`] via
    /// [`LayoutDocument::object_page`].
    pub objects: Vec<PlacedObject>,
    /// Known object creation order, independent of stacking and story indices.
    /// Absence is unknown, never inferred from an imported object's numeric id.
    /// Deleted ids remain so undo can restore their original position; codecs
    /// may omit those absent objects. New authoring records changes in its same
    /// undo transaction. Native imports need explicit ordering evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creation_order: Vec<ObjectId>,
    pub stories: Vec<Story>,
    /// Shared custom text definitions. Names and vector order are not identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_variables: Vec<crate::text_variables::TextVariable>,
    /// Opaque native text-variable definitions retained for recovery. Instances
    /// live at source anchors in Story::structures; shared definitions belong
    /// here once per document, never copied into every instance. The layout
    /// kernel does not parse or evaluate this XML. Codecs also retain malformed
    /// variable metadata as inert XML wrappers rather than silently dropping it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_text_variables: Vec<String>,
    /// Native ChapterNumberPreference. Absent means the application default,
    /// which the public reference renders as chapter 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter_numbering: Option<crate::text_variables::ChapterNumbering>,
    /// Creation, modification and output dates, for date text variables.
    #[serde(default, skip_serializing_if = "DocumentDates::is_empty")]
    pub dates: DocumentDates,
    /// Where the document was opened from or last saved, for file-name text
    /// variables; None before the first save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Document text-wrap composition preferences.
    #[serde(default)]
    pub text_wrap_preferences: crate::text_wrap::WrapPreferences,
    #[serde(
        default,
        skip_serializing_if = "crate::footnotes::FootnoteOptions::is_empty"
    )]
    pub footnotes: crate::footnotes::FootnoteOptions,
    /// Initial local options copied into newly authored rectangular text frames.
    #[serde(
        default,
        skip_serializing_if = "crate::footnotes::FrameFootnotes::is_empty"
    )]
    pub frame_footnote_defaults: crate::footnotes::FrameFootnotes,
    /// Copied into new rectangular frames, never applied retroactively.
    #[serde(default)]
    pub balance_columns_default: bool,
    /// Explicit text flow order, independent of page and layer stacking.
    /// Missing entries use object insertion order for older documents.
    #[serde(default)]
    pub thread_order: Vec<(StoryId, Vec<ObjectId>)>,
    /// Original embedded file bytes, keyed by link path. No raster document
    /// lives here; codecs decode these through the same path as linked files.
    #[serde(default)]
    pub assets: std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    pub styles: StyleSet,
    /// The document's grids, and any named ones.
    pub grids: GridSet,
    pub inks: Vec<Ink>,
    pub ink_manager: InkManager,
    /// Layers, front to back. Index 0 is the topmost.
    pub layers: Vec<LayerId>,
    #[serde(default)]
    pub layer_properties: Vec<LayoutLayer>,
    /// Which layer each object is on, keyed by object id.
    pub object_layers: Vec<(ObjectId, LayerId)>,

    /// Documents printed on both sides of the sheet.
    pub facing_pages: bool,
    #[serde(default)]
    pub page_binding: crate::PageBinding,
    /// The style applied to the document's body copy.
    pub default_paragraph_style: String,
    pub default_character_style: String,
}

impl Default for LayoutDocument {
    fn default() -> Self {
        LayoutDocument::new(vec![Page::a4()])
    }
}

impl LayoutDocument {
    /// Native stroke resources and inline decoration definitions, deduplicated.
    pub fn all_decoration_strokes(&self) -> Vec<crate::decorations::DecorationStroke> {
        let mut strokes = Vec::new();
        for stroke in &self.styles.strokes {
            if !strokes.contains(stroke) {
                strokes.push(stroke.clone());
            }
        }
        for rule in [&self.footnotes.rule, &self.footnotes.continuing_rule] {
            if let Some(stroke) = rule
                .stroke
                .as_ref()
                .and_then(crate::footnotes::FootnoteReference::resolved)
            {
                if !strokes.contains(stroke) {
                    strokes.push(stroke.clone());
                }
            }
        }
        for style in self
            .styles
            .paragraphs
            .iter()
            .flat_map(|s| [&s.underline_style, &s.strike_style])
            .chain(
                self.styles
                    .characters
                    .iter()
                    .flat_map(|s| [&s.underline_style, &s.strike_style]),
            )
        {
            if let Some(stroke) = &style.stroke {
                if !strokes.contains(stroke) {
                    strokes.push(stroke.clone());
                }
            }
        }
        strokes
    }

    /// Resource and inline ink definitions needed by interchange and output.
    /// Shape and character paints need not be registered as named swatches.
    pub fn all_inks(&self) -> Vec<Ink> {
        let mut inks = self.inks.clone();
        let mut add = |ink: &Option<Ink>| {
            if let Some(ink) = ink {
                if !inks.contains(ink) {
                    inks.push(ink.clone());
                }
            }
        };
        for rule in [&self.footnotes.rule, &self.footnotes.continuing_rule] {
            for paint in [&rule.paint, &rule.gap_paint] {
                add(&paint
                    .as_ref()
                    .and_then(crate::footnotes::FootnoteReference::resolved)
                    .cloned());
            }
        }
        for style in &self.styles.paragraphs {
            add(&style.fill);
            add(&style.stroke);
            add(&style
                .underline_style
                .paint
                .as_ref()
                .and_then(crate::decorations::DecorationPaint::ink)
                .cloned());
            add(&style
                .strike_style
                .paint
                .as_ref()
                .and_then(crate::decorations::DecorationPaint::ink)
                .cloned());
            for decoration in [&style.underline_style, &style.strike_style] {
                add(&decoration
                    .gap_paint
                    .as_ref()
                    .and_then(crate::decorations::DecorationPaint::ink)
                    .cloned());
            }
        }
        for style in &self.styles.characters {
            add(&style.fill);
            add(&style.stroke);
            add(&style
                .underline_style
                .paint
                .as_ref()
                .and_then(crate::decorations::DecorationPaint::ink)
                .cloned());
            add(&style
                .strike_style
                .paint
                .as_ref()
                .and_then(crate::decorations::DecorationPaint::ink)
                .cloned());
            for decoration in [&style.underline_style, &style.strike_style] {
                add(&decoration
                    .gap_paint
                    .as_ref()
                    .and_then(crate::decorations::DecorationPaint::ink)
                    .cloned());
            }
        }
        for style in &self.styles.objects {
            add(&style.paint.fill_ink().cloned());
            add(&style.paint.stroke_ink().cloned());
        }
        for object in self.objects.iter().chain(
            self.parents
                .iter()
                .flat_map(|p| p.objects.iter().map(|o| &o.object)),
        ) {
            add(&object.appearance.paint.fill_ink().cloned());
            add(&object.appearance.paint.stroke_ink().cloned());
            if let LayoutObject::Shape { fill, stroke, .. } = &object.object {
                add(fill);
                add(stroke);
            }
        }
        inks
    }

    /// A document with the given pages and everything a blank document
    /// still needs: default styles, a black text ink, and one spread per
    /// page.
    pub fn new(pages: Vec<Page>) -> LayoutDocument {
        let spreads = (0..pages.len()).map(Spread::single).collect();
        let inks = vec![
            Ink::black(),
            Ink::white(),
            Ink::process("Cyan", [0.0, 0.68, 0.94]),
        ];
        LayoutDocument {
            name: String::new(),
            pages,
            spreads,
            parents: Vec::new(),
            objects: Vec::new(),
            creation_order: Vec::new(),
            stories: Vec::new(),
            retained_text_variables: Vec::new(),
            chapter_numbering: None,
            dates: DocumentDates::default(),
            file_path: None,
            text_wrap_preferences: Default::default(),
            text_variables: Vec::new(),
            footnotes: Default::default(),
            frame_footnote_defaults: Default::default(),
            balance_columns_default: false,
            thread_order: Vec::new(),
            assets: Default::default(),
            styles: StyleSet::with_defaults(),
            grids: GridSet::with_defaults(),
            inks,
            ink_manager: InkManager::with_defaults(),
            layers: vec![LayerId(0)],
            layer_properties: Vec::new(),
            object_layers: Vec::new(),
            facing_pages: false,
            page_binding: crate::PageBinding::LeftToRight,
            default_paragraph_style: "Body".into(),
            default_character_style: "Default".into(),
        }
    }

    pub fn object(&self, id: ObjectId) -> Option<&PlacedObject> {
        self.objects.iter().find(|o| o.id == id)
    }

    /// Stories are addressed by their position, which is what makes
    /// [`StoryId`] stable across a session without a side table.
    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(id.0 as usize)
    }

    /// A mutable story, for editing in place.
    ///
    /// Editing through this rather than by hand is what keeps a story's
    /// byte coordinate system honest: the ranges index into it, and
    /// replacing a paragraph without updating them is how a style range
    /// ends up pointing at the wrong characters.
    pub fn story_mut(&mut self, id: StoryId) -> &mut Story {
        &mut self.stories[id.0 as usize]
    }

    pub fn ink(&self, name: &str) -> Option<&Ink> {
        self.inks.iter().find(|i| i.name == name)
    }

    pub fn add_story(&mut self, story: Story) -> StoryId {
        let id = StoryId(self.stories.len() as u32);
        self.stories.push(story);
        id
    }

    pub fn add_object(&mut self, placed: PlacedObject) -> ObjectId {
        let id = placed.id;
        self.creation_order.push(id);
        let layer = self.layers.first().copied().unwrap_or(LayerId(0));
        self.object_layers.push((id, layer));
        self.objects.push(placed);
        id
    }

    /// A known, unambiguous creation position. Missing/duplicate identities
    /// fail rather than guessing chronology from current paint order.
    pub fn creation_rank(&self, id: ObjectId) -> Option<usize> {
        if self.objects.iter().filter(|object| object.id == id).count() != 1 {
            return None;
        }
        let mut positions = self
            .creation_order
            .iter()
            .enumerate()
            .filter_map(|(position, known)| (*known == id).then_some(position));
        let position = positions.next()?;
        positions.next().is_none().then_some(position)
    }

    /// Batch chronology query with the same ambiguity rules as creation_rank.
    /// Composition and export build this call-local index once rather than
    /// scanning every object and chronology entry for each frame.
    pub fn creation_ranks(&self) -> std::collections::BTreeMap<ObjectId, usize> {
        use std::collections::BTreeMap;
        let mut live = BTreeMap::new();
        for object in &self.objects {
            live.entry(object.id)
                .and_modify(|unique| *unique = false)
                .or_insert(true);
        }
        let mut known = BTreeMap::new();
        for (rank, id) in self.creation_order.iter().enumerate() {
            known
                .entry(*id)
                .and_modify(|position| *position = None)
                .or_insert(Some(rank));
        }
        known
            .into_iter()
            .filter_map(|(id, rank)| {
                (live.get(&id) == Some(&true))
                    .then_some(rank)
                    .flatten()
                    .map(|rank| (id, rank))
            })
            .collect()
    }

    /// The page an object sits on, following parent page membership when
    /// the object is not a direct child of one.
    pub fn object_page(&self, id: ObjectId) -> Option<usize> {
        if let Some(page) = self.objects.iter().find(|o| o.id == id).map(|o| o.page) {
            return Some(page);
        }
        // An object a parent page contributes belongs to every page that
        // parent is applied to, so report the first.
        for parent in &self.parents {
            if parent.objects.iter().any(|o| o.object.id == id) {
                return parent.applied_to.first().copied();
            }
        }
        None
    }

    /// Objects owned by a page, including its parent instances, in z-order.
    /// Output uses `page_artwork` to also include artwork crossing the gutter.
    pub fn page_objects(&self, page: usize) -> Vec<std::borrow::Cow<'_, PlacedObject>> {
        let mut out = Vec::new();
        for (index, parent) in self.parents.iter().enumerate() {
            if parent.applied_to.contains(&page) {
                out.extend(
                    crate::parents::inherited(self, index, parent.placement(self, page))
                        .into_iter()
                        .map(std::borrow::Cow::Owned),
                );
            }
        }
        out.extend(
            self.objects
                .iter()
                .filter(|o| o.page == page)
                .map(std::borrow::Cow::Borrowed),
        );
        out.retain(|o| !o.hidden && self.layer_visible(self.object_layer(o.id)));
        // Stable sorting preserves each layer's own object order.
        let order = self.paint_order();
        out.sort_by_key(|o| order[&o.id]);
        for object in &mut out {
            if object.appearance.style.is_some()
                || object.appearance.paint != crate::ObjectPaint::default()
            {
                *object = std::borrow::Cow::Owned(object.resolved_appearance(&self.styles));
            }
        }
        out
    }

    /// Stable back-to-front order across page boundaries. Parent instances stay
    /// below ordinary artwork on the same layer; their hierarchy order is stable.
    pub fn paint_order(
        &self,
    ) -> std::collections::HashMap<ObjectId, (std::cmp::Reverse<usize>, Option<usize>)> {
        // Index once per plan, not once per comparison: large documents should
        // not scan every object and layer for each sorting comparison.
        let layers: std::collections::HashMap<_, _> = self
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| (*layer, std::cmp::Reverse(index)))
            .collect();
        let memberships: std::collections::HashMap<_, _> =
            self.object_layers.iter().rev().copied().collect();
        let key = |id, rank| {
            let layer = memberships
                .get(&id)
                .and_then(|layer| layers.get(layer))
                .copied()
                .unwrap_or(std::cmp::Reverse(0));
            (id, (layer, rank))
        };
        self.parents
            .iter()
            .flat_map(|parent| parent.objects.iter())
            .map(|entry| key(entry.object.id, None))
            .chain(
                self.objects
                    .iter()
                    .enumerate()
                    .map(|(index, object)| key(object.id, Some(index))),
            )
            .collect()
    }

    /// Artwork contributing to a page's output rectangle, in that page's
    /// coordinates. Adjacent pages of the same spread can contribute to trim or
    /// inside bleed. An object's `page` and bounds retain their source meaning:
    /// only its final placement changes, so threaded text is never recomposed in
    /// the destination page's grid. Objects on other spreads never contribute.
    ///
    /// Own-page objects remain present even outside the rectangle, so preflight
    /// still reports their missing resources and invalid transforms.
    pub fn page_artwork(&self, page: usize, clip: Rect) -> Vec<std::borrow::Cow<'_, PlacedObject>> {
        let Some(spread) = self.spread_containing(page) else {
            return self.page_objects(page);
        };
        let destination = spread.pages.iter().position(|p| *p == page).unwrap();
        let origin = spread.page_origin(&self.pages, destination);
        let mut out = Vec::new();
        let mut text_padding = std::collections::HashMap::new();
        for (slot, source) in spread.pages.iter().enumerate() {
            let at = spread.page_origin(&self.pages, slot);
            let offset = Point::new(at.x - origin.x, at.y - origin.y);
            for object in self.page_objects(*source) {
                if *source == page {
                    out.push(object);
                } else {
                    let placed = object.translated_artwork(offset);
                    let bounds = if let LayoutObject::TextFrame {
                        story, text_path, ..
                    } = &placed.object
                    {
                        // Offsets can move ink across the gutter while the frame
                        // stays on its source page. Expand in local axes before
                        // rotation/shear, conservatively covering mixed modes.
                        let mut padding = *text_padding
                            .entry(*story)
                            .or_insert_with(|| self.story_baseline_extent(*story));
                        let mut r = placed.bounds;
                        let has_lists = self.story(*story).is_some_and(|story| {
                            story.points.iter().any(|point| {
                                let crate::StoryPoint::Paragraph { style, .. } = point else {
                                    return false;
                                };
                                matches!(
                                    self.styles.resolve_paragraph(style).list.kind,
                                    Some(
                                        crate::lists::ListKind::Bullet
                                            | crate::lists::ListKind::Numbered
                                    )
                                )
                            })
                        });
                        let has_notes = self
                            .story(*story)
                            .is_some_and(|s| s.structures.iter().any(|s| s.footnote.is_some()));
                        if text_path.is_some() || has_lists || has_notes {
                            let path_padding = padding;
                            // A baseline has no ascent/descent box. Include the
                            // actually rotated glyph outlines before the object
                            // affine, without allocating any coverage bitmaps.
                            if let (Some(flow), Some(story)) = (
                                crate::compose::compose_object(self, &object),
                                self.story(*story),
                            ) {
                                for area in &flow.footnotes {
                                    if let Some(rule) = &area.rule {
                                        r = r.union(rule.bounds);
                                    }
                                }
                                for line in flow.all_lines() {
                                    if text_path.is_none()
                                        && !line.is_generated()
                                        && line.projected.is_none()
                                    {
                                        continue;
                                    }
                                    let metrics = schist_text_engine::measure(
                                        &crate::compose::line_spec(line, story, self),
                                    );
                                    // Decorations bend around baseline corners with a
                                    // four-offset miter bound. Nominal line height also
                                    // covers font-derived automatic offsets/weights,
                                    // including spaces that have no glyph outline.
                                    if let Some(metrics) = &metrics {
                                        padding =
                                            padding.max(4.0 * (path_padding + metrics.height));
                                    }
                                    if let Some([left, top, right, bottom]) =
                                        metrics.and_then(|m| m.ink_bounds)
                                    {
                                        r = r.union(
                                            Rect::new(
                                                left - 1.0,
                                                top - 1.0,
                                                right - left + 2.0,
                                                bottom - top + 2.0,
                                            )
                                            .translated(line.bounds.origin()),
                                        );
                                    }
                                }
                            }
                        }
                        crate::affine::bounds(
                            placed.content_transform(),
                            Rect::new(
                                r.x - padding,
                                r.y - padding,
                                r.width + 2.0 * padding,
                                r.height + 2.0 * padding,
                            ),
                        )
                        .union(placed.paint_bounds())
                    } else {
                        placed.paint_bounds()
                    };
                    if bounds.intersects(clip) {
                        out.push(std::borrow::Cow::Owned(placed));
                    }
                }
            }
        }
        let order = self.paint_order();
        out.sort_by_key(|object| order[&object.id]);
        out
    }

    /// Largest supported ink displacement referenced by this story.
    /// Cached per story by page_artwork. Resolving run styles/font metrics does
    /// not shape text. Includes scripts, explicit offsets and nominal cells
    /// extending before the frame under tight absolute leading.
    fn story_baseline_extent(&self, id: StoryId) -> f32 {
        let Some(story) = self.story(id) else {
            return 0.0;
        };
        story
            .points
            .iter()
            .zip(story.point_offsets())
            .filter_map(|(point, start)| {
                let crate::story::Point::Paragraph { text, style } = point else {
                    return None;
                };
                let spec = crate::compose::spec_for(
                    story,
                    start,
                    start + text.len(),
                    &self.styles,
                    style,
                    &self.default_character_style,
                    0.0,
                );
                let base = schist_text_engine::StyleRun::default();
                Some(
                    spec.runs
                        .iter()
                        .chain(std::iter::once(&base))
                        .map(|run| {
                            let offset = run.baseline_shift.unwrap_or(0.0).abs();
                            let natural = |size| {
                                crate::compose::natural_line_advance(
                                    &crate::styles::ResolvedCharacter {
                                        bold: run.bold.or(Some(spec.bold)),
                                        italic: run.italic.or(Some(spec.italic)),
                                        ..Default::default()
                                    },
                                    size,
                                    run.family.as_deref().unwrap_or(&spec.family),
                                )
                            };
                            // Preferences allow glyphs larger than their nominal em. Reserve
                            // their full natural line height as conservative extra ink room.
                            let growth = run
                                .metric_size
                                .filter(|size| run.size.unwrap_or(spec.size) > *size)
                                .map(|_| natural(run.size.unwrap_or(spec.size)))
                                .unwrap_or(0.0);
                            let caps_growth = if run.capitalization
                                == Some(schist_text_engine::Capitalization::SmallCaps)
                            {
                                run.small_cap_scale
                                    .filter(|scale| {
                                        scale.is_finite() && *scale > 1.0 && *scale <= 2.0
                                    })
                                    .map(|scale| natural(run.size.unwrap_or(spec.size) * scale))
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let tight = run
                                .leading
                                .or(spec.leading)
                                .filter(|v| v.is_finite() && *v >= 0.0)
                                .map(|leading| {
                                    (natural(run.metric_size.or(run.size).unwrap_or(spec.size))
                                        - leading)
                                        .max(0.0)
                                })
                                .unwrap_or(0.0);
                            // Miter limit bounds sharp glyph corners; stroke points
                            // do not change line advance, but may cross the gutter.
                            let stroke = run
                                .stroke
                                .map_or(0.0, schist_text_engine::TextStroke::extent);
                            let decoration =
                                [run.underline_style.clone(), run.strike_style.clone()]
                                    .into_iter()
                                    .enumerate()
                                    .filter_map(|(index, d)| d.map(|d| (index, d)))
                                    .map(|(index, d)| {
                                        let weight = d.weight.unwrap_or_else(|| {
                                            if d.pattern.cap_extension(1.0) > 0.0 {
                                                schist_text_engine::automatic_decoration_weight(
                                                    &spec,
                                                    run.start,
                                                    index == 1,
                                                )
                                            } else {
                                                0.0
                                            }
                                        });
                                        d.offset.unwrap_or(0.0).abs() + weight.max(0.0)
                                    })
                                    .fold(0.0, f32::max);
                            offset + growth + caps_growth + tight + stroke + decoration
                        })
                        .fold(0.0, f32::max),
                )
            })
            .fold(0.0, f32::max)
    }

    pub fn object_layer(&self, object: ObjectId) -> LayerId {
        self.object_layers
            .iter()
            .find(|(id, _)| *id == object)
            .map(|(_, layer)| *layer)
            .unwrap_or_else(|| self.layers.first().copied().unwrap_or(LayerId(0)))
    }

    pub fn layer_ignores_wrap(&self, id: LayerId) -> bool {
        self.layer_properties
            .iter()
            .any(|layer| layer.id == id && layer.ignore_wrap)
    }

    pub fn layer_visible(&self, id: LayerId) -> bool {
        self.layer_properties
            .iter()
            .find(|layer| layer.id == id)
            .is_none_or(|layer| layer.visible)
    }

    pub fn layer_locked(&self, id: LayerId) -> bool {
        self.layer_properties
            .iter()
            .find(|layer| layer.id == id)
            .is_some_and(|layer| layer.locked)
    }

    pub fn object_locked(&self, id: ObjectId) -> bool {
        self.object(id).is_none_or(|object| object.locked)
            || self.layer_locked(self.object_layer(id))
    }

    /// An object's bounds in document space, which is what a canvas view
    /// needs in order to draw it.
    pub fn object_rect(&self, id: ObjectId) -> Option<Rect> {
        let object = self.object(id)?;
        Some(
            object
                .visual_bounds()
                .translated(self.page_origin(object.page)?),
        )
    }

    /// A page's trim origin in document space, using the same computed
    /// spread positions as the pasteboard rather than stored origin hints.
    pub fn page_origin(&self, page: usize) -> Option<Point> {
        self.pages.get(page)?;
        let (index, spread) = self
            .spreads
            .iter()
            .enumerate()
            .find(|(_, spread)| spread.pages.contains(&page))?;
        let position = spread.pages.iter().position(|p| *p == page)?;
        let offset = spread.page_origin(&self.pages, position);
        let origin = self.spread_origins()[index];
        Some(Point::new(origin.x + offset.x, origin.y + offset.y))
    }

    /// The spread a page belongs to.
    pub fn spread_containing(&self, page: usize) -> Option<&Spread> {
        self.spreads.iter().find(|s| s.pages.contains(&page))
    }

    /// The number shown on a page, accounting for the document's start,
    /// style and prefix.
    /// Where each spread sits on the pasteboard, laid out left to right.
    ///
    /// Computed rather than read from [`Spread::origin`], because that
    /// field is a stored hint that nothing maintains: a document built
    /// with `add_page`, or loaded from a file, has it at zero for every
    /// spread, and trusting it draws every spread on top of the first. A
    /// document that looked like half its length on the pasteboard was
    /// the symptom.
    ///
    /// Spreads are separated by [`SPREAD_GAP`] so they read as separate
    /// groups, which is what makes a spread boundary visible at a glance.
    /// The same list positions a spread's pages, so the two cannot
    /// disagree.
    pub fn spread_origins(&self) -> Vec<Point> {
        let mut origins = Vec::with_capacity(self.spreads.len());
        let mut x = 0.0;
        for spread in &self.spreads {
            origins.push(Point::new(x, 0.0));
            x += spread.bounds(&self.pages).width + SPREAD_GAP;
        }
        origins
    }

    /// Add a page to the end, and to the last spread if the document has
    /// one. A spread is a presentation grouping, so appending to the
    /// document means appending inside the existing spread.
    pub fn add_page(&mut self, page: Page) -> usize {
        let index = self.pages.len();
        self.pages.push(page);
        if let Some(last) = self.spreads.last_mut() {
            last.pages.push(index);
        } else {
            self.spreads.push(Spread::single(index));
        }
        index
    }
}

/// A blank document, A4.
pub fn blank_a4() -> LayoutDocument {
    LayoutDocument::new(vec![Page::a4()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{mm, Insets, Point};

    fn text_frame(page: usize, rect: Rect) -> PlacedObject {
        PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: ObjectId::next(),
            page,
            bounds: rect,
            object: LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story: StoryId(0),
                columns: 1,
                gutter: 0.0,
                insets: Insets::ZERO,
                overflow: FrameOverflow::Clip,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Text".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        }
    }

    #[test]
    fn a_new_document_has_one_spread_per_page() {
        let doc = LayoutDocument::new(vec![Page::a4(), Page::a4()]);
        assert_eq!(doc.spreads.len(), 2);
        assert_eq!(doc.spreads[0].pages, vec![0]);
        assert_eq!(doc.spreads[1].pages, vec![1]);
        assert!(doc.styles.paragraph("Body").is_some());
        assert!(doc.ink("Black").is_some());
    }

    #[test]
    fn object_bounds_are_page_relative_and_object_rect_converts() {
        let mut doc = LayoutDocument::new(vec![Page::a4()]);
        let id = doc.add_object(text_frame(
            0,
            Rect::new(mm(20.0), mm(30.0), mm(50.0), mm(10.0)),
        ));
        // The stored bounds are in page space and stay put...
        assert_eq!(doc.object(id).unwrap().bounds.x, mm(20.0));
        // ...and a single-page spread has its origin at zero, so the
        // spread-space rect matches.
        let r = doc.object_rect(id).unwrap();
        assert!((r.x - mm(20.0)).abs() < 0.001);
    }

    #[test]
    fn a_second_page_in_a_spread_is_offset_in_spread_space() {
        let mut doc = LayoutDocument::new(vec![Page::a4()]);
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: None,
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        doc.add_page(Page::a4());
        let id = doc.add_object(text_frame(1, Rect::new(0.0, 0.0, 10.0, 10.0)));
        // Page 1 sits to the right of page 0 in the spread.
        let r = doc.object_rect(id).unwrap();
        assert!((r.x - doc.pages[0].width).abs() < 0.001);
    }

    #[test]
    fn adding_a_page_extends_the_last_spread() {
        let mut doc = LayoutDocument::new(vec![Page::a4()]);
        doc.add_page(Page::a4());
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.spreads.len(), 1);
        assert_eq!(doc.spreads[0].pages, vec![0, 1]);
    }

    #[test]
    fn text_frame_insets_shrink_only_the_content_area() {
        let frame = LayoutObject::TextFrame {
            balance_columns: Some(false),
            footnotes: Default::default(),
            text_path: None,
            story: StoryId(0),
            columns: 1,
            gutter: 0.0,
            insets: Insets::uniform(6.0),
            overflow: FrameOverflow::Clip,
        };
        let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        let content = frame.content_bounds(bounds);
        assert!((content.x - 6.0).abs() < 0.001);
        assert!((content.width - 88.0).abs() < 0.001);
        // A graphic frame has no margins.
        let graphic = LayoutObject::GraphicFrame {
            link: Link::new("/tmp/a.psd"),
            embedded: false,
            fit: GraphicFit::Fill,
            crop: None,
            image_transform: Default::default(),
            clip_path: None,
            scale: 1.0,
        };
        assert_eq!(graphic.content_bounds(bounds), bounds);
        assert!(frame.is_text_frame());
        assert!(!graphic.is_text_frame());
    }

    #[test]
    fn page_numbers_account_for_start_style_and_prefix() {
        let mut doc = LayoutDocument::new(vec![Page::a4(), Page::a4()]);
        assert_eq!(doc.page_number(0), "1");
        doc.pages[0].section = Some(crate::Section {
            start: 5,
            continue_numbering: false,
            ..Default::default()
        });
        assert_eq!(doc.page_number(0), "5");
        assert_eq!(doc.page_number(1), "6");
        doc.pages[0].section.as_mut().unwrap().style = crate::NumberStyle::RomanLower;
        assert_eq!(doc.page_number(0), "v");
        doc.pages[0].section.as_mut().unwrap().prefix = "A-".into();
        doc.pages[0].section.as_mut().unwrap().include_prefix = true;
        assert_eq!(doc.page_number(0), "A-v");
    }

    #[test]
    fn stories_are_addressed_by_position() {
        let mut doc = LayoutDocument::new(vec![Page::a4()]);
        let id = doc.add_story(Story::from_text("Hello", "Default"));
        assert_eq!(id, StoryId(0));
        assert_eq!(doc.story(id).unwrap().text(), "Hello");
        let second = doc.add_story(Story::from_text("Bye", "Default"));
        assert_eq!(second, StoryId(1));
        assert!(doc.story(StoryId(9)).is_none());
    }

    #[test]
    fn a_parent_page_contributes_objects_to_its_pages() {
        let mut doc = LayoutDocument::new(vec![Page::a4(), Page::a4()]);
        let mut parent_object = text_frame(0, Rect::new(0.0, 0.0, 10.0, 10.0));
        parent_object.name = "Masthead".into();
        doc.parents.push(ParentPage {
            name: "A-Master".into(),
            sheets: Vec::new(),
            placements: Vec::new(),
            applied_to: vec![0, 1],
            based_on: None,
            objects: vec![ParentObject {
                object: parent_object.clone(),
                overridden_on: Vec::new(),
            }],
            hidden: false,
        });
        // Both pages see it, and it comes before the page's own objects.
        let on_page_0 = doc.page_objects(0);
        assert_eq!(on_page_0.len(), 1);
        assert_eq!(on_page_0[0].name, "Masthead");
        assert_eq!(doc.page_objects(1).len(), 1);
        // The id resolves to the first applied page.
        assert_eq!(doc.object_page(parent_object.id), Some(0));
    }

    #[test]
    fn a_hidden_parent_contributes_nothing() {
        let mut doc = LayoutDocument::new(vec![Page::a4()]);
        let mut parent_object = text_frame(0, Rect::new(0.0, 0.0, 10.0, 10.0));
        parent_object.name = "Masthead".into();
        doc.parents.push(ParentPage {
            name: "A-Master".into(),
            sheets: Vec::new(),
            placements: Vec::new(),
            applied_to: vec![0],
            based_on: None,
            objects: vec![ParentObject {
                object: parent_object,
                overridden_on: Vec::new(),
            }],
            hidden: true,
        });
        assert!(doc.page_objects(0).is_empty());
    }

    #[test]
    fn an_override_detaches_one_object_on_one_page() {
        let parent_object = ParentObject {
            object: text_frame(0, Rect::new(0.0, 0.0, 10.0, 10.0)),
            overridden_on: vec![1],
        };
        assert!(parent_object.tracks_parent(0));
        assert!(!parent_object.tracks_parent(1));
    }

    #[test]
    fn a_layout_document_round_trips_through_json() {
        let mut doc = blank_a4();
        doc.add_story(Story::from_text("Hello", "Body"));
        doc.add_object(text_frame(0, Rect::new(0.0, 0.0, 100.0, 100.0)));
        let json = serde_json::to_string(&doc).unwrap();
        let back: LayoutDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
    }
}
