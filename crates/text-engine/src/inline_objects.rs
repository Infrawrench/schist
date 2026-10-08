//! Display runs with inline-object semantics. Callers own display projection
//! and source mapping, so shaping and rasterization see the same font domains.
use super::TextSpec;
use std::ops::Range;

impl TextSpec {
    pub(super) fn valid_inline_objects(&self) -> bool {
        let mut end = 0;
        self.inline_objects.iter().all(|span| {
            let valid = span.start >= end
                && self.text.get(span.clone()).is_some_and(|text| {
                    text.strip_prefix('\u{2068}')
                        .and_then(|text| text.strip_suffix('\u{2069}'))
                        .is_some_and(|body| {
                            body.chars().all(|c| {
                                !c.is_control()
                                    && !super::shaping::bidi_control(c)
                                    && c != '\u{2028}'
                                    && c != '\u{2029}'
                            })
                        })
                });
            end = span.end;
            valid
        })
    }

    pub(super) fn in_object(&self, at: usize) -> bool {
        self.inline_objects.iter().any(|span| span.contains(&at))
    }

    /// Shaping/casing context never crosses an object edge, even when adjacent
    /// runs happen to resolve to the same font and character properties.
    pub(super) fn shaping_context(&self, at: usize) -> Range<usize> {
        let mut start = 0;
        for span in &self.inline_objects {
            if at < span.start {
                return start..span.start;
            }
            if at < span.end {
                return span.clone();
            }
            start = span.end;
        }
        start..self.text.len()
    }

    /// UAX #14 sees one object replacement character per display span. Mapping
    /// the resulting boundaries back preserves GL/WJ and adjacent objects.
    pub(super) fn object_breaks(&self, range: Range<usize>) -> Vec<usize> {
        if self.inline_objects.is_empty() {
            return unicode_linebreak::linebreaks(&self.text[range.clone()])
                .map(|(at, _)| range.start + at)
                .collect();
        }
        let mut logical = String::new();
        let mut boundaries = Vec::new();
        let mut cursor = range.start;
        for span in self
            .inline_objects
            .iter()
            .filter(|s| s.start >= range.start && s.end <= range.end)
        {
            for (offset, c) in self.text[cursor..span.start].char_indices() {
                boundaries.push((logical.len(), cursor + offset));
                logical.push(c);
            }
            boundaries.push((logical.len(), span.start));
            logical.push('\u{fffc}');
            cursor = span.end;
        }
        for (offset, c) in self.text[cursor..range.end].char_indices() {
            boundaries.push((logical.len(), cursor + offset));
            logical.push(c);
        }
        boundaries.push((logical.len(), range.end));
        unicode_linebreak::linebreaks(&logical)
            .filter_map(|(at, _)| {
                boundaries
                    .binary_search_by_key(&at, |(at, _)| *at)
                    .ok()
                    .map(|index| boundaries[index].1)
            })
            .collect()
    }
}
