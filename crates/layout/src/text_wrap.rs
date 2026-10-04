//! Text wrap: objects that push other frames' text aside.
//!
//! Settings follow the public IDML TextWrapPreference. Composition works in a
//! text frame's own untransformed box, as all composition does: each obstacle's
//! page-space outline is mapped through the inverse of the frame's content
//! transform, and every line band is intersected with those outlines to find
//! the inline intervals left for text. Unsupported combinations are reported by
//! the frame rather than composed with a guessed geometry.
use crate::{
    History, Insets, LayoutDocument, LayoutEdit, LayoutObject, ObjectId, PlacedObject, Point, Pt,
    Rect,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::Cell;

/// TextWrapMode. `None` leaves other frames' text alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WrapMode {
    #[default]
    None,
    /// The object's page-aligned bounding box plus four offsets.
    BoundingBox,
    /// The object's outline plus one uniform offset.
    Contour,
    /// No text beside the object: lines resume below it.
    JumpObject,
    /// Text reaching the object moves to the next column or frame.
    NextColumn,
}

/// TextWrapSideOptions: which side of an object text may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WrapSide {
    #[default]
    BothSides,
    LeftSide,
    RightSide,
    SideTowardsSpine,
    SideAwayFromSpine,
    LargestArea,
}

/// ContourOptionsTypes. Pixel-derived outlines are not computed by Schist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContourType {
    BoundingBox,
    PhotoshopPath,
    DetectEdges,
    AlphaChannel,
    GraphicFrame,
    SameAsClipping,
}

/// A page item's local text wrap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextWrap {
    pub mode: WrapMode,
    /// Top, left, bottom and right distances in points. Contour uses the top.
    pub offsets: Insets,
    pub side: WrapSide,
    /// Text flows inside the outline instead of around it.
    pub inverse: bool,
    /// ApplyToMasterPageOnly: a parent item does not wrap document pages.
    pub master_only: bool,
    /// ContourOption as read or authored; written back when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contour: Option<ContourType>,
    /// IncludeInsideEdges: text may also use holes. Retained; Schist wraps
    /// the outer outline only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inside_edges: bool,
    /// ContourPathName: the Photoshop path or alpha channel a contour names.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub contour_path: String,
}

impl Default for TextWrap {
    fn default() -> Self {
        Self {
            mode: WrapMode::None,
            offsets: Insets::ZERO,
            side: WrapSide::BothSides,
            inverse: false,
            master_only: false,
            contour: None,
            inside_edges: false,
            contour_path: String::new(),
        }
    }
}

impl TextWrap {
    /// Offsets beyond the pasteboard are refused.
    pub const MAX_OFFSET: Pt = 10_000.0;

    fn valid(&self) -> bool {
        [
            self.offsets.top,
            self.offsets.left,
            self.offsets.bottom,
            self.offsets.right,
        ]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= Self::MAX_OFFSET)
    }

    /// Whether composition must approximate this wrap's outline.
    pub fn approximated(&self) -> bool {
        self.mode == WrapMode::Contour
            && (self.inside_edges
                || matches!(
                    self.contour,
                    Some(
                        ContourType::PhotoshopPath
                            | ContourType::DetectEdges
                            | ContourType::AlphaChannel
                    )
                ))
    }
}

/// TextPreference wrap settings. Defaults are the published preference values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WrapPreferences {
    /// ZOrderTextWrap: wrap only affects text stacked beneath the object.
    pub only_beneath: bool,
    /// AbutTextToTextWrap: resume at the next leading increment below.
    pub abut: bool,
    /// JustifyTextWraps: retained; Schist does not justify beside wraps.
    pub justify: bool,
}

