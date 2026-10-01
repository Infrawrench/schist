//! Straight-segment fitting from the public StrokeCornerAdjustment rules.
//! The specification defines which lengths may change, not the repetition
//! selection algorithm. Schist minimizes proportional change while keeping a
//! dash (or dot center) at both ends. Native rendering is unverified.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DecorationFit {
    #[default]
    None,
    Dashes,
    Gaps,
    DashesAndGaps,
}

/// Resolve at most five dash/gap pairs without enumerating repetitions. All
/// arithmetic is f64: valid subnormal point lengths may repeat more than usize
/// can count. A segment shorter than its fixed first dash simply clips it.
pub(super) fn resolve(lengths: &[f32], span: f64, fitting: DecorationFit) -> Vec<f64> {
    let mut original: Vec<_> = lengths.iter().map(|v| f64::from(*v)).collect();
    if fitting == DecorationFit::None || !span.is_finite() || span <= 0.0 {
        return original;
    }
    let Some((dash_scale, gap_scale)) = scales(&original, span, fitting) else {
        // A zero first dash has no proportional scale. Dash-only fitting can
        // grow that adjustable dash to the span while keeping all gaps fixed.
        // For gap-only fitting, a span inside the fixed first dash just clips it.
        if fitting == DecorationFit::Dashes && original.first() == Some(&0.0) {
            original[0] = span;
        }
        return original;
    };
    original
        .into_iter()
        .enumerate()
        .map(|(i, value)| value * if i % 2 == 0 { dash_scale } else { gap_scale })
        .collect()
}

