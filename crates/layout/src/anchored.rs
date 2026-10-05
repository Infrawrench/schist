//! Page items anchored in text. IDML keeps them inside the story at a
//! character position; their XML is retained for saving, and the inline ones
//! are typed here for composition and drawing.
//!
//! An inline item is set in its line as an empty box the width of its drawn
//! extent: its bottom sits on the baseline, raised by its Y offset. A tall item
//! raises its line under font-metric or Auto leading; fixed leading keeps its
//! step and the item overlaps the line above. A bounding-box wrap widens the box
//! by its left and right offsets. Items above the line or at custom positions are
//! not composed yet and stay reported as unrendered structures.
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

/// A typed anchored item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchoredItem {
    pub position: AnchoredPosition,
    /// AnchorYoffset in points. An inline item rises by this distance.
    pub y_offset: Pt,
    /// The item as drawn, with its bounds at the origin and its own affine.
    pub object: PlacedObject,
}

/// An inline item's line box, for the projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    pub width: Pt,
    pub ascent: Pt,
    pub descent: Pt,
}

impl AnchoredItem {
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
        if self.position != AnchoredPosition::Inline {
            return None;
        }
        let e = self.extent();
        let values = [e.x, e.y, e.width, e.height, self.y_offset];
        if !values.iter().all(|v| v.is_finite()) || e.width < 0.0 || e.height < 0.0 {
            return None;
        }
        let (left, right) = self.wrap_sides();
        Some(LineBox {
            width: (e.width + left + right).max(0.0),
            ascent: (e.height + self.y_offset).max(0.0),
            descent: (-self.y_offset).max(0.0),
        })
    }

    /// The item with its extent's left edge at `left` (after any wrap offset)
    /// and its bottom on the baseline, raised by the Y offset. Coordinates are
    /// those of the line.
    pub fn placed(&self, left: Pt, baseline: Pt) -> PlacedObject {
        let e = self.extent();
        let mut object = self.object.clone();
        object.bounds.x += left + self.wrap_sides().0 - e.x;
        object.bounds.y += baseline - self.y_offset - e.bottom();
        object
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

/// An inline item waiting to be projected into its story's display text.
pub(crate) struct Instance {
    pub structure: usize,
    pub at: usize,
    pub character: crate::ResolvedCharacter,
    pub line_box: LineBox,
}

/// Inline items of `story` that composition sets in their lines: typed,
/// anchored at a known position in horizontal text.
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
            let (point, start) = story.points.iter().zip(&offsets).find(|(point, start)| {
                matches!(point, crate::StoryPoint::Paragraph { .. })
                    && **start <= at
                    && at <= **start + point.text().len()
            })?;
            let crate::StoryPoint::Paragraph { style, .. } = point else {
                return None;
            };
            let _ = start;
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
            if character.leading == Some(crate::styles::Leading::Auto) {
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

/// The inline items set in `lines`, placed in page space through `frame`,
/// with their object styles resolved.
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
        let spec = &projected.spec;
        let positions = schist_text_engine::inline_box_positions(spec);
        let width = schist_text_engine::measure(spec).map_or(0.0, |m| m.width);
        let origin =
            crate::compose::aligned_origin(line.bounds, width, spec.align, spec.writing_mode);
        for (at, structure) in &projected.anchored {
            let Some(position) = positions.iter().find(|p| p.at == *at) else {
                continue;
            };
            let Some(item) = story
                .structures
                .get(*structure)
                .and_then(|s| s.anchored.as_ref())
            else {
                continue;
            };
            let placed = item.placed(origin.x + position.x, origin.y + position.baseline);
            out.push(through_frame(&placed, frame).resolved_appearance(&doc.styles));
        }
    }
    out
}
