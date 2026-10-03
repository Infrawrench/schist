//! Stories: the linear text flow that frames display.
//!
//! This is the idea that separates a page layout document from a word
//! processor's, and the one that makes threading possible. Text does not
//! belong to a frame. It belongs to a **story**, and any number of frames
//! can show any part of that story. Three frames on three pages holding
//! one continuous article is one story, not three. When the text is
//! edited in the first frame, the other two change too, because they are
//! showing the same characters.
//!
//! A story is a flat sequence of [`Point`]s. A point is a piece of text
//! with a paragraph style, and may carry a character style of its own.
//! Nothing in the story knows its size on the page -- that is decided
//! later, when a frame composes it, and is why the same story can appear
//! in a two-column frame on one page and a full-measure frame on another.

use serde::{Deserialize, Serialize};

/// A piece of text within a story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Point {
    /// A paragraph. The style applies to the whole paragraph.
    Paragraph { text: String, style: String },
    /// A forced line break inside a paragraph.
    LineBreak,
    /// A column break. The next point starts a new column, and in a
    /// threaded frame the remainder moves to the next frame.
    ColumnBreak,
    /// A page break, which forces a new page when the story spans one.
    PageBreak,
    /// A forced break to a later odd-numbered page in the thread.
    OddPageBreak,
    /// A forced break to a later even-numbered page in the thread.
    EvenPageBreak,
    /// A linked-frame break: everything after this starts in the next
    /// frame of the thread. This is what a story editor inserts when the
    /// user says "put the rest over there".
    FrameBreak,
    /// A table or an embedded object's anchor, kept as an opaque payload
    /// so the structure survives even when this version cannot interpret
    /// it. Round-tripping beats dropping a customer's table on save.
    Other { kind: String, payload: String },
}

impl Point {
    /// The characters this point contributes, for length measurement.
    pub fn text(&self) -> &str {
        match self {
            Point::Paragraph { text, .. } => text,
            _ => "",
        }
    }

    pub fn is_forced_break(&self) -> bool {
        matches!(
            self,
            Point::LineBreak
                | Point::ColumnBreak
                | Point::PageBreak
                | Point::OddPageBreak
                | Point::EvenPageBreak
                | Point::FrameBreak
        )
    }
}

/// A character style applied to a byte range of a story.
///
/// Ranges are half-open and index into the story's concatenated
/// `Paragraph` text, counting bytes so they can be handed to the text
/// engine's shaping without re-encoding. A range that extends past the
/// end of the text is clamped by [`Story::range`] rather than panicking,
/// because a document edited by another tool can carry a stale range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleRange {
    /// Start byte offset into the story's text.
    pub start: usize,
    /// End byte offset, exclusive.
    pub end: usize,
    /// Character style name, resolved through the style set.
    pub style: String,
}

impl StyleRange {
    pub fn new(start: usize, end: usize, style: impl Into<String>) -> StyleRange {
        StyleRange {
            start,
            end,
            style: style.into(),
        }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An inline structure retained without adding characters to the main body.
/// Exact XML remains recoverable even when typed lowering is unavailable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoryStructure {
    /// UTF-8 byte boundary in the story, or unknown after an external edit.
    pub at: Option<usize>,
    pub kind: String,
    pub payload: String,
    /// Lowered text-only footnotes retain a separate flow and zero-width markers.
    /// Their original XML is still retained alongside supported note composition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footnote: Option<crate::footnotes::FootnoteBody>,
}

/// A linear flow of text, shared by one or more frames.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub points: Vec<Point>,
    pub ranges: Vec<StyleRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structures: Vec<StoryStructure>,
    #[serde(default)]
    pub prefs: StoryPreferences,
}

/// Order of frame columns, independent of each paragraph's bidi direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoryDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

/// Native story orientation. Paragraph writing modes may override it locally.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoryOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryPreferences {
    pub direction: StoryDirection,
    pub orientation: StoryOrientation,
}

