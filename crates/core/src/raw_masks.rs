//! Local adjustments for a camera-raw development: masks and the settings
//! each one applies.
//!
//! A mask is a list of components combined in order — painted brush
//! strokes, a linear or radial gradient, or a detected subject, sky or
//! background — and carries its own small set of development controls.
//! Like [`crate::RawSettings`] this is only the model: the development
//! pipeline in the app turns it into pixels. The rasterisation lives here
//! because it is pure geometry, shared by the preview, the final render,
//! action replay and the tests, and must give the same coverage in all of
//! them.
//!
//! Geometry is resolution independent. Points are fractions of the
//! developed image's width and height; lengths (a brush radius, a radial
//! gradient's radii) are fractions of its longer side, so a circle stays a
//! circle. A detected mask is a coverage raster at its own resolution,
//! sampled bilinearly. That raster is a cache of the detection rather than
//! part of the recipe: it is saved with the document, but not with a
//! recorded action, which detects again on each photo it is replayed on.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// At most this many masks on one development. Lightroom's practical
/// ceiling is lower; this only bounds hostile input.
pub const MAX_MASKS: usize = 32;
/// At most this many components in one mask.
pub const MAX_COMPONENTS: usize = 16;
/// At most this many strokes in one brush component.
pub const MAX_STROKES: usize = 1024;
/// At most this many points in one brush stroke.
pub const MAX_STROKE_POINTS: usize = 16384;
/// A detected mask is stored no larger than this on either side.
pub const MAX_RASTER_SIDE: u32 = 4096;

/// The controls a mask applies, in the Camera Raw dialog's units.
///
/// Exposure is in EV (-4..=4, as Lightroom's local exposure); everything
/// else is -100..=100. Sharpness below zero softens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub sharpness: f32,
}

/// Every local control's key and range, in the order the dialog lists them.
pub const LOCAL_CONTROLS: [(&str, f32, f32); 10] = [
    ("temperature", -100.0, 100.0),
    ("tint", -100.0, 100.0),
    ("exposure", -4.0, 4.0),
    ("contrast", -100.0, 100.0),
    ("highlights", -100.0, 100.0),
    ("shadows", -100.0, 100.0),
    ("clarity", -100.0, 100.0),
    ("dehaze", -100.0, 100.0),
    ("saturation", -100.0, 100.0),
    ("sharpness", -100.0, 100.0),
];

impl LocalAdjustments {
    pub fn get(&self, key: &str) -> f32 {
        match key {
            "exposure" => self.exposure,
            "contrast" => self.contrast,
            "highlights" => self.highlights,
            "shadows" => self.shadows,
            "temperature" => self.temperature,
            "tint" => self.tint,
            "saturation" => self.saturation,
            "clarity" => self.clarity,
            "dehaze" => self.dehaze,
            "sharpness" => self.sharpness,
            _ => 0.0,
        }
    }

    /// Set one control, clamped to its range. Unknown keys and non-finite
    /// values are ignored.
    pub fn set(&mut self, key: &str, value: f32) {
        let Some((_, min, max)) = LOCAL_CONTROLS.iter().find(|(k, ..)| *k == key) else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(*min, *max);
        match key {
            "exposure" => self.exposure = value,
            "contrast" => self.contrast = value,
            "highlights" => self.highlights = value,
            "shadows" => self.shadows = value,
            "temperature" => self.temperature = value,
            "tint" => self.tint = value,
            "saturation" => self.saturation = value,
            "clarity" => self.clarity = value,
            "dehaze" => self.dehaze = value,
            _ => self.sharpness = value,
        }
    }

    pub fn sanitized(self) -> Self {
        let mut out = Self::default();
        for (key, ..) in LOCAL_CONTROLS {
            out.set(key, self.get(key));
        }
        out
    }

    pub fn is_valid(&self) -> bool {
        LOCAL_CONTROLS.iter().all(|(key, min, max)| {
            let v = self.get(key);
            v.is_finite() && (*min..=*max).contains(&v)
        })
    }

    /// Whether applying these would change nothing.
    pub fn is_neutral(&self) -> bool {
        LOCAL_CONTROLS.iter().all(|(key, ..)| self.get(key) == 0.0)
    }
}

/// How a component meets the coverage built up by the ones before it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskCombine {
    #[default]
    Add,
    Subtract,
    Intersect,
}

/// What a detected mask was asked to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectedKind {
    Subject,
    Sky,
    Background,
}

