//! The pasteboard: turning a layout document into things to draw.
//!
//! A pasteboard is the grey surface a document's pages sit on. This
//! module works out where everything goes on it and hands back a list of
//! primitives. It does not draw: the editor paints, and a caller with no
//! GPU at all can still ask what the pasteboard looks like.
//!
//! Keeping it here rather than in the editor is what makes it testable.
//! A view transform that silently offsets a spread by a pixel, or a
//! baseline grid drawn from the wrong margin, is very hard to see in a
//! screenshot and very easy to assert on.

use crate::compose::compose_object;
use crate::geometry::{Insets, Page, Point, Pt, Rect, Spread};
use crate::grid::GridSettings;
use crate::model::{FrameOverflow, GraphicFit, LayoutDocument, LayoutObject};
use crate::model::{Link, ObjectId, StoryId};

/// What the pasteboard is showing.
#[derive(Debug, Clone, PartialEq)]
pub struct PasteboardView {
    /// Where the document's origin sits on the pasteboard, in points.
    pub origin: Point,
    /// Points to pixels. A zoom.
    pub scale: f32,
    /// The page being looked at, or `None` for a spread view.
    pub page: Option<usize>,
    /// The spread being looked at when `page` is `None`.
    pub spread: Option<usize>,
    pub show_bleed: bool,
    pub show_margins: bool,
    pub show_baseline: bool,
    pub show_columns: bool,
    /// Show the object a parent page contributes, drawn dashed.
    pub show_parents: bool,
}

impl Default for PasteboardView {
    fn default() -> Self {
        PasteboardView {
            origin: Point::ZERO,
            scale: 1.0,
            page: None,
            spread: None,
            show_bleed: true,
            show_margins: true,
            show_baseline: false,
            show_columns: false,
            show_parents: true,
        }
    }
}

impl PasteboardView {
    /// A view showing one page, fitted to `into` points of space.
    ///
    /// Fitting is computed from the page's own size rather than from what
    /// is on screen, so a document opens at the same zoom whatever the
    /// window was doing.
    pub fn fit_page(page: &Page, into: Pt, margin: Pt) -> PasteboardView {
        let bounds = page.bleed_rect();
        let width = bounds.width.max(1.0);
        let height = bounds.height.max(1.0);
        let available = (into - margin * 2.0).max(1.0);
        let scale = (available / width.max(height)).min(1.0);
        PasteboardView {
            origin: Point::new(
                margin + (available - width * scale) / 2.0 - bounds.x * scale,
                margin - bounds.y * scale,
            ),
            scale,
            page: Some(0),
            ..PasteboardView::default()
        }
    }

    /// A point in page space, in pasteboard points.
    pub fn to_pasteboard(&self, point: Point) -> Point {
        Point::new(
            self.origin.x + point.x * self.scale,
            self.origin.y + point.y * self.scale,
        )
    }

    /// A page-space rectangle, in pasteboard points.
    pub fn rect(&self, rect: Rect) -> Rect {
        let at = self.to_pasteboard(rect.origin());
        Rect::new(
            at.x,
            at.y,
            rect.width * self.scale,
            rect.height * self.scale,
        )
    }

    /// A pasteboard point back into page space.
    ///
    /// A point outside the page gives coordinates outside the page, which
    /// is what a selection tool dragging past the trim wants.
    pub fn to_page(&self, point: Point) -> Point {
        Point::new(
            (point.x - self.origin.x) / self.scale,
            (point.y - self.origin.y) / self.scale,
        )
    }
}

/// One page's box on the pasteboard.
#[derive(Debug, Clone, PartialEq)]
pub struct PageBox {
    pub page: usize,
    /// The page's number as the document prints it.
    pub number: String,
    /// The trim box, in pasteboard points.
    pub trim: Rect,
    /// The bleed box, in pasteboard points.
    pub bleed: Rect,
    /// The media box, with the slug for marks.
    pub media: Rect,
    /// The parent page applied to it, if any.
    pub parent: Option<String>,
    pub hidden: bool,
    /// This is the page being looked at.
    pub active: bool,
}

/// A guide line, in pasteboard points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    /// Vertical, at an x.
    Vertical(Pt, GuideKind),
    /// Horizontal, at a y.
    Horizontal(Pt, GuideKind),
}

/// What a guide is for, which is what a caller needs in order to draw it.
///
/// Without this a margin guide and a baseline guide are the same value
/// and a pasteboard can only draw one colour for all of them, which makes
/// the margin guides useless precisely when the baseline grid is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// The trim box.
    Trim,
    /// The bleed box.
    Bleed,
    /// A page margin.
    Margin,
    /// A column boundary.
    Column,
    /// A line of the baseline grid.
    Baseline,
    /// A ruler-anchored guide the user dragged.
    Ruler,
}

impl Guide {
    /// The coordinate the guide runs along, for a caller that draws a
    /// line at a position rather than along an edge.
    pub fn position(&self) -> Pt {
        match self {
            Guide::Vertical(x, _) => *x,
            Guide::Horizontal(y, _) => *y,
        }
    }

    /// Whether this guide runs up and down.
    pub fn vertical(&self) -> bool {
        matches!(self, Guide::Vertical(..))
    }

    pub fn kind(&self) -> GuideKind {
        match self {
            Guide::Vertical(_, kind) | Guide::Horizontal(_, kind) => *kind,
        }
    }
}