impl Story {
    /// Replace a textual range, retaining unaffected paragraph and
    /// character styles. Structural points are preserved; a replacement
    /// crossing a forced break or opaque anchor is refused rather than
    /// flattening that structure silently.
    pub fn replace_text(
        &self,
        range: std::ops::Range<usize>,
        insert: &str,
        fallback: &str,
    ) -> Option<Self> {
        let text = self.text();
        let start = floor_char_boundary(&text, range.start.min(text.len()));
        let end = ceil_char_boundary(&text, range.end.max(start).min(text.len()));
        // Insertion at an anchor goes before the structure (right affinity).
        // A replacement may touch either side, but cannot erase its interior.
        // Invalid anchors must not be silently moved to an unrelated character.
        if self
            .structures
            .iter()
            .filter_map(|s| s.at)
            .any(|at| at > text.len() || !text.is_char_boundary(at) || (start < at && at < end))
        {
            return None;
        }
        let structures = self
            .structures
            .iter()
            .cloned()
            .map(|mut structure| {
                structure.at = structure.at.map(|at| {
                    if at >= end {
                        at - end + start + insert.len()
                    } else {
                        at
                    }
                });
                structure
            })
            .collect();
        if self.points.is_empty() {
            return Some(Self {
                prefs: self.prefs,
                structures,
                points: insert
                    .split('\n')
                    .map(|text| Point::Paragraph {
                        text: text.into(),
                        style: fallback.into(),
                    })
                    .collect(),
                ranges: Vec::new(),
            });
        }
        let offsets = self.point_offsets();
        let locate = |at| {
            self.points
                .iter()
                .enumerate()
                .find(|(i, p)| {
                    matches!(p, Point::Paragraph { .. })
                        && offsets[*i] <= at
                        && at <= offsets[*i] + p.text().len()
                })
                .map(|(i, _)| i)
        };
        let first = locate(start)?;
        let last = locate(end)?;
        if self.points[first..=last]
            .iter()
            .any(|p| !matches!(p, Point::Paragraph { .. }))
        {
            return None;
        }
        let Point::Paragraph { text: left, style } = &self.points[first] else {
            return None;
        };
        let right = self.points[last].text();
        let merged = format!(
            "{}{}{}",
            &left[..start - offsets[first]],
            insert,
            &right[end - offsets[last]..]
        );
        let mut points = self.points[..first].to_vec();
        points.extend(merged.split('\n').map(|text| Point::Paragraph {
            text: text.into(),
            style: style.clone(),
        }));
        points.extend_from_slice(&self.points[last + 1..]);
        let shift = |at: usize| at - end + start + insert.len();
        let mut ranges = Vec::new();
        for r in &self.ranges {
            if r.start >= r.end
                || r.end > text.len()
                || !text.is_char_boundary(r.start)
                || !text.is_char_boundary(r.end)
            {
                continue;
            }
            if r.start < start {
                ranges.push(StyleRange::new(r.start, r.end.min(start), &r.style));
            }
            if r.end > end {
                ranges.push(StyleRange::new(
                    shift(r.start.max(end)),
                    shift(r.end),
                    &r.style,
                ));
            }
        }
        let inherited = self
            .ranges
            .iter()
            .rev()
            .find(|r| r.start <= start && r.end > start)
            .or_else(|| {
                self.ranges
                    .iter()
                    .rev()
                    .find(|r| r.start < start && r.end == start)
            });
        if let Some(inherited) = inherited.filter(|_| !insert.is_empty()) {
            ranges.push(StyleRange::new(
                start,
                start + insert.len(),
                &inherited.style,
            ));
        }
        ranges.sort_by_key(|r| (r.start, r.end));
        let mut compact: Vec<StyleRange> = Vec::new();
        for range in ranges.into_iter().filter(|r| !r.is_empty()) {
            if let Some(previous) = compact
                .last_mut()
                .filter(|p| p.style == range.style && p.end == range.start)
            {
                previous.end = range.end;
            } else {
                compact.push(range);
            }
        }
        Some(Self {
            points,
            ranges: compact,
            structures,
            prefs: self.prefs,
        })
    }

