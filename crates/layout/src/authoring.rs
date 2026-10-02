//! Authoring: making a document, not just moving what is in one.
//!
//! Everything here goes through [`History`], so creating a frame, typing
//! into it and deleting it are the same kind of undoable step as moving
//! one. That is not tidiness: an editor where undo works for moves and not
//! for typing is an editor nobody trusts with their work.
//!
//! The operations are free functions over `(&mut LayoutDocument, &mut
//! History)` rather than methods, because they are two-party operations
//! and hiding the history in a method would make it easy to call one
//! without the other — which is exactly the bug this module exists to
//! make impossible.

use crate::edit::{snapshot_object, snapshot_story};
use crate::history::ObjectSnapshot;
use crate::story::StyleRange;
use crate::StoryPoint;
use crate::{
    FrameOverflow, History, Insets, LayoutDocument, LayoutEdit, LayoutObject, Link, ObjectId,
    PlacedObject, Rect, ShapePath, Story, StoryId, SubPath,
};

// `Point` names two things here, as it does throughout the crate: a point
// in a story and a point on a path. Only the path one is needed.
use crate::geometry::Point;

/// The smallest a frame can be and still be clickable.
///
/// A frame of no size cannot be selected, so one created by a click rather
/// than a drag is given this instead — the same reason a zero-height row
/// in the Layers panel still has a hit area.
pub const MIN_FRAME: Pt = 8.0;

type Pt = crate::Pt;

/// A new text frame: a box, a story to put in it, and a link between them.
pub struct TextFrame {
    pub object: ObjectId,
    pub story: StoryId,
}

/// Create a text frame on a page.
///
/// The story starts empty. An empty frame is still clickable and still
/// has an insertion point, which is what a user who has drawn a box
/// expects to type into.
pub fn text_frame(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    bounds: Rect,
) -> Option<TextFrame> {
    let bounds = grown(bounds);
    let story = StoryId(document.stories.len() as u32);
    let id = ObjectId::next();
    let placed = PlacedObject {
        appearance: Default::default(),
        id,
        page,
        bounds,
        object: LayoutObject::TextFrame {
            text_path: None,
            story,
            columns: 1,
            gutter: 0.0,
            insets: Insets::default(),
            overflow: FrameOverflow::Clip,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: schist_i18n::t("design.frame").into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    };
    if page >= document.pages.len() {
        return None;
    }
    let edit = object_creation_edit(document, &placed)?;
    history
        .apply(
            document,
            LayoutEdit::Batch {
                edits: vec![
                    LayoutEdit::AddedStory {
                        id: story.0,
                        story: snapshot_story(&Story::new()),
                    },
                    edit,
                ],
            },
        )
        .then_some(TextFrame { object: id, story })
}

/// How a shape is painted.
///
/// The three paint arguments travel together -- a fill without a colour is
/// not a thing, and a stroke width is meaningless without a stroke -- so
/// they are one value rather than three loose parameters. It also keeps the
/// authoring functions under clippy's argument limit without an `allow`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paint {
    /// An ink name, or `None` for no fill.
    pub fill: Option<String>,
    /// An ink name, or `None` for no stroke.
    pub stroke: Option<String>,
    pub stroke_width: Pt,
}

impl Paint {
    /// No fill and no stroke: a frame a user can select and edit.
    pub fn none() -> Paint {
        Paint::default()
    }

    /// A filled shape, and nothing else.
    pub fn filled(name: impl Into<String>) -> Paint {
        Paint {
            fill: Some(name.into()),
            ..Paint::default()
        }
    }

    /// A stroked shape, and nothing else.
    pub fn stroked(name: impl Into<String>, width: Pt) -> Paint {
        Paint {
            stroke: Some(name.into()),
            stroke_width: width,
            ..Paint::default()
        }
    }
}

/// What a shape tool draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// A closed rectangle.
    Rectangle,
    /// A closed ellipse made from four cubic Bézier arcs.
    Ellipse,
    /// A straight line: an open path across its frame, never filled.
    Line,
    /// A closed regular polygon with this many sides.
    Polygon { sides: usize },
}

impl ShapeKind {
    /// Whether a shape of this kind can have a fill.
    ///
    /// A line cannot: an open path has no inside, and a fill on an open
    /// path is a renderer-specific surprise rather than anything a user
    /// asked for.
    pub fn fillable(self) -> bool {
        !matches!(self, ShapeKind::Line)
    }
}

/// Create an editable path from page-space anchors and handles. Each
/// completed Pen press/release records one edit, including the first anchor.
pub fn path_shape(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    mut path: ShapePath,
    paint: Paint,
) -> Option<ObjectId> {
    if path.subpaths.iter().all(|sub| sub.points.is_empty()) {
        return None;
    }
    if !path.is_finite() {
        return None;
    }
    let bounds = path.bounds();
    if ![bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f32::is_finite)
    {
        return None;
    }
    path.map_points(|p| p - bounds.origin());
    let name = if path.subpaths.len() == 1
        && path.subpaths[0].points.len() == 2
        && path.subpaths[0].handles.is_empty()
        && !path.subpaths[0].closed
    {
        schist_i18n::t("tool.shape.line.label")
    } else {
        schist_i18n::t("common.path")
    };
    let placed = PlacedObject {
        appearance: Default::default(),
        id: ObjectId::next(),
        page,
        bounds,
        object: LayoutObject::Shape {
            fill: paint
                .fill
                .filter(|_| path_can_be_filled(&path))
                .and_then(|name| ink_by_name(document, &name)),
            stroke: paint.stroke.and_then(|name| ink_by_name(document, &name)),
            stroke_width: paint.stroke_width,
            path,
            fill_overprint: false,
            stroke_overprint: false,
            tints: Default::default(),
        },
        rotation: 0.0,
        transform: Default::default(),
        name: name.to_string(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    };
    record_object(document, history, placed)
}

/// Replace a shape's page-space path as one edit, preserving its paint.
pub fn replace_path(
    document: &mut LayoutDocument,
    history: &mut History,
    object: ObjectId,
    mut path: ShapePath,
) -> bool {
    if document.object_locked(object) {
        return false;
    }
    let Some(placed) = document.object(object) else {
        return false;
    };
    let before = snapshot_object(placed);
    let mut changed = placed.clone();
    let Some(target) = changed.object.editable_path_mut() else {
        return false;
    };
    if !path.is_finite() {
        return false;
    }
    let bounds = path.bounds();
    if ![bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f32::is_finite)
    {
        return false;
    }
    path.map_points(|p| p - bounds.origin());
    *target = path;
    changed.bounds = bounds;
    let after = snapshot_object(&changed);
    before != after
        && history.apply(
            document,
            LayoutEdit::ObjectChanged {
                id: object.0,
                before,
                after,
            },
        )
}

/// Create a rectangle on a page.
///
/// `paint` is how it is inked: a frame with no fill and a stroke is a
/// shape, and a shape with neither is still a frame a user can select and
/// edit.
pub fn rectangle(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    bounds: Rect,
    paint: Paint,
) -> Option<ObjectId> {
    shape(document, history, page, bounds, ShapeKind::Rectangle, paint)
}

/// Create a shape of any kind on a page.
pub fn shape(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    bounds: Rect,
    kind: ShapeKind,
    paint: Paint,
) -> Option<ObjectId> {
    let bounds = grown(bounds);
    let (width, height) = (bounds.width, bounds.height);
    let path = path_for(kind, width, height);
    let placed = PlacedObject {
        appearance: Default::default(),
        id: ObjectId::next(),
        page,
        bounds: Rect::new(bounds.x, bounds.y, width, height),
        object: LayoutObject::Shape {
            path,
            // A fill the shape cannot take is not quietly applied anyway.
            fill: paint
                .fill
                .filter(|_| kind.fillable())
                .and_then(|name| ink_by_name(document, &name)),
            stroke: paint.stroke.and_then(|name| ink_by_name(document, &name)),
            stroke_width: paint.stroke_width,
            fill_overprint: false,
            stroke_overprint: false,
            tints: Default::default(),
        },
        rotation: 0.0,
        transform: Default::default(),
        name: kind.name().into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    };
    record_object(document, history, placed)
}

impl ShapeKind {
    /// The name a new shape is given, as it appears in the Layers panel.
    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => schist_i18n::t("tool.shape.rect.label"),
            ShapeKind::Ellipse => schist_i18n::t("tool.shape.ellipse.label"),
            ShapeKind::Line => schist_i18n::t("tool.shape.line.label"),
            ShapeKind::Polygon { .. } => schist_i18n::t("tool.shape.polygon.label"),
        }
    }
}

/// The outline of a shape, relative to its frame's own origin.
///
/// Resizing transforms the path and its handles together.
/// Local geometry shared by authored shapes and live tool previews.
pub fn path_for(kind: ShapeKind, width: Pt, height: Pt) -> ShapePath {
    let (cx, cy) = (width / 2.0, height / 2.0);
    let points = match kind {
        ShapeKind::Rectangle => vec![
            Point::ZERO,
            Point::new(width, 0.0),
            Point::new(width, height),
            Point::new(0.0, height),
        ],
        ShapeKind::Line => {
            // Corner to corner of its own frame.
            //
            // The obvious alternative -- a horizontal path in a frame
            // rotated to the drag's angle -- is wrong here, because
            // `rotation` is not how the pasteboard positions a path: it
            // offsets the points by the frame's origin and nothing else. A
            // rotated frame with a horizontal path would draw a level line
            // inside a slanted box, so the click hit and the ink would
            // disagree. The frame is the bounding box and the path is the
            // diagonal, which both the pasteboard and the exporter already
            // agree on.
            vec![Point::ZERO, Point::new(width, height)]
        }
        ShapeKind::Ellipse => return ShapePath::ellipse(width, height),
        ShapeKind::Polygon { sides } => {
            let sides = sides.max(3);
            (0..sides)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / sides as f32
                        - std::f32::consts::FRAC_PI_2;
                    Point::new(cx + cx * angle.cos(), cy + cy * angle.sin())
                })
                .collect()
        }
    };
    ShapePath {
        subpaths: vec![SubPath {
            handles: Vec::new(),
            points,
            closed: !matches!(kind, ShapeKind::Line),
        }],
        even_odd: false,
    }
}