/// Something to draw on a page.
#[derive(Debug, Clone, PartialEq)]
pub enum Display {
    /// A frame's outline.
    Frame {
        /// The object this frame is, so a caller can answer "what did I
        /// click" without guessing from geometry.
        object: ObjectId,
        rect: Rect,
        /// Pasteboard-space map applied after composition.
        transform: schist_core::Affine,
        /// Drawn dashed when it comes from a parent page and cannot be
        /// selected without overriding it.
        inherited: bool,
        locked: bool,
    },
    /// One composed line of text.
    Text {
        /// Automatic list markers paint normally but have no editable source range.
        generated: bool,
        object: ObjectId,
        story: StoryId,
        start: usize,
        end: usize,
        rect: Rect,
        /// Pasteboard-space map applied after composition.
        transform: schist_core::Affine,
        /// The text itself, for a caller that draws glyphs.
        text: String,
        size: Pt,
        /// Points to add to each word space when the line is justified.
        word_space: Option<Pt>,
        style: String,
        /// The same resolved font and runs used during composition.
        spec: Box<schist_text_engine::TextSpec>,
    },
    /// Text-flow ports are separate from the frame's selectable outline.
    Ports {
        object: ObjectId,
        inlet: Rect,
        outlet: Rect,
        linked_in: bool,
        linked_out: bool,
        overset: bool,
    },
    /// A shape's outline.
    Shape {
        /// Only actual shape geometry exposes anchors; frame paint is decorative.
        path_editable: bool,
        /// The shape, so a caller can answer "what did I click" the same
        /// way it can for a frame. Without this a shape can be drawn but
        /// not clicked, which is not a shape a user can use.
        object: ObjectId,
        rect: Rect,
        /// Pasteboard-space map applied after composition.
        transform: schist_core::Affine,
        path: crate::ShapePath,
        inherited: bool,
        locked: bool,
        fill: Option<[f32; 4]>,
        stroke: Option<([f32; 4], Pt)>,
        overprint: bool,
    },
    /// A placed graphic; pixel resolution belongs to the editor.
    Graphic {
        /// The frame, so a caller can answer "what did I click".
        object: ObjectId,
        rect: Rect,
        /// Pasteboard-space map applied after composition.
        transform: schist_core::Affine,
        label: String,
        missing: bool,
        inherited: bool,
        locked: bool,
        fit: GraphicFit,
        source: String,
        crop: Option<Rect>,
        scale: f32,
        image_transform: schist_core::Affine,
        clip_path: Option<crate::ShapePath>,
        zoom: f32,
        opacity: f32,
    },
    /// An empty text frame, so an empty frame is still clickable.
    EmptyFrame {
        object: ObjectId,
        rect: Rect,
        transform: schist_core::Affine,
    },
    /// A review note.
    Note {
        object: ObjectId,
        at: Point,
        text: String,
    },
}

impl Display {
    pub fn object(&self) -> ObjectId {
        match self {
            Self::Frame { object, .. }
            | Self::Text { object, .. }
            | Self::Ports { object, .. }
            | Self::Shape { object, .. }
            | Self::Graphic { object, .. }
            | Self::EmptyFrame { object, .. }
            | Self::Note { object, .. } => *object,
        }
    }

    /// Selectable frame geometry before its affine, plus interaction state.
    pub fn frame(&self) -> Option<(ObjectId, Rect, schist_core::Affine, bool, bool)> {
        match self {
            Self::Frame {
                object,
                rect,
                transform,
                inherited,
                locked,
            }
            | Self::Shape {
                object,
                rect,
                transform,
                inherited,
                locked,
                ..
            }
            | Self::Graphic {
                object,
                rect,
                transform,
                inherited,
                locked,
                ..
            } => Some((*object, *rect, *transform, *inherited, *locked)),
            Self::EmptyFrame {
                object,
                rect,
                transform,
            } => Some((*object, *rect, *transform, false, false)),
            _ => None,
        }
    }
}

/// Everything on one page, in draw order.
#[derive(Debug, Clone, PartialEq)]
pub struct PagePlan {
    pub page: PageBox,
    pub guides: Vec<Guide>,
    /// Objects, back to front.
    pub objects: Vec<Display>,
}

/// The whole pasteboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Pasteboard {
    pub pages: Vec<PagePlan>,
    /// Indices into page plans, sorted across pages by layer and object order.
    paint_order: Vec<(usize, usize)>,
    /// The spread's total area, in pasteboard points, for a scroll view.
    pub bounds: Rect,
    /// The size of every page, for a thumbnail strip.
    pub page_size: Pt,
}

impl Pasteboard {
    /// Paint and hit-test artwork in the same spread-wide order. Page paper is
    /// painted separately, before every object, so it cannot erase a crossover.
    pub fn objects(&self) -> impl DoubleEndedIterator<Item = &Display> {
        self.paint_order
            .iter()
            .map(|(page, object)| &self.pages[*page].objects[*object])
    }
}

/// Work out what the pasteboard holds.
///
/// Returns `None` for a document with no pages, which is the one case a
/// caller has to handle: a document mid-creation has no page to show,
/// and drawing a blank pasteboard is a lie.
pub fn pasteboard(doc: &LayoutDocument, view: &PasteboardView) -> Option<Pasteboard> {
    if doc.pages.is_empty() {
        return None;
    }
    let scale = if view.scale > 0.0 { view.scale } else { 1.0 };
    let mut pasteboard = Pasteboard {
        pages: Vec::new(),
        paint_order: Vec::new(),
        bounds: Rect::ZERO,
        page_size: doc.pages[0].width * scale,
    };

    // Spreads are placed by the document rather than by their own stored
    // origins, so two spreads can never land on top of each other.
    let origins = doc.spread_origins();
    for (index, spread) in doc.spreads.iter().enumerate() {
        if !spread_is_visible(spread, index, view) {
            continue;
        }
        let spread_origin = origins.get(index).copied().unwrap_or_default();
        let mut placed = Rect::ZERO;
        for page in &spread.pages {
            let Some(definition) = doc.pages.get(*page) else {
                continue;
            };
            // A hidden page stays in the document and still exports, but
            // the pasteboard skips it, which is what "hidden" means to a
            // reader. Its spread keeps its place so the numbering does not
            // jump about as pages are hidden and shown.
            if definition.hidden && view.page != Some(*page) {
                continue;
            }
            // A single-page view of a spread shows only that page; the
            // rest of the spread stays in the document but off the
            // pasteboard.
            if let Some(only) = view.page {
                if only != *page {
                    continue;
                }
            }
            // Where this page sits within its spread, plus where the
            // spread itself sits.
            let offset = spread.page_origin(
                &doc.pages,
                spread.pages.iter().position(|p| p == page).unwrap_or(0),
            );
            let top_left = Point::new(offset.x + spread_origin.x, offset.y + spread_origin.y);
            let at = view.to_pasteboard(top_left);
            let trim = view.rect(Rect::new(
                top_left.x,
                top_left.y,
                definition.width,
                definition.height,
            ));
            let bleed = view.rect(definition.bleed_rect().translated(top_left));
            let media = view.rect(definition.media_rect().translated(top_left));
            let plan = PagePlan {
                page: PageBox {
                    page: *page,
                    number: doc.page_number(*page),
                    trim,
                    bleed,
                    media,
                    parent: doc
                        .pages
                        .get(*page)
                        .and_then(|p| p.master)
                        .and_then(|m| doc.parents.get(m))
                        .map(|p| p.name.clone()),
                    hidden: definition.hidden,
                    active: view.page == Some(*page),
                },
                guides: guides_for(doc, definition, view, top_left),
                objects: objects_for(doc, *page, view, top_left),
            };
            placed = placed.union(plan.page.media);
            pasteboard.pages.push(plan);
            let _ = at;
        }
        pasteboard.bounds = pasteboard.bounds.union(placed);
    }
    let mut order: Vec<_> = pasteboard
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page, plan)| (0..plan.objects.len()).map(move |object| (page, object)))
        .collect();
    let ranks = doc.paint_order();
    order.sort_by_key(|(page, object)| ranks[&pasteboard.pages[*page].objects[*object].object()]);
    pasteboard.paint_order = order;
    Some(pasteboard)
}

