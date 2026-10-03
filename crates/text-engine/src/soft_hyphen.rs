//! Select optional source breaks using the width of the visible hyphen.
use std::ops::Range;

pub(super) struct Line {
    pub range: Range<usize>,
    pub hyphen: bool,
}

impl From<Range<usize>> for Line {
    fn from(range: Range<usize>) -> Self {
        Self {
            range,
            hyphen: false,
        }
    }
}

pub(super) fn lines(
    text: &str,
    range: Range<usize>,
    boundaries: &[usize],
    width: impl Fn(usize) -> Option<f32>,
    measure: impl Fn(Range<usize>, bool, usize) -> f32,
) -> Vec<Line> {
    let mut result = Vec::new();
    let mut start = range.start;
    loop {
        let index = result.len();
        let Some(limit) = width(index) else {
            result.push(Line {
                range: start..range.end,
                hyphen: false,
            });
            return result;
        };
        let mut fitted = None;
        let mut selected = None;
        for &end in boundaries.iter().filter(|end| **end > start) {
            let hyphen = end < range.end && text[start..end].ends_with('\u{ad}');
            // A leading discretionary character is not a line of its own.
            if hyphen
                && text[start..end - '\u{ad}'.len_utf8()]
                    .chars()
                    .next_back()
                    .is_none_or(|c| c.is_whitespace() || c == '\u{ad}')
            {
                continue;
            }
            let natural = measure(start..end, false, index);
            if natural > limit {
                if let Some(previous) = fitted.take() {
                    selected = Some(previous);
                    break;
                }
                if !hyphen {
                    // Retain the existing overlong-word behavior. A hyphen
                    // that does not fit must not create a shorter overflow.
                    selected = Some(Line {
                        range: start..end,
                        hyphen: false,
                    });
                    break;
                }
            } else if !hyphen || measure(start..end, true, index) <= limit {
                fitted = Some(Line {
                    range: start..end,
                    hyphen,
                });
            }
        }
        let line = selected.or(fitted).unwrap_or(Line {
            range: start..range.end,
            hyphen: false,
        });
        start = line.range.end;
        result.push(line);
        if start == range.end {
            return result;
        }
    }
}