/// Create a placed graphic frame, linked to a file.
pub fn graphic_frame(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    bounds: Rect,
    path: impl Into<String>,
    embedded: bool,
) -> Option<ObjectId> {
    graphic_frame_with_link(document, history, page, bounds, Link::new(path), embedded)
}

pub fn graphic_frame_with_link(
    document: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    bounds: Rect,
    link: Link,
    embedded: bool,
) -> Option<ObjectId> {
    let placed = PlacedObject {
        appearance: Default::default(),
        id: ObjectId::next(),
        page,
        bounds: grown(bounds),
        object: LayoutObject::GraphicFrame {
            link,
            embedded,
            fit: Default::default(),
            crop: None,
            image_transform: Default::default(),
            clip_path: None,
            scale: 1.0,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: schist_i18n::t("design.graphic_frame").into(),
        locked: false,
        overprint: false,
        transparency: 1.0,
    };
    record_object(document, history, placed)
}

/// Delete an object.
///
/// **One edit, one undo step.** That is the whole reason this is careful
/// about what it touches: a user who deletes a frame and presses ⌘Z once
/// expects the frame back, so anything else the delete changed has to
/// travel in the same edit or not happen at all.
///
/// The story is left alone. A story with no frame is unreachable, which is
/// untidy, but emptying it is a second edit and a second undo step — and a
/// delete that takes two presses to reverse is worse than a document with
/// a few orphaned stories. Pruning them is a separate, deliberate act.
pub fn delete(document: &mut LayoutDocument, history: &mut History, id: ObjectId) -> bool {
    let Some(index) = document.objects.iter().position(|object| object.id == id) else {
        return false;
    };
    // A locked object does not go away, or a user could delete something
    // they cannot move.
    if document.object_locked(document.objects[index].id) {
        return false;
    }
    let edit = LayoutEdit::RemovedObject {
        index,
        object: snapshot_object(&document.objects[index]),
    };
    if !crate::edit::forward(document, &edit) {
        return false;
    }
    history.record(edit);
    true
}

/// Copy an object, placing the copy just clear of the original.
///
/// The copy is offset rather than dropped exactly on top, because two
/// frames at identical coordinates are indistinguishable on the
/// pasteboard and un-clickable: every click would pick whichever is on top.
///
/// A new id and a new story where the object had one. Sharing a story
/// would make typing in the copy change the original, which is a link the
/// user never asked for and cannot see.
pub fn duplicate(
    document: &mut LayoutDocument,
    history: &mut History,
    id: ObjectId,
) -> Option<ObjectId> {
    let original = document.object(id)?.clone();
    let offset = Point::new(DUPLICATE_OFFSET, DUPLICATE_OFFSET);
    let bounds = original.bounds.translated(offset);

    // A text frame's story is copied, not shared. The copy starts with the
    // same text; from here on the two are independent, and typing in one
    // does not reach the other.
    //
    // The new story and frame are one operation, so undo removes both
    // without exposing orphan copy stories in the Stories panel.
    let mut story_edit = None;
    let object = match &original.object {
        LayoutObject::TextFrame {
            story,
            text_path,
            columns,
            gutter,
            insets,
            overflow,
        } => {
            let copy = document.story(*story)?.clone();
            let new_story = StoryId(document.stories.len() as u32);
            story_edit = Some(LayoutEdit::AddedStory {
                id: new_story.0,
                story: snapshot_story(&copy),
            });
            LayoutObject::TextFrame {
                text_path: text_path.clone(),
                story: new_story,
                columns: *columns,
                gutter: *gutter,
                insets: *insets,
                overflow: *overflow,
            }
        }
        other => other.clone(),
    };

    let placed = PlacedObject {
        appearance: original.appearance.clone(),
        id: ObjectId::next(),
        page: original.page,
        bounds,
        object,
        rotation: original.rotation,
        transform: original.transform,
        name: original.name,
        locked: false,
        overprint: original.overprint,
        transparency: original.transparency,
    };
    let id = placed.id;
    let object_edit = object_creation_edit(document, &placed)?;
    let mut edits: Vec<_> = story_edit.into_iter().collect();
    edits.push(object_edit);
    history
        .apply(document, LayoutEdit::Batch { edits })
        .then_some(id)
}

/// How far a duplicate is moved from what it was copied from.
const DUPLICATE_OFFSET: Pt = 12.0;

/// One point of a shape's outline, addressed by position rather than by
/// value.
///
/// Addressing a point by its coordinates would make "the point at 10, 20"
/// ambiguous the moment two points share coordinates, which is exactly
/// what happens at a shape's corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointRef {
    pub subpath: usize,
    pub index: usize,
    pub part: PointPart,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointPart {
    #[default]
    Anchor,
    Incoming,
    Outgoing,
}

/// Move one point of a shape's outline, as a finished gesture.
///
/// `to` is where the point lands, in the same page space as every other
/// coordinate a document holds. The path stores its points relative to the
/// object's own frame, so the translation is done here rather than by the
/// caller: a caller that moved a point to an absolute page position would
/// have to know that convention, and getting it wrong would move a point by
/// the frame's origin every time the frame moved.
///
/// One edit, and nothing recorded if the point ended where it started.
///
/// Bounds follow the edited curve, while rebasing its local coordinates
/// preserves every other anchor and handle in page space.
pub fn move_point(
    document: &mut LayoutDocument,
    history: &mut History,
    object: ObjectId,
    at: PointRef,
    to: Point,
) -> bool {
    let before = match snapshot_point(document, object) {
        Some(snapshot) => snapshot,
        None => return false,
    };
    if !set_point(document, object, at, to) {
        return false;
    }
    let Some(after) = snapshot_point(document, object) else {
        return false;
    };
    if before == after {
        // A point dragged onto itself is a click, and a click that records
        // an undo entry makes the stack lie about what happened.
        return false;
    }
    history.record(LayoutEdit::ObjectChanged {
        id: object.0,
        before,
        after,
    });
    true
}

/// Put one point of a shape's outline where the pointer is, recording
/// nothing.
///
/// This is the live half of a drag: the gesture is not finished, so there is
/// nothing yet to undo. A caller that has a snapshot from when the drag
/// began can record the difference itself in one step, which is the only
/// way to keep one drag to one undo entry.
pub fn set_point(document: &mut LayoutDocument, object: ObjectId, at: PointRef, to: Point) -> bool {
    if !to.x.is_finite() || !to.y.is_finite() {
        return false;
    }
    let Some(index) = document.objects.iter().position(|o| o.id == object) else {
        return false;
    };
    if document.object_locked(document.objects[index].id) {
        return false;
    }
    let placed = &document.objects[index];
    let Some(path) = placed.object.editable_path() else {
        return false;
    };
    let Some(sub) = path.subpaths.get(at.subpath) else {
        return false;
    };
    let Some(anchor) = sub.points.get(at.index) else {
        return false;
    };
    let handles = sub.handles_at(at.index);
    let current = match at.part {
        PointPart::Anchor => *anchor,
        PointPart::Incoming => handles.incoming.unwrap_or(*anchor),
        PointPart::Outgoing => handles.outgoing.unwrap_or(*anchor),
    };
    if to == crate::affine::point(placed.content_transform(), current + placed.bounds.origin()) {
        return true;
    }
    let matrix = document.objects[index].content_transform();
    let Some(inverse) = matrix.invert() else {
        return false;
    };
    let to = crate::affine::point(inverse, to);
    let origin = document.objects[index].bounds.origin();
    let Some(path) = document.objects[index].object.editable_path_mut() else {
        return false;
    };
    let Some(subpath) = path.subpaths.get_mut(at.subpath) else {
        return false;
    };
    if at.index >= subpath.points.len() {
        return false;
    }
    match at.part {
        PointPart::Anchor => {
            subpath.move_anchor(at.index, to - origin);
        }
        PointPart::Incoming | PointPart::Outgoing => {
            let mut handles = subpath.handles_at(at.index);
            match at.part {
                PointPart::Incoming => handles.incoming = Some(to - origin),
                _ => handles.outgoing = Some(to - origin),
            }
            subpath.set_handles(at.index, handles);
        }
    }
    let bounds = path.bounds();
    path.map_points(|p| p - bounds.origin());
    let object = &mut document.objects[index];
    object.bounds = bounds.translated(origin);
    let inverse_rotation = schist_core::Affine::rotate(-object.rotation.to_radians()).around(
        object.bounds.x + object.bounds.width * 0.5,
        object.bounds.y + object.bounds.height * 0.5,
    );
    object.transform = inverse_rotation
        .then(&matrix)
        .around(-object.bounds.x, -object.bounds.y);
    true
}

/// A shape as it is now, for a caller recording its own edit.
fn snapshot_point(document: &LayoutDocument, object: ObjectId) -> Option<ObjectSnapshot> {
    document
        .objects
        .iter()
        .find(|o| o.id == object)
        .map(snapshot_object)
}

/// Apply a paragraph style to whole text frames.
///
/// The paragraph style of a frame is the style of its text: applying one
/// changes every paragraph in it, which is what a user who clicks a style
/// while a frame is selected means. A range of text is the character style's
/// business, not the paragraph's.
///
/// Objects that are not text frames are skipped rather than refused, for
/// the same reason a mixed fill skips the lines: a selection is usually
/// whatever happened to be selected, and the half of it that can take a
/// style should still get one.
///
/// A style the document does not define is refused. Applying a name that
/// resolves to nothing would silently leave the text looking as it did,
/// which is the hardest kind of nothing to notice.
pub fn set_paragraph_style(
    document: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    style: &str,
) -> bool {
    if document.styles.paragraph(style).is_none() {
        return false;
    }
    let stories: Vec<StoryId> = ids
        .iter()
        .filter_map(|id| {
            let placed = document.object(*id)?;
            if document.object_locked(placed.id) {
                return None;
            }
            match &placed.object {
                LayoutObject::TextFrame { story, .. } => Some(*story),
                _ => None,
            }
        })
        .collect();
    let mut applied = Vec::new();
    for story in stories {
        let Some(existing) = document.story(story) else {
            continue;
        };
        let paragraphs: Vec<&String> = existing
            .points
            .iter()
            .filter_map(|point| match point {
                StoryPoint::Paragraph { style, .. } => Some(style),
                _ => None,
            })
            .collect();
        if paragraphs.is_empty() || paragraphs.iter().all(|s| *s == style) {
            // An empty story has nothing to restyle, and a story whose
            // every paragraph already has this style is not a change.
            // Either way there is nothing for the user to undo.
            continue;
        }
        let before = snapshot_story(existing);
        let mut points = existing.points.clone();
        for point in &mut points {
            if let StoryPoint::Paragraph { style: s, .. } = point {
                *s = style.to_string();
            }
        }
        applied.push((
            story,
            before,
            snapshot_story(&Story {
                ranges: existing.ranges.clone(),
                points,
                prefs: existing.prefs,
                structures: existing.structures.clone(),
            }),
        ));
    }
    if applied.is_empty() {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    let edits = applied
        .into_iter()
        .filter(|(story, _, _)| seen.insert(*story))
        .map(|(story, before, after)| LayoutEdit::StoryChanged {
            id: story.0,
            before,
            after,
        })
        .collect();
    history.apply(document, LayoutEdit::Batch { edits })
}

/// Apply a character style to a range of a story's text.
///
/// A range is given in bytes because that is what a story is addressed in,
/// and it is clamped to the text: a selection that runs off the end of a
/// frame is still a selection, and the part that exists is what the user
/// meant. An empty range is refused rather than recorded, because a
/// character style on nothing is not a change.
pub fn set_character_style(
    document: &mut LayoutDocument,
    history: &mut History,
    story: StoryId,
    range: std::ops::Range<usize>,
    style: &str,
) -> bool {
    if document.styles.character(style).is_none() {
        return false;
    }
    let Some(existing) = document.story(story) else {
        return false;
    };
    let length = text_length(existing);
    let start = range.start.min(length);
    let end = range.end.min(length);
    if start >= end {
        return false;
    }
    // A range already wholly inside a range of the same style is not a
    // change, and the other ranges around it do not need restating.
    let covered = existing
        .ranges
        .iter()
        .any(|other| other.style == style && other.start <= start && other.end >= end);
    if covered {
        return false;
    }
    let before = snapshot_story(existing);
    // Overlaps are trimmed to the parts that survive, not dropped. Dropping
    // a range that sticks out on either side would strip text the user
    // never asked to restyle: applying Italic to the middle of a word in
    // Bold text must not remove the Bold from the rest of the word.
    let mut ranges: Vec<StyleRange> = Vec::with_capacity(existing.ranges.len() + 3);
    for other in &existing.ranges {
        if other.end <= start || other.start >= end {
            ranges.push(other.clone());
            continue;
        }
        if other.start < start {
            ranges.push(StyleRange::new(other.start, start, other.style.clone()));
        }
        if other.end > end {
            ranges.push(StyleRange::new(end, other.end, other.style.clone()));
        }
    }
    ranges.push(StyleRange::new(start, end, style));
    ranges.sort_by_key(|range| (range.start, range.end));
    let after = snapshot_story(&Story {
        points: existing.points.clone(),
        ranges,
        prefs: existing.prefs,
        structures: existing.structures.clone(),
    });
    let edit = LayoutEdit::StoryChanged {
        id: story.0,
        before,
        after,
    };
    if !crate::edit::forward(document, &edit) {
        return false;
    }
    history.record(edit);
    true
}

/// A story's text length in bytes.
///
/// The `+ text.len()` of every paragraph plus one per break: a range has to
/// be counted the same way an offset is used, or a range at the end of one
/// paragraph would cover the start of the next.
fn text_length(story: &Story) -> usize {
    story.text_len()
}

/// Fill several objects with a named ink.
///
/// Objects that cannot take a fill — a line, or anything that is not a
/// shape — are skipped rather than refused, so a mixed selection still does
/// the half of the work that makes sense. A user who selects a line and a
/// rectangle and clicks Cyan means the rectangle to be cyan; refusing
/// everything would be pedantic about a gesture that had an obvious
/// meaning.
///
/// **One edit per object**, for the same reason alignment is: they are
/// independent objects, and a user who filled two of five expects to be
/// able to take one back.
pub fn set_fill(
    document: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    ink_name: &str,
) -> bool {
    let Some(ink) = ink_by_name(document, ink_name) else {
        // An ink that is not in the document cannot be filled with: the
        // prepress stage has no plate for it, so applying it would produce
        // a document that cannot be output.
        return false;
    };
    set_fill_ink(document, history, ids, &ink)
}

/// Fill with the exact swatch definition, retaining the documented per-object
/// undo behavior of set_fill. Same-named swatches must remain distinguishable.
pub fn set_fill_ink(
    document: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    ink: &crate::Ink,
) -> bool {
    if !document.inks.contains(ink) {
        return false;
    }
    let mut applied: Vec<(usize, ObjectId, ObjectSnapshot)> = Vec::new();
    for (index, placed) in document.objects.iter().enumerate() {
        if !ids.contains(&placed.id) || document.object_locked(placed.id) {
            continue;
        }
        if !placed.supports_paint()
            || matches!(&placed.object, LayoutObject::Shape {path, ..} if !path_can_be_filled(path))
            || document.styles.object_paint(placed).fill_ink() == Some(ink)
        {
            continue;
        }
        let mut updated = placed.clone();
        updated.set_local_paint(&crate::ObjectPaint {
            fill: Some(crate::Paint::Ink(ink.clone())),
            ..Default::default()
        });
        applied.push((index, placed.id, snapshot_object(&updated)));
    }
    if applied.is_empty() {
        return false;
    }
    // The snapshots are of the *new* object, so they are the `after` of
    // each edit; the `before` is what is in the document right now.
    let mut edits = Vec::with_capacity(applied.len());
    for (index, id, after) in applied {
        let before = snapshot_object(&document.objects[index]);
        edits.push((id, before, after));
    }
    record_moves(document, history, edits)
}

/// Whether a path has an inside, and so can be filled.
///
/// An open path has no interior: a fill on it is a renderer-specific
/// surprise rather than anything a user asked for.
pub fn path_can_be_filled(path: &ShapePath) -> bool {
    path.subpaths.iter().any(|sub| sub.closed)
}

/// Which edge or middle a set of objects is lined up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    CentreX,
    Right,
    Top,
    MiddleY,
    Bottom,
}

/// Line several objects up, as the Align commands do.
///
/// **One edit per changed object.** This is the documented exception shared
/// with distribute and fill: individual object changes can be undone separately.
/// Aligning several frames can therefore require several undo steps.
///
/// Aligned to the *selection's* extent rather than the page, because that
/// is what a user who has selected a row of frames means: they share an
/// edge with each other. Aligning to the page is a different command, and
/// conflating the two is why so many editors need a modifier nobody can
/// remember.
pub fn align(
    document: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    how: Align,
) -> bool {
    let Some(moved) = aligned_positions(document, ids, how) else {
        return false;
    };
    if moved.is_empty() {
        // Already aligned. Recording an edit that moves nothing would put a
        // step on the stack which undoes nothing.
        return false;
    }
    // A single call still records one undo entry per changed object.
    record_moves(document, history, moved)
}

/// Where each object has to move to, and its two snapshots.
///
/// Returns `None` when any object is missing, so that a selection naming a
/// frame that has gone refuses the whole alignment rather than lining up
/// the survivors and leaving the user to guess which ones were meant.
fn aligned_positions(
    document: &LayoutDocument,
    ids: &[ObjectId],
    how: Align,
) -> Option<Vec<(ObjectId, ObjectSnapshot, ObjectSnapshot)>> {
    let mut objects = Vec::with_capacity(ids.len());
    for id in ids {
        let placed = document.object(*id)?;
        if document.object_locked(placed.id) {
            // A locked frame is not moved, and it is also not the target:
            // aligning to it would move everything else to a position
            // chosen by something the user cannot see or change.
            return None;
        }
        objects.push(placed);
    }
    if objects.is_empty() {
        return None;
    }
    let mut extent = objects[0].bounds;
    for placed in &objects[1..] {
        extent = extent.union(placed.bounds);
    }
    let target = match how {
        Align::Left => extent.x,
        Align::CentreX => extent.x + extent.width / 2.0,
        Align::Right => extent.right(),
        Align::Top => extent.y,
        Align::MiddleY => extent.y + extent.height / 2.0,
        Align::Bottom => extent.bottom(),
    };
    let mut out = Vec::with_capacity(objects.len());
    for placed in objects {
        let before = snapshot_object(placed);
        let mut after = before.clone();
        let bounds = match how {
            Align::Left | Align::CentreX | Align::Right => {
                let mut bounds = placed.bounds;
                bounds.x = match how {
                    Align::Left => target,
                    Align::CentreX => target - bounds.width / 2.0,
                    _ => target - bounds.width,
                };
                bounds
            }
            Align::Top | Align::MiddleY | Align::Bottom => {
                let mut bounds = placed.bounds;
                bounds.y = match how {
                    Align::Top => target,
                    Align::MiddleY => target - bounds.height / 2.0,
                    _ => target - bounds.height,
                };
                bounds
            }
        };
        if bounds == placed.bounds {
            // Already on the line, so nothing to record for it.
            continue;
        }
        after.bounds = [bounds.x, bounds.y, bounds.width, bounds.height];
        out.push((placed.id, before, after));
    }
    Some(out)
}

/// Spread objects out evenly between the first and the last.
///
/// Distributing needs three objects to mean anything: with two, the only
/// even spacing is the spacing they already have. Anything fewer is
/// refused rather than quietly doing nothing.
///
/// The run of objects fills the space between the first one's leading edge
/// and the last one's trailing edge; the two ends keep their positions.
/// The *gaps* are made even, not the centres: for objects of different
/// sizes, equal gaps is what reads as even.
/// Each changed object records its own undo entry, as documented for align.
pub fn distribute(
    document: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    horizontally: bool,
) -> bool {
    let Some(edits) = distributed(document, ids, horizontally) else {
        return false;
    };
    if edits.is_empty() {
        // Already evenly spread. Recording an edit that moves nothing would
        // put a step on the stack which undoes nothing.
        return false;
    }
    record_moves(document, history, edits)
}

/// Where each object has to move to for an even spread, or `None` if it
/// cannot be done.
fn distributed(
    document: &LayoutDocument,
    ids: &[ObjectId],
    horizontally: bool,
) -> Option<Vec<(ObjectId, ObjectSnapshot, ObjectSnapshot)>> {
    let mut objects = Vec::with_capacity(ids.len());
    for id in ids {
        let placed = document.object(*id)?;
        if document.object_locked(placed.id) {
            return None;
        }
        objects.push(placed);
    }
    if objects.len() < 3 {
        return None;
    }
    // Along the chosen axis, by the leading edge, so objects of different
    // sizes sort by where they start rather than by their middles.
    objects.sort_by(|a, b| {
        if horizontally {
            a.bounds.x.total_cmp(&b.bounds.x)
        } else {
            a.bounds.y.total_cmp(&b.bounds.y)
        }
    });
    let first = objects.first()?;
    let last = objects.last()?;
    let (start, span, used) = if horizontally {
        (
            first.bounds.x,
            last.bounds.right() - first.bounds.x,
            objects.iter().map(|o| o.bounds.width).sum::<f32>(),
        )
    } else {
        (
            first.bounds.y,
            last.bounds.bottom() - first.bounds.y,
            objects.iter().map(|o| o.bounds.height).sum::<f32>(),
        )
    };
    let gap = (span - used) / (objects.len() as f32 - 1.0);
    if gap < 0.0 {
        // The frames already overlap end to end, so there is no even
        // spacing to distribute them into. Refusing is better than pushing
        // them apart, which is a layout the user never asked for.
        return None;
    }

    let mut edits = Vec::with_capacity(objects.len());
    let mut cursor = start;
    for placed in &objects {
        let mut bounds = placed.bounds;
        if horizontally {
            bounds.x = cursor;
            cursor += bounds.width + gap;
        } else {
            bounds.y = cursor;
            cursor += bounds.height + gap;
        }
        if bounds == placed.bounds {
            // Already in the right place, so there is nothing to record
            // for it -- but the cursor still moves on.
            continue;
        }
        let before = snapshot_object(placed);
        let mut after = before.clone();
        after.bounds = [bounds.x, bounds.y, bounds.width, bounds.height];
        edits.push((placed.id, before, after));
    }
    Some(edits)
}

/// Record a set of object moves, one edit each, and say whether any stuck.
fn record_moves(
    document: &mut LayoutDocument,
    history: &mut History,
    edits: Vec<(ObjectId, ObjectSnapshot, ObjectSnapshot)>,
) -> bool {
    let mut any = false;
    for (id, before, after) in edits {
        let edit = LayoutEdit::ObjectChanged {
            id: id.0,
            before,
            after,
        };
        if crate::edit::forward(document, &edit) {
            history.record(edit);
            any = true;
        }
    }
    any
}

/// Delete several objects as one undo step.
///
/// **One edit, however many objects.** Deleting four frames as four edits
/// would make the user press Z four times to get back to where they
/// started, and an undo stack that does not match the gestures that filled
/// it is worse than no undo at all.
///
/// The whole deletion is refused if any object is missing or locked, rather
/// than removing the ones it can and leaving the user to work out what
/// happened to the rest. A delete that silently does half of what was asked
/// is the worst of the three outcomes.
///
/// The objects' stories are left alone, for the reason `delete` leaves one
/// alone.
pub fn delete_all(document: &mut LayoutDocument, history: &mut History, ids: &[ObjectId]) -> bool {
    // Nothing to do is not a change, and recording an empty edit would put
    // a step on the stack that undoes nothing.
    if ids.is_empty() {
        return false;
    }
    let mut items: Vec<(usize, ObjectSnapshot)> = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(index) = document.objects.iter().position(|object| object.id == *id) else {
            return false;
        };
        if document.object_locked(document.objects[index].id) {
            return false;
        }
        items.push((index, snapshot_object(&document.objects[index])));
    }
    // The same object twice would be removed once and then have no index
    // left to be removed from, so the list is deduplicated.
    items.sort_by_key(|(index, _)| *index);
    items.dedup_by_key(|(index, _)| *index);
    if items.is_empty() {
        return false;
    }
    let edit = LayoutEdit::RemovedObjects { items };
    if !crate::edit::forward(document, &edit) {
        return false;
    }
    history.record(edit);
    true
}

