//! Stripes use percentages; dash/gap lengths and dot spacing use points.
use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DecorationCap {
    #[default]
    Butt,
    Round,
    Projecting,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "DashDefinition")]
pub struct DecorationDashes {
    pub lengths: Vec<f32>,
    pub cap: DecorationCap,
}

// Accept the earlier array-only representation as butt-ended dashes.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum DashDefinition {
    Lengths(Vec<f32>),
    Settings {
        lengths: Vec<f32>,
        #[serde(default)]
        cap: DecorationCap,
    },
}
impl From<DashDefinition> for DecorationDashes {
    fn from(value: DashDefinition) -> Self {
        match value {
            DashDefinition::Lengths(lengths) => lengths.into(),
            DashDefinition::Settings { lengths, cap } => Self { lengths, cap },
        }
    }
}
impl From<Vec<f32>> for DecorationDashes {
    fn from(lengths: Vec<f32>) -> Self {
        Self {
            lengths,
            cap: DecorationCap::Butt,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TextDecorationPattern {
    #[default]
    Solid,
    /// Strictly increasing start/end pairs, each between zero and one hundred.
    Stripes(Vec<f32>),
    /// Alternating dash/gap lengths, at most five pairs. TextDecoration selects fitting.
    Dashes(DecorationDashes),
    /// At most five center-to-center point intervals; dot diameter is line weight.
    Dots(Vec<f32>),
}

impl TextDecorationPattern {
    pub fn valid(&self) -> bool {
        match self {
            Self::Solid => true,
            Self::Dashes(DecorationDashes { lengths, .. }) => {
                !lengths.is_empty()
                    && lengths.len() <= 10
                    && lengths.len().is_multiple_of(2)
                    && lengths.iter().all(|v| v.is_finite() && *v >= 0.0)
                    && lengths.iter().map(|v| f64::from(*v)).sum::<f64>() > 0.0
            }
            Self::Dots(intervals) => {
                !intervals.is_empty()
                    && intervals.len() <= 5
                    && intervals.iter().all(|v| v.is_finite() && *v >= 0.0)
                    && intervals.iter().map(|v| f64::from(*v)).sum::<f64>() > 0.0
            }
            Self::Stripes(edges) => {
                !edges.is_empty()
                    && edges.len().is_multiple_of(2)
                    && edges
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=100.0).contains(v))
                    && edges.windows(2).all(|pair| pair[0] < pair[1])
            }
        }
    }

    pub fn cap_extension(&self, weight: f32) -> f32 {
        match self {
            Self::Dashes(dashes) if dashes.cap != DecorationCap::Butt => weight / 2.0,
            Self::Dots(_) => weight / 2.0,
            _ => 0.0,
        }
    }

    /// Point lengths scale with resolution; percentage positions do not.
    pub fn scaled(&mut self, scale: f32) {
        let lengths = match self {
            Self::Dashes(dashes) => &mut dashes.lengths,
            Self::Dots(intervals) => intervals,
            _ => return,
        };
        for length in lengths {
            *length *= scale;
        }
    }

    #[cfg(test)]
    pub(super) fn masks(
        &self,
        rect: IntRect,
        geometry: [f32; 4],
        vertical: bool,
        phase: [f32; 2],
    ) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        self.fitted_masks(rect, geometry, vertical, phase, DecorationFit::None)
    }

