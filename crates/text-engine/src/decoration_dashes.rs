//! Bounded coverage of finite straight dashes with geometric end caps.
use crate::DecorationCap;

/// Coverage over one pixel rectangle, relative to the first dash and line center.
/// The base dash segments end at `span.0`; caps extend past their endpoints.
/// `span.1` pins the last dash/circle to that endpoint after fitting.
pub(super) fn area(
    lengths: &[f64],
    cap: DecorationCap,
    radius: f64,
    span: (f64, bool),
    along: [f64; 2],
    across: [f64; 2],
) -> f64 {
    let (span, fitted_end) = span;
    let low = across[0].max(-radius);
    let high = across[1].min(radius);
    if high <= low || along[1] <= along[0] {
        return 0.0;
    }
    if cap == DecorationCap::Projecting {
        return (high - low) * expanded_area(lengths, radius, span, fitted_end, along);
    }
    let section = |y: f64| {
        expanded_area(
            lengths,
            (radius * radius - y * y).max(0.0).sqrt(),
            span,
            fitted_end,
            along,
        )
    };
    // Split at the center: each half is monotone, so narrow endpoint coverage
    // cannot be missed by sampling an otherwise empty interval.
    let integrate = |a: f64, b: f64| {
        if a >= b {
            return 0.0;
        }
        let values = [section(a), section((a + b) / 2.0), section(b)];
        adaptive(&section, a, b, values, 1e-9, 18)
    };
    integrate(low, high.min(0.0)) + integrate(low.max(0.0), high)
}

fn adaptive(
    f: &impl Fn(f64) -> f64,
    a: f64,
    b: f64,
    v: [f64; 3],
    tolerance: f64,
    depth: u8,
) -> f64 {
    let mid = (a + b) / 2.0;
    let left = f((a + mid) / 2.0);
    let right = f((mid + b) / 2.0);
    let coarse = (b - a) * (v[0] + 4.0 * v[1] + v[2]) / 6.0;
    let fine = (b - a) * (v[0] + 4.0 * left + 2.0 * v[1] + 4.0 * right + v[2]) / 12.0;
    if depth == 0 || (fine - coarse).abs() <= 15.0 * tolerance {
        return fine + (fine - coarse) / 15.0;
    }
    adaptive(f, a, mid, [v[0], left, v[1]], tolerance / 2.0, depth - 1)
        + adaptive(f, mid, b, [v[1], right, v[2]], tolerance / 2.0, depth - 1)
}