/// One brush stroke: a polyline of dabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushStroke {
    /// Erasing takes coverage away from the strokes before this one.
    #[serde(default)]
    pub erase: bool,
    /// Radius as a fraction of the image's longer side.
    pub size: f32,
    /// The soft part of the dab, as a percentage of the radius.
    pub feather: f32,
    /// How much each dab adds, as a percentage; overlapping dabs build up.
    pub flow: f32,
    pub points: Vec<[f32; 2]>,
}

/// Coverage found by a detector, one byte a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskRaster {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl MaskRaster {
    /// A raster from coverage in 0..=1. `None` when the size is out of
    /// bounds or disagrees with the data.
    pub fn from_coverage(width: u32, height: u32, coverage: &[f32]) -> Option<Self> {
        let raster = MaskRaster {
            width,
            height,
            data: coverage
                .iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                .collect(),
        };
        raster.is_valid().then_some(raster)
    }

    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_RASTER_SIDE
            && self.height <= MAX_RASTER_SIDE
            && self.data.len() == self.width as usize * self.height as usize
    }

    /// Bilinear sample at a position in raster pixels.
    fn sample(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (self.width as usize, self.height as usize);
        let x = x.clamp(0.0, (w - 1) as f32);
        let y = y.clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.data[y * w + x] as f32 / 255.0;
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

/// The shape of one mask component.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaskShape {
    Brush {
        #[serde(default)]
        strokes: Vec<BrushStroke>,
    },
    /// Full effect up to `start`, fading to none at `end`.
    Linear { start: [f32; 2], end: [f32; 2] },
    /// Full effect inside the inner ellipse, fading to none at the outer.
    Radial {
        center: [f32; 2],
        /// Radii along the rotated axes, fractions of the longer side.
        radius: [f32; 2],
        /// Rotation of the first axis, in degrees clockwise.
        #[serde(default)]
        angle: f32,
        /// The fading band, as a percentage of the radius.
        feather: f32,
    },
    Detected {
        kind: DetectedKind,
        /// The detection, or `None` until it has run on this image.
        #[serde(skip)]
        raster: Option<Arc<MaskRaster>>,
    },
}

