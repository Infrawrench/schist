use super::*;
use schist_core::path::SubPath;

/// An independent copy of one vector subpath, used as the text's baseline.
/// Coordinates are relative to the text layer's origin, so moving the layer
/// moves the path too. Extra lines sit at their usual distance below it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextPath {
    pub curve: SubPath,
    #[serde(default)]
    pub offset: f32,
    /// Optional inline interval used for alignment. The layout composer owns
    /// overflow; the shared renderer still extrapolates to keep text editable.
    /// None retains the raster Type tool's whole-path alignment plus offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<f32>,
}

impl TextPath {
    /// The same flattened arc length used to place glyphs and carets.
    pub fn length(&self) -> Option<f32> {
        Guide::new(self, Align::Left, 0.0)?
            .distances
            .last()
            .copied()
    }

    /// Convert baseline geometry and its inline interval to another resolution.
    pub fn scaled(&mut self, scale: f32) {
        for anchor in &mut self.curve.anchors {
            for point in [
                &mut anchor.point,
                &mut anchor.handle_in,
                &mut anchor.handle_out,
            ] {
                point.0 *= scale;
                point.1 *= scale;
            }
        }
        self.offset *= scale;
        self.span = self.span.map(|value| value * scale);
    }
}

pub(super) struct Guide {
    points: Vec<(f32, f32)>,
    distances: Vec<f32>,
    offset: f32,
}

impl Guide {
    /// Baseline vertices and their aligned inline coordinates. Decorations use
    /// the same flattened curve as glyphs rather than measuring it again.
    pub(super) fn vertices(&self) -> impl Iterator<Item = (f32, (f32, f32))> + '_ {
        self.distances
            .iter()
            .zip(&self.points)
            .map(|(distance, point)| (*distance - self.offset, *point))
    }

    pub fn new(path: &TextPath, align: Align, width: f32) -> Option<Self> {
        let anchors = &path.curve.anchors;
        if anchors.len() < 2
            || !path.offset.is_finite()
            || path
                .span
                .is_some_and(|span| !span.is_finite() || span < 0.0)
            || anchors.iter().any(|a| {
                [a.point, a.handle_in, a.handle_out]
                    .into_iter()
                    .any(|(x, y)| !x.is_finite() || !y.is_finite())
            })
        {
            return None;
        }
        let mut builder = schist_vector::PathBuilder::new();
        builder.move_to(anchors[0].point.0, anchors[0].point.1);
        let count = anchors.len() - usize::from(!path.curve.closed);
        for i in 0..count {
            let a = anchors[i];
            let b = anchors[(i + 1) % anchors.len()];
            // A native straight segment has coincident handles. Flattening
            // that degenerate cubic adds short, rounded edges near its ends;
            // their noisy tangent becomes visible when a caret extrapolates.
            if a.handle_out == (0.0, 0.0) && b.handle_in == (0.0, 0.0) {
                builder.line_to(b.point.0, b.point.1);
                continue;
            }
            builder.cubic_to(
                a.point.0 + a.handle_out.0,
                a.point.1 + a.handle_out.1,
                b.point.0 + b.handle_in.0,
                b.point.1 + b.handle_in.1,
                b.point.0,
                b.point.1,
            );
        }
        let flat = builder.build(0.1);
        let mut points = flat.subpaths.into_iter().next()?;
        points.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 1e-5);
        if points.len() < 2 {
            return None;
        }
        let mut distances = vec![0.0];
        for pair in points.windows(2) {
            distances
                .push(distances.last()? + (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1));
        }
        let length = *distances.last()?;
        if !length.is_finite() || length <= 0.0 {
            return None;
        }
        let interval = path.span.unwrap_or(length);
        let offset = path.offset
            + match align {
                Align::Left => 0.0,
                Align::Center => (interval - width) / 2.0,
                Align::Right => interval - width,
            };
        Some(Self {
            points,
            distances,
            offset,
        })
    }

    /// Extrapolate past an endpoint along its tangent, so overflow stays
    /// editable instead of piling glyphs on the last point of the curve.
    pub fn at(&self, x: f32, y: f32) -> (f32, f32, f32) {
        let distance = x + self.offset;
        let i = self
            .distances
            .partition_point(|d| *d <= distance)
            .saturating_sub(1)
            .min(self.points.len() - 2);
        let a = self.points[i];
        let b = self.points[i + 1];
        let length = self.distances[i + 1] - self.distances[i];
        let (tx, ty) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
        let along = distance - self.distances[i];
        (
            a.0 + tx * along - ty * y,
            a.1 + ty * along + tx * y,
            ty.atan2(tx),
        )
    }

    pub fn caret(&self, mut caret: Caret, baseline: f32) -> Caret {
        let (x, y, angle) = self.at(caret.x, caret.top - baseline);
        caret.x = x;
        caret.top = y;
        caret.angle = angle;
        caret
    }
}

