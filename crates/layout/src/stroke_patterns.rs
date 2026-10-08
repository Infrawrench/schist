//! Dashed, dotted and striped strokes, laid along an item's own path, which
//! stays as it is.
//!
//! The stroke styles are the ones text decorations use: a dashed style's
//! dash and gap lengths in points, a dotted style's dot spacing centre to
//! centre (each dot as wide as the stroke), a striped style's stripes as
//! start and end percentages of the weight, as the public IDML
//! specification's tables 130 to 132 define them. Dashes start at a
//! contour's first point and run on round it; a StrokeCornerAdjustment
//! fits them stretch by stretch between corners and ends, keeping a dash at
//! both ends of each stretch as straight decorations do, so two dashes meet
//! at every corner and join there. A closed contour without corners fits
//! whole cycles instead. Stripes run across the stroke from the edge on its
//! left as the path runs, as decoration stripes run down from their top
//! edge. No public InDesign PDF draws a patterned item stroke, so these are
//! readings of the specification, not measurements.

use crate::{pasteboard::StrokeOptions, ShapePath, StrokeAlignment};
use schist_text_engine::{DecorationCap, DecorationFit, TextDecorationPattern};
use schist_vector::{LineCap, LineJoin, Path, StrokeStyle};

type Pt2 = (f32, f32);

/// More dashes than this and the stroke draws solid, which a pattern that
/// fine looks like anyway.
const MAX_PIECES: usize = 200_000;

/// Lengths closer than this, in the path's units, are the same.
const HAIR: f64 = 1e-4;

/// What a patterned stroke fills, nonzero: its ink, and the band a solid
/// stroke of its weight covers, which a gap colour fills where the ink does
/// not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatternOutlines {
    pub ink: Path,
    pub band: Path,
}

/// The outlines of `path` stroked `width` wide with `options`' pattern, all
/// in the path's units (the pattern's lengths already scaled to them). None
/// when the stroke is solid: no pattern, an invalid one, or one too fine to
/// draw piece by piece.
pub fn outlines(
    path: &ShapePath,
    width: f32,
    options: &StrokeOptions,
    tolerance: f32,
) -> Option<PatternOutlines> {
    let stroke = options.pattern.as_ref()?;
    if matches!(stroke.pattern, TextDecorationPattern::Solid)
        || !stroke.valid()
        || !(width > 0.0 && width.is_finite())
        || !path.is_finite()
    {
        return None;
    }
    let style = options.style(width);
    let mut out = PatternOutlines::default();
    let mut budget = MAX_PIECES;
    for sub in &path.subpaths {
        let (mut points, anchors) = sub.flatten_anchored(tolerance);
        if points.len() < 2 {
            continue;
        }
        let closed = sub.closed && points.len() >= 3;
        // An inside or outside stroke is centred on the path moved half its
        // weight that way, as solid ones are.
        if closed && options.alignment != StrokeAlignment::Center {
            let outward = if signed_area(&points) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let shift = if options.alignment == StrokeAlignment::Inside {
                -width / 2.0
            } else {
                width / 2.0
            };
            points = offset(&points, shift * outward, options.miter_limit);
        }
        let corners: Vec<usize> = sub
            .corners()
            .into_iter()
            .zip(&anchors)
            .filter_map(|(corner, &index)| corner.then_some(index))
            .collect();
        let line = Contour {
            points: &points,
            closed,
            corners: &corners,
        };
        match &stroke.pattern {
            TextDecorationPattern::Stripes(edges) => {
                for [start, end] in edges.as_chunks::<2>().0 {
                    let half = width / 2.0;
                    let low = half - width * end / 100.0;
                    let high = half - width * start / 100.0;
                    if low < 0.0 {
                        band(&mut out.ink, &line, [low, high.min(0.0)], style);
                    }
                    if high > 0.0 {
                        band(&mut out.ink, &line, [low.max(0.0), high], style);
                    }
                }
            }
            TextDecorationPattern::Dashes(dashes) => {
                let cap = match dashes.cap {
                    DecorationCap::Butt => LineCap::Butt,
                    DecorationCap::Round => LineCap::Round,
                    DecorationCap::Projecting => LineCap::Square,
                };
                let pieces = dash(&line, &dashes.lengths, stroke.fitting, &mut budget)?;
                out.ink.subpaths.extend(
                    schist_vector::stroke_path(&pieces, StrokeStyle { cap, ..style }).subpaths,
                );
            }
            TextDecorationPattern::Dots(intervals) => {
                // Dots are round dashes of no length, as decorations draw
                // them; their spacing has no dashes to fit.
                let lengths: Vec<f32> = intervals.iter().flat_map(|gap| [0.0, *gap]).collect();
                let fitting = if stroke.fitting == DecorationFit::Dashes {
                    DecorationFit::None
                } else {
                    stroke.fitting
                };
                let pieces = dash(&line, &lengths, fitting, &mut budget)?;
                out.ink.subpaths.extend(
                    schist_vector::stroke_path(
                        &pieces,
                        StrokeStyle {
                            cap: LineCap::Round,
                            ..style
                        },
                    )
                    .subpaths,
                );
            }
            TextDecorationPattern::Solid => unreachable!(),
        }
        let mut solid = Path::default();
        if closed {
            solid.push_closed(points.clone());
        } else {
            solid.push_open(points.clone());
        }
        out.band
            .subpaths
            .extend(schist_vector::stroke_path(&solid, style).subpaths);
    }
    out.ink.closed = vec![true; out.ink.subpaths.len()];
    out.band.closed = vec![true; out.band.subpaths.len()];
    Some(out)
}

