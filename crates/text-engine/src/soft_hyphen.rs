//! Select optional source breaks using the width of the visible hyphen.
use std::ops::Range;

/// unicode-linebreak 0.1.5 uses Unicode 15, whose LB21a suppresses even an
/// intraword Hebrew soft hyphen. Keep this narrow case consistent with newer
/// UAX #14: a Hebrew continuation can break here. Do not override joiners,
/// punctuation or combining marks following the soft hyphen.
pub(super) fn hebrew_breaks(text: &str) -> Vec<usize> {
    use unicode_linebreak::{break_property, BreakClass};
    let mut previous = None;
    let mut result = Vec::new();
    for (at, c) in text.char_indices() {
        let class = break_property(c as u32);
        if c == '\u{ad}'
            && previous == Some(BreakClass::HebrewLetter)
            && text[at + c.len_utf8()..]
                .chars()
                .next()
                .is_some_and(|next| break_property(next as u32) == BreakClass::HebrewLetter)
        {
            result.push(at + c.len_utf8());
        }
        if class != BreakClass::CombiningMark {
            previous = Some(class);
        }
    }
    result
}

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
    policy: &super::hyphenation::BreakPolicy<'_>,
) -> Vec<Line> {
    let mut result = Vec::new();
    let mut start = range.start;
    let mut consecutive = if range.start == 0 {
        policy.settings.preceding_hyphens
    } else {
        0
    };
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
        let mut plain_width = None;
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
            } else {
                let visible = if hyphen {
                    measure(start..end, true, index)
                } else {
                    natural
                };
                if !hyphen {
                    // Trailing separators are whitespace too: the ragged edge
                    // is the last word's advance, not the next word's start.
                    let trimmed = start + text[start..end].trim_end().len();
                    plain_width = Some(measure(start..trimmed, false, index));
                }
                if visible <= limit
                    && (!hyphen || policy.permits(end, consecutive, limit, visible, plain_width))
                {
                    fitted = Some(Line {
                        range: start..end,
                        hyphen,
                    });
                }
            }
        }
        let line = selected.or(fitted).unwrap_or(Line {
            range: start..range.end,
            hyphen: false,
        });
        let literal = text[line.range.clone()]
            .trim_end()
            .ends_with(['-', '\u{2010}']);
        consecutive = if line.hyphen || literal {
            consecutive.saturating_add(1)
        } else {
            0
        };
        start = line.range.end;
        result.push(line);
        if start == range.end {
            return result;
        }
    }
}