    /// Count structures retained separately from the editable main text.
    /// A composed frame reports which of these its layout path cannot paint.
    pub fn retained_structures(&self) -> usize {
        self.structures.len()
            + self
                .points
                .iter()
                .filter(|p| matches!(p, Point::Other { .. }))
                .count()
    }

    pub fn new() -> Story {
        Story::default()
    }

    /// A story holding a single unstyled paragraph.
    pub fn from_text(text: impl Into<String>, style: impl Into<String>) -> Story {
        Story {
            prefs: StoryPreferences::default(),
            structures: Vec::new(),
            points: vec![Point::Paragraph {
                text: text.into(),
                style: style.into(),
            }],
            ranges: Vec::new(),
        }
    }

    /// Append a paragraph and return the byte range it occupies.
    pub fn push_paragraph(
        &mut self,
        text: impl Into<String>,
        style: impl Into<String>,
    ) -> (usize, usize) {
        let text = text.into();
        self.points.push(Point::Paragraph {
            text: text.clone(),
            style: style.into(),
        });
        let start = self.point_offsets().last().copied().unwrap_or(0);
        (start, start + text.len())
    }

    /// Total length in bytes of the story's textual content.
    ///
    /// This is the story's coordinate system: [`StyleRange`] and
    /// [`Story::point_offsets`] index into it, so all three have to agree
    /// on where one point ends and the next begins. They are measured
    /// together by [`Story::point_offsets`].
    pub fn text_len(&self) -> usize {
        // The rendered text is the coordinate system; deriving the length
        // from it means the two can never disagree.
        self.text().len()
    }

    /// The bytes a point occupies, including any separator that follows it.
    fn segment_len(&self, index: usize) -> usize {
        let text = self.points[index].text().len();
        // The separator belongs to this point when something follows it
        // and `text` puts one there.
        if index + 1 < self.points.len() && self.separator_before(index + 1) {
            text + 1
        } else {
            text
        }
    }

    /// The text between two byte offsets, clamped to char boundaries.
    ///
    /// Offsets from another tool, or from an earlier revision of this
    /// story, can land mid-character. Returning a valid string rather than
    /// invalid UTF-8 matters because the result goes straight to the
    /// shaper.
    pub fn slice(&self, start: usize, end: usize) -> String {
        let text = self.text();
        let start = floor_char_boundary(&text, start.min(text.len()));
        let end = ceil_char_boundary(&text, end.min(text.len()).max(start));
        text[start..end].to_string()
    }

