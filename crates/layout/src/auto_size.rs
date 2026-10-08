//! Text frame auto-size: a frame whose AutoSizingType asks for it is resized
//! to fit its text, about its AutoSizingReferencePoint, and keeps that size
//! as its own geometry. InDesign's PDF of the public paged-media
//! `text-autosize` sample grows a 240 × 40 pt HeightOnly frame anchored at
//! its top left to 156.856 pt: its bottom falls 0.5 pt below its last
//! baseline, the reach of its centred 1 pt stroke, and its text starts the
//! same 0.5 pt in.
//!
//! The same PDF of the public `layout` sample fits the other cases it sets.
//! Two 200 × 36 pt HeightAndWidth frames of eight short 12 pt paragraphs
//! narrow to 29.93 and 30.05 pt, hyphenating "Cen-tre": the narrowest width
//! their widest unbreakable fragment allows, then the height of their lines,
//! the centred one about its centre. A two-column HeightOnly frame grows to
//! hold ten of its twenty lines in each column, to within a point: InDesign's
//! frame ends 1.25 pt below the last baselines where a single column's ends
//! its stroke's reach below. Width-only frames, and height-and-width frames
//! stating UseNoLineBreaksForAutoSizing, which no sample sets, take the width
//! of their longest line, their text broken only where it breaks itself. A
//! threaded frame keeps its size: the public PSU template's own InDesign
//! export threads height-and-width frames whose widths no fit explains
//! (319.44 and 317.28 pt beside each other).
use crate::{LayoutDocument, LayoutObject, ObjectId, PlacedObject, Pt, Rect};
use serde::{Deserialize, Serialize};

/// What a frame fits to its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoSizing {
    HeightOnly,
    WidthOnly,
    HeightAndWidth,
    /// Kept and saved; Schist does not fit it.
    HeightAndWidthProportionally,
}

/// The point that stays where it is while a frame is fitted. The centre, as
/// every frame InDesign's own exports in the repository's fixtures states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReferencePoint {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl ReferencePoint {
    /// Where the point sits across and down the frame: 0, ½ or 1.
    fn anchor(self) -> (Pt, Pt) {
        let across = match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => 0.0,
            Self::Top | Self::Center | Self::Bottom => 0.5,
            Self::TopRight | Self::Right | Self::BottomRight => 1.0,
        };
        let down = match self {
            Self::TopLeft | Self::Top | Self::TopRight => 0.0,
            Self::Left | Self::Center | Self::Right => 0.5,
            Self::BottomLeft | Self::Bottom | Self::BottomRight => 1.0,
        };
        (across, down)
    }
}

/// A text frame's auto-size settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoSize {
    pub sizing: AutoSizing,
    #[serde(default)]
    pub reference: ReferencePoint,
    /// The frame is never fitted shorter than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_height: Option<Pt>,
    /// The frame is never fitted narrower than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_width: Option<Pt>,
    /// UseNoLineBreaksForAutoSizing, kept and saved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_line_breaks: bool,
}

impl AutoSize {
    /// Whether Schist fits frames of this kind.
    pub fn fitted(&self) -> bool {
        self.sizing != AutoSizing::HeightAndWidthProportionally
    }
}

/// Far enough for any page's text not to need more.
const ROOM: Pt = 100_000.0;

/// The frame's lines composed at `width` × `height`, the top left kept, and
/// whether text was left over.
fn trial(
    roomy: &mut LayoutDocument,
    frame: &PlacedObject,
    story: crate::StoryId,
    width: Pt,
    height: Pt,
) -> Option<(Vec<crate::ComposedLine>, bool)> {
    let object = roomy.objects.iter_mut().find(|o| o.id == frame.id)?;
    object.bounds.width = width;
    object.bounds.height = height;
    let composed = crate::compose::compose_story(roomy, story);
    let lines = composed
        .frames
        .iter()
        .find(|composed| composed.object == frame.id)?
        .lines
        .clone();
    Some((lines, composed.has_overflow()))
}