/// A flattened contour and the indices of its corner points.
struct Contour<'a> {
    points: &'a [Pt2],
    closed: bool,
    corners: &'a [usize],
}

impl Contour<'_> {
    /// The stretches a pattern fits to, each an open polyline: the whole
    /// contour unfitted, else from corner to corner (and end to end). A
    /// closed contour without corners is one stretch round to its start.
    fn stretches(&self, fitting: DecorationFit) -> Vec<Vec<Pt2>> {
        let points = self.points;
        let around = || {
            let mut all = points.to_vec();
            all.push(points[0]);
            all
        };
        if fitting == DecorationFit::None {
            return vec![if self.closed {
                around()
            } else {
                points.to_vec()
            }];
        }
        let mut bounds: Vec<usize> = self.corners.to_vec();
        if !self.closed {
            bounds.push(0);
            bounds.push(points.len() - 1);
        }
        bounds.sort_unstable();
        bounds.dedup();
        if bounds.is_empty() {
            return vec![around()];
        }
        let mut out: Vec<Vec<Pt2>> = bounds
            .windows(2)
            .map(|pair| points[pair[0]..=pair[1]].to_vec())
            .collect();
        if self.closed {
            let (first, last) = (bounds[0], bounds[bounds.len() - 1]);
            let mut wrap = points[last..].to_vec();
            wrap.extend_from_slice(&points[..=first]);
            out.push(wrap);
        }
        out
    }
}

/// The dashes of `lengths` along a contour, as open polylines to stroke
/// with their cap. None when there would be more than the budget allows.
fn dash(
    line: &Contour<'_>,
    lengths: &[f32],
    fitting: DecorationFit,
    budget: &mut usize,
) -> Option<Path> {
    let stretches = line.stretches(fitting);
    let mut dasher = Dasher::default();
    let cornerless = line.closed && line.corners.is_empty();
    for stretch in &stretches {
        let span: f64 = stretch
            .windows(2)
            .map(|e| f64::from(e[1].0 - e[0].0).hypot(f64::from(e[1].1 - e[0].1)))
            .sum();
        let fitted = if fitting != DecorationFit::None && cornerless {
            cycles(lengths, span, fitting)
        } else {
            schist_text_engine::fit_dashes(lengths, span, fitting)
        };
        if !dasher.run(stretch, &fitted, fitting != DecorationFit::None, *budget) {
            return None;
        }
        // A dash only continues round a corner a fitted pattern pins.
        if fitting == DecorationFit::None {
            dasher.finish(false);
        }
    }
    // Fitted round a closed contour, the last dash meets the first.
    dasher.finish(line.closed && fitting != DecorationFit::None);
    *budget = budget.saturating_sub(dasher.pieces.len());
    let mut out = Path::default();
    for piece in dasher.pieces {
        out.push_open(piece);
    }
    Some(out)
}