/// A frame's size, never smaller than a click target.
fn grown(bounds: Rect) -> Rect {
    Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(MIN_FRAME),
        bounds.height.max(MIN_FRAME),
    )
}

/// Add an object and record the edit that added it.
fn record_object(
    document: &mut LayoutDocument,
    history: &mut History,
    placed: PlacedObject,
) -> Option<ObjectId> {
    let id = placed.id;
    let edit = object_creation_edit(document, &placed)?;
    history.apply(document, edit).then_some(id)
}

fn object_creation_edit(document: &LayoutDocument, placed: &PlacedObject) -> Option<LayoutEdit> {
    if placed.page >= document.pages.len() {
        return None;
    }
    let layer = document
        .layers
        .iter()
        .copied()
        .find(|id| document.layer_visible(*id) && !document.layer_locked(*id))?;
    let before = crate::structure::Layers::of(document);
    let mut after = before.clone();
    after.objects.push((placed.id, layer));
    let mut created = document.creation_order.clone();
    created.push(placed.id);
    Some(LayoutEdit::Batch {
        edits: vec![
            LayoutEdit::AddedObject {
                index: document.objects.len(),
                object: snapshot_object(placed),
            },
            LayoutEdit::LayersChanged {
                before: Box::new(before),
                after: Box::new(after),
            },
            LayoutEdit::CreationOrderChanged {
                before: document.creation_order.clone(),
                after: created,
            },
        ],
    })
}