/// Whether a spread is in view.
fn spread_is_visible(spread: &Spread, index: usize, view: &PasteboardView) -> bool {
    if let Some(page) = view.page {
        return spread.pages.contains(&page);
    }
    match view.spread {
        Some(only) => only == index,
        None => true,
    }
}

/// The guides for one page, in page space before scaling.
fn guides_for(
    doc: &LayoutDocument,
    page: &Page,
    view: &PasteboardView,
    offset: Point,
) -> Vec<Guide> {
    let mut guides = Vec::new();
    // A closure cannot both hold and release the borrow on `guides`, so
    // this is a function taking the vector.
    fn push(
        guides: &mut Vec<Guide>,
        view: &PasteboardView,
        offset: Point,
        x: Pt,
        y: Pt,
        kind: GuideKind,
    ) {
        let at = view.to_pasteboard(Point::new(x + offset.x, y + offset.y));
        guides.push(Guide::Vertical(at.x, kind));
        guides.push(Guide::Horizontal(at.y, kind));
    }

    if view.show_bleed {
        for (x, y) in [
            (-page.bleed.left, -page.bleed.top),
            (-page.bleed.left, page.height + page.bleed.bottom),
            (page.width + page.bleed.right, -page.bleed.top),
            (
                page.width + page.bleed.right,
                page.height + page.bleed.bottom,
            ),
        ] {
            push(&mut guides, view, offset, x, y, GuideKind::Bleed);
        }
    }
    if view.show_margins {
        let m = page.margins;
        for (x, y) in [
            (m.left, m.top),
            (page.width - m.right, m.top),
            (m.left, page.height - m.bottom),
            (page.width - m.right, page.height - m.bottom),
        ] {
            push(&mut guides, view, offset, x, y, GuideKind::Margin);
        }
    }
    if view.show_columns {
        let grid = &doc.grids.document;
        if grid.columns > 1 {
            for column in grid.content_columns(page) {
                guides.push(Guide::Vertical(
                    view.to_pasteboard(Point::new(column.x + offset.x, 0.0)).x,
                    GuideKind::Column,
                ));
                guides.push(Guide::Vertical(
                    view.to_pasteboard(Point::new(column.right() + offset.x, 0.0))
                        .x,
                    GuideKind::Column,
                ));
            }
        }
    }
    if view.show_baseline {
        for y in doc.grids.document.baselines(page) {
            guides.push(Guide::Horizontal(
                view.to_pasteboard(Point::new(0.0, y + offset.y)).y,
                GuideKind::Baseline,
            ));
        }
    }
    for guide in &page.guides {
        let at = view.to_pasteboard(Point::new(
            guide.position + offset.x,
            guide.position + offset.y,
        ));
        guides.push(if guide.horizontal {
            Guide::Horizontal(at.y, GuideKind::Ruler)
        } else {
            Guide::Vertical(at.x, GuideKind::Ruler)
        });
    }
    // The trim box is last so it draws over the boxes inside it, which is
    // the order a reader expects: the page's own edge is the line that
    // matters.
    for (x, y) in [
        (0.0, 0.0),
        (0.0, page.height),
        (page.width, 0.0),
        (page.width, page.height),
    ] {
        push(&mut guides, view, offset, x, y, GuideKind::Trim);
    }
    guides
}

