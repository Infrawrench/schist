//! Page items anchored in text. IDML keeps them inside the story at a
//! character position; their XML is retained for saving, and they are typed
//! here for composition and drawing.
//!
//! An inline item is set in its line as an empty box the width of its visual
//! extent: its bottom sits on the baseline, raised by its Y offset. A tall item
//! raises its line under font-metric or Auto leading; fixed leading keeps its
//! step and the item overlaps the line above. A bounding-box wrap widens the box
//! by its left and right offsets.
//!
//! An item above the line takes room above its anchor's line: its space above,
//! its height and its space below (the Y offset) lower that line. It is aligned
//! in the line's column. An item at a custom position takes no room: one of its
//! nine points is placed at a reference point on the anchor, its line, the
//! column, the frame, the page margins or the page, then offset.
//!
//! The rules and their evidence are in `docs/idml-format.md`.
use crate::{ComposedLine, LayoutDocument, PlacedObject, Pt, Rect, Story};
use serde::{Deserialize, Serialize};

/// AnchoredObjectSetting AnchoredPosition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AnchoredPosition {
    #[default]
    Inline,
    AboveLine,
    Anchored,
}

/// AnchorPoint: the point of the item placed at the reference point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AnchorPoint {
    TopLeft,
    TopCenter,
    TopRight,
    LeftCenter,
    Center,
    RightCenter,
    BottomLeft,
    BottomCenter,
    #[default]
    BottomRight,
}

impl AnchorPoint {
    /// Fractions of the item's width and height from its top left.
    pub fn fractions(self) -> (Pt, Pt) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::LeftCenter => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::RightCenter => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }

    fn mirrored(self) -> Self {
        match self {
            Self::TopLeft => Self::TopRight,
            Self::TopRight => Self::TopLeft,
            Self::LeftCenter => Self::RightCenter,
            Self::RightCenter => Self::LeftCenter,
            Self::BottomLeft => Self::BottomRight,
            Self::BottomRight => Self::BottomLeft,
            other => other,
        }
    }
}

/// HorizontalAlignment: where on the reference the item goes. Text uses the
/// alignment of the anchor's paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HorizontalAlignment {
    #[default]
    Left,
    Center,
    Right,
    Text,
}

impl HorizontalAlignment {
    fn mirrored(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            other => other,
        }
    }
}

/// HorizontalReferencePoint (AnchoredRelativeTo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HorizontalReference {
    #[default]
    TextFrame,
    ColumnEdge,
    PageMargins,
    PageEdge,
    AnchorLocation,
}

/// VerticalAlignment: where on a box reference the item goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VerticalAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

/// VerticalReferencePoint (VerticallyRelativeTo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VerticalReference {
    #[default]
    LineBaseline,
    LineXHeight,
    LineAscent,
    CapHeight,
    TopOfLeading,
    ColumnEdge,
    TextFrame,
    PageMargins,
    PageEdge,
}

impl VerticalReference {
    fn is_line(self) -> bool {
        matches!(
            self,
            Self::LineBaseline
                | Self::LineXHeight
                | Self::LineAscent
                | Self::CapHeight
                | Self::TopOfLeading
        )
    }
}

/// The AnchoredObjectSetting values that place items above the line or at a
/// custom position. Defaults are IDML's (Appendix C).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub anchor_point: AnchorPoint,
    pub horizontal_alignment: HorizontalAlignment,
    pub horizontal_reference: HorizontalReference,
    pub vertical_alignment: VerticalAlignment,
    pub vertical_reference: VerticalReference,
    /// AnchorXoffset in points, away from the side the item is aligned to.
    pub x_offset: Pt,
    /// AnchorSpaceAbove: room above an item above the line.
    pub space_above: Pt,
    /// SpineRelative: mirror left and right on left-hand pages.
    pub spine_relative: bool,
    /// PinPosition: keep a line-relative item within its frame's top and
    /// bottom.
    pub pin_position: bool,
    /// LockPosition: retained; it only affects manual positioning.
    pub lock_position: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            anchor_point: AnchorPoint::default(),
            horizontal_alignment: HorizontalAlignment::default(),
            horizontal_reference: HorizontalReference::default(),
            vertical_alignment: VerticalAlignment::default(),
            vertical_reference: VerticalReference::default(),
            x_offset: 0.0,
            space_above: 0.0,
            spine_relative: false,
            pin_position: true,
            lock_position: false,
        }
    }
}