impl PartialEq for MaskShape {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Brush { strokes: a }, Self::Brush { strokes: b }) => a == b,
            (Self::Linear { start: a, end: b }, Self::Linear { start: c, end: d }) => {
                a == c && b == d
            }
            (
                Self::Radial {
                    center: a,
                    radius: b,
                    angle: c,
                    feather: d,
                },
                Self::Radial {
                    center: e,
                    radius: f,
                    angle: g,
                    feather: h,
                },
            ) => a == e && b == f && c == g && d == h,
            (Self::Detected { kind: a, raster: b }, Self::Detected { kind: c, raster: d }) => {
                a == c
                    && match (b, d) {
                        (Some(b), Some(d)) => Arc::ptr_eq(b, d) || b == d,
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

impl MaskShape {
    /// A gradient from a third of the way down to two thirds, which is
    /// where a graduated filter usually goes: over a sky.
    pub fn default_linear() -> Self {
        MaskShape::Linear {
            start: [0.5, 0.3],
            end: [0.5, 0.6],
        }
    }

    pub fn default_radial() -> Self {
        MaskShape::Radial {
            center: [0.5, 0.5],
            radius: [0.25, 0.18],
            angle: 0.0,
            feather: 50.0,
        }
    }
}

/// One shape and how it combines with the components before it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskComponent {
    pub shape: MaskShape,
    #[serde(default)]
    pub combine: MaskCombine,
    #[serde(default)]
    pub invert: bool,
}

impl MaskComponent {
    pub fn new(shape: MaskShape) -> Self {
        MaskComponent {
            shape,
            combine: MaskCombine::Add,
            invert: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// A mask and the local adjustments it applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMask {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub invert: bool,
    pub components: Vec<MaskComponent>,
    #[serde(default)]
    pub adjustments: LocalAdjustments,
}

impl LocalMask {
    pub fn new(shape: MaskShape) -> Self {
        LocalMask {
            enabled: true,
            invert: false,
            components: vec![MaskComponent::new(shape)],
            adjustments: LocalAdjustments::default(),
        }
    }

    /// Whether rendering this mask would change the picture.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.components.is_empty() && !self.adjustments.is_neutral()
    }

    /// Whether any detected component still needs its detector run.
    pub fn needs_detection(&self) -> bool {
        self.components
            .iter()
            .any(|c| matches!(c.shape, MaskShape::Detected { raster: None, .. }))
    }

    /// Coverage in 0..=1 for every pixel of a `width` x `height` image,
    /// combining the components in order. A detected component that has
    /// not been detected yet covers nothing.
    pub fn coverage(&self, width: usize, height: usize) -> Vec<f32> {
        let mut acc = vec![0.0f32; width * height];
        for (i, component) in self.components.iter().enumerate() {
            let c = component_coverage(&component.shape, width, height);
            let combine = if i == 0 {
                // The first component has nothing to subtract from or
                // intersect with; Lightroom treats it as the base.
                MaskCombine::Add
            } else {
                component.combine
            };
            let invert = component.invert;
            acc.par_iter_mut().zip(c.par_iter()).for_each(|(a, &c)| {
                let c = if invert { 1.0 - c } else { c };
                *a = match combine {
                    MaskCombine::Add => a.max(c),
                    MaskCombine::Subtract => a.min(1.0 - c),
                    MaskCombine::Intersect => a.min(c),
                };
            });
        }
        if self.invert {
            acc.par_iter_mut().for_each(|a| *a = 1.0 - *a);
        }
        acc
    }

    /// Constrain everything to the ranges and limits the editor uses.
    /// `None` when nothing usable is left.
    pub fn sanitized(mut self) -> Option<Self> {
        self.adjustments = self.adjustments.sanitized();
        self.components.truncate(MAX_COMPONENTS);
        self.components = self
            .components
            .into_iter()
            .filter_map(|mut c| {
                c.shape = sanitize_shape(c.shape)?;
                Some(c)
            })
            .collect();
        (!self.components.is_empty()).then_some(self)
    }

    /// Strict check for input that must be rejected rather than repaired,
    /// such as a recorded action.
    pub fn is_valid(&self) -> bool {
        self.adjustments.is_valid()
            && !self.components.is_empty()
            && self.components.len() <= MAX_COMPONENTS
            && self
                .components
                .iter()
                .all(|c| sanitize_shape(c.shape.clone()).as_ref() == Some(&c.shape))
    }
}

/// Sanitize a whole list, as read from a file.
pub fn sanitize_masks(masks: Vec<LocalMask>) -> Vec<LocalMask> {
    masks
        .into_iter()
        .take(MAX_MASKS)
        .filter_map(LocalMask::sanitized)
        .collect()
}

pub fn masks_valid(masks: &[LocalMask]) -> bool {
    masks.len() <= MAX_MASKS && masks.iter().all(LocalMask::is_valid)
}

fn finite_point(p: [f32; 2]) -> Option<[f32; 2]> {
    // Points may lie off the image (a gradient's far end often does), but
    // not absurdly far.
    (p[0].is_finite() && p[1].is_finite()).then(|| [p[0].clamp(-4.0, 5.0), p[1].clamp(-4.0, 5.0)])
}

fn sanitize_shape(shape: MaskShape) -> Option<MaskShape> {
    Some(match shape {
        MaskShape::Brush { strokes } => MaskShape::Brush {
            strokes: strokes
                .into_iter()
                .take(MAX_STROKES)
                .filter_map(|s| {
                    if !(s.size.is_finite() && s.feather.is_finite() && s.flow.is_finite()) {
                        return None;
                    }
                    let points: Vec<_> = s
                        .points
                        .into_iter()
                        .take(MAX_STROKE_POINTS)
                        .map(finite_point)
                        .collect::<Option<_>>()?;
                    (!points.is_empty()).then_some(BrushStroke {
                        erase: s.erase,
                        size: s.size.clamp(0.0005, 0.5),
                        feather: s.feather.clamp(0.0, 100.0),
                        flow: s.flow.clamp(1.0, 100.0),
                        points,
                    })
                })
                .collect(),
        },
        MaskShape::Linear { start, end } => MaskShape::Linear {
            start: finite_point(start)?,
            end: finite_point(end)?,
        },
        MaskShape::Radial {
            center,
            radius,
            angle,
            feather,
        } => {
            if !(radius[0].is_finite()
                && radius[1].is_finite()
                && angle.is_finite()
                && feather.is_finite())
            {
                return None;
            }
            MaskShape::Radial {
                center: finite_point(center)?,
                radius: [radius[0].clamp(0.001, 4.0), radius[1].clamp(0.001, 4.0)],
                angle: angle.rem_euclid(360.0),
                feather: feather.clamp(0.0, 100.0),
            }
        }
        MaskShape::Detected { kind, raster } => MaskShape::Detected {
            kind,
            raster: raster.filter(|r| r.is_valid()),
        },
    })
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Coverage of a round, feathered dab at distance `d` from its centre,
/// `d` already divided by the radius.
fn falloff(d: f32, inner: f32) -> f32 {
    if d <= inner {
        1.0
    } else if d >= 1.0 {
        0.0
    } else {
        smoothstep((1.0 - d) / (1.0 - inner).max(1e-6))
    }
}

/// One component's coverage, before combining or inverting.
pub fn component_coverage(shape: &MaskShape, width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; width * height];
    if width == 0 || height == 0 {
        return out;
    }
    let (w, h) = (width as f32, height as f32);
    let long = w.max(h);
    match shape {
        MaskShape::Brush { strokes } => {
            for stroke in strokes {
                paint_stroke(&mut out, width, height, stroke, long);
            }
        }
        MaskShape::Linear { start, end } => {
            let s = (start[0] * w, start[1] * h);
            let d = (end[0] * w - s.0, end[1] * h - s.1);
            let len2 = d.0 * d.0 + d.1 * d.1;
            out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
                let py = y as f32 + 0.5 - s.1;
                for (x, v) in row.iter_mut().enumerate() {
                    let px = x as f32 + 0.5 - s.0;
                    let t = if len2 < 1e-6 {
                        // A gradient with no length is a hard edge.
                        if px * d.0 + py * d.1 <= 0.0 {
                            0.0
                        } else {
                            1.0
                        }
                    } else {
                        (px * d.0 + py * d.1) / len2
                    };
                    *v = 1.0 - smoothstep(t);
                }
            });
        }
        MaskShape::Radial {
            center,
            radius,
            angle,
            feather,
        } => {
            let c = (center[0] * w, center[1] * h);
            let (rx, ry) = ((radius[0] * long).max(0.5), (radius[1] * long).max(0.5));
            let (sin, cos) = angle.to_radians().sin_cos();
            let inner = 1.0 - feather.clamp(0.0, 100.0) / 100.0;
            out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
                let dy = y as f32 + 0.5 - c.1;
                for (x, v) in row.iter_mut().enumerate() {
                    let dx = x as f32 + 0.5 - c.0;
                    let u = dx * cos + dy * sin;
                    let t = -dx * sin + dy * cos;
                    *v = falloff((u / rx).hypot(t / ry), inner);
                }
            });
        }
        MaskShape::Detected { raster, .. } => {
            let Some(raster) = raster.as_deref().filter(|r| r.is_valid()) else {
                return out;
            };
            let sx = raster.width as f32 / w;
            let sy = raster.height as f32 / h;
            out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
                let ry = (y as f32 + 0.5) * sy - 0.5;
                for (x, v) in row.iter_mut().enumerate() {
                    *v = raster.sample((x as f32 + 0.5) * sx - 0.5, ry);
                }
            });
        }
    }
    out
}

