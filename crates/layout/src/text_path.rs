//! A bounded baseline for one path-text container, in local page points.
use crate::{Point, ShapePath};
use serde::{Deserialize, Serialize};

/// The supported baseline path model. Native effects/alignment beyond rainbow,
/// center-of-stroke and baseline alignment must be diagnosed by the codec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathText {
    pub path: ShapePath,
    #[serde(default)]
    pub start: f32,
    /// None follows the geometric end as the path is edited. Explicit brackets
    /// remain arc distances in points, independently of font size and DPI.
    #[serde(default)]
    pub end: Option<f32>,
}

impl PathText {
    /// A single contour maps to one native TextPath. Never merge disjoint
    /// contours into an invented connector or silently choose just the first.
    pub fn engine_path(&self) -> Option<schist_text_engine::TextPath> {
        let [curve] = self.path.subpaths.as_slice() else {
            return None;
        };
        if curve.points.len() < 2 || !self.start.is_finite() || self.start < 0.0 {
            return None;
        }
        let mut path = schist_text_engine::TextPath {
            curve: schist_core::path::SubPath {
                anchors: curve
                    .points
                    .iter()
                    .enumerate()
                    .map(|(index, p)| {
                        let handles = curve.handles.get(index).copied().unwrap_or_default();
                        let offset = |h: Option<Point>| {
                            let d = h.unwrap_or(*p) - *p;
                            (d.x, d.y)
                        };
                        schist_core::path::Anchor {
                            point: (p.x, p.y),
                            handle_in: offset(handles.incoming),
                            handle_out: offset(handles.outgoing),
                        }
                    })
                    .collect(),
                closed: curve.closed,
            },
            offset: 0.0,
            span: None,
        };
        let length = path.length()?;
        let end = self.end.unwrap_or(length);
        if !end.is_finite() || end < self.start || self.start > length || end > length {
            return None;
        }
        path.offset = self.start;
        path.span = Some(end - self.start);
        Some(path)
    }
}

/// Convert one existing contour into an empty text container without changing
/// its identity, layer, geometry, affine or frame paint. Story and object creation
/// are one undo operation, so undo cannot leave an orphan story behind.
pub fn attach(
    doc: &mut crate::LayoutDocument,
    history: &mut crate::History,
    id: crate::ObjectId,
) -> Option<crate::authoring::TextFrame> {
    if doc.object_locked(id) {
        return None;
    }
    let original = doc.object(id)?;
    let crate::LayoutObject::Shape { path, .. } = &original.object else {
        return None;
    };
    let path = PathText {
        path: path.clone(),
        start: 0.0,
        end: None,
    };
    path.engine_path()?;
    let story = crate::StoryId(doc.stories.len() as u32);
    let mut changed = original.clone();
    changed.appearance.paint = original.appearance.paint.over(&original.legacy_paint());
    changed.appearance.outline = None;
    changed.object = crate::LayoutObject::TextFrame {
        story,
        text_path: Some(path),
        columns: 1,
        gutter: 0.0,
        insets: crate::Insets::ZERO,
        overflow: crate::FrameOverflow::Clip,
    };
    let edits = vec![
        crate::LayoutEdit::AddedStory {
            id: story.0,
            story: crate::edit::snapshot_story(&crate::Story::new()),
        },
        crate::LayoutEdit::ObjectChanged {
            id: id.0,
            before: crate::edit::snapshot_object(original),
            after: crate::edit::snapshot_object(&changed),
        },
    ];
    history
        .apply(doc, crate::LayoutEdit::Batch { edits })
        .then_some(crate::authoring::TextFrame { object: id, story })
}

#[derive(Clone, Copy)]
pub enum Bracket {
    Start(f32),
    End(Option<f32>),
}

/// Captured multi-object bracket edits validate the entire selection before
/// applying once. Empty End follows future geometry edits; explicit ends retain
/// absolute arc distances and may become overset after shortening a curve.
pub fn set_bracket(
    doc: &mut crate::LayoutDocument,
    history: &mut crate::History,
    ids: &[crate::ObjectId],
    bracket: Bracket,
) -> bool {
    let mut edits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        if doc.object_locked(*id) {
            return false;
        }
        let Some(original) = doc.object(*id) else {
            return false;
        };
        let mut changed = original.clone();
        let crate::LayoutObject::TextFrame {
            text_path: Some(path),
            ..
        } = &mut changed.object
        else {
            return false;
        };
        match bracket {
            Bracket::Start(v) => path.start = v,
            Bracket::End(v) => path.end = v,
        }
        if path.engine_path().is_none() {
            return false;
        }
        if changed != *original {
            edits.push(crate::LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::edit::snapshot_object(original),
                after: crate::edit::snapshot_object(&changed),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, crate::LayoutEdit::Batch { edits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BezierHandles, SubPath};

    #[test]
    fn baseline_intervals_preserve_handles_and_measure_the_same_geometry_as_glyph_placement() {
        let path = ShapePath {
            subpaths: vec![SubPath {
                points: vec![Point::new(10.0, 20.0), Point::new(110.0, 20.0)],
                handles: vec![
                    BezierHandles {
                        outgoing: Some(Point::new(35.0, -20.0)),
                        ..Default::default()
                    },
                    BezierHandles {
                        incoming: Some(Point::new(85.0, 60.0)),
                        ..Default::default()
                    },
                ],
                closed: false,
            }],
            even_odd: false,
        };
        let mut baseline = PathText {
            path,
            start: 0.0,
            end: None,
        };
        let full = baseline.engine_path().unwrap();
        assert_eq!(full.curve.anchors[0].handle_out, (25.0, -40.0));
        assert_eq!(full.curve.anchors[1].handle_in, (-25.0, 40.0));
        let length = full.length().unwrap();
        assert!(length > 100.0);
        for start in [0.0, length * 0.25, length] {
            for end in [start, (start + length) * 0.5, length] {
                baseline.start = start;
                baseline.end = Some(end);
                let engine = baseline.engine_path().unwrap();
                assert_eq!(engine.offset, start);
                assert_eq!(engine.span, Some(end - start));
                assert_eq!(engine.length(), Some(length));
            }
        }
        for (start, end) in [
            (f32::NAN, None),
            (-1.0, None),
            (0.0, Some(f32::INFINITY)),
            (length, Some(0.0)),
            (0.0, Some(length + 1.0)),
        ] {
            baseline.start = start;
            baseline.end = end;
            assert!(baseline.engine_path().is_none());
        }
        baseline.start = 0.0;
        baseline.end = None;
        baseline
            .path
            .subpaths
            .push(baseline.path.subpaths[0].clone());
        assert!(baseline.engine_path().is_none());
    }
}
