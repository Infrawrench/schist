//! Toolkit-independent, parameter-backed controls in filter-buffer coordinates.
//!
//! Hosts map these points through the layer placement and viewport transforms.
//! Every gesture updates the same `FilterValues` used by numeric controls.
use crate::{FilterParam, FilterValues};

#[derive(Debug, Clone, Copy)]
pub enum RadiusScale {
    ShortSide,
    LongSide,
    Diagonal,
}
impl RadiusScale {
    fn pixels(self, size: (f32, f32)) -> f32 {
        match self {
            Self::ShortSide => size.0.min(size.1),
            Self::LongSide => size.0.max(size.1),
            Self::Diagonal => size.0.hypot(size.1),
        }
    }
}

/// All position/size parameters are percentages; angles are in degrees.
#[derive(Debug, Clone, Copy)]
pub enum FilterCanvasControl {
    Point {
        x: &'static str,
        y: &'static str,
    },
    Ellipse {
        x: &'static str,
        y: &'static str,
        radius: &'static str,
        scale: RadiusScale,
        /// Interpolates a circle toward the buffer's aspect ratio.
        roundness: Option<&'static str>,
        /// Inward feather, expressed as a percentage of the radius.
        feather: Option<&'static str>,
    },
    Band {
        position: &'static str,
        angle: &'static str,
        /// Angle of the band's normal relative to the angle parameter.
        normal_offset: f32,
        /// Full sharp-band width; None means a sharp line.
        width: Option<&'static str>,
        /// Outward feather, expressed as a percentage of projected extent.
        feather: &'static str,
    },
    Direction {
        angle: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct CanvasHandle {
    pub point: (f32, f32),
    /// Parameter key: hosts obtain its translated label from FilterParam.
    pub key: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct CanvasGeometry {
    pub handles: Vec<CanvasHandle>,
    pub guides: Vec<Vec<(f32, f32)>>,
}

fn add(a: (f32, f32), b: (f32, f32), scale: f32) -> (f32, f32) {
    (a.0 + b.0 * scale, a.1 + b.1 * scale)
}
fn dot(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1
}
fn center(v: &FilterValues, x: &str, y: &str, size: (f32, f32)) -> (f32, f32) {
    (v.get(x) * size.0 / 100.0, v.get(y) * size.1 / 100.0)
}
fn stretch(v: &FilterValues, roundness: Option<&str>, size: (f32, f32)) -> (f32, f32) {
    let round = roundness.map(|k| v.get(k) / 100.0).unwrap_or(1.0);
    (
        1.0 + (1.0 - round) * (size.0 / size.1 - 1.0).max(0.0),
        1.0 + (1.0 - round) * (size.1 / size.0 - 1.0).max(0.0),
    )
}
fn band(
    v: &FilterValues,
    size: (f32, f32),
    position: &str,
    angle: &str,
    offset: f32,
) -> ((f32, f32), (f32, f32), f32) {
    let a = (v.get(angle) + offset).to_radians();
    let normal = (a.cos(), a.sin());
    let extent = (size.0 * normal.0).abs() + (size.1 * normal.1).abs();
    let middle = (size.0 / 2.0, size.1 / 2.0);
    (
        add(
            middle,
            normal,
            extent * v.get(position) / 100.0 - dot(middle, normal),
        ),
        normal,
        extent,
    )
}

impl FilterCanvasControl {
    pub fn geometry(self, v: &FilterValues, size: (f32, f32)) -> CanvasGeometry {
        let mut g = CanvasGeometry::default();
        if !size.0.is_finite() || !size.1.is_finite() || size.0 <= 0.0 || size.1 <= 0.0 {
            return g;
        }
        let mut handle = |point, key| g.handles.push(CanvasHandle { point, key });
        match self {
            Self::Point { x, y } => handle(center(v, x, y, size), x),
            Self::Ellipse {
                x,
                y,
                radius,
                scale,
                roundness,
                feather,
            } => {
                let c = center(v, x, y, size);
                let r = v.get(radius) / 100.0 * scale.pixels(size);
                let s = stretch(v, roundness, size);
                handle(c, x);
                handle((c.0 + r * s.0, c.1), radius);
                let mut rings = vec![1.0];
                if let Some(feather) = feather {
                    let inner = 1.0 - v.get(feather) / 100.0;
                    handle((c.0, c.1 - r * s.1 * inner), feather);
                    rings.push(inner);
                }
                // A square's roundness parameter has no visual effect.
                if let Some(roundness) = roundness.filter(|_| (size.0 - size.1).abs() > 1e-6) {
                    handle(
                        if size.0 > size.1 {
                            (c.0 - r * s.0, c.1)
                        } else {
                            (c.0, c.1 + r * s.1)
                        },
                        roundness,
                    );
                }
                for ring in rings {
                    g.guides.push(
                        (0..=96)
                            .map(|i| {
                                let a = i as f32 * std::f32::consts::TAU / 96.0;
                                (
                                    c.0 + a.cos() * r * s.0 * ring,
                                    c.1 + a.sin() * r * s.1 * ring,
                                )
                            })
                            .collect(),
                    );
                }
            }
            Self::Band {
                position,
                angle,
                normal_offset,
                width,
                feather,
            } => {
                let (c, n, extent) = band(v, size, position, angle, normal_offset);
                let tangent = (n.1, -n.0);
                let half = width.map(|k| v.get(k) * extent / 200.0).unwrap_or(0.0);
                let fade = v.get(feather) * extent / 100.0;
                handle(c, position);
                handle(add(c, tangent, size.0.min(size.1) / 4.0), angle);
                if let Some(width) = width {
                    handle(add(c, n, half), width);
                }
                handle(add(c, n, half + fade), feather);
                for distance in [0.0, half, -half, half + fade, -half - fade] {
                    let edge = add(c, n, distance);
                    g.guides.push(vec![
                        add(edge, tangent, -size.0.hypot(size.1)),
                        add(edge, tangent, size.0.hypot(size.1)),
                    ]);
                }
            }
            Self::Direction { angle } => {
                let c = (size.0 / 2.0, size.1 / 2.0);
                let a = v.get(angle).to_radians();
                let p = add(c, (a.cos(), a.sin()), size.0.min(size.1) / 4.0);
                handle(p, angle);
                g.guides.push(vec![c, p]);
            }
        }
        g
    }

    /// Move a handle to a filter-buffer point, constraining every value exactly
    /// as numeric sliders do. Invalid points and missing parameters are ignored.
    pub fn move_handle(
        self,
        index: usize,
        point: (f32, f32),
        size: (f32, f32),
        values: &mut FilterValues,
        specs: &[FilterParam],
    ) -> bool {
        if !point.0.is_finite() || !point.1.is_finite() || size.0 <= 0.0 || size.1 <= 0.0 {
            return false;
        }
        let previous = values.clone();
        let v = &previous;
        let mut set = |key, value: f32| {
            if let Some(spec) = specs.iter().find(|p| p.key == key) {
                if value.is_finite() {
                    let value = value.clamp(spec.min, spec.max);
                    values.set(
                        key,
                        if spec.choices.is_empty() {
                            value
                        } else {
                            value.round()
                        },
                    );
                }
            }
        };
        match self {
            Self::Point { x, y } if index == 0 => {
                set(x, point.0 / size.0 * 100.0);
                set(y, point.1 / size.1 * 100.0);
            }
            Self::Ellipse {
                x,
                y,
                radius,
                scale,
                roundness,
                feather,
            } => {
                let c = center(v, x, y, size);
                let delta = (point.0 - c.0, point.1 - c.1);
                let s = stretch(v, roundness, size);
                match index {
                    0 => {
                        set(x, point.0 / size.0 * 100.0);
                        set(y, point.1 / size.1 * 100.0);
                    }
                    1 => set(
                        radius,
                        (delta.0 / s.0).hypot(delta.1 / s.1) / scale.pixels(size) * 100.0,
                    ),
                    2 if feather.is_some() => {
                        if let Some(feather) = feather {
                            let r = v.get(radius) / 100.0 * scale.pixels(size);
                            set(
                                feather,
                                (1.0 - (delta.0 / s.0).hypot(delta.1 / s.1) / r.max(1e-6)) * 100.0,
                            );
                        }
                    }
                    i if i == 2 + usize::from(feather.is_some()) => {
                        if let Some(roundness) = roundness {
                            let aspect = size.0.max(size.1) / size.0.min(size.1);
                            let r = v.get(radius) / 100.0 * scale.pixels(size);
                            let along = if size.0 > size.1 {
                                delta.0.abs()
                            } else {
                                delta.1.abs()
                            };
                            if aspect > 1.0 && r > 0.0 {
                                set(
                                    roundness,
                                    (1.0 - (along / r - 1.0) / (aspect - 1.0)) * 100.0,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            Self::Band {
                position,
                angle,
                normal_offset,
                width,
                feather,
            } => {
                let (c, n, extent) = band(v, size, position, angle, normal_offset);
                let delta = (point.0 - c.0, point.1 - c.1);
                match index {
                    0 => set(position, dot(point, n) / extent * 100.0),
                    1 => set(
                        angle,
                        (delta.1.atan2(delta.0).to_degrees() - normal_offset + 90.0)
                            .rem_euclid(360.0),
                    ),
                    2 if width.is_some() => {
                        set(width.unwrap(), dot(delta, n).abs() / extent * 200.0)
                    }
                    i if i == 2 + usize::from(width.is_some()) => {
                        let half = width.map(|k| v.get(k) / 2.0).unwrap_or(0.0);
                        set(feather, dot(delta, n).abs() / extent * 100.0 - half);
                    }
                    _ => {}
                }
            }
            Self::Direction { angle } if index == 0 => set(
                angle,
                ((point.1 - size.1 / 2.0)
                    .atan2(point.0 - size.0 / 2.0)
                    .to_degrees())
                .rem_euclid(360.0),
            ),
            _ => {}
        }
        *values != previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn specs(keys: &[&'static str]) -> Vec<FilterParam> {
        keys.iter()
            .map(|&key| FilterParam {
                key,
                label: key,
                min: 0.0,
                max: 100.0,
                default: 50.0,
                suffix: "%",
                choices: &[],
            })
            .collect()
    }
    #[test]
    fn point_clamps_and_rejects_non_finite_input() {
        let c = FilterCanvasControl::Point { x: "x", y: "y" };
        let s = specs(&["x", "y"]);
        let mut v = FilterValues::defaults(&s);
        assert!(c.move_handle(0, (-10.0, 300.0), (200.0, 100.0), &mut v, &s));
        assert_eq!((v.get("x"), v.get("y")), (0.0, 100.0));
        assert!(!c.move_handle(0, (f32::NAN, 10.0), (200.0, 100.0), &mut v, &s));
        assert!(c.geometry(&v, (0.0, 10.0)).handles.is_empty());
    }
    #[test]
    fn tilt_shift_width_and_feather_follow_the_rendered_band() {
        let c = FilterCanvasControl::Band {
            position: "position",
            angle: "angle",
            normal_offset: 90.0,
            width: Some("band"),
            feather: "feather",
        };
        let s = specs(&["position", "angle", "band", "feather"]);
        let mut v = FilterValues::defaults(&s);
        v.set("angle", 0.0);
        v.set("band", 20.0);
        v.set("feather", 30.0);
        let g = c.geometry(&v, (200.0, 100.0));
        assert!((g.handles[0].point.1 - 50.0).abs() < 0.0001);
        assert!((g.handles[2].point.1 - 60.0).abs() < 0.0001);
        assert!((g.handles[3].point.1 - 90.0).abs() < 0.0001);
        c.move_handle(2, (100.0, 70.0), (200.0, 100.0), &mut v, &s);
        assert!((v.get("band") - 40.0).abs() < 0.0001);
        c.move_handle(3, (100.0, 80.0), (200.0, 100.0), &mut v, &s);
        assert!((v.get("feather") - 10.0).abs() < 0.0001);
    }
    #[test]
    fn iris_ellipse_tracks_aspect_ratio_and_inward_feather() {
        let c = FilterCanvasControl::Ellipse {
            x: "x",
            y: "y",
            radius: "radius",
            scale: RadiusScale::ShortSide,
            roundness: Some("roundness"),
            feather: Some("feather"),
        };
        let s = specs(&["x", "y", "radius", "roundness", "feather"]);
        let mut v = FilterValues::defaults(&s);
        v.set("roundness", 0.0);
        let g = c.geometry(&v, (200.0, 100.0));
        assert_eq!(g.handles[1].point, (200.0, 50.0));
        assert_eq!(g.handles[2].point, (100.0, 25.0));
        c.move_handle(1, (150.0, 50.0), (200.0, 100.0), &mut v, &s);
        assert_eq!(v.get("radius"), 25.0);
        c.move_handle(3, (62.5, 50.0), (200.0, 100.0), &mut v, &s);
        assert_eq!(v.get("roundness"), 50.0);
        assert_eq!(c.geometry(&v, (100.0, 100.0)).handles.len(), 3);
    }
}