/// Lay a stroke's dabs into `out`, a quarter of a radius apart.
fn paint_stroke(out: &mut [f32], width: usize, height: usize, stroke: &BrushStroke, long: f32) {
    let r = (stroke.size * long).max(0.5);
    let inner = 1.0 - stroke.feather.clamp(0.0, 100.0) / 100.0;
    let flow = stroke.flow.clamp(0.0, 100.0) / 100.0;
    let spacing = (r * 0.25).max(0.5);
    let to_px = |p: [f32; 2]| (p[0] * width as f32, p[1] * height as f32);
    let mut dab = |cx: f32, cy: f32| {
        let x0 = (cx - r).floor().max(0.0) as usize;
        let y0 = (cy - r).floor().max(0.0) as usize;
        let x1 = ((cx + r).ceil().max(0.0) as usize).min(width);
        let y1 = ((cy + r).ceil().max(0.0) as usize).min(height);
        for y in y0..y1 {
            let dy = y as f32 + 0.5 - cy;
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - cx;
                let k = flow * falloff(dx.hypot(dy) / r, inner);
                if k <= 0.0 {
                    continue;
                }
                let v = &mut out[y * width + x];
                *v = if stroke.erase {
                    *v * (1.0 - k)
                } else {
                    *v + (1.0 - *v) * k
                };
            }
        }
    };
    let mut points = stroke.points.iter().map(|p| to_px(*p));
    let Some(mut last) = points.next() else {
        return;
    };
    dab(last.0, last.1);
    // Distance carried over from the previous segment, so dabs stay
    // evenly spaced across the joins.
    let mut carry = 0.0f32;
    for p in points {
        let (dx, dy) = (p.0 - last.0, p.1 - last.1);
        let len = dx.hypot(dy);
        let mut at = spacing - carry;
        while at <= len {
            let t = at / len;
            dab(last.0 + dx * t, last.1 + dy * t);
            at += spacing;
        }
        carry = len - (at - spacing);
        last = p;
    }
}

