//! Corner options: a rectangle's corners rounded, rounded inward, bevelled
//! or inset, each by its own radius. The settings stay with the item and its
//! outline is drawn from them, so saving writes the rectangle and its corner
//! settings, never a rounded path InDesign would round again.
//!
//! InDesign's PDF of the public paged-media `stroke-inset` sample draws a
//! 12 pt rounded corner as one cubic whose handles reach 6.627 pt along each
//! edge toward the corner: the usual quarter-circle approximation.
use crate::{BezierHandles, Point, Pt, ShapePath, SubPath};
use serde::{Deserialize, Serialize};

/// The handle reach of a quarter circle, as a fraction of its radius.
const KAPPA: f32 = 0.552_284_8;

/// One corner's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CornerShape {
    #[default]
    None,
    /// A convex quarter circle.
    Rounded,
    /// A concave quarter circle about the corner.
    InverseRounded,
    /// A straight cut.
    Bevel,
    /// A square notch.
    Inset,
    /// InDesign's decorative corner. Kept for saving, drawn square.
    Fancy,
}

/// The four corners, top-left, top-right, bottom-right and bottom-left in
/// the item's own coordinates, and their radii.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Corners {
    pub shapes: [CornerShape; 4],
    pub radii: [Pt; 4],
}

impl Corners {
    /// Whether every corner is drawn square.
    pub fn square(&self) -> bool {
        self.shapes.iter().zip(&self.radii).all(|(shape, radius)| {
            matches!(shape, CornerShape::None | CornerShape::Fancy) || *radius <= 0.0
        })
    }

    /// Whether a corner asks for the decorative shape Schist draws square.
    pub fn fancy(&self) -> bool {
        self.shapes
            .iter()
            .zip(&self.radii)
            .any(|(shape, radius)| *shape == CornerShape::Fancy && *radius > 0.0)
    }

    /// The `width` × `height` rectangle at `origin` with these corners, a
    /// radius never more than half the shorter side.
    pub fn path(&self, origin: Point, width: Pt, height: Pt) -> ShapePath {
        let limit = (width.min(height) / 2.0).max(0.0);
        let (x, y) = (origin.x, origin.y);
        let corners = [
            Point::new(x, y),
            Point::new(x + width, y),
            Point::new(x + width, y + height),
            Point::new(x, y + height),
        ];
        let mut points = Vec::new();
        let mut handles = Vec::new();
        for (index, corner) in corners.iter().copied().enumerate() {
            let previous = corners[(index + 3) % 4];
            let next = corners[(index + 1) % 4];
            let radius = self.radii[index].clamp(0.0, limit);
            let shape = if radius > 0.0 {
                self.shapes[index]
            } else {
                CornerShape::None
            };
            let toward = |to: Point| {
                let (dx, dy) = (to.x - corner.x, to.y - corner.y);
                let length = (dx * dx + dy * dy).sqrt().max(1e-6);
                Point::new(
                    corner.x + dx / length * radius,
                    corner.y + dy / length * radius,
                )
            };
            // Where the corner starts on the edge in and ends on the edge out.
            let (start, end) = (toward(previous), toward(next));
            let plain = BezierHandles::default();
            match shape {
                CornerShape::None | CornerShape::Fancy => {
                    points.push(corner);
                    handles.push(plain);
                }
                CornerShape::Rounded => {
                    // Each handle runs from its end toward the corner.
                    let reach = |from: Point| {
                        Point::new(
                            from.x + (corner.x - from.x) * KAPPA,
                            from.y + (corner.y - from.y) * KAPPA,
                        )
                    };
                    points.extend([start, end]);
                    handles.extend([
                        BezierHandles {
                            incoming: None,
                            outgoing: Some(reach(start)),
                        },
                        BezierHandles {
                            incoming: Some(reach(end)),
                            outgoing: None,
                        },
                    ]);
                }
                CornerShape::InverseRounded => {
                    // The arc about the corner: each handle runs parallel to
                    // the other edge, into the item.
                    let along = |from: Point, edge: Point| {
                        Point::new(
                            from.x + (edge.x - corner.x) * KAPPA,
                            from.y + (edge.y - corner.y) * KAPPA,
                        )
                    };
                    points.extend([start, end]);
                    handles.extend([
                        BezierHandles {
                            incoming: None,
                            outgoing: Some(along(start, end)),
                        },
                        BezierHandles {
                            incoming: Some(along(end, start)),
                            outgoing: None,
                        },
                    ]);
                }
                CornerShape::Bevel => {
                    points.extend([start, end]);
                    handles.extend([plain, plain]);
                }
                CornerShape::Inset => {
                    let inner = Point::new(start.x + end.x - corner.x, start.y + end.y - corner.y);
                    points.extend([start, inner, end]);
                    handles.extend([plain; 3]);
                }
            }
        }
        if handles.iter().all(|h| *h == BezierHandles::default()) {
            handles.clear();
        }
        ShapePath {
            subpaths: vec![SubPath {
                points,
                handles,
                closed: true,
            }],
            even_odd: false,
        }
    }
}

/// The box of a path that is exactly an upright rectangle, the outline
/// corner options shape: four corner points along its edges.
pub fn rectangle(path: &ShapePath) -> Option<crate::Rect> {
    let [sub] = path.subpaths.as_slice() else {
        return None;
    };
    if !sub.closed || sub.points.len() != 4 {
        return None;
    }
    let near = |a: Pt, b: Pt| (a - b).abs() <= 1e-3;
    for (index, point) in sub.points.iter().enumerate() {
        let handles = sub.handles_at(index);
        let still =
            |handle: Option<Point>| handle.is_none_or(|h| near(h.x, point.x) && near(h.y, point.y));
        if !still(handles.incoming) || !still(handles.outgoing) {
            return None;
        }
        let next = sub.points[(index + 1) % 4];
        if !near(point.x, next.x) && !near(point.y, next.y) {
            return None;
        }
    }
    let (mut left, mut top) = (Pt::INFINITY, Pt::INFINITY);
    let (mut right, mut bottom) = (Pt::NEG_INFINITY, Pt::NEG_INFINITY);
    for point in &sub.points {
        left = left.min(point.x);
        top = top.min(point.y);
        right = right.max(point.x);
        bottom = bottom.max(point.y);
    }
    if right - left <= 1e-3 || bottom - top <= 1e-3 {
        return None;
    }
    // Four points along axis-parallel edges are the box only when they are
    // its four corners.
    let mut seen = [false; 4];
    for point in &sub.points {
        let x = if near(point.x, left) {
            0
        } else if near(point.x, right) {
            1
        } else {
            return None;
        };
        let y = if near(point.y, top) {
            0
        } else if near(point.y, bottom) {
            2
        } else {
            return None;
        };
        if std::mem::replace(&mut seen[x + y], true) {
            return None;
        }
    }
    Some(crate::Rect::new(left, top, right - left, bottom - top))
}