/// A typed anchored item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchoredItem {
    pub position: AnchoredPosition,
    /// AnchorYoffset in points. An inline item rises by this distance; an
    /// item above the line keeps it below itself; a custom item moves down.
    pub y_offset: Pt,
    #[serde(default)]
    pub placement: Placement,
    /// The item as drawn, with its bounds at the origin and its own affine.
    pub object: PlacedObject,
}

/// The room an item takes in its line, for the projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    pub width: Pt,
    pub ascent: Pt,
    pub descent: Pt,
    /// Room above the line, for an item above it.
    pub above: Pt,
}

impl AnchoredItem {
    /// An inline item with IDML's default settings.
    pub fn inline(object: PlacedObject) -> Self {
        Self {
            position: AnchoredPosition::Inline,
            y_offset: 0.0,
            placement: Placement::default(),
            object,
        }
    }

    /// The item's extent as InDesign's visual bounds: its frame grown by half
    /// its stroke, through its own affine. A public native sample sets a
    /// 60 × 36 pt frame stroked 0.5 pt as 60.5 × 36.5 pt.
    pub fn extent(&self) -> Rect {
        let half = self.stroke_weight() * 0.5;
        let b = self.object.bounds;
        let grown = Rect::new(
            b.x - half,
            b.y - half,
            b.width + 2.0 * half,
            b.height + 2.0 * half,
        );
        crate::affine::bounds(self.object.content_transform(), grown)
    }

    /// The item's own stroke weight, or zero when it has no stroke.
    fn stroke_weight(&self) -> Pt {
        let weight = match &self.object.object {
            crate::LayoutObject::Shape {
                stroke: Some(_),
                stroke_width,
                ..
            } => *stroke_width,
            crate::LayoutObject::Shape { .. } => 0.0,
            _ => {
                let paint = &self.object.appearance.paint;
                paint
                    .stroke_ink()
                    .map_or(0.0, |_| paint.stroke_width.unwrap_or(0.0))
            }
        };
        if weight.is_finite() {
            weight.max(0.0)
        } else {
            0.0
        }
    }

    /// The room the item takes in its line, or None when it cannot be set.
    pub fn line_box(&self) -> Option<LineBox> {
        let e = self.extent();
        let p = &self.placement;
        let values = [
            e.x,
            e.y,
            e.width,
            e.height,
            self.y_offset,
            p.x_offset,
            p.space_above,
        ];
        if !values.iter().all(|v| v.is_finite()) || e.width < 0.0 || e.height < 0.0 {
            return None;
        }
        Some(match self.position {
            AnchoredPosition::Inline => {
                let (left, right) = self.wrap_sides();
                LineBox {
                    width: (e.width + left + right).max(0.0),
                    ascent: (e.height + self.y_offset).max(0.0),
                    descent: (-self.y_offset).max(0.0),
                    above: 0.0,
                }
            }
            AnchoredPosition::AboveLine => LineBox {
                width: 0.0,
                ascent: 0.0,
                descent: 0.0,
                above: (p.space_above + e.height + self.y_offset).max(0.0),
            },
            AnchoredPosition::Anchored => LineBox {
                width: 0.0,
                ascent: 0.0,
                descent: 0.0,
                above: 0.0,
            },
        })
    }

    /// The item with its extent's top left at (`left`, `top`).
    fn moved(&self, left: Pt, top: Pt) -> PlacedObject {
        let e = self.extent();
        let mut object = self.object.clone();
        object.bounds.x += left - e.x;
        object.bounds.y += top - e.y;
        object
    }

    /// An inline item with its extent's left edge at `left` (after any wrap
    /// offset) and its bottom on the baseline, raised by the Y offset.
    /// Coordinates are those of the line.
    pub fn placed(&self, left: Pt, baseline: Pt) -> PlacedObject {
        let e = self.extent();
        self.moved(
            left + self.wrap_sides().0,
            baseline - self.y_offset - e.height,
        )
    }

    /// Room a bounding-box wrap keeps beside an inline item. InDesign's
    /// export of a public sample moves the item right by its left offset and
    /// the following text by both; the top and bottom offsets change nothing.
    fn wrap_sides(&self) -> (Pt, Pt) {
        match &self.object.appearance.text_wrap {
            Some(wrap) if wrap.mode == crate::text_wrap::WrapMode::BoundingBox => {
                let finite = |v: Pt| if v.is_finite() { v } else { 0.0 };
                (finite(wrap.offsets.left), finite(wrap.offsets.right))
            }
            _ => (0.0, 0.0),
        }
    }