/// Whole cycles round a closed contour without corners, so the pattern
/// meets itself where it began, the lengths `fitting` names changed by the
/// least proportion.
fn cycles(lengths: &[f32], span: f64, fitting: DecorationFit) -> Vec<f64> {
    let original: Vec<f64> = lengths.iter().map(|v| f64::from(*v)).collect();
    let dashes: f64 = original.iter().step_by(2).sum();
    let gaps: f64 = original.iter().skip(1).step_by(2).sum();
    let period = dashes + gaps;
    if !(span > 0.0 && period > 0.0 && span.is_finite()) {
        return original;
    }
    let ideal = span / period;
    let mut best: Option<(f64, f64, f64)> = None;
    for count in [ideal.floor(), ideal.ceil()] {
        if count < 1.0 {
            continue;
        }
        let each = span / count;
        let (dash, gap) = match fitting {
            DecorationFit::DashesAndGaps => (each / period, each / period),
            DecorationFit::Dashes if dashes > 0.0 => ((each - gaps) / dashes, 1.0),
            DecorationFit::Gaps if gaps > 0.0 => (1.0, (each - dashes) / gaps),
            _ => continue,
        };
        let scale = if fitting == DecorationFit::Gaps {
            gap
        } else {
            dash
        };
        if !(scale > 0.0 && scale.is_finite()) {
            continue;
        }
        let cost = scale.ln().abs();
        if best.is_none_or(|(old, _, _)| cost < old) {
            best = Some((cost, dash, gap));
        }
    }
    match best {
        Some((_, dash, gap)) => original
            .iter()
            .enumerate()
            .map(|(i, v)| v * if i % 2 == 0 { dash } else { gap })
            .collect(),
        None => original,
    }
}

/// Walks polylines laying dashes along them. A dash still open at the end
/// of one stretch carries on into the next, so it joins round the corner.
#[derive(Default)]
struct Dasher {
    pieces: Vec<Vec<Pt2>>,
    current: Option<Vec<Pt2>>,
    direction: Pt2,
}

impl Dasher {
    /// Lay `lengths` from the stretch's first point, a dash first; `pinned`
    /// when they are fitted to end with a dash too. False when the pieces
    /// exceed the budget.
    fn run(&mut self, points: &[Pt2], lengths: &[f64], pinned: bool, budget: usize) -> bool {
        let period: f64 = lengths.iter().sum();
        if lengths.is_empty() || !(period > 1e-9 && period.is_finite()) || points.is_empty() {
            return false;
        }
        let mut index = 0;
        let mut left = lengths[0];
        let mut on = true;
        if self.current.is_none() {
            self.current = Some(vec![points[0]]);
        }
        for edge in points.windows(2) {
            let (a, b) = (edge[0], edge[1]);
            let (dx, dy) = (f64::from(b.0 - a.0), f64::from(b.1 - a.1));
            let length = dx.hypot(dy);
            if length <= 0.0 {
                continue;
            }
            self.direction = ((dx / length) as f32, (dy / length) as f32);
            let mut at = 0.0;
            // An element ending within a hair of the edge's end ends at the
            // start of the next, or at the stretch's end.
            while length - at > left + HAIR {
                at += left;
                let point = (
                    (f64::from(a.0) + dx * at / length) as f32,
                    (f64::from(a.1) + dy * at / length) as f32,
                );
                if on {
                    let mut piece = self.current.take().unwrap_or_default();
                    piece.push(point);
                    self.emit(piece);
                    if self.pieces.len() > budget {
                        return false;
                    }
                } else {
                    self.current = Some(vec![point]);
                }
                on = !on;
                index = (index + 1) % lengths.len();
                left = lengths[index];
            }
            left -= length - at;
            if let Some(current) = self.current.as_mut().filter(|_| on) {
                current.push(b);
            }
        }
        // A dash reaching the stretch's end stays open, to carry on into
        // the next; a gap reaching a fitted stretch's end opens a dash
        // there, so it has a dash (or dot) at both ends.
        if pinned && !on && left <= HAIR {
            if let Some(&end) = points.last() {
                self.current = Some(vec![end]);
                on = true;
            }
        }
        if !on {
            self.current = None;
        }
        true
    }

    /// End the open dash; `wrap` joins it to the first, which began where
    /// a closed contour ends.
    fn finish(&mut self, wrap: bool) {
        let Some(piece) = self.current.take() else {
            return;
        };
        match self.pieces.first_mut() {
            Some(first) if wrap => {
                let mut joined = piece;
                joined.extend_from_slice(&first[1..]);
                *first = joined;
            }
            _ => self.emit(piece),
        }
    }