/// An ink by name, for a shape's fill or stroke.
fn ink_by_name(document: &LayoutDocument, name: &str) -> Option<crate::Ink> {
    document.inks.iter().find(|ink| ink.name == name).cloned()
}

/// Set a frame's text, as a text tool's typing does.
///
/// **One call is one edit.** A frame's text is replaced rather than
/// appended to, so this is a change of the whole text rather than an
/// insertion at the caret.
///
/// Coalescing a run of these into a single undo step is the caller's job,
/// not this function's, and the split is deliberate: this layer knows about
/// documents and edits, and knows nothing about when a user has finished
/// typing. `schist-layout` has no timer and no window. A caller that types
/// a word at a time records one edit per word, and undo takes the user back
/// a word at a time.
///
/// The caret is not part of the document, so a second call at a different
/// offset in the same frame is a genuinely different edit, as it should be.
pub fn set_text(
    document: &mut LayoutDocument,
    history: &mut History,
    story: StoryId,
    text: impl Into<String>,
) -> bool {
    let Some(existing) = document.story(story) else {
        return false;
    };
    let text = text.into();
    let old = existing.text();
    if old == text {
        return false;
    }
    let prefix = old
        .chars()
        .zip(text.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(text[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    replace_text(
        document,
        history,
        story,
        prefix..old.len() - suffix,
        &text[prefix..text.len() - suffix],
    )
}

/// One insertion/replacement, with the caller's exact story byte range.
pub fn replace_text(
    document: &mut LayoutDocument,
    history: &mut History,
    story: StoryId,
    range: std::ops::Range<usize>,
    text: &str,
) -> bool {
    if document
        .story_frames(story)
        .iter()
        .any(|o| document.object_locked(o.id))
    {
        return false;
    }
    let Some(existing) = document.story(story) else {
        return false;
    };
    let Some(after) = existing.replace_text(range, text, &document.default_paragraph_style) else {
        return false;
    };
    if after == *existing {
        return false;
    }
    history.apply(
        document,
        LayoutEdit::StoryChanged {
            id: story.0,
            before: snapshot_story(existing),
            after: snapshot_story(&after),
        },
    )
}

/// Use exactly the same byte coordinate system as composition and styles.
pub fn text_of(document: &LayoutDocument, story: StoryId) -> String {
    document.story(story).map(Story::text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StoryPoint, StyleRange};

    /// A document with one page and an empty history.
    fn blank() -> (LayoutDocument, History) {
        (crate::blank_a4(), History::default())
    }

    #[test]
    fn a_text_frame_comes_with_a_story_to_type_into() {
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(50.0, 60.0, 200.0, 40.0),
        )
        .expect("a frame is created");
        let placed = document.object(frame.object).expect("it is there");
        let LayoutObject::TextFrame { story, .. } = &placed.object else {
            panic!("expected a text frame");
        };
        // The frame and the story have to point at each other, or the
        // frame is a box with nothing in it.
        assert_eq!(*story, frame.story);
        assert!(document.story(frame.story).is_some());
        assert_eq!(placed.bounds, Rect::new(50.0, 60.0, 200.0, 40.0));
        assert_eq!(history.undo_depth(), 1, "creating is one undo step");
    }

    #[test]
    fn a_frame_drawn_as_a_click_is_still_clickable() {
        // A click rather than a drag makes a frame of no size, and a
        // frame of no size cannot be selected — which looks like the tool
        // not working.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(10.0, 10.0, 0.0, 0.0),
        )
        .expect("a frame is created");
        let placed = document.object(frame.object).expect("it is there");
        assert!(placed.bounds.width >= MIN_FRAME);
        assert!(placed.bounds.height >= MIN_FRAME);
    }

    #[test]
    fn a_rectangles_outline_is_relative_to_its_own_origin() {
        // So resizing a frame does not have to rewrite its geometry, the
        // same convention a group's children use.
        let (mut document, mut history) = blank();
        let id = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(20.0, 30.0, 100.0, 50.0),
            Paint::none(),
        )
        .expect("a rectangle is created");
        let placed = document.object(id).expect("it is there");
        let Some(path) = placed.object.editable_path() else {
            panic!("expected a shape");
        };
        let points = &path.subpaths[0].points;
        assert_eq!(points[0], Point::ZERO, "the outline starts at the origin");
        assert_eq!(points[2], Point::new(100.0, 50.0), "and spans the frame");
    }

    #[test]
    fn a_shape_filled_by_name_uses_the_document_s_ink() {
        let (mut document, mut history) = blank();
        let id = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Paint::filled("Cyan"),
        )
        .expect("a rectangle is created");
        let placed = document.object(id).expect("it is there");
        let LayoutObject::Shape { fill, .. } = &placed.object else {
            panic!("expected a shape");
        };
        assert_eq!(fill.as_ref().map(|ink| ink.name.as_str()), Some("Cyan"));
    }

    #[test]
    fn deleting_a_frame_undoes_in_one_press() {
        // One press, not two. A delete that takes two presses to reverse
        // is a bug that feels like the undo stack is lying.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
        )
        .expect("a frame is created");
        document.stories[frame.story.0 as usize]
            .points
            .push(StoryPoint::Paragraph {
                text: "hello".into(),
                style: "Body".into(),
            });
        let before = document.clone();
        let after_create = history.undo_depth();

        assert!(delete(&mut document, &mut history, frame.object));
        assert!(
            history.undo_depth() == after_create + 1,
            "a delete is exactly one more undo step"
        );
        assert!(document.object(frame.object).is_none(), "the frame is gone");

        history.undo(&mut document);
        assert!(document.object(frame.object).is_some(), "the frame is back");
        assert_eq!(document, before, "one undo returned the document exactly");
    }

    #[test]
    fn a_locked_frame_is_not_deleted() {
        // A user can delete something they cannot move, which is a trap
        // with no way out.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
        )
        .expect("a frame is created");
        if let Some(placed) = document
            .objects
            .iter_mut()
            .find(|object| object.id == frame.object)
        {
            placed.locked = true;
        }
        assert!(!delete(&mut document, &mut history, frame.object));
        assert!(document.object(frame.object).is_some());
    }

    #[test]
    fn deleting_something_that_is_not_there_changes_nothing() {
        let (mut document, mut history) = blank();
        let before = document.clone();
        assert!(!delete(&mut document, &mut history, ObjectId::next()));
        assert_eq!(document, before);
        assert_eq!(history.undo_depth(), 0);
    }

    #[test]
    fn several_frames_each_get_their_own_undo_step() {
        // One gesture per frame, so undo takes back the last one made
        // rather than all of them at once.
        let (mut document, mut history) = blank();
        for index in 0..3 {
            text_frame(
                &mut document,
                &mut history,
                0,
                Rect::new(index as Pt * 10.0, 0.0, 50.0, 20.0),
            )
            .expect("a frame is created");
        }
        assert_eq!(history.undo_depth(), 3);
        history.undo(&mut document);
        assert_eq!(document.objects.len(), 2);
    }

    #[test]
    fn typing_replaces_a_frames_text_and_keeps_its_style() {
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
        )
        .expect("a frame is created");
        // A styled frame, so the style has somewhere to be lost.
        if let Some(style) = document.styles.paragraphs.first_mut() {
            style.name = "Body".into();
        }
        assert!(set_text(&mut document, &mut history, frame.story, "Hello"));
        assert_eq!(text_of(&document, frame.story), "Hello");
        let StoryPoint::Paragraph { style, .. } = &document.story(frame.story).unwrap().points[0]
        else {
            panic!("expected a paragraph");
        };
        assert_eq!(style, "Body", "typing does not drop the frame's style");
    }

    #[test]
    fn each_set_text_call_records_its_own_undo_step() {
        // No coalescing is implied by the whole-text setter.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
        )
        .expect("a frame is created");
        let before = history.undo_depth();
        set_text(&mut document, &mut history, frame.story, "H");
        set_text(&mut document, &mut history, frame.story, "He");
        set_text(&mut document, &mut history, frame.story, "Hello");
        assert_eq!(history.undo_depth(), before + 3, "each call is one step");
        history.undo(&mut document);
        assert_eq!(text_of(&document, frame.story), "He");
    }

    #[test]
    fn a_character_range_survives_typing_over_its_text() {
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
        )
        .expect("a frame is created");
        set_text(&mut document, &mut history, frame.story, "Hello");
        document.stories[frame.story.0 as usize]
            .ranges
            .push(StyleRange::new(0, 2, "Emphasis"));
        set_text(&mut document, &mut history, frame.story, "Hello world");
        let ranges = &document.story(frame.story).unwrap().ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].end <= 11, "a range past the new text is not kept");
    }

    #[test]
    fn typing_into_a_story_that_is_not_there_changes_nothing() {
        let (mut document, mut history) = blank();
        let before = document.clone();
        assert!(!set_text(&mut document, &mut history, StoryId(404), "x"));
        assert_eq!(document, before);
        assert_eq!(history.undo_depth(), 0);
    }

    fn shape_of(
        document: &mut LayoutDocument,
        kind: ShapeKind,
        bounds: Rect,
    ) -> (ObjectId, ShapePath, Rect) {
        let object = shape(
            document,
            &mut History::default(),
            0,
            bounds,
            kind,
            Paint::none(),
        )
        .expect("a shape");
        let placed = document.object(object).expect("it is there");
        let LayoutObject::Shape { path, .. } = placed.object.clone() else {
            panic!("expected a shape");
        };
        (object, path, placed.bounds)
    }

    #[test]
    fn an_ellipse_is_four_editable_cubic_arcs() {
        let (mut document, _) = blank();
        let (_, path, bounds) = shape_of(
            &mut document,
            ShapeKind::Ellipse,
            Rect::new(0.0, 0.0, 200.0, 100.0),
        );
        let subpath = &path.subpaths[0];
        assert!(subpath.closed, "an ellipse is a closed path");
        assert_eq!(subpath.points.len(), 4);
        assert_eq!(subpath.handles.len(), 4);
        // It must fill its frame, or the ink would not reach where the
        // user drew to.
        let outline = path.bounds();
        assert!(
            (outline.width - bounds.width).abs() < 0.5,
            "fills the width"
        );
        assert!(
            (outline.height - bounds.height).abs() < 0.5,
            "fills the height"
        );
    }

    #[test]
    fn a_polygon_has_the_number_of_sides_asked_for() {
        let (mut document, _) = blank();
        let (_, path, _) = shape_of(
            &mut document,
            ShapeKind::Polygon { sides: 5 },
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        let subpath = &path.subpaths[0];
        assert_eq!(subpath.points.len(), 5);
        assert!(subpath.closed);
        // A polygon has at least three sides whatever it is asked for: a
        // two-sided "polygon" is a line, and the tool that draws it is a
        // different one.
        let (_, path, _) = shape_of(
            &mut document,
            ShapeKind::Polygon { sides: 1 },
            Rect::new(0.0, 0.0, 100.0, 100.0),
        );
        assert_eq!(path.subpaths[0].points.len(), 3);
    }

    #[test]
    fn a_line_is_an_open_path_from_corner_to_corner() {
        let (mut document, _) = blank();
        let (_, path, bounds) = shape_of(
            &mut document,
            ShapeKind::Line,
            Rect::new(10.0, 20.0, 200.0, 50.0),
        );
        let subpath = &path.subpaths[0];
        assert!(!subpath.closed, "a line has no inside to close");
        assert_eq!(subpath.points, vec![Point::ZERO, Point::new(200.0, 50.0)]);
        // And the frame is its bounding box, so a click anywhere in that
        // box reaches the line.
        assert_eq!(bounds, Rect::new(10.0, 20.0, 200.0, 50.0));
    }

    #[test]
    fn a_line_cannot_be_filled() {
        // An open path has no inside, so a fill is not something a user
        // could have meant.
        let (mut document, _) = blank();
        let ink_name = document.inks[0].name.clone();
        let object = shape(
            &mut document,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 100.0, 0.0),
            ShapeKind::Line,
            Paint::filled(ink_name),
        )
        .expect("a line");
        let LayoutObject::Shape { fill, .. } = &document.object(object).unwrap().object else {
            panic!("expected a shape");
        };
        assert!(fill.is_none(), "a fill on a line is not applied anyway");
    }

    #[test]
    fn a_shape_is_named_for_its_kind() {
        let (mut document, _) = blank();
        for (kind, name) in [
            (ShapeKind::Rectangle, "Rectangle"),
            (ShapeKind::Ellipse, "Ellipse"),
            (ShapeKind::Line, "Line"),
            (ShapeKind::Polygon { sides: 6 }, "Polygon"),
        ] {
            let object = shape(
                &mut document,
                &mut History::default(),
                0,
                Rect::new(0.0, 0.0, 50.0, 50.0),
                kind,
                Paint::none(),
            )
            .expect("a shape");
            assert_eq!(document.object(object).unwrap().name, name);
        }
    }

    #[test]
    fn a_duplicate_sits_clear_of_what_it_was_copied_from() {
        let (mut document, mut history) = blank();
        let first = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(10.0, 20.0, 100.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let copy = duplicate(&mut document, &mut history, first).expect("a copy");
        let original_bounds = document.object(first).expect("the original").bounds;
        let copy_bounds = document.object(copy).expect("the copy").bounds;
        assert_ne!(copy, first, "a copy has its own id");
        assert_eq!(copy_bounds.x, original_bounds.x + DUPLICATE_OFFSET);
        assert_eq!(copy_bounds.y, original_bounds.y + DUPLICATE_OFFSET);
        // Clear of it, so a click can tell them apart.
        assert!(copy_bounds.x > original_bounds.x);
    }

    #[test]
    fn a_duplicated_text_frame_does_not_share_its_story() {
        // Sharing would make typing in the copy change the original, which
        // is a link the user never asked for and cannot see.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 40.0),
        )
        .expect("a frame");
        set_text(&mut document, &mut history, frame.story, "original");
        let copy = duplicate(&mut document, &mut history, frame.object).expect("a copy");
        let copied_story = match &document.object(copy).expect("the copy").object {
            LayoutObject::TextFrame { story, .. } => *story,
            _ => panic!("expected a text frame"),
        };
        assert_ne!(copied_story, frame.story, "a copy has its own story");
        assert_eq!(
            text_of(&document, copied_story),
            "original",
            "with the same text"
        );
        set_text(&mut document, &mut history, copied_story, "changed");
        assert_eq!(
            text_of(&document, frame.story),
            "original",
            "the original is untouched"
        );
    }

    #[test]
    fn undoing_a_duplicate_takes_one_press() {
        let (mut document, mut history) = blank();
        let first = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let before = history.undo_depth();
        duplicate(&mut document, &mut history, first).expect("a copy");
        assert_eq!(history.undo_depth(), before + 1);
        history.undo(&mut document);
        assert_eq!(document.objects.len(), 1, "one undo removed the copy");
    }

    #[test]
    fn deleting_several_objects_is_one_undo_step() {
        // The whole point of the batch edit. Four frames, one gesture,
        // one press to get back.
        let (mut document, mut history) = blank();
        let mut made = Vec::new();
        for i in 0..4 {
            made.push(
                rectangle(
                    &mut document,
                    &mut history,
                    0,
                    Rect::new(i as f32 * 100.0, 0.0, 50.0, 50.0),
                    Paint::none(),
                )
                .expect("a shape"),
            );
        }
        let before = document.clone();
        let depth = history.undo_depth();
        assert!(delete_all(&mut document, &mut history, &made));
        assert_eq!(history.undo_depth(), depth + 1, "one step for four frames");
        assert!(document.objects.is_empty());
        history.undo(&mut document);
        assert_eq!(document, before, "one undo brings all four back");
    }

    #[test]
    fn a_batch_delete_puts_the_objects_back_in_their_original_order() {
        // Removing back to front and reinserting front to back is the
        // whole subtlety; getting it wrong leaves the z-order shuffled,
        // which is invisible until someone drags one of them.
        let (mut document, mut history) = blank();
        let mut made = Vec::new();
        for i in 0..5 {
            made.push(
                shape(
                    &mut document,
                    &mut history,
                    0,
                    Rect::new(0.0, i as f32 * 60.0, 50.0, 50.0),
                    ShapeKind::Rectangle,
                    Paint::none(),
                )
                .expect("a shape"),
            );
        }
        // Delete an odd subset, in an order that is not the document's.
        let doomed = vec![made[3], made[0], made[4]];
        let survivors: Vec<ObjectId> = made
            .iter()
            .copied()
            .filter(|id| !doomed.contains(id))
            .collect();
        assert!(delete_all(&mut document, &mut history, &doomed));
        assert_eq!(
            document.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            survivors,
            "the survivors are untouched"
        );
        history.undo(&mut document);
        assert_eq!(
            document.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            made,
            "and come back in the order they went in"
        );
    }

    #[test]
    fn a_batch_delete_refuses_rather_than_deleting_half() {
        // Half a delete is the worst outcome: the user asked for four
        // frames to go and two did, with no explanation.
        let (mut document, mut history) = blank();
        let a = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let b = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(60.0, 0.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        document.objects[1].locked = true;
        let before = document.clone();
        assert!(!delete_all(&mut document, &mut history, &[a, b]));
        assert_eq!(document, before, "nothing was removed");
        assert!(
            !delete_all(&mut document, &mut history, &[a, ObjectId(9999)]),
            "a missing object refuses it too"
        );
        assert_eq!(document, before);
    }

    #[test]
    fn deleting_nothing_records_nothing() {
        let (mut document, mut history) = blank();
        assert!(!delete_all(&mut document, &mut history, &[]));
        assert_eq!(history.undo_depth(), 0, "an empty edit would undo nothing");
    }

    #[test]
    fn moving_an_anchor_preserves_all_other_page_space_points() {
        // Local geometry is rebased when its bounds change, so the other
        // anchors must retain their page-space positions.
        let (mut document, mut history) = blank();
        let object = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(100.0, 100.0, 200.0, 100.0),
            ShapeKind::Rectangle,
            Paint::none(),
        )
        .expect("a shape");
        let frame_before = document.object(object).expect("it is there").bounds;
        let LayoutObject::Shape { path, .. } = document.object(object).unwrap().object.clone()
        else {
            panic!("expected a shape");
        };
        let others_before = path.subpaths[0].points.clone();

        // Drag the top-left corner out past the frame's own edge.
        assert!(move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 0,
                index: 0,
                part: PointPart::Anchor
            },
            Point::new(50.0, 40.0),
        ));
        let placed = document.object(object).expect("it is there");
        assert_eq!(
            placed.bounds,
            Rect::new(50.0, 40.0, 250.0, 160.0),
            "bounds follow the edited path"
        );
        let Some(path) = placed.object.editable_path() else {
            panic!("expected a shape");
        };
        // The local origin changed; the other anchors stay in page space.
        assert_eq!(
            path.subpaths[0].points[0] + placed.bounds.origin(),
            Point::new(50.0, 40.0)
        );
        for (index, point) in path.subpaths[0].points.iter().enumerate().skip(1) {
            assert_eq!(
                *point + placed.bounds.origin(),
                others_before[index] + frame_before.origin(),
                "the other points held in page space"
            );
        }
    }

    #[test]
    fn moving_a_point_is_one_undo_step() {
        let (mut document, mut history) = blank();
        let object = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            ShapeKind::Rectangle,
            Paint::none(),
        )
        .expect("a shape");
        let before = document.clone();
        let depth = history.undo_depth();
        assert!(move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 0,
                index: 1,
                part: PointPart::Anchor
            },
            Point::new(250.0, 100.0),
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        history.undo(&mut document);
        assert_eq!(document, before, "one undo put the point back");
    }

    #[test]
    fn a_point_dropped_where_it_started_records_nothing() {
        let (mut document, mut history) = blank();
        let object = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            ShapeKind::Rectangle,
            Paint::none(),
        )
        .expect("a shape");
        let before = document.clone();
        let depth = history.undo_depth();
        assert!(!move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 0,
                index: 1,
                part: PointPart::Anchor
            },
            Point::new(200.0, 0.0),
        ));
        assert_eq!(document, before);
        assert_eq!(history.undo_depth(), depth, "a click is not a move");
    }

    #[test]
    fn a_point_that_is_not_there_is_refused() {
        let (mut document, mut history) = blank();
        let object = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            ShapeKind::Rectangle,
            Paint::none(),
        )
        .expect("a shape");
        let before = document.clone();
        assert!(!move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 9,
                index: 9,
                part: PointPart::Anchor
            },
            Point::new(1.0, 1.0),
        ));
        assert!(!move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 0,
                index: 99,
                part: PointPart::Anchor
            },
            Point::new(1.0, 1.0),
        ));
        assert!(!move_point(
            &mut document,
            &mut history,
            ObjectId(9999),
            PointRef {
                subpath: 0,
                index: 0,
                part: PointPart::Anchor
            },
            Point::new(1.0, 1.0),
        ));
        assert_eq!(document, before);
    }

    #[test]
    fn a_point_of_a_locked_shape_does_not_move() {
        let (mut document, mut history) = blank();
        let object = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            ShapeKind::Rectangle,
            Paint::none(),
        )
        .expect("a shape");
        document.objects[0].locked = true;
        let before = document.clone();
        assert!(!move_point(
            &mut document,
            &mut history,
            object,
            PointRef {
                subpath: 0,
                index: 0,
                part: PointPart::Anchor
            },
            Point::new(1.0, 1.0),
        ));
        assert_eq!(document, before);
    }

    #[test]
    fn a_text_frame_has_no_points_to_move() {
        // A frame is a box, not an outline, so a corner is not one of its
        // points and the tool must not pretend otherwise.
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 100.0),
        )
        .expect("a frame");
        assert!(!move_point(
            &mut document,
            &mut history,
            frame.object,
            PointRef {
                subpath: 0,
                index: 0,
                part: PointPart::Anchor
            },
            Point::new(1.0, 1.0),
        ));
    }

    /// Three frames in a row, unevenly spaced, for align and distribute.
    fn three_in_a_row() -> (LayoutDocument, History, Vec<ObjectId>) {
        let (mut document, mut history) = blank();
        let mut made = Vec::new();
        // 10..50, 100..120, 130..190: the gaps are 50 and 10, so there is
        // something to fix.
        for (x, width) in [(10.0, 40.0), (100.0, 20.0), (130.0, 60.0)] {
            made.push(
                rectangle(
                    &mut document,
                    &mut history,
                    0,
                    Rect::new(x, 50.0, width, 20.0),
                    Paint::none(),
                )
                .expect("a shape"),
            );
        }
        (document, history, made)
    }

    #[test]
    fn aligning_lines_the_selection_up_with_itself_not_the_page() {
        // A user who selects a row of frames means they share an edge with
        // each other. Aligning to the page is a different command.
        let (mut document, mut history, made) = three_in_a_row();
        assert!(align(&mut document, &mut history, &made, Align::Left));
        let xs: Vec<f32> = made
            .iter()
            .map(|id| document.object(*id).expect("there").bounds.x)
            .collect();
        assert_eq!(xs, vec![10.0, 10.0, 10.0], "all on the left-most edge");
    }

    #[test]
    fn every_edge_aligns_to_the_right_measurement() {
        // The selection spans 10..190, so the measurements are its left
        // edge, its middle and its right edge.
        let cases = [
            (Align::Left, 10.0),
            (Align::CentreX, 100.0),
            (Align::Right, 190.0),
        ];
        for (how, x) in cases {
            let (mut document, mut history, made) = three_in_a_row();
            assert!(align(&mut document, &mut history, &made, how), "{how:?}");
            for id in &made {
                let bounds = document.object(*id).expect("there").bounds;
                match how {
                    Align::Left => assert_eq!(bounds.x, x),
                    Align::CentreX => {
                        assert!((bounds.x + bounds.width / 2.0 - x).abs() < 0.01)
                    }
                    _ => assert!((bounds.right() - x).abs() < 0.01),
                }
            }
        }
    }

    #[test]
    fn aligning_one_object_to_itself_moves_nothing() {
        let (mut document, mut history, made) = three_in_a_row();
        let before = document.clone();
        let depth = history.undo_depth();
        assert!(!align(&mut document, &mut history, &made[..1], Align::Left));
        assert_eq!(document, before);
        assert_eq!(history.undo_depth(), depth, "nothing to undo");
    }

    #[test]
    fn a_locked_frame_refuses_the_whole_alignment() {
        // A locked frame is not moved, and it is not the target either:
        // aligning to something the user cannot see or change is how a
        // layout ends up somewhere nobody chose.
        let (mut document, mut history, made) = three_in_a_row();
        document
            .objects
            .iter_mut()
            .find(|o| o.id == made[0])
            .expect("there")
            .locked = true;
        let before = document.clone();
        assert!(!align(&mut document, &mut history, &made, Align::Right));
        assert_eq!(document, before);
    }

    #[test]
    fn distributing_makes_the_gaps_even_and_leaves_the_ends_alone() {
        let (mut document, mut history, made) = three_in_a_row();
        assert!(distribute(&mut document, &mut history, &made, true));
        let bounds: Vec<Rect> = made
            .iter()
            .map(|id| document.object(*id).expect("there").bounds)
            .collect();
        // The first and last keep their outer edges...
        assert_eq!(bounds[0].x, 10.0);
        assert_eq!(bounds[2].right(), 190.0);
        // ...and the gap between the first and second equals the gap
        // between the second and third.
        let first_gap = bounds[1].x - bounds[0].right();
        let second_gap = bounds[2].x - bounds[1].right();
        assert!(
            (first_gap - second_gap).abs() < 0.01,
            "{first_gap} vs {second_gap}"
        );
        assert!(first_gap > 0.0, "and they really are spread");
        assert_eq!(bounds[1].x, 80.0, "the middle frame moved to close the gap");
    }

    #[test]
    fn distributing_down_a_page_works_too() {
        let (mut document, mut history) = blank();
        let mut made = Vec::new();
        for y in [10.0, 40.0, 300.0] {
            made.push(
                rectangle(
                    &mut document,
                    &mut history,
                    0,
                    Rect::new(20.0, y, 40.0, 20.0),
                    Paint::none(),
                )
                .expect("a shape"),
            );
        }
        assert!(distribute(&mut document, &mut history, &made, false));
        let ys: Vec<f32> = made
            .iter()
            .map(|id| document.object(*id).expect("there").bounds.y)
            .collect();
        // 10..30, 40..60, 300..320: span 310, used 60, so each gap is 125.
        assert_eq!(ys, vec![10.0, 155.0, 300.0]);
    }

    #[test]
    fn two_objects_cannot_be_distributed() {
        // The only even spacing for two is the one they already have.
        let (mut document, mut history, made) = three_in_a_row();
        let before = document.clone();
        assert!(!distribute(&mut document, &mut history, &made[..2], true));
        assert_eq!(document, before);
    }

    #[test]
    fn overlapping_frames_are_not_pushed_apart() {
        // Distributing them would invent a layout the user never asked
        // for, so it refuses instead.
        let (mut document, mut history) = blank();
        let mut made = Vec::new();
        for x in [0.0, 10.0, 20.0] {
            made.push(
                rectangle(
                    &mut document,
                    &mut history,
                    0,
                    Rect::new(x, 0.0, 50.0, 20.0),
                    Paint::none(),
                )
                .expect("a shape"),
            );
        }
        let before = document.clone();
        assert!(!distribute(&mut document, &mut history, &made, true));
        assert_eq!(document, before);
    }

    #[test]
    fn filling_a_shape_sets_the_named_ink() {
        let (mut document, mut history) = blank();
        let cyan = document.inks[0].name.clone();
        let object = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        assert!(set_fill(&mut document, &mut history, &[object], &cyan));
        let LayoutObject::Shape { fill, .. } = &document.object(object).unwrap().object else {
            panic!("expected a shape");
        };
        assert_eq!(
            fill.as_ref().map(|ink| ink.name.as_str()),
            Some(cyan.as_str())
        );
    }

    #[test]
    fn filling_is_one_undo_step_per_object() {
        // Deliberately *not* one step for the pair: a user who filled two
        // of five shapes expects to be able to take one back without
        // losing the other.
        let (mut document, mut history) = blank();
        let black = document.inks[0].name.clone();
        let a = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let b = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(60.0, 0.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let before = document.clone();
        let depth = history.undo_depth();
        assert!(set_fill(&mut document, &mut history, &[a, b], &black));
        assert_eq!(history.undo_depth(), depth + 2, "two objects, two steps");

        // Undo takes the last one back, which is the second shape -- the
        // first keeps its fill, because the two are independent objects.
        history.undo(&mut document);
        assert!(
            fill_of(&document, b).flatten().is_none(),
            "the shape filled last was taken back"
        );
        assert!(
            fill_of(&document, a).flatten().is_some(),
            "the other one kept its fill"
        );
        history.undo(&mut document);
        assert_eq!(document, before);
    }

    /// A shape's fill name.
    ///
    /// Nested on purpose: the outer `Option` says "this is a shape at
    /// all", which is a different question from "does it have a fill", and
    /// collapsing the two makes a typo read as a passing assertion.
    fn fill_of(document: &LayoutDocument, id: ObjectId) -> Option<Option<String>> {
        let LayoutObject::Shape { fill, .. } = &document.object(id)?.object else {
            return None;
        };
        Some(fill.as_ref().map(|ink| ink.name.clone()))
    }

    #[test]
    fn an_ink_the_document_does_not_have_cannot_be_filled_with() {
        // The prepress stage would have no plate for it, so this would
        // produce a document that cannot be output.
        let (mut document, mut history) = blank();
        let object = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        let before = document.clone();
        assert!(!set_fill(
            &mut document,
            &mut history,
            &[object],
            "PANTONE 123"
        ));
        assert_eq!(document, before);
    }

    #[test]
    fn a_line_is_not_filled() {
        // An open path has no inside, so there is nothing to fill.
        let (mut document, mut history) = blank();
        let cyan = document.inks[0].name.clone();
        let line = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
            ShapeKind::Line,
            Paint::none(),
        )
        .expect("a line");
        let before = document.clone();
        assert!(!set_fill(&mut document, &mut history, &[line], &cyan));
        assert_eq!(document, before);
    }

    #[test]
    fn a_mixed_selection_fills_the_half_that_takes_a_fill() {
        // A user who selects a line and a rectangle and clicks Cyan means
        // the rectangle.
        let (mut document, mut history) = blank();
        let cyan = document.inks[0].name.clone();
        let line = shape(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 0.0),
            ShapeKind::Line,
            Paint::none(),
        )
        .expect("a line");
        let box_shape = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 200.0, 100.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        assert!(set_fill(
            &mut document,
            &mut history,
            &[line, box_shape],
            &cyan
        ));
        let LayoutObject::Shape { fill, .. } = &document.object(box_shape).unwrap().object else {
            panic!("expected a shape");
        };
        assert!(fill.is_some(), "the shape was filled");
        let LayoutObject::Shape { fill, .. } = &document.object(line).unwrap().object else {
            panic!("expected a shape");
        };
        assert!(fill.is_none(), "the line was left alone");
    }

    #[test]
    fn filling_a_frame_that_is_already_that_colour_records_nothing() {
        let (mut document, mut history) = blank();
        let cyan = document.inks[0].name.clone();
        let object = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Paint::filled(cyan.clone()),
        )
        .expect("a shape");
        let depth = history.undo_depth();
        assert!(!set_fill(&mut document, &mut history, &[object], &cyan));
        assert_eq!(
            history.undo_depth(),
            depth,
            "nothing changed, nothing to undo"
        );
    }

    /// A text frame whose story has one paragraph, so style tests have
    /// something to change.
    fn styled_frame(style: &str, text: &str) -> (LayoutDocument, History, TextFrame) {
        let (mut document, mut history) = blank();
        let frame = text_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 200.0, 60.0),
        )
        .expect("a frame");
        set_text(&mut document, &mut history, frame.story, text);
        if let Some(point) = document.stories[frame.story.0 as usize]
            .points
            .iter_mut()
            .find(|p| matches!(p, StoryPoint::Paragraph { .. }))
        {
            let StoryPoint::Paragraph { style: s, .. } = point else {
                unreachable!()
            };
            *s = style.into();
        }
        (document, history, frame)
    }

    /// The style of a frame's only paragraph.
    fn paragraph_style_of(document: &LayoutDocument, frame: &TextFrame) -> String {
        let StoryPoint::Paragraph { style, .. } = &document
            .story(frame.story)
            .expect("the story is there")
            .points
            .iter()
            .find(|p| matches!(p, StoryPoint::Paragraph { .. }))
            .expect("a paragraph")
        else {
            panic!("expected a paragraph");
        };
        style.clone()
    }

    #[test]
    fn applying_a_paragraph_style_changes_the_frames_text() {
        let (mut document, mut history, frame) = styled_frame("Body", "hello");
        assert!(set_paragraph_style(
            &mut document,
            &mut history,
            &[frame.object],
            "Default"
        ));
        assert_eq!(paragraph_style_of(&document, &frame), "Default");
    }

    #[test]
    fn a_style_the_document_does_not_define_is_refused() {
        // Applying a name that resolves to nothing would leave the text
        // looking as it did, which is the hardest kind of nothing to
        // notice.
        let (mut document, mut history, frame) = styled_frame("Body", "hello");
        let before = document.clone();
        assert!(!set_paragraph_style(
            &mut document,
            &mut history,
            &[frame.object],
            "Nonexistent"
        ));
        assert_eq!(document, before);
    }

    #[test]
    fn applying_the_style_a_frame_already_has_records_nothing() {
        let (mut document, mut history, frame) = styled_frame("Body", "hello");
        let depth = history.undo_depth();
        assert!(!set_paragraph_style(
            &mut document,
            &mut history,
            &[frame.object],
            "Body"
        ));
        assert_eq!(history.undo_depth(), depth);
    }

    #[test]
    fn a_mixed_selection_restyles_the_frames_in_it() {
        // A selection is usually whatever was selected, and the half that
        // can take a style should still get one.
        let (mut document, mut history, frame) = styled_frame("Body", "hello");
        let shape = rectangle(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 200.0, 50.0, 50.0),
            Paint::none(),
        )
        .expect("a shape");
        assert!(set_paragraph_style(
            &mut document,
            &mut history,
            &[shape, frame.object],
            "Default"
        ));
        assert_eq!(paragraph_style_of(&document, &frame), "Default");
    }

    #[test]
    fn applying_a_character_style_marks_the_range_it_was_given() {
        let (mut document, mut history, frame) = styled_frame("Body", "Hello");
        assert!(set_character_style(
            &mut document,
            &mut history,
            frame.story,
            0..2,
            "Bold"
        ));
        let story = document.story(frame.story).expect("the story");
        assert_eq!(story.ranges.len(), 1);
        assert_eq!(story.ranges[0].style, "Bold");
        assert_eq!((story.ranges[0].start, story.ranges[0].end), (0, 2));
    }

    #[test]
    fn a_character_range_off_the_end_of_the_text_is_clamped() {
        // A selection that runs past the end of a frame is still a
        // selection, and the part that exists is what the user meant.
        let (mut document, mut history, frame) = styled_frame("Body", "Hello");
        assert!(set_character_style(
            &mut document,
            &mut history,
            frame.story,
            3..900,
            "Bold"
        ));
        let story = document.story(frame.story).expect("the story");
        assert_eq!((story.ranges[0].start, story.ranges[0].end), (3, 5));
    }

    #[test]
    fn a_character_range_over_another_splits_it() {
        // Overlapping ranges are not a thing: a style has to be one style
        // over one span, or nothing can be said about where the boundary
        // between them is. So applying one over another trims rather than
        // dropping.
        let (mut document, mut history, frame) = styled_frame("Body", "Hello world");
        // A blank document has no Italic, and a style it does not define
        // is refused rather than applied as a name that resolves to
        // nothing -- so the test has to define one.
        document.styles.add_character(crate::CharacterStyle {
            name: "Italic".into(),
            italic: Some(true),
            ..Default::default()
        });
        assert!(set_character_style(
            &mut document,
            &mut history,
            frame.story,
            0..5,
            "Bold"
        ));
        assert!(set_character_style(
            &mut document,
            &mut history,
            frame.story,
            3..8,
            "Italic"
        ));
        let story = document.story(frame.story).expect("the story");
        let spans: Vec<(usize, usize, &str)> = story
            .ranges
            .iter()
            .map(|r| (r.start, r.end, r.style.as_str()))
            .collect();
        // The first range ended at 5, inside the new one, so only its left
        // remainder survives. The tail from 8 to the end was never styled
        // and stays that way -- restyling part of a range must not extend
        // it past what the user selected.
        assert_eq!(spans, vec![(0, 3, "Bold"), (3, 8, "Italic")]);
    }

    #[test]
    fn a_character_style_already_covering_the_range_records_nothing() {
        let (mut document, mut history, frame) = styled_frame("Body", "Hello world");
        set_character_style(&mut document, &mut history, frame.story, 0..6, "Bold");
        let depth = history.undo_depth();
        assert!(!set_character_style(
            &mut document,
            &mut history,
            frame.story,
            1..4,
            "Bold"
        ));
        assert_eq!(history.undo_depth(), depth);
    }

    #[test]
    fn a_character_style_on_nothing_is_not_a_change() {
        let (mut document, mut history, frame) = styled_frame("Body", "Hello");
        let before = document.clone();
        assert!(!set_character_style(
            &mut document,
            &mut history,
            frame.story,
            2..2,
            "Bold"
        ));
        assert!(!set_character_style(
            &mut document,
            &mut history,
            frame.story,
            0..2,
            "Nonexistent"
        ));
        assert_eq!(document, before);
    }

    #[test]
    fn a_story_with_two_paragraphs_counts_its_breaks() {
        // A range at the end of one paragraph must not be counted as
        // covering the start of the next, or applying a style would bleed
        // across the break.
        let (document, _history, frame) = styled_frame("Body", "one\ntwo");

        let story = document.story(frame.story).expect("the story");
        // A paragraph break occupies one byte in every story API.
        assert_eq!(text_length(story), 7);
        let mut two_paragraphs = story.clone();
        two_paragraphs.points = vec![
            StoryPoint::Paragraph {
                text: "one".into(),
                style: "Body".into(),
            },
            StoryPoint::Paragraph {
                text: "two".into(),
                style: "Body".into(),
            },
        ];
        assert_eq!(text_length(&two_paragraphs), two_paragraphs.text().len());
        assert_eq!(text_length(&two_paragraphs), 7);
    }

    #[test]
    fn a_graphic_frame_links_to_a_file_and_says_whether_it_is_embedded() {
        let (mut document, mut history) = blank();
        let id = graphic_frame(
            &mut document,
            &mut history,
            0,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            "images/photo.png",
            true,
        )
        .expect("a frame is created");
        let placed = document.object(id).expect("it is there");
        let LayoutObject::GraphicFrame { link, embedded, .. } = &placed.object else {
            panic!("expected a graphic frame");
        };
        assert_eq!(link.path, "images/photo.png");
        assert!(embedded, "an embedded frame carries its pixels");
    }
}