    /// The byte offset at which each point starts.
    pub fn point_offsets(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.points.len());
        let mut offset = 0usize;
        for i in 0..self.points.len() {
            out.push(offset);
            offset += self.segment_len(i);
        }
        out
    }

    /// The story's text, concatenated.
    ///
    /// A newline appears exactly where [`Story::point_offsets`] says a
    /// separator byte is, so the string and the byte coordinate system
    /// agree. A break or opaque point sits in the stream without
    /// contributing a character.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, point) in self.points.iter().enumerate() {
            if i > 0 && self.separator_before(i) {
                out.push('\n');
            }
            out.push_str(point.text());
        }
        out
    }

    /// Whether a separator byte sits between point `i - 1` and point `i`.
    ///
    /// This is the single definition both `text` and `point_offsets` use,
    /// so they cannot drift apart.
    fn separator_before(&self, i: usize) -> bool {
        if i == 0 {
            return false;
        }
        matches!(
            self.points[i - 1],
            Point::Paragraph { .. } | Point::LineBreak
        )
    }

    /// The character styles covering `start..end`, clamped to the story.
    ///
    /// A caller asking about bytes past the end gets `None` rather than a
    /// panic; a stale range from another tool should not take the
    /// document down.
    pub fn range(&self, start: usize, end: usize) -> Option<&StyleRange> {
        let len = self.text_len();
        if start >= len || end <= start {
            return None;
        }
        self.ranges.iter().find(|r| r.start < end && r.end > start)
    }

    /// Which paragraphs a byte range falls inside.
    ///
    /// This is how a story editor maps a click on the story text back to
    /// the paragraphs a frame will lay out.
    pub fn paragraphs_in(&self, start: usize, end: usize) -> Vec<usize> {
        let offsets = self.point_offsets();
        let mut out = Vec::new();
        for (i, point) in self.points.iter().enumerate() {
            if !matches!(point, Point::Paragraph { .. }) {
                continue;
            }
            let from = offsets[i];
            let to = from + point.text().len();
            if to > start && from < end.max(start) {
                out.push(i);
            }
        }
        out
    }

    /// Apply a character style to a byte range, splitting any range it
    /// lands inside.
    pub fn apply_style(&mut self, start: usize, end: usize, style: impl Into<String>) {
        let style = style.into();
        if end <= start {
            return;
        }
        let mut out: Vec<StyleRange> = Vec::new();
        for range in self.ranges.drain(..) {
            // No overlap: keep as is.
            if range.end <= start || range.start >= end {
                out.push(range);
                continue;
            }
            // Partial overlap: the pieces either side survive.
            if range.start < start {
                out.push(StyleRange::new(range.start, start, range.style.clone()));
            }
            if range.end > end {
                out.push(StyleRange::new(end, range.end, range.style.clone()));
            }
        }
        out.push(StyleRange::new(start, end, style));
        out.sort_by_key(|r| r.start);
        self.ranges = out;
    }

    /// Remove any character styling over a byte range.
    pub fn clear_style(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let mut out: Vec<StyleRange> = Vec::new();
        for range in self.ranges.drain(..) {
            if range.end <= start || range.start >= end {
                out.push(range);
                continue;
            }
            if range.start < start {
                out.push(StyleRange::new(range.start, start, range.style.clone()));
            }
            if range.end > end {
                out.push(StyleRange::new(end, range.end, range.style.clone()));
            }
        }
        out.sort_by_key(|r| r.start);
        self.ranges = out;
    }
}