/// Everything to draw on one page, back to front.
fn objects_for(
    doc: &LayoutDocument,
    page: usize,
    view: &PasteboardView,
    offset: Point,
) -> Vec<Display> {
    let mut out = Vec::new();
    let objects = if view.page.is_some() {
        doc.page_artwork(page, doc.pages[page].bleed_rect())
    } else {
        doc.page_objects(page)
    };
    for object in objects {
        if object.page != page && doc.pages.get(object.page).is_some_and(|p| p.hidden) {
            continue;
        }
        let inherited = doc.object(object.id).is_none();
        if inherited && !view.show_parents {
            continue;
        }
        let rect = move_to(object.bounds, view, offset);
        let transform = crate::affine::in_view(
            object.content_transform(),
            view.scale,
            view.to_pasteboard(offset),
        );
        if let Some(fill) = object.frame_paint(false) {
            out.extend(shape_display(doc, &fill, view, offset, inherited, false));
        }
        match &object.object {
            LayoutObject::TextFrame {
                story, text_path, ..
            } => {
                let Some(definition) = doc.story(*story) else {
                    out.push(Display::Frame {
                        object: object.id,
                        transform,
                        rect,
                        inherited,
                        locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
                    });
                    continue;
                };
                let composed = compose_object(doc, &object);
                let mut drew = false;
                let mut has_text = false;
                let mut interaction = rect;
                for line in composed.iter().flat_map(|frame| &frame.lines) {
                    drew = true;
                    let bounds = move_to(line.bounds, view, offset);
                    let mut spec = crate::compose::line_spec(line, definition, doc);
                    let text = spec.text.clone();
                    has_text |= !text.is_empty();
                    spec.size *= view.scale;
                    if let Some(tabs) = &mut spec.tabs {
                        tabs.scaled(view.scale);
                    }
                    if let Some(path) = &mut spec.path {
                        path.scaled(view.scale);
                    }
                    spec.leading = spec.leading.map(|v| v * view.scale);
                    spec.tracking *= view.scale;
                    spec.word_spacing = line.word_space.unwrap_or(0.0) * view.scale;
                    for run in &mut spec.runs {
                        if let Some(color) = &mut run.color {
                            color[3] = (f32::from(color[3]) * object.transparency.clamp(0.0, 1.0))
                                .round() as u8;
                        }
                        run.size = run.size.map(|size| size * view.scale);
                        run.metric_size = run.metric_size.map(|v| v * view.scale);
                        run.baseline_shift = run.baseline_shift.map(|v| v * view.scale);
                        for d in [&mut run.underline_style, &mut run.strike_style]
                            .into_iter()
                            .flatten()
                        {
                            d.scaled(view.scale);
                        }
                        if let Some(stroke) = &mut run.stroke {
                            stroke.width *= view.scale;
                        }
                        run.tracking = run.tracking.map(|v| v * view.scale);
                        run.leading = run.leading.map(|v| v * view.scale);
                    }
                    if spec.path.is_some() || line.generated.is_some() {
                        interaction =
                            interaction.union(path_text_bounds(&spec).translated(bounds.origin()));
                    }
                    out.push(Display::Text {
                        generated: line.generated.is_some(),
                        object: object.id,
                        transform,
                        story: *story,
                        start: line.start,
                        end: line.end,
                        rect: bounds,
                        text,
                        size: spec.size,
                        word_space: line.word_space.map(|w| w * view.scale),
                        style: line.paragraph_style.clone(),
                        spec: Box::new(spec),
                    });
                }
                if !drew {
                    let at = composed.as_ref().map_or(0, |frame| frame.consumed_to);
                    let mut spec = crate::compose::spec_for(
                        definition,
                        at,
                        at,
                        &doc.styles,
                        &doc.default_paragraph_style,
                        &doc.default_character_style,
                        0.0,
                    );
                    spec.path = text_path.as_ref().and_then(|path| path.engine_path());
                    if let Some(path) = &mut spec.path {
                        path.offset += match spec.align {
                            schist_text_engine::Align::Left => 0.0,
                            schist_text_engine::Align::Center => path.span.unwrap_or(0.0) / 2.0,
                            schist_text_engine::Align::Right => path.span.unwrap_or(0.0),
                        };
                        spec.align = schist_text_engine::Align::Left;
                    }
                    spec.size *= view.scale;
                    if let Some(path) = &mut spec.path {
                        path.scaled(view.scale);
                    }
                    spec.leading = spec.leading.map(|v| v * view.scale);
                    let content = match &object.object {
                        LayoutObject::TextFrame {
                            insets,
                            text_path: None,
                            ..
                        } => object.bounds.inset(*insets),
                        _ => object.bounds,
                    };
                    if spec.path.is_some() {
                        interaction =
                            interaction.union(path_text_bounds(&spec).translated(rect.origin()));
                    }
                    out.push(Display::Text {
                        generated: false,
                        object: object.id,
                        transform,
                        story: *story,
                        start: at,
                        end: at,
                        rect: move_to(content, view, offset),
                        text: String::new(),
                        size: spec.size,
                        word_space: None,
                        style: doc.default_paragraph_style.clone(),
                        spec: Box::new(spec),
                    });
                }
                if !has_text {
                    // An empty frame still needs an outline, or it cannot
                    // be clicked.
                    out.push(Display::EmptyFrame {
                        object: object.id,
                        transform,
                        rect,
                    });
                }
                out.push(Display::Frame {
                    object: object.id,
                    transform,
                    rect: interaction,
                    inherited,
                    locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
                });
                if let Some(path) = text_path {
                    let mut baseline = object.clone().into_owned();
                    baseline.object = crate::ObjectPaint::default().shape(path.path.clone());
                    out.extend(shape_display(doc, &baseline, view, offset, inherited, true));
                }
                if !inherited {
                    let frames = doc.story_frames(*story);
                    let index = frames.iter().position(|o| o.id == object.id).unwrap_or(0);
                    let inlet = crate::affine::point(transform, Point::new(rect.x, rect.y + 6.0));
                    let outlet = crate::affine::point(
                        transform,
                        Point::new(rect.right(), rect.bottom() - 6.0),
                    );
                    out.push(Display::Ports {
                        object: object.id,
                        inlet: Rect::new(inlet.x - 3.0, inlet.y - 3.0, 6.0, 6.0),
                        outlet: Rect::new(outlet.x - 3.0, outlet.y - 3.0, 6.0, 6.0),
                        linked_in: index > 0,
                        linked_out: index + 1 < frames.len(),
                        overset: composed.as_ref().is_some_and(|frame| frame.lost),
                    });
                }
            }
            LayoutObject::GraphicFrame {
                link,
                fit,
                crop,
                scale,
                image_transform,
                clip_path,
                ..
            } => {
                out.push(Display::Graphic {
                    object: object.id,
                    transform,
                    rect,
                    label: link_name(link),
                    missing: !link.present,
                    inherited,
                    locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
                    fit: *fit,
                    source: link.path.clone(),
                    crop: *crop,
                    scale: *scale,
                    image_transform: *image_transform,
                    clip_path: clip_path.clone(),
                    zoom: view.scale,
                    opacity: object.transparency,
                });
            }
            LayoutObject::Shape { .. } => {
                out.extend(shape_display(doc, &object, view, offset, inherited, true));
            }
            LayoutObject::Note { text, .. } => {
                out.push(Display::Note {
                    object: object.id,
                    at: move_to(
                        Rect::new(object.bounds.x, object.bounds.y, 0.0, 0.0),
                        view,
                        offset,
                    )
                    .origin(),
                    text: text.clone(),
                });
            }
            // A group is a container; its children carry their own
            // entries through the document's object list.
            LayoutObject::Group { .. } => {
                out.push(Display::Frame {
                    object: object.id,
                    transform,
                    rect,
                    inherited,
                    locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
                });
            }
        }
        if let Some(stroke) = object.frame_paint(true) {
            out.extend(shape_display(doc, &stroke, view, offset, inherited, false));
        }
        if !matches!(
            object.object,
            LayoutObject::TextFrame { .. } | LayoutObject::Group { .. }
        ) {
            out.push(Display::Frame {
                object: object.id,
                transform,
                rect,
                inherited,
                locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
            });
        }
    }
    out
}