    /// Text wrap the item asks for that composition does not apply: any wrap
    /// of an item at a custom position, and an inline item's wrap other than
    /// a bounding box's side offsets.
    pub fn wrap_unapplied(&self) -> bool {
        let Some(wrap) = &self.object.appearance.text_wrap else {
            return false;
        };
        match self.position {
            _ if wrap.mode == crate::text_wrap::WrapMode::None => false,
            AnchoredPosition::Inline => wrap.mode != crate::text_wrap::WrapMode::BoundingBox,
            AnchoredPosition::AboveLine => false,
            AnchoredPosition::Anchored => true,
        }
    }
}

/// `object`, drawn through `frame`'s affine: frame space becomes page space.
/// The result keeps the item's geometry; its affine is the linear part of
/// frame∘item and its origin is where that map sends the item's origin.
pub fn through_frame(object: &PlacedObject, frame: &PlacedObject) -> PlacedObject {
    let item = object.content_transform();
    let map = frame.content_transform().then(&item);
    let origin = object.bounds.origin();
    let (x, y) = map.apply(origin.x, origin.y);
    let mut out = object.clone();
    out.rotation = 0.0;
    out.transform = crate::affine::Affine {
        a: map.a,
        b: map.b,
        c: map.c,
        d: map.d,
        tx: 0.0,
        ty: 0.0,
    };
    out.bounds.x = x;
    out.bounds.y = y;
    out.page = frame.page;
    out
}

/// An item waiting to be projected into its story's display text.
pub(crate) struct Instance {
    pub structure: usize,
    pub at: usize,
    pub character: crate::ResolvedCharacter,
    pub line_box: LineBox,
}

/// Items of `story` that composition sets: typed, anchored at a known
/// position in horizontal text.
pub(crate) fn instances(doc: &LayoutDocument, story: &Story) -> Vec<Instance> {
    let offsets = story.point_offsets();
    story
        .structures
        .iter()
        .enumerate()
        .filter_map(|(index, structure)| {
            let item = structure.anchored.as_ref()?;
            let at = structure.at?;
            let line_box = item.line_box()?;
            let (point, _) = story.points.iter().zip(&offsets).find(|(point, start)| {
                matches!(point, crate::StoryPoint::Paragraph { .. })
                    && **start <= at
                    && at <= **start + point.text().len()
            })?;
            let crate::StoryPoint::Paragraph { style, .. } = point else {
                return None;
            };
            let paragraph = doc.styles.resolve_paragraph(if style.is_empty() {
                &doc.default_paragraph_style
            } else {
                style
            });
            if paragraph
                .writing_mode
                .is_some_and(|mode| mode != crate::WritingMode::Horizontal)
            {
                return None;
            }
            let run = story
                .ranges
                .iter()
                .rev()
                .find(|r| r.start < at && at <= r.end)
                .map_or("", |r| r.style.as_str());
            let mut character = crate::text_variables::instance_character(doc, story, at, run)?;
            // Auto leading resolves to points before the engine sees the
            // line, so it never meets the box. InDesign's export of a public
            // sample steps the line by the item's height above the baseline
            // plus the text's own extra leading (36.5 + 14.4 − 12 pt), never
            // less than the text's Auto leading.
            if item.position == AnchoredPosition::Inline
                && character.leading == Some(crate::styles::Leading::Auto)
            {
                let size = character.point_size.unwrap_or(11.0);
                character.leading = crate::styles::Leading::Auto
                    .points(size, paragraph.auto_leading)
                    .map(|text| {
                        crate::styles::Leading::Points(text.max(line_box.ascent + text - size))
                    });
            }
            Some(Instance {
                structure: index,
                at,
                character,
                line_box,
            })
        })
        .collect()
}

/// Whether any item set in `lines` asks for text wrap composition does not
/// apply, for Preflight.
pub(crate) fn wrap_unapplied<'a>(
    story: &Story,
    lines: impl IntoIterator<Item = &'a ComposedLine>,
) -> bool {
    lines
        .into_iter()
        .filter_map(|line| line.projected.as_ref())
        .flat_map(|projected| &projected.anchored)
        .filter_map(|(_, structure)| story.structures.get(*structure)?.anchored.as_ref())
        .any(|item| item.wrap_unapplied())
}

