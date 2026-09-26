//! Cubic geometry shared by blending, curvature combs and G2 handle snapping.
use crate::{Anchor, SubPath, VectorPath};

pub type Point = (f32, f32);
pub fn add(a: Point, b: Point) -> Point {
    (a.0 + b.0, a.1 + b.1)
}
pub fn sub(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}
pub fn mul(a: Point, s: f32) -> Point {
    (a.0 * s, a.1 * s)
}
pub fn lerp(a: Point, b: Point, t: f32) -> Point {
    add(a, mul(sub(b, a), t))
}
pub fn length(a: Point) -> f32 {
    a.0.hypot(a.1)
}
fn cross(a: Point, b: Point) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

#[derive(Clone, Copy, Debug)]
pub struct Cubic(pub [Point; 4]);
impl Cubic {
    pub fn between(a: Anchor, b: Anchor) -> Self {
        // Straight path edges are parameterised uniformly, including derivatives.
        if a.handle_out == (0.0, 0.0) && b.handle_in == (0.0, 0.0) {
            return Self([
                a.point,
                lerp(a.point, b.point, 1.0 / 3.0),
                lerp(a.point, b.point, 2.0 / 3.0),
                b.point,
            ]);
        }
        Self([
            a.point,
            add(a.point, a.handle_out),
            add(b.point, b.handle_in),
            b.point,
        ])
    }
    pub fn point(self, t: f32) -> Point {
        let [a, b, c, d] = self.0;
        let ab = lerp(a, b, t);
        let bc = lerp(b, c, t);
        let cd = lerp(c, d, t);
        lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
    }
    pub fn derivative(self, t: f32) -> Point {
        let [a, b, c, d] = self.0;
        mul(
            lerp(
                lerp(sub(b, a), sub(c, b), t),
                lerp(sub(c, b), sub(d, c), t),
                t,
            ),
            3.0,
        )
    }
    pub fn curvature(self, t: f32) -> f32 {
        let [a, b, c, d] = self.0;
        let v = self.derivative(t);
        let acc = mul(
            lerp(add(sub(c, mul(b, 2.0)), a), add(sub(d, mul(c, 2.0)), b), t),
            6.0,
        );
        let speed = length(v);
        if speed < 1e-6 {
            0.0
        } else {
            cross(v, acc) / speed.powi(3)
        }
    }
    pub fn split(self, t: f32) -> (Self, Self) {
        let [a, b, c, d] = self.0;
        let ab = lerp(a, b, t);
        let bc = lerp(b, c, t);
        let cd = lerp(c, d, t);
        let abc = lerp(ab, bc, t);
        let bcd = lerp(bc, cd, t);
        let p = lerp(abc, bcd, t);
        (Self([a, ab, abc, p]), Self([p, bcd, cd, d]))
    }
    fn flatten(self, depth: u8, out: &mut Vec<Point>) {
        let [a, b, c, d] = self.0;
        let polygon = length(sub(b, a)) + length(sub(c, b)) + length(sub(d, c));
        // Also bound the chord length. Collinear backtracking must subdivide.
        if depth >= 18 || (polygon - length(sub(d, a)) <= 0.002 && polygon <= 8.0) {
            out.push(d);
        } else {
            let (l, r) = self.split(0.5);
            l.flatten(depth + 1, out);
            r.flatten(depth + 1, out);
        }
    }
}

pub fn segments(sub: &SubPath) -> impl Iterator<Item = Cubic> + '_ {
    let n = sub.anchors.len();
    (0..if sub.closed && n > 1 {
        n
    } else {
        n.saturating_sub(1)
    })
        .map(move |i| Cubic::between(sub.anchors[i], sub.anchors[(i + 1) % n]))
}