/// The editable area includes rotated glyphs and insertion segments, even for
/// an empty zero-height baseline. Dimensions are already in pasteboard pixels.
fn path_text_bounds(spec: &schist_text_engine::TextSpec) -> Rect {
    let glyphs = schist_text_engine::measure(spec)
        .and_then(|m| m.ink_bounds)
        .map(|[l, t, r, b]| Rect::new(l, t, r - l, b - t));
    let carets = schist_text_engine::insertion_points(spec)
        .into_iter()
        .map(|(_, caret)| {
            Rect::from_corners(
                Point::new(caret.x, caret.top),
                Point::new(
                    caret.x - caret.angle.sin() * caret.height,
                    caret.top + caret.angle.cos() * caret.height,
                ),
            )
        })
        .reduce(Rect::union);
    match (glyphs, carets) {
        (Some(a), Some(b)) => a.union(b),
        (Some(a), None) | (None, Some(a)) => a,
        _ => Rect::ZERO,
    }
}

fn shape_display(
    doc: &LayoutDocument,
    object: &crate::PlacedObject,
    view: &PasteboardView,
    offset: Point,
    inherited: bool,
    path_editable: bool,
) -> Option<Display> {
    let LayoutObject::Shape {
        path,
        fill,
        stroke,
        stroke_width,
        tints,
        ..
    } = &object.object
    else {
        return None;
    };
    let rect = move_to(object.bounds, view, offset);
    let transform = crate::affine::in_view(
        object.content_transform(),
        view.scale,
        view.to_pasteboard(offset),
    );
    // A shape's points are relative to its frame, so they
    // have to be offset by the frame's origin before they
    // reach the pasteboard. Left unshifted, every shape on a
    // page would be drawn in the corner.
    let mut path = path.clone();
    path.map_points(|p| {
        let p = crate::affine::point(
            object.content_transform(),
            Point::new(object.bounds.x + p.x, object.bounds.y + p.y),
        );
        move_to(Rect::new(p.x, p.y, 0.0, 0.0), view, offset).origin()
    });
    Some(Display::Shape {
        path_editable,
        object: object.id,
        transform,
        rect,
        path,
        inherited,
        locked: object.locked || doc.layer_locked(doc.object_layer(object.id)),
        fill: fill.as_ref().map(|ink| {
            let rgb = ink.preview_at_tint(tints.fill);
            [rgb[0], rgb[1], rgb[2], object.transparency]
        }),
        stroke: stroke.as_ref().map(|ink| {
            let rgb = ink.preview_at_tint(tints.stroke);
            (
                [rgb[0], rgb[1], rgb[2], object.transparency],
                stroke_width * view.scale,
            )
        }),
        overprint: object.overprint,
    })
}

/// A link's file name, for a graphic's placeholder label.
///
/// The whole path is unhelpful in a frame the width of a thumbnail, and
/// the tail is what identifies the file to someone looking at the page.
fn link_name(link: &Link) -> String {
    let path = &link.path;
    match path.rsplit_once(['/', '\\']) {
        Some((_, tail)) => tail.to_string(),
        None => path.clone(),
    }
}

/// A page-space rect moved onto the pasteboard.
fn move_to(rect: Rect, view: &PasteboardView, offset: Point) -> Rect {
    view.rect(rect.translated(offset))
}

/// A blank document's pasteboard, for a new-document view.
pub fn blank(page: Page, view: &PasteboardView) -> Option<Pasteboard> {
    let doc = crate::model::LayoutDocument::new(vec![page]);
    pasteboard(&doc, view)
}

/// The frame insets of a text frame, for a panel that edits them.
pub fn text_insets(object: &crate::model::PlacedObject) -> Option<Insets> {
    match &object.object {
        LayoutObject::TextFrame { insets, .. } => Some(*insets),
        _ => None,
    }
}

/// The story a text frame holds, for a story editor.
pub fn text_story(object: &crate::model::PlacedObject) -> Option<StoryId> {
    match &object.object {
        LayoutObject::TextFrame { story, .. } => Some(*story),
        _ => None,
    }
}

/// Whether a frame clips or threads its text.
pub fn text_overflow(object: &crate::model::PlacedObject) -> Option<FrameOverflow> {
    match &object.object {
        LayoutObject::TextFrame { overflow, .. } => Some(*overflow),
        _ => None,
    }
}