/// The smallest rectangle, in pixels, holding every non-zero sample.
/// `None` when the coverage is empty.
pub fn coverage_bounds(coverage: &[f32], width: usize) -> Option<(usize, usize, usize, usize)> {
    if width == 0 {
        return None;
    }
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (y, row) in coverage.chunks(width).enumerate() {
        let Some(first) = row.iter().position(|v| *v > 0.0) else {
            continue;
        };
        let last = row.iter().rposition(|v| *v > 0.0).unwrap_or(first);
        bounds = Some(match bounds {
            None => (first, y, last + 1, y + 1),
            Some((l, t, r, _)) => (l.min(first), t, r.max(last + 1), y + 1),
        });
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(c: &[f32], w: usize, x: usize, y: usize) -> f32 {
        c[y * w + x]
    }

    #[test]
    fn linear_gradient_runs_from_full_to_none() {
        let shape = MaskShape::Linear {
            start: [0.0, 0.25],
            end: [0.0, 0.75],
        };
        let c = component_coverage(&shape, 8, 100);
        assert_eq!(at(&c, 8, 4, 10), 1.0, "before the start is full effect");
        assert_eq!(at(&c, 8, 4, 90), 0.0, "past the end is no effect");
        let middle = at(&c, 8, 4, 50);
        assert!((middle - 0.5).abs() < 0.05, "midway is half: {middle}");
        assert!(at(&c, 8, 4, 30) > at(&c, 8, 4, 60), "monotonic");
    }

    #[test]
    fn radial_gradient_is_full_inside_and_none_outside() {
        let shape = MaskShape::Radial {
            center: [0.5, 0.5],
            radius: [0.25, 0.25],
            angle: 0.0,
            feather: 50.0,
        };
        let c = component_coverage(&shape, 100, 100);
        assert_eq!(at(&c, 100, 50, 50), 1.0);
        assert_eq!(at(&c, 100, 2, 2), 0.0);
        let edge = at(&c, 100, 50 + 19, 50);
        assert!(edge > 0.0 && edge < 1.0, "the feather band is soft: {edge}");
    }

    #[test]
    fn radial_rotation_turns_the_long_axis() {
        let shape = |angle| MaskShape::Radial {
            center: [0.5, 0.5],
            radius: [0.4, 0.1],
            angle,
            feather: 0.0,
        };
        let flat = component_coverage(&shape(0.0), 100, 100);
        let upright = component_coverage(&shape(90.0), 100, 100);
        assert_eq!(at(&flat, 100, 85, 50), 1.0);
        assert_eq!(at(&flat, 100, 50, 85), 0.0);
        assert_eq!(at(&upright, 100, 50, 85), 1.0);
        assert_eq!(at(&upright, 100, 85, 50), 0.0);
    }

    #[test]
    fn brush_paints_builds_up_and_erases() {
        let stroke = |erase, flow| BrushStroke {
            erase,
            size: 0.1,
            feather: 0.0,
            flow,
            points: vec![[0.2, 0.5], [0.8, 0.5]],
        };
        let painted = MaskShape::Brush {
            strokes: vec![stroke(false, 100.0)],
        };
        let c = component_coverage(&painted, 100, 100);
        assert_eq!(at(&c, 100, 50, 50), 1.0);
        assert_eq!(at(&c, 100, 50, 10), 0.0);

        let light = MaskShape::Brush {
            strokes: vec![stroke(false, 20.0)],
        };
        let once = at(&component_coverage(&light, 100, 100), 100, 50, 50);
        assert!(
            once > 0.2 && once < 1.0,
            "overlapping dabs build up: {once}"
        );

        let erased = MaskShape::Brush {
            strokes: vec![stroke(false, 100.0), stroke(true, 100.0)],
        };
        assert_eq!(at(&component_coverage(&erased, 100, 100), 100, 50, 50), 0.0);
    }

    #[test]
    fn components_add_subtract_intersect_and_invert() {
        let left = MaskShape::Linear {
            start: [0.6, 0.0],
            end: [0.6001, 0.0],
        };
        let disc = MaskShape::Radial {
            center: [0.5, 0.5],
            radius: [0.2, 0.2],
            angle: 0.0,
            feather: 0.0,
        };
        let mut mask = LocalMask::new(left.clone());
        let left_only = mask.coverage(100, 10);
        assert_eq!(at(&left_only, 100, 10, 5), 1.0);
        assert_eq!(at(&left_only, 100, 90, 5), 0.0);

        mask.components.push(MaskComponent {
            shape: disc.clone(),
            combine: MaskCombine::Subtract,
            invert: false,
        });
        let cut = mask.coverage(100, 100);
        assert_eq!(at(&cut, 100, 50, 50), 0.0, "subtracted");
        assert_eq!(at(&cut, 100, 10, 50), 1.0, "kept outside the disc");

        mask.components[1].combine = MaskCombine::Intersect;
        let both = mask.coverage(100, 100);
        assert_eq!(at(&both, 100, 50, 50), 1.0);
        assert_eq!(at(&both, 100, 10, 50), 0.0);

        mask.components[1].invert = true;
        let outside = mask.coverage(100, 100);
        assert_eq!(at(&outside, 100, 50, 50), 0.0);
        assert_eq!(at(&outside, 100, 10, 50), 1.0);

        mask.invert = true;
        let inverted = mask.coverage(100, 100);
        assert_eq!(at(&inverted, 100, 10, 50), 0.0);
    }

    #[test]
    fn detected_raster_is_sampled_at_any_size() {
        let raster = MaskRaster::from_coverage(2, 1, &[1.0, 0.0]).unwrap();
        let shape = MaskShape::Detected {
            kind: DetectedKind::Subject,
            raster: Some(Arc::new(raster)),
        };
        let c = component_coverage(&shape, 40, 4);
        assert_eq!(at(&c, 40, 0, 2), 1.0);
        assert_eq!(at(&c, 40, 39, 2), 0.0);
        let undetected = MaskShape::Detected {
            kind: DetectedKind::Sky,
            raster: None,
        };
        assert!(component_coverage(&undetected, 4, 4)
            .iter()
            .all(|v| *v == 0.0));
        assert!(MaskRaster::from_coverage(3, 3, &[0.0; 4]).is_none());
    }

    #[test]
    fn json_omits_the_detection_and_keeps_the_recipe() {
        let mut mask = LocalMask::new(MaskShape::Detected {
            kind: DetectedKind::Sky,
            raster: Some(Arc::new(MaskRaster::from_coverage(1, 1, &[1.0]).unwrap())),
        });
        mask.adjustments.exposure = -0.5;
        mask.components.push(MaskComponent {
            shape: MaskShape::default_radial(),
            combine: MaskCombine::Intersect,
            invert: true,
        });
        let json = serde_json::to_string(&mask).unwrap();
        assert!(!json.contains("raster"), "{json}");
        let back: LocalMask = serde_json::from_str(&json).unwrap();
        assert!(back.needs_detection());
        assert_eq!(back.components[1], mask.components[1]);
        assert_eq!(back.adjustments, mask.adjustments);
    }

    #[test]
    fn hostile_values_are_repaired_or_rejected() {
        let mut mask = LocalMask::new(MaskShape::Radial {
            center: [f32::NAN, 0.5],
            radius: [0.2, 0.2],
            angle: 0.0,
            feather: 10.0,
        });
        mask.adjustments.exposure = 99.0;
        assert!(!mask.is_valid());
        assert!(mask.clone().sanitized().is_none(), "no usable component");
        mask.components.push(MaskComponent::new(MaskShape::Brush {
            strokes: vec![BrushStroke {
                erase: false,
                size: 9.0,
                feather: 500.0,
                flow: -1.0,
                points: vec![[0.5, 0.5]],
            }],
        }));
        let repaired = mask.sanitized().unwrap();
        assert_eq!(repaired.components.len(), 1);
        assert_eq!(repaired.adjustments.exposure, 4.0);
        assert!(repaired.is_valid());
        let MaskShape::Brush { strokes } = &repaired.components[0].shape else {
            unreachable!()
        };
        assert_eq!(strokes[0].size, 0.5);
        assert_eq!(strokes[0].feather, 100.0);
        assert_eq!(strokes[0].flow, 1.0);
    }

    #[test]
    fn bounds_cover_exactly_the_painted_pixels() {
        let mut c = vec![0.0; 10 * 10];
        c[3 * 10 + 4] = 0.5;
        c[6 * 10 + 2] = 1.0;
        assert_eq!(coverage_bounds(&c, 10), Some((2, 3, 5, 7)));
        assert_eq!(coverage_bounds(&[0.0; 4], 2), None);
    }
}
