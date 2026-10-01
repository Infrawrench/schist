//! Stroke actual font outlines; bitmap dilation would lose joins and scale.
use super::TextStrokeJoin;
use super::{text_path::Guide, ColoredRaster, LoadedFace, PaintKind, PlacedGlyph, TextStroke};
use schist_vector::{FillRule, LineJoin, PathBuilder, StrokeStyle};

struct Outline {
    path: PathBuilder,
    current: (f32, f32),
    scale: f32,
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current = (x, y);
        self.path.move_to(x * self.scale, -y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current = (x, y);
        self.path.line_to(x * self.scale, -y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (px, py) = self.current;
        self.curve_to(
            px + (x1 - px) * 2.0 / 3.0,
            py + (y1 - py) * 2.0 / 3.0,
            x + (x1 - x) * 2.0 / 3.0,
            y + (y1 - y) * 2.0 / 3.0,
            x,
            y,
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let s = self.scale;
        self.path
            .cubic_to(x1 * s, -y1 * s, x2 * s, -y2 * s, x * s, -y * s);
        self.current = (x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

pub(super) fn raster(
    font: &LoadedFace,
    size: f32,
    glyph: &PlacedGlyph,
    stroke: TextStroke,
    include_fill: bool,
    guide: Option<&Guide>,
    baseline: f32,
) -> Option<ColoredRaster> {
    let TextStroke { width, outside, .. } = stroke;
    let face = ttf_parser::Face::parse(&font.data, font.index).ok()?;
    let mut outline = Outline {
        path: PathBuilder::new(),
        current: (0.0, 0.0),
        scale: size / face.units_per_em() as f32,
    };
    face.outline_glyph(ttf_parser::GlyphId(glyph.glyph), &mut outline)?;
    let mut fill_path = outline.path.build(0.05);
    let mut path = schist_vector::stroke_path(
        &fill_path,
        StrokeStyle {
            width: if outside { width * 2.0 } else { width },
            join: match stroke.join {
                TextStrokeJoin::Miter => LineJoin::Miter,
                TextStrokeJoin::Round => LineJoin::Round,
                TextStrokeJoin::Bevel => LineJoin::Bevel,
            },
            miter_limit: stroke.miter_limit,
            ..Default::default()
        },
    );
    let along = guide.map(|guide| {
        let center = font.font.metrics_indexed(glyph.glyph, size).advance_width / 2.0;
        let (x, y, angle) = guide.at(glyph.x + center, glyph.baseline - baseline);
        (x, y, angle.sin_cos(), center)
    });
    for point in path
        .subpaths
        .iter_mut()
        .chain(&mut fill_path.subpaths)
        .flatten()
    {
        let (x, y) = *point;
        *point = if let Some((px, py, (sin, cos), center)) = along {
            (
                px + (x - center) * cos - y * sin,
                py + (x - center) * sin + y * cos,
            )
        } else if glyph.sideways {
            (-y, x)
        } else {
            (x, y)
        };
    }
    let rect = path.bounds();
    if rect.is_empty() {
        return None;
    }
    let mut bitmap = schist_vector::rasterize(&path, rect, FillRule::NonZero);
    if outside || include_fill {
        let fill = schist_vector::rasterize(&fill_path, rect, FillRule::NonZero);
        for (stroke, fill) in bitmap.iter_mut().zip(fill) {
            *stroke = if include_fill {
                (*stroke).max(fill)
            } else {
                stroke.saturating_sub(fill)
            };
        }
    }
    // Ordinary glyph placement is an integer translation, just like bitmap
    // fills. Keep it out of floating-point outline rasterization so distant
    // tabs cannot change edge coverage through coordinate rounding.
    let rect = if along.is_none() {
        rect.translated(
            super::glyph_pixel_start(glyph.x),
            super::glyph_pixel_start(glyph.baseline),
        )
    } else {
        rect
    };
    Some(ColoredRaster {
        kind: PaintKind::Stroke,
        rect,
        bitmap,
        byte: glyph.byte,
    })
}