impl Default for WrapPreferences {
    fn default() -> Self {
        Self {
            only_beneath: false,
            abut: true,
            justify: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Both,
    Left,
    Right,
    Largest,
}

#[derive(Clone)]
struct Obstacle {
    mode: WrapMode,
    side: Side,
    inverse: bool,
    /// Outline rings in the frame's untransformed box.
    rings: Vec<Vec<Point>>,
    /// Contour distance applied around the outline in each band.
    grow: Pt,
    /// The rings' extent.
    shape: Rect,
    /// The extent text is pushed from: `shape` grown by the offset.
    bounds: Rect,
}

/// What a line band may use.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// Inline intervals, left to right.
    Segments(Vec<(Pt, Pt)>),
    /// Nothing fits beside an obstacle; the band below `resume` may.
    Blocked { resume: Pt },
    /// Next-column wrap: the column ends here.
    Stop,
}

/// Obstacles affecting one text frame on one page.
pub struct WrapField {
    obstacles: Vec<Obstacle>,
    abut: bool,
    /// Set when composition could not apply wrap to some text.
    ignored: Cell<bool>,
    /// Set when an outline was approximated by its frame path.
    approximated: Cell<bool>,
}

fn rings_bounds(rings: &[Vec<Point>]) -> Option<Rect> {
    let mut points = rings.iter().flatten();
    let first = points.next()?;
    Some(
        points.fold(Rect::new(first.x, first.y, 0.0, 0.0), |rect, p| {
            rect.union(Rect::new(p.x, p.y, 0.0, 0.0))
        }),
    )
}

/// The horizontal extent of closed rings within the band `top..bottom`: every
/// edge clipped to the band. Edges that only touch the band's top or bottom do
/// not count, so text may sit directly against an obstacle.
fn band_extent(rings: &[Vec<Point>], top: Pt, bottom: Pt) -> Option<(Pt, Pt)> {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    let mut include = |x: Pt| {
        low = low.min(x);
        high = high.max(x);
    };
    for ring in rings {
        for (index, a) in ring.iter().enumerate() {
            let b = ring[(index + 1) % ring.len()];
            if a.y == b.y {
                if a.y > top && a.y < bottom {
                    include(a.x);
                    include(b.x);
                }
                continue;
            }
            let (y0, y1) = (a.y.min(b.y).max(top), a.y.max(b.y).min(bottom));
            if y1 <= y0 {
                continue;
            }
            for y in [y0, y1] {
                include(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
            }
        }
    }
    (low <= high).then_some((low, high))
}

/// The even-odd inside intervals of the rings along the horizontal line `y`.
fn inside_at(rings: &[Vec<Point>], y: Pt) -> Vec<(Pt, Pt)> {
    let mut crossings = Vec::new();
    for ring in rings {
        for (index, a) in ring.iter().enumerate() {
            let b = ring[(index + 1) % ring.len()];
            if (a.y <= y && y < b.y) || (b.y <= y && y < a.y) {
                crossings.push(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
            }
        }
    }
    crossings.sort_by(f32::total_cmp);
    crossings
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[a, b]| (a, b))
        .filter(|(a, b)| b > a)
        .collect()
}

fn intersect(a: &[(Pt, Pt)], b: &[(Pt, Pt)]) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    for (a0, a1) in a {
        for (b0, b1) in b {
            let (x0, x1) = (a0.max(*b0), a1.min(*b1));
            if x1 > x0 {
                out.push((x0, x1));
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

/// The intervals inside the rings along the whole band `top..bottom`. Edges are
/// straight between vertices, so sampling just inside every vertex height and
/// both band edges finds the narrowest interior exactly.
fn band_inside(rings: &[Vec<Point>], top: Pt, bottom: Pt) -> Vec<(Pt, Pt)> {
    const EPSILON: Pt = 0.001;
    let mut heights = vec![top + EPSILON, bottom - EPSILON];
    for p in rings.iter().flatten() {
        if p.y > top && p.y < bottom {
            heights.extend([p.y - EPSILON, p.y + EPSILON]);
        }
    }
    if top + EPSILON >= bottom - EPSILON {
        heights = vec![(top + bottom) * 0.5];
    }
    let mut out = inside_at(rings, heights[0]);
    for y in &heights[1..] {
        if out.is_empty() {
            break;
        }
        out = intersect(&out, &inside_at(rings, *y));
    }
    out
}

/// Subtract `cut` from every interval.
fn subtract(intervals: Vec<(Pt, Pt)>, cut: (Pt, Pt)) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    for (a, b) in intervals {
        if cut.1 <= a || cut.0 >= b {
            out.push((a, b));
            continue;
        }
        if cut.0 > a {
            out.push((a, cut.0));
        }
        if cut.1 < b {
            out.push((cut.1, b));
        }
    }
    out
}

/// Every visible item that can wrap `page`'s text, in page coordinates. Items
/// owned by another page of the same spread are moved into this page's space,
/// so artwork crossing the gutter wraps text on both pages.
fn spread_objects(doc: &LayoutDocument, page: usize) -> Vec<Cow<'_, PlacedObject>> {
    let mut objects = doc.page_objects(page);
    let (Some(spread), Some(origin)) = (doc.spread_containing(page), doc.page_origin(page)) else {
        return objects;
    };
    for other in spread.pages.iter().copied().filter(|p| *p != page) {
        let Some(at) = doc.page_origin(other) else {
            continue;
        };
        for object in doc.page_objects(other) {
            let mut object = object.into_owned();
            object.bounds.x += at.x - origin.x;
            object.bounds.y += at.y - origin.y;
            objects.push(Cow::Owned(object));
        }
    }
    objects
}

impl WrapField {
    /// The wrap affecting `frame` on `page`, or None when nothing intersects it.
    /// Frames ignoring wrap, objects on hidden layers or hidden objects, the
    /// frame itself and master-only parent items do not take part.
    pub fn for_frame(doc: &LayoutDocument, frame: ObjectId, page: usize) -> Option<Self> {
        let any = doc
            .objects
            .iter()
            .chain(
                doc.parents
                    .iter()
                    .flat_map(|p| p.objects.iter().map(|o| &o.object)),
            )
            .any(|o| {
                o.appearance
                    .text_wrap
                    .as_ref()
                    .is_some_and(|w| w.mode != WrapMode::None)
            });
        if !any {
            return None;
        }
        let objects = spread_objects(doc, page);
        let host = objects.iter().find(|o| o.id == frame)?;
        if host.appearance.ignore_wrap || doc.layer_ignores_wrap(doc.object_layer(host.id)) {
            return None;
        }
        let inverse = crate::affine::inverse(host.content_transform())?;
        let order = doc
            .text_wrap_preferences
            .only_beneath
            .then(|| doc.paint_order());
        let spine_right = doc.facing_pages && doc.page_is_left(page);
        let mut obstacles = Vec::new();
        let mut approximated = false;
        for object in &objects {
            let Some(wrap) = object.appearance.text_wrap.as_ref() else {
                continue;
            };
            if wrap.mode == WrapMode::None || object.id == host.id {
                continue;
            }
            // A parent item marked master-only never wraps a document page.
            if wrap.master_only && doc.object(object.id).is_none() {
                continue;
            }
            // Painted later is above: only those wrap text when restricted.
            if order
                .as_ref()
                .is_some_and(|order| order.get(&object.id) <= order.get(&host.id))
            {
                continue;
            }
            let side = match wrap.side {
                WrapSide::BothSides => Side::Both,
                WrapSide::LeftSide => Side::Left,
                WrapSide::RightSide => Side::Right,
                WrapSide::LargestArea => Side::Largest,
                WrapSide::SideTowardsSpine if spine_right => Side::Right,
                WrapSide::SideTowardsSpine => Side::Left,
                WrapSide::SideAwayFromSpine if spine_right => Side::Left,
                WrapSide::SideAwayFromSpine => Side::Right,
            };
            let (outline, grow) = if wrap.mode == WrapMode::Contour
                && wrap.contour != Some(ContourType::BoundingBox)
            {
                approximated |= wrap.approximated();
                (outline(object), wrap.offsets.top)
            } else {
                let b = object.visual_bounds();
                let o = wrap.offsets;
                let r = Rect::new(
                    b.x - o.left,
                    b.y - o.top,
                    b.width + o.left + o.right,
                    b.height + o.top + o.bottom,
                );
                if r.width <= 0.0 || r.height <= 0.0 {
                    continue;
                }
                (vec![crate::affine::corners(r).to_vec()], 0.0)
            };
            let rings: Vec<Vec<Point>> = outline
                .into_iter()
                .map(|ring| {
                    ring.into_iter()
                        .map(|p| crate::affine::point(inverse, p))
                        .collect()
                })
                .collect();
            let Some(shape) = rings_bounds(&rings) else {
                continue;
            };
            let bounds = Rect::new(
                shape.x - grow,
                shape.y - grow,
                shape.width + 2.0 * grow,
                shape.height + 2.0 * grow,
            );
            if !bounds.intersects(host.bounds) {
                continue;
            }
            obstacles.push(Obstacle {
                mode: wrap.mode,
                side,
                inverse: wrap.inverse
                    && matches!(wrap.mode, WrapMode::BoundingBox | WrapMode::Contour),
                rings,
                grow,
                shape,
                bounds,
            });
        }
        (!obstacles.is_empty()).then(|| Self {
            obstacles,
            abut: doc.text_wrap_preferences.abut,
            ignored: Cell::new(false),
            approximated: Cell::new(approximated),
        })
    }

    /// Whether any obstacle reaches `area` at all. An inverse wrap reaches
    /// every column of a frame it overlaps: outside its outline is wrap area.
    pub fn affects(&self, area: Rect) -> bool {
        self.obstacles
            .iter()
            .any(|o| o.inverse || o.bounds.intersects(area))
    }

    pub fn abut(&self) -> bool {
        self.abut
    }

    pub fn note_ignored(&self) {
        self.ignored.set(true);
    }

    pub fn ignored(&self) -> bool {
        self.ignored.get()
    }

    pub fn approximated(&self) -> bool {
        self.approximated.get()
    }

    /// The inline intervals of `left..right` that a line band may use.
    /// Intervals narrower than `minimum` are not offered to text.
    pub fn row(&self, top: Pt, height: Pt, left: Pt, right: Pt, minimum: Pt) -> Row {
        let bottom = top + height;
        let mut free = vec![(left, right)];
        let mut resume = f32::INFINITY;
        for obstacle in &self.obstacles {
            let g = obstacle.grow;
            if obstacle.inverse {
                // Text stays inside the outline, shrunk by the offset.
                let inside: Vec<_> = band_inside(&obstacle.rings, top - g, bottom + g)
                    .into_iter()
                    .map(|(a, b)| (a + g, b - g))
                    .filter(|(a, b)| b > a)
                    .collect();
                if inside.is_empty() {
                    if top >= obstacle.shape.bottom() - g {
                        return Row::Stop;
                    }
                    // Above the outline, or where it is too narrow: step down.
                    resume = resume.min(if bottom <= obstacle.shape.y + g {
                        obstacle.shape.y + g
                    } else {
                        top + 1.0
                    });
                    free.clear();
                } else {
                    free = intersect(&free, &inside);
                    if free.is_empty() {
                        resume = resume.min(top + 1.0);
                    }
                }
                continue;
            }
            let Some((a, b)) =
                band_extent(&obstacle.rings, top - g, bottom + g).map(|(a, b)| (a - g, b + g))
            else {
                continue;
            };
            if b <= left || a >= right {
                continue;
            }
            let obstacle_bottom = obstacle.bounds.bottom();
            match obstacle.mode {
                WrapMode::NextColumn => return Row::Stop,
                WrapMode::JumpObject => {
                    resume = resume.min(obstacle_bottom);
                    free.clear();
                }
                _ => {
                    // Largest area is decided once per column, not per band,
                    // so an irregular outline cannot make text change sides.
                    let side = match obstacle.side {
                        Side::Largest
                            if obstacle.bounds.x - left >= right - obstacle.bounds.right() =>
                        {
                            Side::Left
                        }
                        Side::Largest => Side::Right,
                        side => side,
                    };
                    let cut = match side {
                        Side::Left => (a, right),
                        Side::Right => (left, b),
                        _ => (a, b),
                    };
                    free = subtract(free, cut);
                    resume = resume.min(obstacle_bottom);
                }
            }
        }
        free.retain(|(a, b)| b - a >= minimum.max(0.001));
        if free.is_empty() {
            Row::Blocked {
                resume: if resume.is_finite() && resume > top {
                    resume
                } else {
                    top + height.max(1.0)
                },
            }
        } else {
            Row::Segments(free)
        }
    }
}

/// An object's outline rings in page space: its shape, clipping path or
/// text-frame outline, otherwise its frame.
fn outline(object: &PlacedObject) -> Vec<Vec<Point>> {
    let transform = object.content_transform();
    let size = object.bounds;
    let flattened = |path: &crate::ShapePath, scale: (Pt, Pt)| -> Vec<Vec<Point>> {
        path.flatten(0.25)
            .subpaths
            .into_iter()
            .filter(|ring| ring.len() >= 3)
            .map(|ring| {
                ring.into_iter()
                    .map(|(x, y)| Point::new(size.x + x * scale.0, size.y + y * scale.1))
                    .collect()
            })
            .collect()
    };
    let rings = match &object.object {
        LayoutObject::Shape { path, .. } => flattened(path, (1.0, 1.0)),
        LayoutObject::GraphicFrame {
            clip_path: Some(path),
            ..
        } => flattened(path, (1.0, 1.0)),
        // Text-frame outlines are stored normalized to the frame.
        LayoutObject::TextFrame {
            text_path: None, ..
        } => object
            .appearance
            .outline
            .as_ref()
            .map(|path| flattened(path, (size.width, size.height)))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let rings = if rings.is_empty() {
        vec![crate::affine::corners(object.bounds).to_vec()]
    } else {
        rings
    };
    rings
        .into_iter()
        .map(|ring| {
            ring.into_iter()
                .map(|p| crate::affine::point(transform, p))
                .collect()
        })
        .collect()
}

/// Edit every listed item's local wrap as one undo step. Locked items, notes
/// and offsets beyond the pasteboard refuse the whole edit.
pub fn edit_wrap(
    doc: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    edit: impl Fn(&mut TextWrap),
) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        let Some(object) = doc.object(*id).filter(|_| !doc.object_locked(*id)) else {
            return false;
        };
        if matches!(object.object, LayoutObject::Note { .. }) {
            return false;
        }
        let mut wrap = object.appearance.text_wrap.clone().unwrap_or_default();
        edit(&mut wrap);
        if !wrap.valid() {
            return false;
        }
        let mut after = object.clone();
        after.appearance.text_wrap = (wrap != TextWrap::default()).then_some(wrap);
        if after != *object {
            edits.push(LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

/// Set whether text frames' own text ignores wrap, as one undo step.
pub fn set_ignore_wrap(
    doc: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    value: bool,
) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        let Some(object) = doc.object(*id).filter(|_| !doc.object_locked(*id)) else {
            return false;
        };
        if !matches!(
            object.object,
            LayoutObject::TextFrame {
                text_path: None,
                ..
            }
        ) {
            return false;
        }
        let mut after = object.clone();
        after.appearance.ignore_wrap = value;
        if after != *object {
            edits.push(LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

/// Change the document's wrap preferences as one settings edit.
pub fn set_preferences(
    doc: &mut LayoutDocument,
    history: &mut History,
    preferences: WrapPreferences,
) -> bool {
    if doc.text_wrap_preferences == preferences {
        return false;
    }
    let before = crate::snapshot_settings(doc);
    let mut after = before.clone();
    after.text_wrap_preferences = preferences;
    history.apply(
        doc,
        LayoutEdit::DocumentChanged {
            before: Box::new(before),
            after: Box::new(after),
        },
    )
}