    /// Exact rectangular area; circular caps use bounded adaptive integration.
    /// Adjacent inks partition coverage before independent quantization.
    pub(super) fn fitted_masks(
        &self,
        rect: IntRect,
        geometry: [f32; 4],
        vertical: bool,
        phase: [f32; 2],
        fitting: DecorationFit,
    ) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        if matches!(self, Self::Solid) || !self.valid() {
            return None;
        }
        if let Self::Dots(intervals) = self {
            // The layout/codec rejects dash-only fitting for dots. Keep malformed
            // direct TextSpecs circular too, rather than growing a dot into a dash.
            let fitting = if fitting == DecorationFit::Dashes {
                DecorationFit::None
            } else {
                fitting
            };
            // The model retains dot spacing as native DotArray values. Rendering
            // uses zero-length round dashes, whose unions also handle overlap.
            return Self::Dashes(DecorationDashes {
                lengths: intervals.iter().flat_map(|gap| [0.0, *gap]).collect(),
                cap: DecorationCap::Round,
            })
            .fitted_masks(rect, geometry, vertical, phase, fitting);
        }
        let lengths = match self {
            Self::Dashes(dashes) => super::decoration_fitting::resolve(
                &dashes.lengths,
                f64::from(phase[1] - phase[0]),
                fitting,
            ),
            _ => Vec::new(),
        };
        let [x, y, width, height] = geometry;
        let (cross, weight, inline, length) = if vertical {
            (x, width, y, height)
        } else {
            (y, height, x, width)
        };
        let mut out = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
        let mut gaps = Vec::with_capacity(out.capacity());
        let mut combined = Vec::with_capacity(out.capacity());
        let extension = self.cap_extension(weight);
        let piece_start = inline - if inline == phase[0] { extension } else { 0.0 };
        let piece_end = inline
            + length
            + if inline + length == phase[1] {
                extension
            } else {
                0.0
            };
        for row in rect.top..rect.bottom {
            for col in rect.left..rect.right {
                let (across, along_pixel) = if vertical { (col, row) } else { (row, col) };
                let start = (along_pixel as f32).max(inline);
                let end = (along_pixel as f32 + 1.0).min(inline + length);
                let along = (end - start).max(0.0);
                let whole = ((across as f32 + 1.0).min(cross + weight)
                    - (across as f32).max(cross))
                .max(0.0);
                let area = match self {
                    Self::Stripes(edges) => {
                        along
                            * edges
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|pair| {
                                    let a = cross + weight * pair[0] / 100.0;
                                    let b = cross + weight * pair[1] / 100.0;
                                    ((across as f32 + 1.0).min(b) - (across as f32).max(a)).max(0.0)
                                })
                                .sum::<f32>()
                    }
                    Self::Dashes(_) => whole * dash_area(&lengths, start - phase[0], along),
                    Self::Solid | Self::Dots(_) => unreachable!(),
                };
                let (area, inside) = match self {
                    Self::Dashes(dashes) if dashes.cap != DecorationCap::Butt => {
                        let start = f64::from((along_pixel as f32).max(piece_start) - phase[0]);
                        let end = f64::from((along_pixel as f32 + 1.0).min(piece_end) - phase[0]);
                        let span = f64::from(phase[1] - phase[0]);
                        let cross_center = f64::from(cross) + f64::from(weight) / 2.0;
                        let cross_range = [
                            f64::from(across) - cross_center,
                            f64::from(across + 1) - cross_center,
                        ];
                        let coverage = |range| {
                            super::decoration_dashes::area(
                                &lengths,
                                dashes.cap,
                                f64::from(weight) / 2.0,
                                (span, fitting != DecorationFit::None),
                                range,
                                cross_range,
                            ) as f32
                        };
                        let area = coverage([start, end]);
                        let inside = if start >= 0.0 && end <= span {
                            area
                        } else {
                            coverage([start.max(0.0), end.min(span)])
                        };
                        (area, inside)
                    }
                    _ => (area, area),
                };
                out.push((area * 255.0).round().clamp(0.0, 255.0) as u8);
                combined.push(
                    ((along * whole + (area - inside)) * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8,
                );
                // Quantize each ink once. Subtracting an already quantized
                // stripe biases half-coverage gap edges down by one level.
                gaps.push(
                    ((along * whole - inside).max(0.0) * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8,
                );
            }
        }
        Some((out, gaps, combined))
    }
}

