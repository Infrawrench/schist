//! Editable cubic geometry; flattening is confined to rendering.
use crate::{BezierHandles, Point, Rect, ShapePath, SubPath};

impl SubPath {
    pub fn handles_at(&self, index: usize) -> BezierHandles {
        self.handles.get(index).copied().unwrap_or_default()
    }

    pub fn set_handles(&mut self, index: usize, handles: BezierHandles) -> bool {
        if index >= self.points.len() {
            return false;
        }
        self.handles
            .resize(self.points.len(), BezierHandles::default());
        self.handles[index] = handles;
        true
    }

    /// Move an anchor and carry both handles by exactly the same delta.
    pub fn move_anchor(&mut self, index: usize, to: Point) -> bool {
        let Some(point) = self.points.get_mut(index) else {
            return false;
        };
        let delta = to - *point;
        *point = to;
        if let Some(handles) = self.handles.get_mut(index) {
            handles.incoming = handles.incoming.map(|p| p + delta);
            handles.outgoing = handles.outgoing.map(|p| p + delta);
        }
        true
    }

    /// The contour flattened like [`ShapePath::flatten`], with the index of
    /// each anchor among the points. A closed contour does not repeat its
    /// first point.
    pub(crate) fn flatten_anchored(&self, tolerance: f32) -> (Vec<(f32, f32)>, Vec<usize>) {
        let Some(first) = self.points.first() else {
            return (Vec::new(), Vec::new());
        };
        let mut points = vec![(first.x, first.y)];
        let mut anchors = vec![0];
        for curve in self.segments() {
            flatten(curve, tolerance, 0, &mut points);
            anchors.push(points.len() - 1);
        }
        if self.closed && points.len() > 1 {
            points.pop();
            anchors.pop();
        }
        (points, anchors)
    }

    /// Whether the contour turns at each anchor rather than running
    /// smoothly through it: its tangents in and out differ by more than a
    /// degree. An open contour's ends count as corners.
    pub(crate) fn corners(&self) -> Vec<bool> {
        let count = self.points.len();
        let segments: Vec<_> = self.segments().collect();
        let direction = |vectors: [Point; 3]| {
            vectors
                .into_iter()
                .find(|v| v.x.hypot(v.y) > 1e-6)
                .map(|v| v.scale(1.0 / v.x.hypot(v.y)))
        };
        (0..count)
            .map(|index| {
                let outgoing = (self.closed || index + 1 < count)
                    .then(|| segments.get(index))
                    .flatten();
                let incoming = if self.closed {
                    segments.get((index + count - 1) % count)
                } else {
                    index.checked_sub(1).and_then(|i| segments.get(i))
                };
                let (Some(out), Some(into)) = (outgoing, incoming) else {
                    return true;
                };
                let leaving = direction([out[1] - out[0], out[2] - out[0], out[3] - out[0]]);
                let arriving = direction([into[3] - into[2], into[3] - into[1], into[3] - into[0]]);
                match (leaving, arriving) {
                    (Some(a), Some(b)) => {
                        a.x * b.x + a.y * b.y < 0.999_85 // cos 1°
                    }
                    _ => true,
                }
            })
            .collect()
    }

    fn segments(&self) -> impl Iterator<Item = [Point; 4]> + '_ {
        let count = if self.closed {
            self.points.len()
        } else {
            self.points.len().saturating_sub(1)
        };
        (0..count).map(|index| {
            let next = (index + 1) % self.points.len();
            let a = self.points[index];
            let b = self.points[next];
            [
                a,
                self.handles_at(index).outgoing.unwrap_or(a),
                self.handles_at(next).incoming.unwrap_or(b),
                b,
            ]
        })
    }
}

impl ShapePath {
    pub fn is_finite(&self) -> bool {
        self.subpaths.iter().all(|sub| {
            sub.points
                .iter()
                .copied()
                .chain(
                    sub.handles
                        .iter()
                        .flat_map(|h| [h.incoming, h.outgoing].into_iter().flatten()),
                )
                .all(|p| p.x.is_finite() && p.y.is_finite())
        })
    }

    pub fn is_empty(&self) -> bool {
        self.subpaths.iter().all(|s| s.points.len() < 2)
    }