    /// A dash of no length still faces along the path, for its caps.
    fn emit(&mut self, mut piece: Vec<Pt2>) {
        let start = piece[0];
        if piece
            .iter()
            .all(|p| (p.0 - start.0).abs() <= 1e-6 && (p.1 - start.1).abs() <= 1e-6)
        {
            piece = vec![
                start,
                (
                    start.0 + self.direction.0 * 1e-3,
                    start.1 + self.direction.1 * 1e-3,
                ),
            ];
        }
        self.pieces.push(piece);
    }
}

/// One stripe on one side of the path: the points `across[0]` to
/// `across[1]` to its left (negative to its right), both on the same side.
/// Each stretch between two points is cut along the bisectors at its ends,
/// so stripes mitre round corners and never cross the path; where the
/// stroke's join bevels or rounds the outside of a corner, so does the
/// stripe. Open ends take the stroke's cap.
fn band(ink: &mut Path, line: &Contour<'_>, across: [f32; 2], style: StrokeStyle) {
    let points = dedup(line.points);
    let count = points.len();
    if count < 2 {
        return;
    }
    let closed = line.closed && count >= 3;
    let edges = if closed { count } else { count - 1 };
    let unit = |a: Pt2, b: Pt2| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        (dx / length, dy / length)
    };
    let direction: Vec<Pt2> = (0..edges)
        .map(|i| unit(points[i], points[(i + 1) % count]))
        .collect();
    let left = |d: Pt2| (d.1, -d.0);
    let side = if across[1] > 0.0 { 1.0 } else { -1.0 };
    let half = style.width / 2.0;
    let at = |p: Pt2, v: Pt2, t: f32| (p.0 + v.0 * t, p.1 + v.1 * t);
    // How the stripe meets vertex `j`, coming from `into` and leaving along
    // `out`: along the bisector, or square across with a join piece.
    let mitre = |into: Pt2, out: Pt2| -> Option<(Pt2, f32)> {
        let (n_in, n_out) = (left(into), left(out));
        let sum = (n_in.0 + n_out.0, n_in.1 + n_out.1);
        let length = sum.0.hypot(sum.1);
        if length < 1e-6 {
            return None;
        }
        let m = (sum.0 / length, sum.1 / length);
        let cos = m.0 * n_in.0 + m.1 * n_in.1;
        let turn = into.0 * out.1 - into.1 * out.0;
        let outer = side * turn > 1e-6;
        if !outer {
            return Some((m, 1.0 / cos.max(0.1)));
        }
        (style.join == LineJoin::Miter && 1.0 / cos <= style.miter_limit).then_some((m, 1.0 / cos))
    };
    for i in 0..edges {
        let (p0, p1) = (points[i], points[(i + 1) % count]);
        let d = direction[i];
        let n = left(d);
        let start_free = !closed && i == 0;
        let end_free = !closed && i + 1 == edges;
        let start = if start_free {
            None
        } else {
            mitre(direction[(i + edges - 1) % edges], d)
        };
        let end = if end_free {
            None
        } else {
            mitre(d, direction[(i + 1) % edges])
        };
        let extend = |free: bool| {
            if free && style.cap == LineCap::Square {
                half
            } else {
                0.0
            }
        };
        let from = at(p0, d, -extend(start_free));
        let to = at(p1, d, extend(end_free));
        let point = |p: Pt2, joint: Option<(Pt2, f32)>, t: f32| match joint {
            Some((m, scale)) => at(p, m, t * scale),
            None => at(p, n, t),
        };
        push_wound(
            ink,
            vec![
                point(from, start, across[0]),
                point(to, end, across[0]),
                point(to, end, across[1]),
                point(from, start, across[1]),
            ],
        );
        // The outside of a corner that does not mitre: a bevelled or round
        // piece of the stripe, once per corner.
        if !end_free && end.is_none() {
            let next = left(direction[(i + 1) % edges]);
            let turn = d.0 * direction[(i + 1) % edges].1 - d.1 * direction[(i + 1) % edges].0;
            if side * turn > 1e-6 {
                push_wound(ink, corner_piece(p1, n, next, across, style.join));
            }
        }
        if style.cap == LineCap::Round {
            if start_free {
                push_wound(ink, round_cap(p0, (-d.0, -d.1), n, across, half));
            }
            if end_free {
                push_wound(ink, round_cap(p1, d, n, across, half));
            }
        }
    }
}