/// The smallest value in `low..=high` for which `fits` holds, to 0.01 pt,
/// `fits(high)` taken to hold.
fn least(mut low: Pt, mut high: Pt, mut fits: impl FnMut(Pt) -> bool) -> Pt {
    while high - low > 0.01 {
        let middle = (low + high) / 2.0;
        if fits(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// The bounds `frame`'s auto-size settings give it, or None when it has
/// none Schist applies: no settings, a proportional fit, a threaded or
/// shaped frame, or no lines to fit.
pub fn fitted(doc: &LayoutDocument, frame: &PlacedObject) -> Option<Rect> {
    let auto = frame.appearance.auto_size.filter(AutoSize::fitted)?;
    let LayoutObject::TextFrame {
        story,
        columns,
        text_path: None,
        insets,
        ..
    } = &frame.object
    else {
        return None;
    };
    if frame.appearance.outline.is_some() || doc.story_frames(*story).len() != 1 {
        return None;
    }
    let story = *story;
    let fit_height = auto.sizing != AutoSizing::WidthOnly;
    let stroke = doc.styles.stroke_inset(frame);
    let across = insets.left + insets.right + 2.0 * stroke;
    let down = insets.top + insets.bottom + 2.0 * stroke;
    let mut roomy = doc.clone();
    let mut bounds = frame.bounds;
    // The width first: the longest line, unbroken, or the narrowest the
    // widest unbreakable fragment allows.
    if auto.sizing != AutoSizing::HeightOnly {
        let (lines, _) = trial(&mut roomy, frame, story, ROOM, ROOM)?;
        let longest = lines
            .iter()
            .map(|line| {
                line.natural_width
                    + line.paragraph.left_indent.unwrap_or(0.0).max(0.0)
                    + line.paragraph.right_indent.unwrap_or(0.0).max(0.0)
            })
            .fold(0.0, Pt::max);
        let unbroken = (across + longest).max(1.0);
        bounds.width = if auto.sizing == AutoSizing::WidthOnly || auto.no_line_breaks {
            unbroken
        } else {
            least(across.max(1.0), unbroken, |width| {
                trial(&mut roomy, frame, story, width, ROOM).is_some_and(|(lines, _)| {
                    lines
                        .iter()
                        .all(|line| line.natural_width <= line.bounds.width + 0.01)
                })
            })
        };
        bounds.width = bounds.width.max(auto.minimum_width.unwrap_or(0.0));
    }
    if fit_height {
        let (lines, _) = trial(&mut roomy, frame, story, bounds.width, ROOM)?;
        let (first, last) = (lines.first()?, lines.last()?);
        // Its last baseline and what lies below it, measured from the first
        // line so vertical justification cannot move it.
        let single = down + last.baseline - first.bounds.y;
        let fit = if *columns > 1 {
            // Columns share the lines: the shortest height that leaves no
            // text over.
            least(down.max(1.0), single, |height| {
                trial(&mut roomy, frame, story, bounds.width, height).is_some_and(|(_, over)| !over)
            })
        } else {
            single
        };
        bounds.height = fit.max(auto.minimum_height.unwrap_or(0.0)).max(1.0);
    }
    // The reference point stays put.
    let (across, down) = auto.reference.anchor();
    bounds.x = frame.bounds.x + (frame.bounds.width - bounds.width) * across;
    bounds.y = frame.bounds.y + (frame.bounds.height - bounds.height) * down;
    Some(bounds)
}

/// Fit every auto-sized frame, returning the edits that did it, for a caller
/// to fold into the edit that called for them.
pub fn refit(doc: &mut LayoutDocument) -> Vec<crate::LayoutEdit> {
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|o| o.appearance.auto_size.is_some())
        .map(|o| o.id)
        .collect();
    let mut edits = Vec::new();
    for id in ids {
        let Some(object) = doc.object(id) else {
            continue;
        };
        let Some(bounds) = fitted(doc, object) else {
            continue;
        };
        let near = |a: Pt, b: Pt| (a - b).abs() < 0.001;
        let current = object.bounds;
        if near(bounds.x, current.x)
            && near(bounds.y, current.y)
            && near(bounds.width, current.width)
            && near(bounds.height, current.height)
        {
            continue;
        }
        let before = crate::edit::snapshot_object(object);
        let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.bounds = bounds;
        let after = crate::edit::snapshot_object(object);
        edits.push(crate::LayoutEdit::ObjectChanged {
            id: id.0,
            before,
            after,
        });
    }
    edits
}