/// Frame-space rectangles an item can be placed against.
struct References {
    frame: Rect,
    column: Rect,
    margins: Option<Rect>,
    page: Option<Rect>,
    /// The page is left of the binding spine.
    left_page: bool,
}

impl References {
    fn new(doc: &LayoutDocument, frame: &PlacedObject, line: &ComposedLine) -> Self {
        let (content, count, gutter) = match &frame.object {
            crate::LayoutObject::TextFrame {
                columns,
                gutter,
                insets,
                ..
            } => (frame.bounds.inset(*insets), *columns, *gutter),
            _ => (frame.bounds, 1, 0.0),
        };
        let middle = line.bounds.x + line.bounds.width / 2.0;
        let column = crate::compose::columns(content, count, gutter)
            .into_iter()
            .find(|c| middle >= c.x && middle <= c.right())
            .unwrap_or(content);
        // Page rectangles in the frame's own space, through its inverse
        // affine. They are exact for upright frames.
        let to_frame = crate::affine::inverse(frame.content_transform());
        let page = doc.pages.get(frame.page);
        let rect = |r: Rect| to_frame.map(|m| crate::affine::bounds(m, r));
        let margins = page.and_then(|p| {
            let m = p.margins;
            rect(Rect::new(
                m.left,
                m.top,
                p.width - m.left - m.right,
                p.height - m.top - m.bottom,
            ))
        });
        Self {
            frame: frame.bounds,
            column,
            margins,
            page: page.and_then(|p| rect(Rect::new(0.0, 0.0, p.width, p.height))),
            left_page: doc.facing_pages && doc.page_is_left(frame.page),
        }
    }
}

/// Where a line sets its text: the origin the spec's carets are relative to.
fn line_origin(line: &ComposedLine) -> (crate::Point, &schist_text_engine::TextSpec) {
    let spec = &line.projected.as_ref().expect("projected line").spec;
    let width = schist_text_engine::measure(spec).map_or(0.0, |m| m.width);
    (
        crate::compose::aligned_origin(line.bounds, width, spec.align, spec.writing_mode),
        spec,
    )
}

/// The x of `alignment` on `rect`; Text follows the line's alignment.
fn aligned_x(rect: Rect, alignment: HorizontalAlignment, align: schist_text_engine::Align) -> Pt {
    let alignment = match (alignment, align) {
        (HorizontalAlignment::Text, schist_text_engine::Align::Center) => {
            HorizontalAlignment::Center
        }
        (HorizontalAlignment::Text, schist_text_engine::Align::Right) => HorizontalAlignment::Right,
        (HorizontalAlignment::Text, _) => HorizontalAlignment::Left,
        (other, _) => other,
    };
    match alignment {
        HorizontalAlignment::Center => rect.x + rect.width / 2.0,
        HorizontalAlignment::Right => rect.right(),
        _ => rect.x,
    }
}