fn scales(lengths: &[f64], span: f64, fitting: DecorationFit) -> Option<(f64, f64)> {
    let dashes = lengths.iter().step_by(2).sum::<f64>();
    let gaps = lengths.iter().skip(1).step_by(2).sum::<f64>();
    let period = dashes + gaps;
    if !period.is_finite() || period <= 0.0 {
        return None;
    }
    let mut best: Option<(f64, f64, f64)> = None;
    let mut prefix_dash = 0.0;
    let mut prefix_gap = 0.0;
    for pair in lengths.as_chunks::<2>().0 {
        prefix_dash += pair[0];
        // A candidate is k complete cycles followed by a prefix ending in a
        // dash, without its trailing gap. The scale is monotone in k, so its
        // optimum lies on either side of the unadjusted length (or at zero).
        let ideal = ((span - prefix_dash - prefix_gap) / period).max(0.0);
        for cycles in [0.0, 1.0, ideal.floor(), ideal.ceil()] {
            let dash = cycles * dashes + prefix_dash;
            let gap = cycles * gaps + prefix_gap;
            let (change, fixed) = match fitting {
                DecorationFit::Dashes => (dash, gap),
                DecorationFit::Gaps => (gap, dash),
                DecorationFit::DashesAndGaps => (dash + gap, 0.0),
                DecorationFit::None => return None,
            };
            if change <= 0.0 || fixed >= span {
                continue;
            }
            let scale = (span - fixed) / change;
            let cost = scale.ln().abs();
            if !cost.is_finite() || best.is_some_and(|(old, _, _)| old <= cost) {
                continue;
            }
            let dash_scale = if fitting == DecorationFit::Gaps {
                1.0
            } else {
                scale
            };
            let gap_scale = if fitting == DecorationFit::Dashes {
                1.0
            } else {
                scale
            };
            best = Some((cost, dash_scale, gap_scale));
        }
        prefix_gap += pair[1];
    }
    best.map(|(_, dashes, gaps)| (dashes, gaps)).or_else(|| {
        // Some short spans cannot hold two complete fixed-length dashes. Close
        // their gaps, retain every dash length, and clip the final dash at the
        // segment endpoint. A span inside the first dash needs no adjustment.
        (fitting == DecorationFit::Gaps && dashes > 0.0 && span > lengths[0]).then_some((1.0, 0.0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_cover_both_ends_and_only_change_the_selected_lengths() {
        for lengths in [
            vec![6.0_f32, 3.0],
            vec![1.5, 2.0, 4.0, 0.5],
            vec![0.0, 5.0, 0.0, 7.0],
            vec![0.0, 5.0, 3.0, 7.0],
        ] {
            for fitting in [
                DecorationFit::Dashes,
                DecorationFit::Gaps,
                DecorationFit::DashesAndGaps,
            ] {
                for steps in 1..401 {
                    let span = f64::from(steps) / 4.0;
                    let fitted = resolve(&lengths, span, fitting);
                    for (index, (&before, &after)) in lengths.iter().zip(&fitted).enumerate() {
                        if (fitting == DecorationFit::Dashes && index % 2 == 1)
                            || (fitting == DecorationFit::Gaps && index % 2 == 0)
                        {
                            assert_eq!(f64::from(before), after);
                        }
                        assert!(after.is_finite() && after >= 0.0);
                    }
                    if fitting == DecorationFit::Gaps && span <= f64::from(lengths[0]) {
                        assert_eq!(
                            fitted,
                            lengths.iter().map(|v| f64::from(*v)).collect::<Vec<_>>()
                        );
                        continue;
                    }
                    let period = fitted.iter().sum::<f64>();
                    let mut edge = 0.0;
                    let mut found = fitting == DecorationFit::Gaps
                        && fitted.iter().skip(1).step_by(2).all(|v| *v == 0.0);
                    for pair in fitted.as_chunks::<2>().0 {
                        edge += pair[0];
                        let repetitions = ((span - edge) / period).round();
                        found |= repetitions >= 0.0
                            && (repetitions * period + edge - span).abs() < 1e-10;
                        edge += pair[1];
                    }
                    assert!(found, "{lengths:?}, {span}, {fitting:?}: {fitted:?}");
                    let scale = 3.0;
                    let large = resolve(
                        &lengths.iter().map(|v| v * scale as f32).collect::<Vec<_>>(),
                        span * scale,
                        fitting,
                    );
                    assert!(large
                        .iter()
                        .zip(&fitted)
                        .all(|(a, b)| (a - b * scale).abs() < 1e-10));
                }
            }
        }
    }

    #[test]
    fn fitting_minimizes_change_against_exhaustive_finite_candidates_and_bounds_tiny_periods() {
        for fitting in [
            DecorationFit::Dashes,
            DecorationFit::Gaps,
            DecorationFit::DashesAndGaps,
        ] {
            for lengths in [vec![6.0_f32, 3.0], vec![1.5, 2.0, 4.0, 0.5]] {
                let original: Vec<_> = lengths.iter().map(|v| f64::from(*v)).collect();
                for step in 1..400 {
                    let span = f64::from(step) / 4.0;
                    let Some((d, g)) = scales(&original, span, fitting) else {
                        continue;
                    };
                    let cost = if fitting == DecorationFit::Gaps { g } else { d }
                        .ln()
                        .abs();
                    let mut dash = 0.0;
                    let mut gap = 0.0;
                    for pair in original.as_chunks::<2>().0.iter().cycle().take(1000) {
                        dash += pair[0];
                        let (changed, fixed) = match fitting {
                            DecorationFit::Dashes => (dash, gap),
                            DecorationFit::Gaps => (gap, dash),
                            _ => (dash + gap, 0.0),
                        };
                        if changed > 0.0 && fixed < span {
                            let candidate = ((span - fixed) / changed).ln().abs();
                            assert!(
                                cost <= candidate + 1e-12,
                                "{fitting:?}, {lengths:?}, {span}"
                            );
                        }
                        gap += pair[1];
                    }
                }
            }
            for tiny in [f32::from_bits(1), f32::MIN_POSITIVE, 1e-8] {
                let result = resolve(&[tiny, tiny], 120.25, fitting);
                assert!(result.iter().all(|v| v.is_finite() && *v > 0.0));
            }
        }
    }
}
