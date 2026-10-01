//! Affine geometry shared by layout, preview and print. Composition remains
//! in the untransformed frame; only placement and raster sampling use this map.
use crate::{Point, Rect};
pub use schist_core::Affine;
use schist_core::IntRect;

pub fn point(matrix: Affine, p: Point) -> Point {
    let (x, y) = matrix.apply(p.x, p.y);
    Point::new(x, y)
}

pub fn corners(rect: Rect) -> [Point; 4] {
    [
        rect.origin(),
        Point::new(rect.right(), rect.y),
        Point::new(rect.right(), rect.bottom()),
        Point::new(rect.x, rect.bottom()),
    ]
}

pub fn bounds(matrix: Affine, rect: Rect) -> Rect {
    let points = corners(rect).map(|p| point(matrix, p));
    points[1..]
        .iter()
        .fold(Rect::new(points[0].x, points[0].y, 0.0, 0.0), |rect, p| {
            rect.union(Rect::new(p.x, p.y, 0.0, 0.0))
        })
}

pub fn finite(matrix: Affine) -> bool {
    [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
        .iter()
        .all(|v| v.is_finite())
}

/// Invert with f64 intermediates so finite large/small transforms do not turn
/// into a zero inverse when their f32 determinant overflows or underflows.
pub fn inverse(matrix: Affine) -> Option<Affine> {
    if !finite(matrix) {
        return None;
    }
    let [a, b, c, d, tx, ty] =
        [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(f64::from);
    let det = a * d - b * c;
    if det == 0.0 {
        return None;
    }
    let result = Affine {
        a: (d / det) as f32,
        b: (-b / det) as f32,
        c: (-c / det) as f32,
        d: (a / det) as f32,
        tx: ((c * ty - d * tx) / det) as f32,
        ty: ((b * tx - a * ty) / det) as f32,
    };
    finite(result).then_some(result)
}

/// Express a transform in a different uniform scale/translation coordinate system.
pub fn in_view(matrix: Affine, scale: f32, origin: Point) -> Affine {
    let view = Affine::translate(origin.x, origin.y).then(&Affine::scale(scale, scale));
    view.then(&matrix).then(&view.invert().unwrap_or_default())
}

/// Maximum linear stretch (largest singular value), including shear. Rotation
/// and reflection do not change effective resolution.
pub fn stretch(matrix: Affine) -> f32 {
    linear_stretch(
        matrix.a as f64,
        matrix.b as f64,
        matrix.c as f64,
        matrix.d as f64,
    ) as f32
}
fn linear_stretch(a: f64, b: f64, c: f64, d: f64) -> f64 {
    // This form avoids subtracting almost equal squares for rotations/scales.
    ((a + d).hypot(b - c) + (a - d).hypot(b + c)) * 0.5
}

/// Lowest effective image resolution after placement, including anisotropic
/// image fitting and shear. Keep the intermediate ratios in f64 so a nominal
/// half-dpi value rounds consistently in preflight.
pub fn effective_dpi(matrix: Affine, image: Rect, pixels: (u32, u32)) -> Option<f32> {
    if pixels.0 == 0 || pixels.1 == 0 || !finite(matrix) {
        return None;
    }
    let x = image.width as f64 / pixels.0 as f64;
    let y = image.height as f64 / pixels.1 as f64;
    let stretch = linear_stretch(
        matrix.a as f64 * x,
        matrix.b as f64 * x,
        matrix.c as f64 * y,
        matrix.d as f64 * y,
    );
    (stretch.is_finite() && stretch > 0.0).then(|| (72.0 / stretch) as f32)
}

/// A bounded inverse map. Callers choose the destination allocation budget;
/// clipping happens after transformation, so off-page content can rotate in.
pub struct Warp {
    pub rect: IntRect,
    inverse: Affine,
}
impl Warp {
    pub fn new(source: Rect, matrix: Affine, clip: Rect, max_pixels: usize) -> Option<Self> {
        if !finite(matrix)
            || ![
                source.x,
                source.y,
                source.width,
                source.height,
                clip.x,
                clip.y,
                clip.width,
                clip.height,
            ]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let inverse = inverse(matrix)?;
        let dest = bounds(matrix, source).intersection(clip);
        if dest.width <= 0.0 || dest.height <= 0.0 {
            return Some(Self {
                rect: IntRect::EMPTY,
                inverse,
            });
        }
        let rect = IntRect::new(
            dest.x.floor() as i32,
            dest.y.floor() as i32,
            dest.right().ceil() as i32,
            dest.bottom().ceil() as i32,
        );
        let width = i64::from(rect.right) - i64::from(rect.left);
        let height = i64::from(rect.bottom) - i64::from(rect.top);
        if width > i64::from(i32::MAX)
            || height > i64::from(i32::MAX)
            || width.checked_mul(height)? > max_pixels as i64
        {
            return None;
        }
        Some(Self { rect, inverse })
    }
    pub fn source(&self, x: i32, y: i32) -> Point {
        point(self.inverse, Point::new(x as f32 + 0.5, y as f32 + 0.5))
    }
}

/// Bilinear sampling in premultiplied space, returned as straight channels and
/// alpha. Transparent neighbors cannot darken RGB or dilute native CMYK inks.
/// `source` locates the pixel grid; pixel colors/alpha are in 0..=1.
pub fn sample<const N: usize>(
    source: Rect,
    size: (u32, u32),
    at: Point,
    pixel: impl Fn(usize) -> ([f32; N], f32),
) -> ([f32; N], f32) {
    if size.0 == 0 || size.1 == 0 || source.width <= 0.0 || source.height <= 0.0 {
        return ([0.0; N], 0.0);
    }
    // Cancel matching grid dimensions before applying coordinates. Dividing
    // and multiplying by the same width introduces padding-dependent rounding
    // at half-coverage edges, even for an unscaled pixel-aligned source.
    let x = (at.x - source.x) * (size.0 as f32 / source.width) - 0.5;
    let y = (at.y - source.y) * (size.1 as f32 / source.height) - 0.5;
    let left = x.floor();
    let top = y.floor();
    let dx = x - left;
    let dy = y - top;
    let mut channels = [0.0; N];
    let mut alpha = 0.0;
    for (x, y, weight) in [
        (left, top, (1.0 - dx) * (1.0 - dy)),
        (left + 1.0, top, dx * (1.0 - dy)),
        (left, top + 1.0, (1.0 - dx) * dy),
        (left + 1.0, top + 1.0, dx * dy),
    ] {
        if x < 0.0 || y < 0.0 || x >= size.0 as f32 || y >= size.1 as f32 {
            continue;
        }
        let (color, a) = pixel(y as usize * size.0 as usize + x as usize);
        alpha += a * weight;
        for (out, value) in channels.iter_mut().zip(color) {
            *out += value * a * weight;
        }
    }
    if alpha > 0.0 {
        for value in &mut channels {
            *value /= alpha;
        }
    }
    (channels, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_padding_does_not_change_pixel_aligned_bilinear_coverage() {
        for width in [7, 11, 37, 83, 257] {
            for height in [9, 31, 67, 131] {
                for x in 0..95 {
                    for y in 0..115 {
                        let at = Point::new(x as f32 / 10.0, y as f32 / 10.0);
                        let read = |width: u32, height: u32| {
                            let (_, alpha) = sample::<0>(
                                Rect::new(0.0, 0.0, width as f32, height as f32),
                                (width, height),
                                at,
                                |i| {
                                    let x = i % width as usize;
                                    let y = i / width as usize;
                                    (
                                        [],
                                        if (2..5).contains(&x) && (3..7).contains(&y) {
                                            0.7
                                        } else {
                                            0.0
                                        },
                                    )
                                },
                            );
                            (alpha * 255.0).round() as u8
                        };
                        assert_eq!(
                            read(width, height),
                            read(7, 9),
                            "{width}x{height}, at={at:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn inverses_remain_valid_when_the_f32_determinant_exceeds_its_range() {
        for scale in [1e-20, 1e-10, 1.0, 1e10, 1e20] {
            let matrix = Affine::scale(scale, -scale);
            let inverse = inverse(matrix).unwrap();
            for p in [Point::new(0.2, -0.4), Point::new(-1.0, 1.0)] {
                let actual = point(inverse, point(matrix, p));
                assert!((actual.x - p.x).abs() < 1e-6 && (actual.y - p.y).abs() < 1e-6);
            }
        }
        assert!(inverse(Affine::scale(0.0, 1.0)).is_none());
        assert!(inverse(Affine::translate(f32::INFINITY, 0.0)).is_none());
    }

    #[test]
    fn inverse_sampling_preserves_pixels_under_quarter_turns_and_reflections() {
        let source = Rect::new(-3.0, 4.0, 5.0, 3.0);
        for matrix in [
            Affine::IDENTITY,
            Affine::scale(-1.0, 1.0),
            Affine {
                a: 0.0,
                b: 1.0,
                c: -1.0,
                d: 0.0,
                tx: 20.0,
                ty: 30.0,
            },
            Affine {
                a: 0.0,
                b: -1.0,
                c: 1.0,
                d: 0.0,
                tx: -12.0,
                ty: 7.0,
            },
        ] {
            let warp =
                Warp::new(source, matrix, Rect::new(-100.0, -100.0, 200.0, 200.0), 100).unwrap();
            let mut seen = Vec::new();
            for y in warp.rect.top..warp.rect.bottom {
                for x in warp.rect.left..warp.rect.right {
                    let (color, alpha) =
                        sample(source, (5, 3), warp.source(x, y), |i| ([i as f32], 1.0));
                    assert_eq!(alpha, 1.0);
                    seen.push(color[0] as usize);
                }
            }
            seen.sort_unstable();
            assert_eq!(seen, (0..15).collect::<Vec<_>>());
        }
    }

    #[test]
    fn transparency_never_dilutes_rgb_or_process_channels() {
        for alpha in [0.1, 0.5, 1.0] {
            for x in [0.0, 0.2, 0.5, 0.8, 1.0] {
                let (color, a) = sample(
                    Rect::new(0.0, 0.0, 1.0, 1.0),
                    (1, 1),
                    Point::new(x, 0.5),
                    |_| ([0.17, 0.42, 0.63, 0.91], alpha),
                );
                for (actual, expected) in color.into_iter().zip([0.17, 0.42, 0.63, 0.91]) {
                    assert!((actual - expected).abs() < 1e-6);
                }
                assert!(a > 0.0 && a <= alpha);
            }
        }
    }

    #[test]
    fn view_changes_commute_with_geometry_and_warps_are_bounded() {
        let matrix = Affine::rotate(0.7).then(&Affine::skew(0.3, -0.2));
        for scale in [0.1, 1.0, 4.0] {
            for origin in [Point::ZERO, Point::new(-17.0, 42.0)] {
                let view = Affine::translate(origin.x, origin.y).then(&Affine::scale(scale, scale));
                let converted = in_view(matrix, scale, origin);
                for p in [Point::ZERO, Point::new(3.0, 29.0), Point::new(-50.0, -31.0)] {
                    let a = point(view, point(matrix, p));
                    let b = point(converted, point(view, p));
                    assert!((a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001);
                }
            }
        }
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(Warp::new(rect, Affine::IDENTITY, rect, 9999).is_none());
        assert!(Warp::new(rect, Affine::scale(0.0, 1.0), rect, 10000).is_none());
        assert!(Warp::new(rect, Affine::translate(f32::NAN, 0.0), rect, 10000).is_none());
        assert_eq!(
            Warp::new(rect, Affine::translate(300.0, 0.0), rect, 0)
                .unwrap()
                .rect,
            IntRect::EMPTY
        );
    }
}