/// Adaptive arc-length table, never parameter-space spacing.
pub struct ArcPath {
    points: Vec<Point>,
    distances: Vec<f32>,
    pub length: f32,
}
impl ArcPath {
    pub fn new(path: &SubPath) -> Self {
        let mut points = path
            .anchors
            .first()
            .map(|a| vec![a.point])
            .unwrap_or_default();
        for segment in segments(path) {
            segment.flatten(0, &mut points);
        }
        let mut distances = vec![0.0; points.len()];
        for i in 1..points.len() {
            distances[i] = distances[i - 1] + length(sub(points[i], points[i - 1]));
        }
        let length = distances.last().copied().unwrap_or(0.0);
        Self {
            points,
            distances,
            length,
        }
    }
    pub fn sample(&self, fraction: f32) -> (Point, Point) {
        if self.points.len() < 2 || self.length <= 1e-6 {
            return (self.points.first().copied().unwrap_or_default(), (1.0, 0.0));
        }
        let distance = fraction.clamp(0.0, 1.0) * self.length;
        let i = self
            .distances
            .partition_point(|d| *d <= distance)
            .clamp(1, self.points.len() - 1);
        let delta = sub(self.points[i], self.points[i - 1]);
        let span = length(delta).max(1e-8);
        (
            lerp(
                self.points[i - 1],
                self.points[i],
                (distance - self.distances[i - 1]) / span,
            ),
            mul(delta, 1.0 / span),
        )
    }
}

/// The comb's signed normal lengths are proportional to curvature.
pub fn curvature_comb(path: &VectorPath, samples: usize, scale: f32) -> Vec<(Point, Point)> {
    let mut lines = Vec::new();
    for sub in &path.subpaths {
        for cubic in segments(sub) {
            for i in 0..=samples.clamp(2, 128) {
                let t = i as f32 / samples.clamp(2, 128) as f32;
                let p = cubic.point(t);
                let v = cubic.derivative(t);
                let speed = length(v);
                if speed > 1e-6 {
                    let size = (cubic.curvature(t) * scale).clamp(-10000.0, 10000.0);
                    lines.push((p, add(p, mul((-v.1 / speed, v.0 / speed), size))));
                }
            }
        }
    }
    lines
}

/// Match signed curvature at a join, preserving the dragged handle and both
/// anchors. Adjust the opposite handle length; leave impossible inflections
/// untouched rather than introducing a cusp. Returns whether a G2 solution exists.
pub fn snap_g2(subpath: &mut SubPath, index: usize, outgoing: bool) -> bool {
    let n = subpath.anchors.len();
    if n < 3 || index >= n || (!subpath.closed && (index == 0 || index + 1 == n)) {
        return false;
    }
    let prev = subpath.anchors[(index + n - 1) % n];
    let next = subpath.anchors[(index + 1) % n];
    let mut join = subpath.anchors[index];
    let fixed = if outgoing {
        join.handle_out
    } else {
        join.handle_in
    };
    let l = length(fixed);
    if l < 1e-5 {
        return false;
    }
    let opposite = mul(fixed, -1.0 / l);
    let k = if outgoing {
        Cubic::between(join, next).curvature(0.0)
    } else {
        Cubic::between(prev, join).curvature(1.0)
    };
    // Evaluate the opposite side with a unit handle: its curvature scales 1/L².
    if outgoing {
        join.handle_in = opposite;
    } else {
        join.handle_out = opposite;
    }
    let unit_k = if outgoing {
        Cubic::between(prev, join).curvature(1.0)
    } else {
        Cubic::between(join, next).curvature(0.0)
    };
    let solved = if k.abs() < 1e-7 && unit_k.abs() < 1e-7 {
        l
    } else if k * unit_k > 0.0 {
        (unit_k / k).sqrt()
    } else {
        return false;
    };
    if !solved.is_finite() || !(1e-4..=1e6).contains(&solved) {
        return false;
    }
    if outgoing {
        join.handle_in = mul(opposite, solved);
    } else {
        join.handle_out = mul(opposite, solved);
    }
    subpath.anchors[index] = join;
    true
}