/// At a fixed cross-axis coordinate every cap is an interval extension. Shrink
/// each gap by twice that extension instead of enumerating tiny repeated dashes.
fn expanded_area(
    lengths: &[f64],
    expansion: f64,
    span: f64,
    fitted_end: bool,
    along: [f64; 2],
) -> f64 {
    let period = lengths.iter().copied().sum::<f64>();
    let tail = span.rem_euclid(period);
    let mut cursor = 0.0;
    let mut last = 0.0;
    let mut gaps = Vec::with_capacity(lengths.len() / 2);
    for pair in lengths.as_chunks::<2>().0 {
        let end = cursor + pair[0];
        if cursor <= tail {
            last = end.min(tail);
        }
        let gap = pair[1];
        if gap > 2.0 * expansion {
            gaps.push((end + expansion, gap - 2.0 * expansion));
        }
        cursor = end + gap;
    }
    // Fitting deliberately lands on a dash endpoint (or dot center). A tiny
    // remainder error must not select the previous dash and erase the final cap.
    let last = if fitted_end { span } else { span - tail + last };
    let a = along[0].max(-expansion);
    let b = along[1].min(last + expansion);
    if a >= b {
        return 0.0;
    }
    let length = b - a;
    let cycles = (length / period).floor();
    let remainder = length.rem_euclid(period);
    let phase = a.rem_euclid(period);
    let partial = |a: f64, b: f64| {
        gaps.iter()
            .map(|(start, length)| ((start + length).min(b) - start.max(a)).max(0.0))
            .sum::<f64>()
    };
    let missing = cycles * gaps.iter().map(|(_, length)| length).sum::<f64>()
        + partial(phase, (phase + remainder).min(period))
        + partial(0.0, (phase + remainder - period).max(0.0));
    (length - missing).clamp(0.0, length)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projecting_caps_match_independent_interval_unions_even_when_caps_overlap() {
        for lengths in [vec![5.0, 3.0], vec![0.0, 3.0], vec![1.5, 0.5, 2.0, 4.0]] {
            for span in [0.5_f64, 3.0, 8.0, 19.25] {
                for radius in [0.25, 1.0, 3.0, 8.0] {
                    let mut intervals: Vec<(f64, f64)> = Vec::new();
                    let mut cursor = 0.0;
                    while cursor <= span {
                        for pair in lengths.as_chunks::<2>().0 {
                            if cursor <= span {
                                let start = cursor - radius;
                                let end = (cursor + pair[0]).min(span) + radius;
                                if let Some(last) =
                                    intervals.last_mut().filter(|last| last.1 >= start)
                                {
                                    last.1 = last.1.max(end);
                                } else {
                                    intervals.push((start, end));
                                }
                            }
                            cursor += pair[0] + pair[1];
                        }
                    }
                    for pixel in -40..160 {
                        let a = f64::from(pixel) / 4.0;
                        let b = a + 1.0;
                        let expected = intervals
                            .iter()
                            .map(|(start, end)| (end.min(b) - start.max(a)).max(0.0))
                            .sum::<f64>();
                        assert!(
                            (expanded_area(&lengths, radius, span, false, [a, b]) - expected).abs()
                                < 1e-10,
                            "{lengths:?},span={span},radius={radius},pixel={pixel}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn round_caps_have_analytic_circle_and_capsule_area_without_double_painting_overlaps() {
        for radius in [0.125, 0.75, 2.0, 5.5] {
            for length in [0.0, 1.25, 8.0] {
                let lengths = [length, 50.0];
                let mut total = 0.0;
                for y in (-radius - 1.0_f64).floor() as i32..(radius + 1.0).ceil() as i32 {
                    for x in (-radius - 1.0).floor() as i32..(length + radius + 1.0).ceil() as i32 {
                        total += area(
                            &lengths,
                            DecorationCap::Round,
                            radius,
                            (length, false),
                            [f64::from(x), f64::from(x + 1)],
                            [f64::from(y), f64::from(y + 1)],
                        );
                    }
                }
                let expected = std::f64::consts::PI * radius * radius + length * 2.0 * radius;
                assert!(
                    (total - expected).abs() < 1e-6,
                    "r={radius},length={length}: {total} != {expected}"
                );
            }
        }
        for radius in [0.75_f64, 2.0, 5.5] {
            for proportion in [0.25, 0.75, 1.5] {
                let distance = radius * proportion;
                let distance = f64::from(distance as f32);
                let mut total = 0.0;
                for y in (-radius - 1.0).floor() as i32..(radius + 1.0).ceil() as i32 {
                    for x in (-radius - 1.0).floor() as i32..(distance + radius + 1.0).ceil() as i32
                    {
                        total += area(
                            &[0.0, distance],
                            DecorationCap::Round,
                            radius,
                            (distance, false),
                            [f64::from(x), f64::from(x + 1)],
                            [f64::from(y), f64::from(y + 1)],
                        );
                    }
                }
                let lens = 2.0 * radius * radius * (distance / (2.0 * radius)).acos()
                    - distance / 2.0 * (4.0 * radius * radius - distance * distance).sqrt();
                let expected = 2.0 * std::f64::consts::PI * radius * radius - lens;
                assert!(
                    (total - expected).abs() < 1e-6,
                    "overlapping circles: r={radius},d={distance}, {total} != {expected}"
                );
            }
        }
        // Dashes much shorter than a pixel retain bounded work, including the
        // finite line's end caps. Overlap is a union, never repeated opacity.
        for tiny in [f32::from_bits(1), f32::MIN_POSITIVE, 1e-8] {
            assert_eq!(
                area(
                    &[f64::from(tiny), f64::from(tiny)],
                    DecorationCap::Round,
                    2.0,
                    (50.0, false),
                    [10.0, 11.0],
                    [-0.5, 0.5]
                ),
                1.0
            );
            assert!(
                area(
                    &[f64::from(tiny), f64::from(tiny)],
                    DecorationCap::Round,
                    2.0,
                    (50.0, false),
                    [-1.0, 0.0],
                    [-0.5, 0.5]
                ) > 0.9
            );
        }
    }
}