    /// Transform anchors and handles together, without changing topology.
    pub fn map_points(&mut self, map: impl Fn(Point) -> Point) {
        for sub in &mut self.subpaths {
            for point in &mut sub.points {
                *point = map(*point);
            }
            for handles in &mut sub.handles {
                handles.incoming = handles.incoming.map(&map);
                handles.outgoing = handles.outgoing.map(&map);
            }
        }
    }

    /// Tight curve bounds, including extrema between anchors. Control
    /// handles themselves need not lie inside the curve's bounding box.
    pub fn bounds(&self) -> Rect {
        let mut bounds: Option<Rect> = None;
        let mut include = |point: Point| {
            let rect = Rect::new(point.x, point.y, 0.0, 0.0);
            bounds = Some(bounds.map_or(rect, |r| r.union(rect)));
        };
        for sub in &self.subpaths {
            for point in &sub.points {
                include(*point);
            }
            for curve in sub.segments() {
                for axis in [curve.map(|p| p.x), curve.map(|p| p.y)] {
                    for t in extrema(axis).into_iter().flatten() {
                        if t > 0.0 && t < 1.0 {
                            include(evaluate(curve, t));
                        }
                    }
                }
            }
        }
        bounds.unwrap_or(Rect::ZERO)
    }

    pub fn to_vector_path(&self) -> schist_vector::Path {
        self.flatten(0.1)
    }

    /// Rendering tolerance in this path's coordinates. Subdivision stops
    /// when both handles are within tolerance of the chord *segment*, so
    /// collinear reversals and loops are preserved too. Depth is bounded
    /// for damaged or enormous input; editing always keeps the cubics.
    pub fn flatten(&self, tolerance: f32) -> schist_vector::Path {
        if !self.is_finite() {
            return schist_vector::Path::default();
        }
        let tolerance = if tolerance.is_finite() {
            tolerance.max(0.0001)
        } else {
            0.1
        };
        let mut out = schist_vector::Path::default();
        for sub in &self.subpaths {
            let Some(first) = sub.points.first() else {
                continue;
            };
            let mut points = vec![(first.x, first.y)];
            for curve in sub.segments() {
                flatten(curve, tolerance, 0, &mut points);
            }
            if sub.closed {
                // The closing cubic ends at the first point; the raster
                // contour records closure separately.
                if points.len() > 1 {
                    points.pop();
                }
                out.push_closed(points);
            } else {
                out.push_open(points);
            }
        }
        out
    }

    pub fn ellipse(width: f32, height: f32) -> Self {
        let (rx, ry) = (width * 0.5, height * 0.5);
        let (hx, hy) = (rx * 0.552_284_8, ry * 0.552_284_8);
        Self {
            subpaths: vec![SubPath {
                points: vec![
                    Point::new(width, ry),
                    Point::new(rx, height),
                    Point::new(0.0, ry),
                    Point::new(rx, 0.0),
                ],
                handles: vec![
                    BezierHandles {
                        incoming: Some(Point::new(width, ry - hy)),
                        outgoing: Some(Point::new(width, ry + hy)),
                    },
                    BezierHandles {
                        incoming: Some(Point::new(rx + hx, height)),
                        outgoing: Some(Point::new(rx - hx, height)),
                    },
                    BezierHandles {
                        incoming: Some(Point::new(0.0, ry + hy)),
                        outgoing: Some(Point::new(0.0, ry - hy)),
                    },
                    BezierHandles {
                        incoming: Some(Point::new(rx - hx, 0.0)),
                        outgoing: Some(Point::new(rx + hx, 0.0)),
                    },
                ],
                closed: true,
            }],
            even_odd: false,
        }
    }
}

