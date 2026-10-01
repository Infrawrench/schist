//! Bend already resolved decoration paints along the shared baseline.
//!
//! Pattern fitting and ink partitioning happen in inline coordinates first.
//! A strip mesh then maps that coverage onto the flattened guide. Joins use a
//! miter up to four times the offset, then a bevel (including reversals).
//! This is Schist's bounded raster policy, not a claim about native corner fitting.
use super::{text_path::Guide, ColoredRaster, TextSpec};
use schist_core::IntRect;
use std::collections::HashMap;

type Point = [f64; 2];

#[derive(Clone, Copy)]
struct Section {
    inline: f64,
    point: Point,
    normal: Point,
}

#[derive(Clone, Copy)]
struct Vertex {
    point: Point,
    source: Point,
}

struct Triangle {
    vertices: [Vertex; 3],
    determinant: f64,
    bounds: IntRect,
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn subtract(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn bounds(points: impl Iterator<Item = Point>) -> Option<IntRect> {
    let mut edges = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in points {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        edges = [
            edges[0].min(x),
            edges[1].min(y),
            edges[2].max(x),
            edges[3].max(y),
        ];
    }
    let [left, top, right, bottom] = [
        edges[0].floor(),
        edges[1].floor(),
        edges[2].ceil(),
        edges[3].ceil(),
    ];
    if left <= f64::from(i32::MIN)
        || top <= f64::from(i32::MIN)
        || right >= f64::from(i32::MAX)
        || bottom >= f64::from(i32::MAX)
        || right - left > f64::from(i32::MAX)
        || bottom - top > f64::from(i32::MAX)
    {
        return None;
    }
    Some(IntRect::new(
        left as i32,
        top as i32,
        right as i32,
        bottom as i32,
    ))
}

impl Triangle {
    fn new(vertices: [Vertex; 3]) -> Option<Self> {
        let determinant = cross(
            subtract(vertices[1].point, vertices[0].point),
            subtract(vertices[2].point, vertices[0].point),
        );
        if !determinant.is_finite() || determinant.abs() < 1e-12 {
            return None;
        }
        Some(Self {
            bounds: bounds(vertices.iter().map(|v| v.point))?,
            vertices,
            determinant,
        })
    }

    fn source_at(&self, point: Point) -> Option<Point> {
        let [a, b, c] = self.vertices;
        let relative = subtract(point, a.point);
        let u = cross(relative, subtract(c.point, a.point)) / self.determinant;
        let v = cross(subtract(b.point, a.point), relative) / self.determinant;
        // Shared triangle edges must own the same samples despite roundoff.
        if u < -1e-10 || v < -1e-10 || u + v > 1.0 + 1e-10 {
            return None;
        }
        Some([
            a.source[0] + u * (b.source[0] - a.source[0]) + v * (c.source[0] - a.source[0]),
            a.source[1] + u * (b.source[1] - a.source[1]) + v * (c.source[1] - a.source[1]),
        ])
    }
}

fn mesh(guide: &Guide, rect: IntRect, baseline: f32) -> Vec<Triangle> {
    let vertices: Vec<_> = guide
        .vertices()
        .map(|(x, (a, b))| (f64::from(x), [f64::from(a), f64::from(b)]))
        .collect();
    let normals: Vec<_> = vertices
        .windows(2)
        .map(|pair| {
            let d = subtract(pair[1].1, pair[0].1);
            let length = d[0].hypot(d[1]);
            [-d[1] / length, d[0] / length]
        })
        .collect();
    let mut sections = Vec::new();
    for (index, &(inline, point)) in vertices.iter().enumerate() {
        let before = normals[index.saturating_sub(1)];
        let after = normals[index.min(normals.len() - 1)];
        let denominator = 1.0 + before[0] * after[0] + before[1] * after[1];
        if denominator >= 0.125 {
            sections.push(Section {
                inline,
                point,
                normal: [
                    (before[0] + after[0]) / denominator,
                    (before[1] + after[1]) / denominator,
                ],
            });
        } else {
            // Two sections at the same arc coordinate make a bevel. Their
            // join has zero inline length, so it cannot reset the dash phase.
            sections.push(Section {
                inline,
                point,
                normal: before,
            });
            sections.push(Section {
                inline,
                point,
                normal: after,
            });
        }
    }
    let left = f64::from(rect.left);
    let right = f64::from(rect.right);
    let extend = |section: Section, inline: f64| Section {
        inline,
        point: [
            section.point[0] + section.normal[1] * (inline - section.inline),
            section.point[1] - section.normal[0] * (inline - section.inline),
        ],
        ..section
    };
    if left < sections[0].inline {
        sections.insert(0, extend(sections[0], left));
    }
    if right > sections.last().unwrap().inline {
        sections.push(extend(*sections.last().unwrap(), right));
    }
    let vertex = |section: Section, y: i32| {
        let cross = f64::from(y) - f64::from(baseline);
        Vertex {
            point: [
                section.point[0] + section.normal[0] * cross,
                section.point[1] + section.normal[1] * cross,
            ],
            source: [section.inline, f64::from(y)],
        }
    };
    let mut triangles = Vec::new();
    for pair in sections.windows(2) {
        let [mut a, mut b] = [pair[0], pair[1]];
        if b.inline < left || a.inline > right {
            continue;
        }
        let width = b.inline - a.inline;
        if width > 0.0 {
            let at = |inline| {
                let t = (inline - a.inline) / width;
                Section {
                    inline,
                    point: lerp(a.point, b.point, t),
                    normal: lerp(a.normal, b.normal, t),
                }
            };
            let start = at(a.inline.max(left));
            let end = at(b.inline.min(right));
            a = start;
            b = end;
        }
        let [a, b, c, d] = [
            vertex(a, rect.top),
            vertex(a, rect.bottom),
            vertex(b, rect.top),
            vertex(b, rect.bottom),
        ];
        triangles.extend(Triangle::new([a, b, c]));
        triangles.extend(Triangle::new([b, d, c]));
    }
    triangles
}

/// One row of subpixel unions keeps scratch memory independent of image height.
/// Coverage is unioned before averaging and opacity, including folded strips.
fn warp(guide: &Guide, baseline: f32, rect: IntRect, source: &[u8]) -> Option<(IntRect, Vec<u8>)> {
    let triangles = mesh(guide, rect, baseline);
    let output = bounds(triangles.iter().flat_map(|t| t.vertices.map(|v| v.point)))?;
    let width = usize::try_from(output.width()).ok()?;
    let height = usize::try_from(output.height()).ok()?;
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(width.checked_mul(height)?).ok()?;
    let mut samples = Vec::new();
    samples.try_reserve_exact(width.checked_mul(16)?).ok()?;
    samples.resize(width * 16, 0u8);
    for y in output.top..output.bottom {
        samples.fill(0);
        for triangle in triangles
            .iter()
            .filter(|t| t.bounds.top <= y && y < t.bounds.bottom)
        {
            for x in triangle.bounds.left..triangle.bounds.right {
                for row in 0..4 {
                    for col in 0..4 {
                        let point = [
                            f64::from(x) + (f64::from(col) + 0.5) / 4.0,
                            f64::from(y) + (f64::from(row) + 0.5) / 4.0,
                        ];
                        let Some([sx, sy]) = triangle.source_at(point) else {
                            continue;
                        };
                        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
                        if sx < rect.left || sx >= rect.right || sy < rect.top || sy >= rect.bottom
                        {
                            continue;
                        }
                        let value =
                            source[((sy - rect.top) * rect.width() + sx - rect.left) as usize];
                        let slot = &mut samples
                            [(x - output.left) as usize * 16 + (row * 4 + col) as usize];
                        *slot = (*slot).max(value);
                    }
                }
            }
        }
        bitmap.extend(
            samples
                .as_chunks::<16>()
                .0
                .iter()
                .map(|pixel| ((pixel.iter().map(|v| u32::from(*v)).sum::<u32>() + 8) / 16) as u8),
        );
    }
    Some((output, bitmap))
}

pub(super) fn bend(
    spec: &TextSpec,
    guide: &Guide,
    baseline: f32,
    mut fragments: Vec<ColoredRaster>,
    groups: &HashMap<usize, usize>,
) -> Vec<ColoredRaster> {
    let key = |fragment: &ColoredRaster| {
        (
            groups.get(&fragment.byte).copied().unwrap_or(0),
            fragment.kind,
        )
    };
    fragments.sort_by_key(&key);
    let mut out = Vec::new();
    let mut from = 0;
    while from < fragments.len() {
        let first = &fragments[from];
        let color = first.kind.color(&spec.style_at(first.byte));
        let mut to = from + 1;
        let mut rect = first.rect;
        while to < fragments.len()
            && key(&fragments[to]) == key(first)
            && fragments[to].kind.color(&spec.style_at(fragments[to].byte)) == color
        {
            rect = rect.union(&fragments[to].rect);
            to += 1;
        }
        // Merge equal consecutive paints before resampling. Bending each
        // character's independently antialiased mask would dim shared edges.
        let mut source = vec![0u8; rect.width() as usize * rect.height() as usize];
        for part in &fragments[from..to] {
            for y in part.rect.top..part.rect.bottom {
                for x in part.rect.left..part.rect.right {
                    let value = part.bitmap
                        [((y - part.rect.top) * part.rect.width() + x - part.rect.left) as usize];
                    let slot =
                        &mut source[((y - rect.top) * rect.width() + x - rect.left) as usize];
                    *slot = (*slot).max(value);
                }
            }
        }
        if let Some((rect, bitmap)) = warp(guide, baseline, rect, &source) {
            out.push(ColoredRaster {
                kind: first.kind,
                byte: first.byte,
                rect,
                bitmap,
            });
        }
        from = to;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Align, TextPath};
    use schist_core::path::{Anchor, SubPath};

    #[test]
    fn circular_baselines_follow_independent_annular_geometry_at_several_offsets_and_weights() {
        // The expected annulus uses radius/angle only, not production guide
        // vertices, normals, mesh triangles or inverse texture coordinates.
        for radius in [16.0_f32, 32.0, 80.0] {
            for offset in [-4, 0, 7] {
                for weight in [4, 8] {
                    let mut a = Anchor::corner(radius, 0.0);
                    let mut b = Anchor::corner(0.0, radius);
                    let handle = radius * 4.0 * (std::f32::consts::PI / 8.0).tan() / 3.0;
                    a.handle_out = (0.0, handle);
                    b.handle_in = (handle, 0.0);
                    let guide = Guide::new(
                        &TextPath {
                            curve: SubPath {
                                anchors: vec![a, b],
                                closed: false,
                            },
                            offset: 0.0,
                            span: None,
                        },
                        Align::Left,
                        0.0,
                    )
                    .unwrap();
                    let rect = IntRect::new(
                        0,
                        offset - weight / 2,
                        (radius * std::f32::consts::FRAC_PI_2).floor() as i32,
                        offset + weight / 2,
                    );
                    let source = vec![255; rect.width() as usize * rect.height() as usize];
                    let (bounds, actual) = warp(&guide, 0.0, rect, &source).unwrap();
                    let inner = radius - rect.bottom as f32;
                    let outer = radius - rect.top as f32;
                    let mut interior = 0;
                    let mut exterior = 0;
                    for y in 0..(radius as i32 + 12) {
                        for x in 0..(radius as i32 + 12) {
                            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                            let angle = py.atan2(px);
                            if !(0.15..1.4).contains(&angle) {
                                continue;
                            }
                            let distance = px.hypot(py);
                            let coverage = if x >= bounds.left
                                && x < bounds.right
                                && y >= bounds.top
                                && y < bounds.bottom
                            {
                                actual
                                    [((y - bounds.top) * bounds.width() + x - bounds.left) as usize]
                            } else {
                                0
                            };
                            // One pixel excludes the antialiased edge and the
                            // cubic-circle/0.1px flattening approximation.
                            if distance > inner + 1.0 && distance < outer - 1.0 {
                                assert_eq!(
                                    coverage, 255,
                                    "r={radius},offset={offset},weight={weight},pixel=({x},{y})"
                                );
                                interior += 1;
                            } else if distance < inner - 1.0 || distance > outer + 1.0 {
                                assert_eq!(
                                    coverage, 0,
                                    "r={radius},offset={offset},weight={weight},pixel=({x},{y})"
                                );
                                exterior += 1;
                            }
                        }
                    }
                    assert!(interior > 0 && exterior > 0);
                }
            }
        }
    }

    #[test]
    fn subdivisions_do_not_change_coverage_and_reversals_union_before_opacity() {
        let rect = IntRect::new(-3, -4, 240, 7);
        let source = vec![128; rect.width() as usize * rect.height() as usize];
        let mut expected = None;
        for count in [1, 2, 4, 8] {
            let anchors = (0..=count)
                .map(|i| Anchor::corner(10.0 + i as f32 * 200.0 / count as f32, 30.0))
                .collect();
            let guide = Guide::new(
                &TextPath {
                    curve: SubPath {
                        anchors,
                        closed: false,
                    },
                    offset: 0.0,
                    span: None,
                },
                Align::Left,
                0.0,
            )
            .unwrap();
            let actual = warp(&guide, 2.0, rect, &source).unwrap();
            if let Some(expected) = &expected {
                assert_eq!(&actual, expected);
            } else {
                expected = Some(actual);
            }
        }
        for y in [20.0, 20.001, 22.0] {
            let guide = Guide::new(
                &TextPath {
                    curve: SubPath {
                        anchors: vec![
                            Anchor::corner(10.0, 20.0),
                            Anchor::corner(110.0, 20.0),
                            Anchor::corner(10.0, y),
                            Anchor::corner(110.0, y),
                        ],
                        closed: false,
                    },
                    offset: 0.0,
                    span: None,
                },
                Align::Left,
                0.0,
            )
            .unwrap();
            let (bounds, actual) = warp(&guide, 2.0, rect, &source).unwrap();
            assert!(bounds.width() < 130 && bounds.height() < 30);
            assert!(actual.contains(&128));
            assert!(actual.iter().all(|v| *v <= 128));
        }
    }

    #[test]
    fn cardinal_baselines_preserve_every_coverage_sample_without_seams_or_opacity_changes() {
        let rect = IntRect::new(-5, -3, 29, 7);
        let source: Vec<_> = (0..rect.width() * rect.height())
            .map(|i| (i % 256) as u8)
            .collect();
        for (tx, ty) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
            let guide = Guide::new(
                &TextPath {
                    curve: SubPath {
                        anchors: vec![
                            Anchor::corner(50.0, 60.0),
                            Anchor::corner(50.0 + tx as f32 * 20.0, 60.0 + ty as f32 * 20.0),
                        ],
                        closed: false,
                    },
                    offset: 3.0,
                    span: None,
                },
                Align::Left,
                0.0,
            )
            .unwrap();
            let (out, actual) = warp(&guide, 2.0, rect, &source).unwrap();
            assert_eq!(actual.len(), source.len());
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let px =
                        50 + tx * (x + 3) - ty * (y - 2) + if tx < 0 || ty > 0 { -1 } else { 0 };
                    let py =
                        60 + ty * (x + 3) + tx * (y - 2) + if ty < 0 || tx < 0 { -1 } else { 0 };
                    assert_eq!(
                        actual[((py - out.top) * out.width() + px - out.left) as usize],
                        source[((y - rect.top) * rect.width() + x - rect.left) as usize],
                        "tangent=({tx},{ty}), ({x},{y})"
                    );
                }
            }
        }
    }
}