/// Integrate a periodic rectangular wave without enumerating repetitions.
/// Tiny but valid point lengths must neither hang nor overflow a pixel loop.
fn dash_area(lengths: &[f64], start: f32, length: f32) -> f32 {
    let period = lengths.iter().copied().sum::<f64>();
    let ink = lengths.iter().step_by(2).copied().sum::<f64>();
    let length = f64::from(length);
    let cycles = (length / period).floor();
    let remainder = length.rem_euclid(period);
    let phase = f64::from(start).rem_euclid(period);
    let partial = |a: f64, b: f64| {
        let mut edge = 0.0;
        let mut area = 0.0;
        for pair in lengths.as_chunks::<2>().0 {
            let end = edge + pair[0];
            area += (end.min(b) - edge.max(a)).max(0.0);
            edge = end + pair[1];
        }
        area
    };
    let residual = partial(phase, (phase + remainder).min(period))
        + partial(0.0, (phase + remainder - period).max(0.0));
    (cycles * ink + residual).clamp(0.0, length) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitted_dots_keep_the_terminal_circle_when_unequal_intervals_have_inexact_quotients() {
        for intervals in [vec![5.0, 7.0], vec![1.5, 3.25, 2.0, 0.75, 6.0]] {
            for fitting in [DecorationFit::Gaps, DecorationFit::DashesAndGaps] {
                for span in 1..193 {
                    let pattern = TextDecorationPattern::Dots(intervals.clone());
                    let rect = IntRect::new(-1, -1, span + 1, 1);
                    let (ink, _, _) = pattern
                        .fitted_masks(
                            rect,
                            [0.0, -0.75, span as f32, 1.5],
                            false,
                            [0.0, span as f32],
                            fitting,
                        )
                        .unwrap();
                    let width = rect.width() as usize;
                    for row in 0..rect.height() as usize {
                        let first = ink[row * width];
                        let last = ink[row * width + width - 1];
                        assert!(first > 0);
                        assert_eq!(
                            first, last,
                            "{intervals:?},span={span},{fitting:?},row={row}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fitted_lines_cover_endpoints_symmetrically_and_transpose_without_resolving_per_piece() {
        for fitting in [
            DecorationFit::Dashes,
            DecorationFit::Gaps,
            DecorationFit::DashesAndGaps,
        ] {
            for pattern in [
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![6.0, 3.0],
                    cap: DecorationCap::Butt,
                }),
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![6.0, 3.0],
                    cap: DecorationCap::Round,
                }),
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![6.0, 3.0],
                    cap: DecorationCap::Projecting,
                }),
                TextDecorationPattern::Dots(vec![6.0]),
            ] {
                if fitting == DecorationFit::Dashes
                    && matches!(pattern, TextDecorationPattern::Dots(_))
                {
                    continue;
                }
                for span in [1.0_f32, 7.0, 13.0, 25.0, 49.0] {
                    for weight in [0.75_f32, 1.5, 4.0] {
                        let padding = pattern.cap_extension(weight).ceil() as i32;
                        let radius = weight / 2.0;
                        let rect = IntRect::new(
                            -padding,
                            (-radius).floor() as i32,
                            span as i32 + padding,
                            radius.ceil() as i32,
                        );
                        let (a, _, _) = pattern
                            .fitted_masks(
                                rect,
                                [0.0, -radius, span, weight],
                                false,
                                [0.0, span],
                                fitting,
                            )
                            .unwrap();
                        let (b, _, _) = pattern
                            .fitted_masks(
                                IntRect::new(rect.top, rect.left, rect.bottom, rect.right),
                                [-radius, 0.0, weight, span],
                                true,
                                [0.0, span],
                                fitting,
                            )
                            .unwrap();
                        let width = rect.width() as usize;
                        let height = rect.height() as usize;
                        for row in 0..height {
                            for col in 0..width {
                                assert_eq!(a[row * width + col], b[col * height + row]);
                                assert_eq!(
                                    a[row * width + col],
                                    a[row * width + width - col - 1],
                                    "{pattern:?},{fitting:?},span={span},weight={weight},col={col}"
                                );
                            }
                        }
                        for x in [0, span as i32 - 1] {
                            assert!(
                                (0..height).any(|y| a[y * width + (x - rect.left) as usize] > 0)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dots_keep_centers_when_weight_changes_and_transpose_without_area_or_spacing_drift() {
        for intervals in [vec![5.0, 7.0], vec![0.0, 6.0]] {
            let count = 5.0;
            for weight in [0.75_f32, 1.5, 4.0] {
                let radius = weight / 2.0;
                let pattern = TextDecorationPattern::Dots(intervals.clone());
                let rect = IntRect::new(
                    (-radius).floor() as i32,
                    (-radius).floor() as i32,
                    (24.0 + radius).ceil() as i32,
                    radius.ceil() as i32,
                );
                let (horizontal, _, _) = pattern
                    .masks(rect, [0.0, -radius, 24.0, weight], false, [0.0, 24.0])
                    .unwrap();
                let (vertical, _, _) = pattern
                    .masks(
                        IntRect::new(rect.top, rect.left, rect.bottom, rect.right),
                        [-radius, 0.0, weight, 24.0],
                        true,
                        [0.0, 24.0],
                    )
                    .unwrap();
                for row in 0..rect.height() as usize {
                    for col in 0..rect.width() as usize {
                        assert_eq!(
                            horizontal[row * rect.width() as usize + col],
                            vertical[col * rect.height() as usize + row]
                        );
                    }
                }
                let area = horizontal
                    .iter()
                    .map(|v| f32::from(*v) / 255.0)
                    .sum::<f32>();
                let expected = count * std::f32::consts::PI * radius * radius;
                assert!(
                    (area - expected).abs() <= horizontal.len() as f32 / 510.0 + 1e-5,
                    "{intervals:?},weight={weight}: {area} != {expected}"
                );
                let column = |x: i32| {
                    (rect.top..rect.bottom)
                        .map(|y| {
                            horizontal[((y - rect.top) * rect.width() + x - rect.left) as usize]
                                as u32
                        })
                        .sum::<u32>()
                };
                let centers = if intervals[0] == 0.0 {
                    vec![0, 6, 12, 18, 24]
                } else {
                    vec![0, 5, 12, 17, 24]
                };
                for center in centers {
                    assert_eq!(column(center - 1), column(center));
                    assert!(column(center) > 0);
                }
                let mut scaled = pattern.clone();
                scaled.scaled(2.0);
                assert_eq!(
                    scaled,
                    TextDecorationPattern::Dots(intervals.iter().map(|v| v * 2.0).collect())
                );
            }
        }
        for values in [
            vec![],
            vec![0.0],
            vec![1.0; 6],
            vec![-1.0, 2.0],
            vec![f32::INFINITY],
            vec![f32::NAN],
        ] {
            assert!(!TextDecorationPattern::Dots(values).valid());
        }
    }

    #[test]
    fn dash_caps_keep_legacy_arrays_and_dimensionless_settings_through_scaling_and_serialization() {
        let old: TextDecorationPattern = serde_json::from_str(r#"{"Dashes":[6,3]}"#).unwrap();
        assert_eq!(old, TextDecorationPattern::Dashes(vec![6.0, 3.0].into()));
        for cap in [
            DecorationCap::Butt,
            DecorationCap::Round,
            DecorationCap::Projecting,
        ] {
            let mut pattern = TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![6.0, 3.0],
                cap,
            });
            pattern.scaled(2.0);
            assert_eq!(
                pattern,
                TextDecorationPattern::Dashes(DecorationDashes {
                    lengths: vec![12.0, 6.0],
                    cap
                })
            );
            assert_eq!(
                serde_json::from_str::<TextDecorationPattern>(
                    &serde_json::to_string(&pattern).unwrap()
                )
                .unwrap(),
                pattern
            );
        }
    }

    #[test]
    fn dash_area_matches_independent_rectangles_and_keeps_phase_at_every_offset() {
        for lengths in [
            vec![5.0, 3.0],
            vec![0.0, 2.0],
            vec![3.0, 0.0],
            vec![1.5, 0.75, 0.5, 2.25],
        ] {
            let period = lengths.iter().sum::<f32>();
            for start in -128..128 {
                let start = start as f32 / 8.0;
                for width in [0.25, 1.0, 3.5, 8.0] {
                    let mut expected = 0.0;
                    for repetition in -20..20 {
                        let mut at = repetition as f32 * period;
                        for pair in lengths.as_chunks::<2>().0 {
                            expected +=
                                ((at + pair[0]).min(start + width) - at.max(start)).max(0.0);
                            at += pair[0] + pair[1];
                        }
                    }
                    assert_eq!(
                        dash_area(
                            &lengths.iter().map(|v| f64::from(*v)).collect::<Vec<_>>(),
                            start,
                            width
                        ),
                        expected,
                        "{lengths:?}, {start}, {width}"
                    );
                }
            }
        }
        for tiny in [f32::MIN_POSITIVE, f32::from_bits(1), 0.00000001] {
            assert!(
                (dash_area(&[f64::from(tiny), f64::from(tiny)], -120.5, 1.0) - 0.5).abs()
                    < 0.000001
            );
        }
    }

    #[test]
    fn dashes_scale_in_points_while_stripes_stay_percentages_and_invalid_cycles_are_rejected() {
        for values in [
            vec![],
            vec![1.0],
            vec![0.0, 0.0],
            vec![-1.0, 2.0],
            vec![1.0; 12],
            vec![f32::INFINITY, 1.0],
            vec![f32::NAN, 1.0],
        ] {
            assert!(!TextDecorationPattern::Dashes(values.into()).valid());
        }
        for scale in [0.5, 1.0, 2.0, 3.0] {
            let mut decoration = TextDecoration {
                weight: Some(2.0),
                offset: Some(3.0),
                pattern: TextDecorationPattern::Dashes(vec![6.0, 3.0].into()),
                ..Default::default()
            };
            decoration.scaled(scale);
            assert_eq!(
                decoration.pattern,
                TextDecorationPattern::Dashes(vec![6.0 * scale, 3.0 * scale].into())
            );
            assert_eq!(decoration.weight, Some(2.0 * scale));
            decoration.pattern = TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]);
            decoration.scaled(scale);
            assert_eq!(
                decoration.pattern,
                TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0])
            );
        }
    }

    #[test]
    fn stripe_masks_preserve_area_and_transpose_at_fractional_pixel_edges() {
        for edges in [
            vec![0.0, 100.0],
            vec![0.0, 25.0, 75.0, 100.0],
            vec![11.0, 19.0, 32.0, 81.0],
        ] {
            let pattern = TextDecorationPattern::Stripes(edges.clone());
            let proportion = edges
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| (p[1] - p[0]) / 100.0)
                .sum::<f32>();
            for weight in [0.25, 0.75, 1.5, 4.0, 11.3] {
                for origin in [-3.75_f32, -0.5, 0.0, 1.2] {
                    let geometry = [2.0, origin, 9.0, weight];
                    let rect = IntRect::new(
                        2,
                        origin.floor() as i32,
                        11,
                        (origin + weight).ceil() as i32,
                    );
                    let a = pattern
                        .masks(rect, geometry, false, [0.0, 100.0])
                        .unwrap()
                        .0;
                    let b = pattern
                        .masks(
                            IntRect::new(rect.top, rect.left, rect.bottom, rect.right),
                            [origin, 2.0, weight, 9.0],
                            true,
                            [0.0, 100.0],
                        )
                        .unwrap()
                        .0;
                    for row in 0..rect.height() as usize {
                        for col in 0..rect.width() as usize {
                            assert_eq!(
                                a[row * rect.width() as usize + col],
                                b[col * rect.height() as usize + row]
                            );
                        }
                    }
                    let area = a.iter().map(|v| f32::from(*v) / 255.0).sum::<f32>();
                    assert!(
                        (area - 9.0 * weight * proportion).abs() <= a.len() as f32 / 510.0 + 0.0001
                    );
                }
            }
        }
    }

    #[test]
    fn half_pixel_stripes_and_gaps_quantize_independently_without_rounding_bias() {
        for vertical in [false, true] {
            let pattern = TextDecorationPattern::Stripes(vec![0.0, 25.0, 75.0, 100.0]);
            let (rect, geometry) = if vertical {
                (IntRect::new(0, 0, 2, 8), [0.0, 0.0, 2.0, 8.0])
            } else {
                (IntRect::new(0, 0, 8, 2), [0.0, 0.0, 8.0, 2.0])
            };
            let (stripe, gap, _) = pattern.masks(rect, geometry, vertical, [0.0, 8.0]).unwrap();
            assert_eq!(stripe, vec![128; 16]);
            assert_eq!(gap, vec![128; 16]);
        }
    }

    #[test]
    fn stripe_validation_rejects_undefined_geometry_and_retains_legacy_defaults() {
        for edges in [
            vec![],
            vec![0.0],
            vec![0.0, 20.0, 40.0],
            vec![0.0, 0.0],
            vec![30.0, 20.0],
            vec![-1.0, 100.0],
            vec![0.0, 101.0],
            vec![0.0, f32::NAN],
            vec![0.0, f32::INFINITY],
        ] {
            let pattern = TextDecorationPattern::Stripes(edges);
            assert!(!pattern.valid());
            assert!(pattern
                .masks(
                    IntRect::new(0, 0, 1, 1),
                    [0.0, 0.0, 1.0, 1.0],
                    false,
                    [0.0, 1.0]
                )
                .is_none());
        }
        let old: TextDecoration = serde_json::from_str(r#"{"weight":1.5,"offset":2.0}"#).unwrap();
        assert_eq!(old.pattern, TextDecorationPattern::Solid);
        assert_eq!(old.gap_color, None);
    }
}