fn evaluate([a, b, c, d]: [Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    a.scale(u * u * u) + b.scale(3.0 * u * u * t) + c.scale(3.0 * u * t * t) + d.scale(t * t * t)
}

fn extrema(values: [f32; 4]) -> [Option<f32>; 2] {
    let [p, q, r, s] = values.map(f64::from);
    let (a, b, c) = (-p + 3.0 * q - 3.0 * r + s, 2.0 * (p - 2.0 * q + r), q - p);
    if a.abs() < f64::EPSILON {
        return [
            if b.abs() < f64::EPSILON {
                None
            } else {
                Some((-c / b) as f32)
            },
            None,
        ];
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return [None, None];
    }
    let root = discriminant.sqrt();
    [
        Some(((-b + root) / (2.0 * a)) as f32),
        Some(((-b - root) / (2.0 * a)) as f32),
    ]
}

fn distance(point: Point, a: Point, b: Point) -> f32 {
    let delta = b - a;
    let square = delta.x * delta.x + delta.y * delta.y;
    let t = if square > 0.0 {
        (((point.x - a.x) * delta.x + (point.y - a.y) * delta.y) / square).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let near = a + delta.scale(t);
    (point.x - near.x).hypot(point.y - near.y)
}

fn flatten(curve: [Point; 4], tolerance: f32, depth: u8, out: &mut Vec<(f32, f32)>) {
    let [a, b, c, d] = curve;
    if depth >= 16 || (distance(b, a, d) <= tolerance && distance(c, a, d) <= tolerance) {
        out.push((d.x, d.y));
        return;
    }
    let ab = (a + b).scale(0.5);
    let bc = (b + c).scale(0.5);
    let cd = (c + d).scale(0.5);
    let abc = (ab + bc).scale(0.5);
    let bcd = (bc + cd).scale(0.5);
    let middle = (abc + bcd).scale(0.5);
    flatten([a, ab, abc, middle], tolerance, depth + 1, out);
    flatten([middle, bcd, cd, d], tolerance, depth + 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_polygon_documents_remain_corners_and_lines_are_not_empty() {
        let path: ShapePath = serde_json::from_str(r#"{"subpaths":[{"points":[{"x":0,"y":0},{"x":10,"y":20}],"closed":false}],"even_odd":false}"#).unwrap();
        assert!(!path.is_empty());
        assert_eq!(
            path.flatten(0.1).subpaths,
            vec![vec![(0.0, 0.0), (10.0, 20.0)]]
        );
    }

    #[test]
    fn ellipse_extents_and_tangents_are_preserved_at_every_size() {
        for width in [1.0, 100.0, 14400.0] {
            for height in [1.0, 50.0, 7200.0] {
                let path = ShapePath::ellipse(width, height);
                assert_eq!(path.bounds(), Rect::new(0.0, 0.0, width, height));
                assert_eq!(path.subpaths[0].points.len(), 4);
                for (i, p) in path.subpaths[0].points.iter().enumerate() {
                    let h = path.subpaths[0].handles_at(i);
                    assert!(
                        ((h.incoming.unwrap() + h.outgoing.unwrap()).scale(0.5).x - p.x).abs()
                            < 0.001
                    );
                    assert!(
                        ((h.incoming.unwrap() + h.outgoing.unwrap()).scale(0.5).y - p.y).abs()
                            < 0.001
                    );
                }
                let json = serde_json::to_string(&path).unwrap();
                assert_eq!(path, serde_json::from_str::<ShapePath>(&json).unwrap());
            }
        }
    }

    #[test]
    fn bounds_and_flattening_include_loops_extrema_and_collinear_reversals() {
        for curve in [
            [
                Point::ZERO,
                Point::new(0.0, 100.0),
                Point::new(100.0, 100.0),
                Point::new(100.0, 0.0),
            ],
            [
                Point::ZERO,
                Point::new(100.0, 0.0),
                Point::new(-100.0, 0.0),
                Point::ZERO,
            ],
            [
                Point::ZERO,
                Point::new(100.0, 100.0),
                Point::new(-100.0, 100.0),
                Point::ZERO,
            ],
        ] {
            let path = ShapePath {
                subpaths: vec![SubPath {
                    points: vec![curve[0], curve[3]],
                    handles: vec![
                        BezierHandles {
                            outgoing: Some(curve[1]),
                            ..Default::default()
                        },
                        BezierHandles {
                            incoming: Some(curve[2]),
                            ..Default::default()
                        },
                    ],
                    closed: false,
                }],
                even_odd: false,
            };
            let bounds = path.bounds();
            let loose = Rect::new(
                bounds.x - 0.0001,
                bounds.y - 0.0001,
                bounds.width + 0.0002,
                bounds.height + 0.0002,
            );
            let poly = path.flatten(0.05);
            for i in 0..=1000 {
                let p = evaluate(curve, i as f32 / 1000.0);
                assert!(loose.contains(p), "{p:?} outside {bounds:?}");
                let error = poly.subpaths[0]
                    .windows(2)
                    .map(|pair| {
                        distance(
                            p,
                            Point::new(pair[0].0, pair[0].1),
                            Point::new(pair[1].0, pair[1].1),
                        )
                    })
                    .fold(f32::INFINITY, f32::min);
                assert!(error <= 0.051, "deviation {error}");
            }
        }
    }
}