/// A stripe's piece of a corner's outside, from normal `a` to normal `b`:
/// a sector between the stripe's edges when the join rounds, a bevel
/// otherwise.
fn corner_piece(p: Pt2, a: Pt2, b: Pt2, across: [f32; 2], join: LineJoin) -> Vec<Pt2> {
    let at = |v: Pt2, t: f32| (p.0 + v.0 * t, p.1 + v.1 * t);
    if join != LineJoin::Round {
        return vec![
            at(a, across[0]),
            at(a, across[1]),
            at(b, across[1]),
            at(b, across[0]),
        ];
    }
    let (start, mut sweep) = (a.1.atan2(a.0), b.1.atan2(b.0) - a.1.atan2(a.0));
    if sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    } else if sweep < -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }
    let steps = ((sweep.abs() * across[0].abs().max(across[1].abs())).ceil() as usize).clamp(2, 48);
    let arc = |t: f32| {
        (0..=steps).map(move |k| {
            let angle = start + sweep * k as f32 / steps as f32;
            (p.0 + angle.cos() * t, p.1 + angle.sin() * t)
        })
    };
    let mut out: Vec<Pt2> = arc(across[1].abs()).collect();
    let inner: Vec<Pt2> = arc(across[0].abs()).collect();
    out.extend(inner.into_iter().rev());
    out
}

/// A stripe's piece of a round cap at an open end `p`, facing `outward`:
/// the half disc of the stroke's radius between the stripe's edges.
fn round_cap(p: Pt2, outward: Pt2, n: Pt2, across: [f32; 2], radius: f32) -> Vec<Pt2> {
    let place = |s: f32, t: f32| (p.0 + outward.0 * s + n.0 * t, p.1 + outward.1 * s + n.1 * t);
    let reach = |t: f32| (radius * radius - t * t).max(0.0).sqrt();
    let steps = (((across[1] - across[0]) * 2.0).ceil() as usize).clamp(4, 48);
    let mut out = vec![place(0.0, across[0])];
    for k in 0..=steps {
        let t = across[0] + (across[1] - across[0]) * k as f32 / steps as f32;
        out.push(place(reach(t), t));
    }
    out.push(place(0.0, across[1]));
    out
}

/// A closed polyline moved `distance` to its left as it runs (negative to
/// its right), point for point, each corner mitred and the mitre held to
/// `limit` times the distance.
fn offset(points: &[Pt2], distance: f32, limit: f32) -> Vec<Pt2> {
    let count = points.len();
    let normal = |a: Pt2, b: Pt2| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        (length > 1e-6).then(|| (dy / length, -dx / length))
    };
    (0..count)
        .map(|i| {
            let p = points[i];
            let before = (1..count).find_map(|k| normal(points[(i + count - k) % count], p));
            let after = (1..count).find_map(|k| normal(p, points[(i + k) % count]));
            let (Some(a), Some(b)) = (before, after) else {
                return p;
            };
            let sum = (a.0 + b.0, a.1 + b.1);
            let length = sum.0.hypot(sum.1);
            if length < 1e-6 {
                return (p.0 + a.0 * distance, p.1 + a.1 * distance);
            }
            let m = (sum.0 / length, sum.1 / length);
            let scale = (1.0 / (m.0 * a.0 + m.1 * a.1)).min(limit.max(1.0));
            (p.0 + m.0 * distance * scale, p.1 + m.1 * distance * scale)
        })
        .collect()
}

/// Positive when the contour runs clockwise on the page (y down).
fn signed_area(points: &[Pt2]) -> f32 {
    let count = points.len();
    (0..count)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % count]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f32>()
        / 2.0
}

/// Every piece shares one winding, so pieces union under the nonzero rule.
fn push_wound(out: &mut Path, mut piece: Vec<Pt2>) {
    if signed_area(&piece) < 0.0 {
        piece.reverse();
    }
    out.push_closed(piece);
}

fn dedup(points: &[Pt2]) -> Vec<Pt2> {
    let mut out: Vec<Pt2> = Vec::with_capacity(points.len());
    for &p in points {
        if out
            .last()
            .is_none_or(|l| (l.0 - p.0).abs() > 1e-6 || (l.1 - p.1).abs() > 1e-6)
        {
            out.push(p);
        }
    }
    out
}