/// The items set in `lines`, placed in page space through `frame`, with
/// their object styles resolved. Items of one line come in anchor order.
pub fn placements<'a>(
    doc: &LayoutDocument,
    story: &Story,
    frame: &PlacedObject,
    lines: impl IntoIterator<Item = &'a ComposedLine>,
) -> Vec<PlacedObject> {
    let mut out = Vec::new();
    for line in lines {
        let Some(projected) = &line.projected else {
            continue;
        };
        if projected.anchored.is_empty() {
            continue;
        }
        let (origin, spec) = line_origin(line);
        let positions = schist_text_engine::inline_box_positions(spec);
        let references = References::new(doc, frame, line);
        let items: Vec<_> = projected
            .anchored
            .iter()
            .filter_map(|(at, structure)| {
                let position = positions.iter().find(|p| p.at == *at)?;
                let item = story.structures.get(*structure)?.anchored.as_deref()?;
                Some((position, item))
            })
            .collect();
        // Items above the line stack down from where the line's text began,
        // in anchor order.
        let above: Pt = items
            .iter()
            .filter(|(_, item)| item.position == AnchoredPosition::AboveLine)
            .filter_map(|(_, item)| item.line_box())
            .map(|b| b.above)
            .sum();
        let mut stack = None;
        for (position, item) in items {
            let baseline = origin.y + position.baseline;
            let placed = match item.position {
                AnchoredPosition::Inline => item.placed(origin.x + position.x, baseline),
                AnchoredPosition::AboveLine => {
                    // InDesign's export of a public sample puts a Space After
                    // of 0 at half the em plus half the cap height above the
                    // baseline (10.2832 pt for 12 pt Open Sans), and lowers the
                    // line by the item's height.
                    let y = stack.get_or_insert(
                        baseline - above - (position.size + position.cap_height) / 2.0,
                    );
                    let e = item.extent();
                    *y += item.placement.space_above;
                    let top = *y;
                    *y += e.height + item.y_offset;
                    let mut alignment = item.placement.horizontal_alignment;
                    if item.placement.spine_relative && references.left_page {
                        alignment = alignment.mirrored();
                    }
                    let x = aligned_x(references.column, alignment, spec.align);
                    let left = match alignment {
                        HorizontalAlignment::Left => x,
                        HorizontalAlignment::Right => x - e.width,
                        HorizontalAlignment::Center => x - e.width / 2.0,
                        HorizontalAlignment::Text => match spec.align {
                            schist_text_engine::Align::Center => x - e.width / 2.0,
                            schist_text_engine::Align::Right => x - e.width,
                            _ => x,
                        },
                    };
                    item.moved(left, top)
                }
                AnchoredPosition::Anchored => {
                    let Some(placed) =
                        custom(item, &references, line, origin, position, spec.align)
                    else {
                        continue;
                    };
                    placed
                }
            };
            out.push(through_frame(&placed, frame).resolved_appearance(&doc.styles));
        }
    }
    out
}

/// An item at a custom position, in the frame's space.
fn custom(
    item: &AnchoredItem,
    references: &References,
    line: &ComposedLine,
    origin: crate::Point,
    position: &schist_text_engine::InlineBoxPosition,
    align: schist_text_engine::Align,
) -> Option<PlacedObject> {
    let p = item.placement;
    let mirror = p.spine_relative && references.left_page;
    let (alignment, point) = if mirror {
        (p.horizontal_alignment.mirrored(), p.anchor_point.mirrored())
    } else {
        (p.horizontal_alignment, p.anchor_point)
    };
    let rect = |reference| match reference {
        HorizontalReference::TextFrame => Some(references.frame),
        HorizontalReference::ColumnEdge => Some(references.column),
        HorizontalReference::PageMargins => references.margins,
        HorizontalReference::PageEdge => references.page,
        HorizontalReference::AnchorLocation => None,
    };
    let x = match p.horizontal_reference {
        HorizontalReference::AnchorLocation => origin.x + position.x,
        reference => aligned_x(rect(reference)?, alignment, align),
    };
    let baseline = origin.y + position.baseline;
    let box_y = |r: Rect| match p.vertical_alignment {
        VerticalAlignment::Top => r.y,
        VerticalAlignment::Center => r.y + r.height / 2.0,
        VerticalAlignment::Bottom => r.bottom(),
    };
    let y = match p.vertical_reference {
        VerticalReference::LineBaseline => baseline,
        VerticalReference::LineXHeight => baseline - position.x_height,
        VerticalReference::LineAscent => baseline - position.ascent,
        VerticalReference::CapHeight => baseline - position.cap_height,
        VerticalReference::TopOfLeading => baseline - line.advance,
        VerticalReference::ColumnEdge => box_y(references.column),
        VerticalReference::TextFrame => box_y(references.frame),
        VerticalReference::PageMargins => box_y(references.margins?),
        VerticalReference::PageEdge => box_y(references.page?),
    };
    // A public native sample moves a left-aligned item 24 pt left for an X
    // offset of 24; Schist reads the offset as away from the aligned side.
    let dx = match alignment {
        HorizontalAlignment::Left => -p.x_offset,
        _ => p.x_offset,
    };
    let e = item.extent();
    let (fx, fy) = point.fractions();
    let left = x + dx - fx * e.width;
    let mut top = y + item.y_offset - fy * e.height;
    if p.pin_position && p.vertical_reference.is_line() {
        let frame = references.frame;
        if top + e.height > frame.bottom() {
            top = frame.bottom() - e.height;
        }
        top = top.max(frame.y);
    }
    Some(item.moved(left, top))
}