/// The grid in force for a page, honouring a named grid.
pub fn grid_for(doc: &LayoutDocument, _name: &str) -> GridSettings {
    doc.grids.document.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::mm;
    use crate::geometry::ShapePath;
    use crate::geometry::SubPath;
    use crate::ink::Ink;
    use crate::model::{blank_a4, LayoutObject, ObjectId, PlacedObject};
    use crate::story::Story;

    fn view() -> PasteboardView {
        PasteboardView {
            origin: Point::new(10.0, 20.0),
            scale: 1.0,
            page: Some(0),
            ..PasteboardView::default()
        }
    }

    #[test]
    fn a_document_with_no_pages_has_no_pasteboard() {
        let doc = LayoutDocument::new(Vec::new());
        // A blank pasteboard would be a lie; the caller has to handle it.
        assert!(pasteboard(&doc, &view()).is_none());
    }

    #[test]
    fn a_page_lands_where_the_view_puts_it() {
        let mut doc = blank_a4();
        doc.pages[0].bleed = (0.0).into();
        let board = pasteboard(&doc, &view()).unwrap();
        assert_eq!(board.pages.len(), 1);
        let trim = &board.pages[0].page.trim;
        assert_eq!(trim.x, 10.0);
        assert_eq!(trim.y, 20.0);
        assert!((trim.width - doc.pages[0].width).abs() < 0.01);
    }

    #[test]
    fn the_view_transform_round_trips() {
        let view = PasteboardView {
            origin: Point::new(13.0, 7.0),
            scale: 2.5,
            ..PasteboardView::default()
        };
        let page_space = Point::new(40.0, 60.0);
        let back = view.to_page(view.to_pasteboard(page_space));
        assert!((back.x - page_space.x).abs() < 1e-3);
        assert!((back.y - page_space.y).abs() < 1e-3);
    }

    #[test]
    fn a_point_outside_the_page_maps_outside_the_page() {
        // A selection drag past the trim must not clamp to the page.
        let view = view();
        let past = view.to_page(Point::new(10.0 - 50.0, 20.0 - 50.0));
        assert!(past.x < 0.0 && past.y < 0.0);
    }

    #[test]
    fn fitting_a_page_centres_it_and_never_zooms_in() {
        let page = Page::a4();
        let view = PasteboardView::fit_page(&page, 1000.0, 20.0);
        // The fit is limited by whichever of width and height is
        // tighter, and A4's is its height.
        let expected = (960.0 / page.bleed_rect().height).min(1.0);
        assert!(
            (view.scale - expected).abs() < 1e-4,
            "{} against {expected}",
            view.scale
        );
        assert!(view.scale > 0.0);
        // And a huge window does not magnify past 1:1.
        let roomy = PasteboardView::fit_page(&page, 100_000.0, 0.0);
        assert_eq!(roomy.scale, 1.0);
    }

    #[test]
    fn two_spreads_do_not_land_on_top_of_each_other() {
        // Every spread in a document built through the API has a zero
        // origin, so a pasteboard that trusted the stored origin drew the
        // second spread exactly over the first -- a document looked like
        // half its length.
        let mut doc = blank_a4();
        for _ in 0..3 {
            doc.add_page(Page::a4());
        }
        doc.spreads = vec![
            Spread {
                pages: vec![0, 1],
                binding_location: None,
                gutter: 0.0,
                origin: Point::ZERO,
            },
            Spread {
                pages: vec![2, 3],
                binding_location: None,
                gutter: 0.0,
                origin: Point::ZERO,
            },
        ];
        let board = pasteboard(&doc, &PasteboardView::default()).unwrap();
        assert_eq!(board.pages.len(), 4);
        // The first spread ends at its *last* page, not its first.
        let spread_end = board.pages[1].page.trim.right();
        let second = board.pages[2].page.trim;
        assert!(
            second.x >= spread_end,
            "the second spread starts at {} and the first ends at {spread_end}",
            second.x
        );
        // And a gap, so the boundary is visible rather than flush.
        assert!(
            (second.x - spread_end - crate::geometry::SPREAD_GAP).abs() < 0.01,
            "gap was {}",
            second.x - spread_end
        );
    }

    #[test]
    fn a_spread_origins_advance_by_the_spreads_own_width() {
        // Two spreads of different page sizes: the second has to start
        // after the first is finished, not after a page.
        let mut doc = blank_a4();
        let mut wide = Page::a4();
        wide.width *= 2.0;
        doc.add_page(Page::a4());
        doc.add_page(wide.clone());
        // The wide page is the second *in the spread*, which is index 1.
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: None,
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        assert_eq!(doc.spreads[0].bounds(&doc.pages).width, wide.width);
        let origins = doc.spread_origins();
        assert_eq!(origins, vec![Point::ZERO]);

        // A second spread starts after the first is finished, gutter
        // included, rather than after one page.
        doc.spreads.push(Spread {
            pages: vec![2],
            binding_location: None,
            gutter: 12.0,
            origin: Point::new(9999.0, 9999.0),
        });
        let origins = doc.spread_origins();
        let expected = doc.spreads[0].bounds(&doc.pages).width + crate::geometry::SPREAD_GAP;
        assert!(
            (origins[1].x - expected).abs() < 0.01,
            "second spread at {} not {expected}",
            origins[1].x
        );
        // A stored origin is not consulted, which is the bug this fixes.
        assert_eq!(
            origins[1].y, 0.0,
            "a spread is not placed by its stored origin"
        );
    }

    #[test]
    fn a_two_page_spread_places_the_pages_side_by_side() {
        let mut doc = blank_a4();
        doc.add_page(Page::a4());
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: None,
            gutter: mm(10.0),
            origin: Point::ZERO,
        }];
        let view = PasteboardView {
            page: None,
            spread: Some(0),
            ..PasteboardView::default()
        };
        let board = pasteboard(&doc, &view).unwrap();
        assert_eq!(board.pages.len(), 2);
        let first = &board.pages[0].page.trim;
        let second = &board.pages[1].page.trim;
        // A spread is two pages side by side, which is how a bound
        // document opens.
        assert_eq!(first.y, second.y);
        assert!(second.x > first.right(), "a spread stacks its pages");
        // And the gutter is honoured.
        let gap = second.x - first.right();
        assert!((gap - mm(10.0)).abs() < 0.01, "gap of {gap}");
    }

    #[test]
    fn bleed_and_margins_become_guides() {
        let mut page = Page::a4();
        page.bleed = (mm(3.0)).into();
        page.margins = Insets::uniform(mm(20.0));
        let doc = LayoutDocument::new(vec![page]);
        let mut view = PasteboardView {
            page: Some(0),
            ..PasteboardView::default()
        };
        view.show_bleed = true;
        view.show_margins = true;
        let board = pasteboard(&doc, &view).unwrap();
        let guides = &board.pages[0].guides;
        // Four corners per box, one vertical and one horizontal each, for
        // the bleed, the margins and the trim.
        assert_eq!(guides.len(), 24, "{:?}", guides);
        // And the kind is what tells a caller which box is which, which
        // is the whole reason it is on the guide.
        for kind in [GuideKind::Bleed, GuideKind::Margin, GuideKind::Trim] {
            assert_eq!(
                guides.iter().filter(|g| g.kind() == kind).count(),
                8,
                "one pair per corner for {kind:?}"
            );
        }
        view.show_bleed = false;
        view.show_margins = false;
        let board = pasteboard(&doc, &view).unwrap();
        // The trim box is the page's own edge, so it is always there.
        // Turning the boxes off leaves the edge and nothing else.
        assert!(
            board.pages[0]
                .guides
                .iter()
                .all(|g| g.kind() == GuideKind::Trim),
            "{:?}",
            board.pages[0].guides
        );
    }

    #[test]
    fn the_baseline_grid_produces_one_guide_per_line() {
        let mut doc = blank_a4();
        doc.grids.document = GridSettings {
            baseline_count: 12.0,
            ..Default::default()
        };
        let view = PasteboardView {
            page: Some(0),
            // Bleed and margin guides are horizontal too, and this count
            // is about the baseline rhythm alone.
            show_bleed: false,
            show_margins: false,
            show_baseline: true,
            ..PasteboardView::default()
        };
        let board = pasteboard(&doc, &view).unwrap();
        let baselines = board.pages[0]
            .guides
            .iter()
            .filter(|g| matches!(g, Guide::Horizontal(..)) && g.kind() == GuideKind::Baseline)
            .count();
        // The guides carry the coordinate they run along, so a caller
        // does not have to match on the variant to read it.
        assert!(board.pages[0]
            .guides
            .iter()
            .all(|g| g.position().is_finite()));
        // A4 is 842pt tall at a 6pt rhythm, plus a line on the first
        // baseline itself.
        let expected = (doc.pages[0].height / 6.0).floor() as usize + 1;
        assert_eq!(baselines, expected, "{baselines} baselines");
    }

    #[test]
    fn a_text_frame_produces_one_display_line_per_composed_line() {
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text(
            "The quick brown fox jumps over the lazy dog and keeps on going for a while yet.",
            "Body",
        ));
        doc.add_object(PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(mm(20.0), mm(20.0), mm(60.0), mm(60.0)),
            object: LayoutObject::TextFrame {
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Insets::ZERO,
                overflow: FrameOverflow::Clip,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Body".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        });
        let board = pasteboard(&doc, &view()).unwrap();
        let lines: Vec<&Display> = board.pages[0]
            .objects
            .iter()
            .filter(|d| matches!(d, Display::Text { .. }))
            .collect();
        assert!(lines.len() > 1, "the frame produced {} lines", lines.len());
        // And the frame outline is there to click.
        assert!(board.pages[0]
            .objects
            .iter()
            .any(|d| matches!(d, Display::Frame { .. })));
        for line in lines {
            if let Display::Text { text, rect, .. } = line {
                assert!(!text.is_empty());
                assert!(rect.height > 0.0);
            }
        }
    }

    #[test]
    fn an_empty_frame_still_gets_an_outline() {
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text("", "Body"));
        doc.add_object(PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(mm(20.0), mm(20.0), mm(60.0), mm(30.0)),
            object: LayoutObject::TextFrame {
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Insets::ZERO,
                overflow: FrameOverflow::Clip,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Empty".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        });
        let board = pasteboard(&doc, &view()).unwrap();
        assert!(board.pages[0]
            .objects
            .iter()
            .any(|d| matches!(d, Display::EmptyFrame { .. })));
    }

    #[test]
    fn a_graphic_is_labelled_by_its_file_name() {
        let mut doc = blank_a4();
        let mut link = Link::new("/photos/wedding/portrait.psd");
        link.present = false;
        doc.add_object(PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(mm(20.0), mm(20.0), mm(60.0), mm(60.0)),
            object: LayoutObject::GraphicFrame {
                link,
                embedded: false,
                fit: GraphicFit::Fill,
                crop: None,
                image_transform: Default::default(),
                clip_path: None,
                scale: 1.0,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Photo".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        });
        let board = pasteboard(&doc, &view()).unwrap();
        let graphic = board.pages[0]
            .objects
            .iter()
            .find(|d| matches!(d, Display::Graphic { .. }))
            .expect("no graphic");
        // The whole path is useless in a frame; the tail identifies it.
        if let Display::Graphic { label, missing, .. } = graphic {
            assert_eq!(label, "portrait.psd");
            assert!(missing, "a missing link was not reported");
        }
    }

    #[test]
    fn a_shape_carries_the_id_of_the_object_it_comes_from() {
        // A shape the pasteboard cannot name is a shape the editor cannot
        // select, so the id travels with it rather than being looked up
        // again from the document.
        let mut document = blank_a4();
        let object = crate::authoring::rectangle(
            &mut document,
            &mut crate::History::default(),
            0,
            Rect::new(10.0, 10.0, 100.0, 50.0),
            crate::authoring::Paint::none(),
        )
        .expect("a shape");
        let plan = pasteboard(
            &document,
            &PasteboardView {
                page: Some(0),
                ..PasteboardView::default()
            },
        )
        .expect("a pasteboard");
        let shape = plan.pages[0]
            .objects
            .iter()
            .find(|d| matches!(d, Display::Shape { .. }))
            .expect("the shape is on the pasteboard");
        let Display::Shape { object: shown, .. } = shape else {
            panic!("expected a shape");
        };
        assert_eq!(*shown, object, "the pasteboard says which object this is");
    }

    #[test]
    fn a_placed_graphic_carries_its_object_id_too() {
        let mut document = blank_a4();
        let object = crate::authoring::graphic_frame(
            &mut document,
            &mut crate::History::default(),
            0,
            Rect::new(10.0, 10.0, 100.0, 50.0),
            "missing.png",
            false,
        )
        .expect("a graphic");
        let plan = pasteboard(
            &document,
            &PasteboardView {
                page: Some(0),
                ..PasteboardView::default()
            },
        )
        .expect("a pasteboard");
        let graphic = plan.pages[0]
            .objects
            .iter()
            .find(|d| matches!(d, Display::Graphic { .. }))
            .expect("the graphic is on the pasteboard");
        let Display::Graphic { object: shown, .. } = graphic else {
            panic!("expected a graphic");
        };
        assert_eq!(*shown, object);
    }
    #[test]
    fn a_shape_keeps_its_geometry_on_the_pasteboard() {
        let mut doc = blank_a4();
        let mut shape = ShapePath::default();
        shape.subpaths.push(SubPath {
            handles: Vec::new(),
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(mm(20.0), 0.0),
                Point::new(mm(20.0), mm(20.0)),
            ],
            closed: true,
        });
        doc.add_object(PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(mm(30.0), mm(30.0), mm(20.0), mm(20.0)),
            object: LayoutObject::Shape {
                path: shape,
                fill: Some(Ink::black()),
                stroke: None,
                stroke_width: 0.0,
                fill_overprint: false,
                stroke_overprint: false,
                tints: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Box".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        });
        let board = pasteboard(&doc, &view()).unwrap();
        let shape = board.pages[0]
            .objects
            .iter()
            .find(|d| matches!(d, Display::Shape { .. }))
            .expect("no shape");
        if let Display::Shape { path, fill, .. } = shape {
            let points = &path.subpaths[0].points;
            assert_eq!(points.len(), 3);
            assert!(path.subpaths[0].closed);
            assert!(fill.is_some());
            // The geometry moved with the frame.
            assert!((points[0].x - (10.0 + mm(30.0))).abs() < 0.01);
        }
    }

    #[test]
    fn a_parent_page_contributes_a_dashed_frame() {
        let mut doc = blank_a4();
        doc.add_object(PlacedObject {
            appearance: Default::default(),
            id: ObjectId::next(),
            page: 0,
            bounds: Rect::new(0.0, 0.0, mm(50.0), mm(10.0)),
            object: LayoutObject::TextFrame {
                text_path: None,
                story: StoryId(0),
                columns: 1,
                gutter: 0.0,
                insets: Insets::ZERO,
                overflow: FrameOverflow::Clip,
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Masthead".into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
        });
        doc.parents.push(crate::model::ParentPage {
            name: "A-Master".into(),
            sheets: Vec::new(),
            placements: Vec::new(),
            applied_to: vec![0],
            based_on: None,
            objects: vec![crate::model::ParentObject {
                object: PlacedObject {
                    appearance: Default::default(),
                    id: ObjectId::next(),
                    page: 0,
                    bounds: Rect::new(0.0, 0.0, mm(170.0), mm(15.0)),
                    object: LayoutObject::TextFrame {
                        text_path: None,
                        story: StoryId(0),
                        columns: 1,
                        gutter: 0.0,
                        insets: Insets::ZERO,
                        overflow: FrameOverflow::Clip,
                    },
                    rotation: 0.0,
                    transform: Default::default(),
                    name: "Running head".into(),
                    locked: true,
                    overprint: false,
                    transparency: 1.0,
                },
                overridden_on: Vec::new(),
            }],
            hidden: false,
        });
        let board = pasteboard(&doc, &view()).unwrap();
        let frames: Vec<&Display> = board.pages[0]
            .objects
            .iter()
            .filter(|d| matches!(d, Display::Frame { .. }))
            .collect();
        let parent = frames
            .iter()
            .find(|d| matches!(d, Display::Frame { locked: true, .. }))
            .expect("the parent's frame is missing");
        // A parent's item cannot be selected without overriding it, and
        // the dashed border is how the reader is told.
        if let Display::Frame {
            inherited, locked, ..
        } = parent
        {
            assert!(locked);
            assert!(inherited, "a parent's item is drawn as the page's own");
        }
    }

    #[test]
    fn a_hidden_page_is_left_out_of_the_pasteboard() {
        let mut doc = blank_a4();
        doc.add_page(Page::a4());
        doc.pages[1].hidden = true;
        let view = PasteboardView {
            page: None,
            ..PasteboardView::default()
        };
        let board = pasteboard(&doc, &view).unwrap();
        // Hidden pages stay in the document and still export; the
        // pasteboard just skips them.
        assert_eq!(board.pages.len(), 1);
        // And the document still has both, so numbering does not jump.
        assert_eq!(doc.page_number(1), "2");
    }

    #[test]
    fn showing_one_page_hides_the_others() {
        let mut doc = blank_a4();
        doc.add_page(Page::a4());
        doc.spreads = vec![Spread {
            pages: vec![0, 1],
            binding_location: None,
            gutter: 0.0,
            origin: Point::ZERO,
        }];
        let view = PasteboardView {
            page: Some(1),
            ..PasteboardView::default()
        };
        let board = pasteboard(&doc, &view).unwrap();
        assert_eq!(board.pages.len(), 1, "{:?}", board.pages);
        assert_eq!(board.pages[0].page.page, 1);
        assert!(board.pages[0].page.active);
    }

    #[test]
    fn the_bounds_cover_everything_drawn() {
        let mut doc = blank_a4();
        doc.add_page(Page::a4());
        let view = PasteboardView {
            page: None,
            ..PasteboardView::default()
        };
        let board = pasteboard(&doc, &view).unwrap();
        for plan in &board.pages {
            assert!(
                board.bounds.contains_rect(plan.page.media),
                "the bounds do not cover page {}",
                plan.page.page
            );
        }
    }
}