/// The largest char boundary at or below `i`.
pub(crate) fn floor_char_boundary(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    let mut i = i;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The smallest char boundary at or above `i`.
pub(crate) fn ceil_char_boundary(text: &str, i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    let mut i = i;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_paragraph_ranges_match_the_story_coordinate_system() {
        let mut story = Story::new();
        for text in ["é", "", "中", "two\nlines", "😀"] {
            let (start, end) = story.push_paragraph(text, "Body");
            assert_eq!(story.slice(start, end), text);
            assert_eq!(story.point_offsets().last(), Some(&start));
        }
    }

    #[test]
    fn text_length_counts_the_joining_newlines() {
        let mut story = Story::new();
        story.push_paragraph("Hello", "Default");
        story.push_paragraph("World", "Default");
        // "Hello" + separator + "World"
        assert_eq!(story.text_len(), 11);
        assert_eq!(story.text(), "Hello\nWorld");
    }

    #[test]
    fn text_len_and_point_offsets_agree() {
        // These are the same coordinate system. If they drift, a style
        // range applied to a story lands on the wrong characters.
        let mut story = Story::new();
        story.push_paragraph("Hello", "Default");
        story.push_paragraph("World", "Default");
        let offsets = story.point_offsets();
        assert_eq!(offsets, vec![0, 6]);
        // The last point's end is the total length.
        let last = offsets.last().copied().unwrap() + story.points.last().unwrap().text().len();
        assert_eq!(last, story.text_len());
    }

    #[test]
    fn a_break_point_contributes_no_separator_byte() {
        let story = Story {
            prefs: StoryPreferences::default(),
            structures: Vec::new(),
            points: vec![
                Point::Paragraph {
                    text: "A".into(),
                    style: "Default".into(),
                },
                Point::FrameBreak,
                Point::Paragraph {
                    text: "B".into(),
                    style: "Default".into(),
                },
            ],
            ranges: Vec::new(),
        };
        // "A" + separator + (frame break: nothing) + "B" + (last: none)
        assert_eq!(story.text_len(), 3);
        assert_eq!(story.point_offsets(), vec![0, 2, 2]);
    }

    #[test]
    fn point_offsets_track_the_text_coordinate_system() {
        let mut story = Story::new();
        story.push_paragraph("Hello", "Default");
        story.push_paragraph("World", "Default");
        assert_eq!(story.point_offsets(), vec![0, 6]);
    }

    #[test]
    fn applying_a_style_splits_an_existing_range() {
        let mut story = Story::from_text("Hello World", "Default");
        story.apply_style(0, 11, "Default");
        // Italicise "World", the last five bytes.
        story.apply_style(6, 11, "Italic");
        let italic = story.ranges.iter().find(|r| r.style == "Italic").unwrap();
        assert_eq!(italic.start, 6);
        assert_eq!(italic.end, 11);
        // And the rest is still Default, split into two pieces.
        let defaults: Vec<_> = story
            .ranges
            .iter()
            .filter(|r| r.style == "Default")
            .collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].start, 0);
        assert_eq!(defaults[0].end, 6);
    }

    #[test]
    fn clearing_a_style_leaves_the_rest_intact() {
        let mut story = Story::from_text("Hello World", "Default");
        story.apply_style(0, 5, "Bold");
        story.apply_style(6, 11, "Bold");
        story.clear_style(0, 5);
        assert!(story
            .ranges
            .iter()
            .all(|r| r.style != "Bold" || r.start == 6));
    }

    #[test]
    fn a_range_past_the_end_of_the_story_is_empty_not_a_panic() {
        let story = Story::from_text("Short", "Default");
        assert!(story.range(100, 200).is_none());
        assert!(story.range(3, 1).is_none());
    }

    #[test]
    fn paragraphs_in_maps_bytes_back_to_paragraphs() {
        let mut story = Story::new();
        story.push_paragraph("First", "Default");
        story.push_paragraph("Second", "Default");
        // The first paragraph's bytes.
        assert_eq!(story.paragraphs_in(0, 5), vec![0]);
        // Across the boundary.
        assert_eq!(story.paragraphs_in(4, 8), vec![0, 1]);
    }

    #[test]
    fn break_points_contribute_no_characters() {
        let story = Story {
            prefs: StoryPreferences::default(),
            structures: Vec::new(),
            points: vec![
                Point::Paragraph {
                    text: "A".into(),
                    style: "Default".into(),
                },
                Point::FrameBreak,
                Point::Paragraph {
                    text: "B".into(),
                    style: "Default".into(),
                },
            ],
            ranges: Vec::new(),
        };
        // Only the two paragraphs carry characters.
        assert_eq!(
            story.points.iter().map(|p| p.text().len()).sum::<usize>(),
            2
        );
        assert!(story.points[1].is_forced_break());
        assert!(!story.points[0].is_forced_break());
    }

    #[test]
    fn an_opaque_point_survives_a_round_trip() {
        // A table anchor this version cannot interpret must not be
        // dropped on save.
        let story = Story {
            prefs: StoryPreferences::default(),
            structures: Vec::new(),
            points: vec![Point::Other {
                kind: "TableAnchor".into(),
                payload: "<table/>".into(),
            }],
            ranges: Vec::new(),
        };
        let json = serde_json::to_string(&story).unwrap();
        let back: Story = serde_json::from_str(&json).unwrap();
        assert_eq!(back, story);
    }
}