/// Rotate an already rasterized glyph about its baseline origin. Inverse
/// bilinear sampling retains antialiasing without holes from forward splats.
pub(super) fn glyph_bitmap(
    guide: &Guide,
    glyph: &PlacedGlyph,
    baseline: f32,
    metrics: &fontdue::Metrics,
    bitmap: Vec<u8>,
) -> (IntRect, Vec<u8>) {
    let center = metrics.advance_width / 2.0;
    let (px, py, angle) = guide.at(glyph.x + center, glyph.baseline - baseline);
    let (sin, cos) = angle.sin_cos();
    // Cardinal rotations should preserve integer translations exactly;
    // sin/cos otherwise leave tiny residuals that add an empty border.
    let snap = |v: f32| {
        if (v - v.round()).abs() < 1e-4 {
            v.round()
        } else {
            v
        }
    };
    let (sin, cos) = (snap(sin), snap(cos));
    let left = metrics.xmin as f32 - center;
    let top = -(metrics.height as f32) - metrics.ymin as f32;
    let transform = |x: f32, y: f32| (px + x * cos - y * sin, py + x * sin + y * cos);
    let corners = [
        (left, top),
        (left + metrics.width as f32, top),
        (left, top + metrics.height as f32),
        (left + metrics.width as f32, top + metrics.height as f32),
    ]
    .map(|(x, y)| {
        let (x, y) = transform(x, y);
        (snap(x), snap(y))
    });
    let min_x = corners
        .iter()
        .map(|p| p.0)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let min_y = corners
        .iter()
        .map(|p| p.1)
        .fold(f32::INFINITY, f32::min)
        .floor() as i32;
    let max_x = corners
        .iter()
        .map(|p| p.0)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    let max_y = corners
        .iter()
        .map(|p| p.1)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as i32;
    let bounds = IntRect::new(min_x, min_y, max_x, max_y);
    let mut out = vec![0; bounds.width() as usize * bounds.height() as usize];
    for y in min_y..max_y {
        for x in min_x..max_x {
            let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
            let (sx, sy) = (
                dx * cos + dy * sin - left - 0.5,
                -dx * sin + dy * cos - top - 0.5,
            );
            let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
            let (fx, fy) = (sx - sx.floor(), sy - sy.floor());
            let mut value = 0.0;
            for (ox, wx) in [(0, 1.0 - fx), (1, fx)] {
                for (oy, wy) in [(0, 1.0 - fy), (1, fy)] {
                    let (bx, by) = (ix + ox, iy + oy);
                    if bx >= 0 && by >= 0 && bx < metrics.width as i32 && by < metrics.height as i32
                    {
                        value += bitmap[by as usize * metrics.width + bx as usize] as f32 * wx * wy;
                    }
                }
            }
            out[(y - min_y) as usize * bounds.width() as usize + (x - min_x) as usize] =
                value.round() as u8;
        }
    }
    (bounds, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_alignment_scaling_and_legacy_paths_share_one_arc_coordinate_system() {
        for angle in [
            0.0_f32,
            0.5,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        ] {
            let (sin, cos) = angle.sin_cos();
            let original = TextPath {
                curve: SubPath {
                    anchors: vec![
                        schist_core::path::Anchor::corner(13.0, 29.0),
                        schist_core::path::Anchor::corner(13.0 + 200.0 * cos, 29.0 + 200.0 * sin),
                    ],
                    closed: false,
                },
                offset: 20.0,
                span: Some(120.0),
            };
            for span in [None, Some(120.0), Some(0.0)] {
                for align in [Align::Left, Align::Center, Align::Right] {
                    let mut path = original.clone();
                    path.span = span;
                    for scale in [0.5, 1.0, 3.0] {
                        let mut scaled = path.clone();
                        scaled.scaled(scale);
                        let guide = Guide::new(&scaled, align, 30.0 * scale).unwrap();
                        let interval = span.unwrap_or(200.0);
                        let distance = 20.0
                            + match align {
                                Align::Left => 0.0,
                                Align::Center => (interval - 30.0) / 2.0,
                                Align::Right => interval - 30.0,
                            };
                        for x in [-10.0, 0.0, 25.0, 230.0] {
                            let (a, b, _) = guide.at(x * scale, 7.0 * scale);
                            assert!(
                                (a - (13.0 + (distance + x) * cos - 7.0 * sin) * scale).abs()
                                    < 0.001
                            );
                            assert!(
                                (b - (29.0 + (distance + x) * sin + 7.0 * cos) * scale).abs()
                                    < 0.001
                            );
                        }
                    }
                }
            }
            let mut json = serde_json::to_value(&original).unwrap();
            json.as_object_mut().unwrap().remove("span");
            assert_eq!(serde_json::from_value::<TextPath>(json).unwrap().span, None);
        }
    }
}
