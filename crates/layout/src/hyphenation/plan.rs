//! Cache opportunities in source text before generated reference numbers enter
//! layout. The displayed numbers must not change a word or its dictionary.
use crate::{inline_text::Projection, Story, StoryPoint, StyleSet};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Disposable dictionary opportunities and their complete owning words.
/// Generated reference text never becomes part of dictionary input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BreakPlan {
    positions: Vec<usize>,
    words: Vec<Range<usize>>,
}

impl BreakPlan {
    pub fn new(
        story: &Story,
        styles: &StyleSet,
        default_paragraph: &str,
        default_character: &str,
    ) -> Self {
        Self::with_paragraphs(story, story, styles, default_paragraph, default_character)
    }

    /// Use original words/ranges and effective projected paragraph styles, then
    /// map to display coordinates. Inline numbers never become source letters.
    /// A mismatched source/projection cannot supply a trustworthy break plan.
    pub fn projected(
        source: &Story,
        projection: &Projection,
        styles: &StyleSet,
        default_paragraph: &str,
        default_character: &str,
    ) -> Option<Self> {
        if source.text() != projection.positions.original_text()
            || source.points.len() != projection.story.points.len()
            || source
                .points
                .iter()
                .zip(&projection.story.points)
                .any(|(a, b)| {
                    !matches!(
                        (a, b),
                        (StoryPoint::Paragraph { .. }, StoryPoint::Paragraph { .. })
                    ) && a != b
                })
        {
            return None;
        }
        let plan = Self::with_paragraphs(
            source,
            &projection.story,
            styles,
            default_paragraph,
            default_character,
        );
        Some(Self {
            positions: plan
                .positions
                .into_iter()
                // A break coincident with a reference needs an explicit glyph-
                // ownership policy. Do not detach the number from its word or
                // give a hyphen the superscript number's style by accident.
                .filter(|at| {
                    !projection
                        .positions
                        .generated
                        .iter()
                        .any(|span| span.source == *at)
                })
                .map(|at| projection.positions.after(at))
                .collect(),
            words: plan
                .words
                .into_iter()
                .map(|word| {
                    projection.positions.before(word.start)..projection.positions.after(word.end)
                })
                .collect(),
        })
    }

    fn with_paragraphs(
        source: &Story,
        effective: &Story,
        styles: &StyleSet,
        default_paragraph: &str,
        default_character: &str,
    ) -> Self {
        let fallback = styles.resolve_character(default_character);
        let mut positions = Vec::new();
        for ((original, effective), at) in source
            .points
            .iter()
            .zip(&effective.points)
            .zip(source.point_offsets())
        {
            let (StoryPoint::Paragraph { text, .. }, StoryPoint::Paragraph { style, .. }) =
                (original, effective)
            else {
                continue;
            };
            let paragraph = styles.resolve_paragraph(if style.is_empty() {
                default_paragraph
            } else {
                style
            });
            let character = paragraph.character(fallback.clone());
            positions.extend(
                super::opportunities(source, at..at + text.len(), styles, &paragraph, &character)
                    .into_iter()
                    .map(|position| at + position),
            );
        }
        let words = source
            .text()
            .unicode_word_indices()
            .filter_map(|(at, word)| {
                let end = at + word.len();
                let first = positions.partition_point(|position| *position <= at);
                positions
                    .get(first)
                    .filter(|position| **position < end)
                    .map(|_| at..end)
            })
            .collect();
        Self { positions, words }
    }

    /// Transient offsets relative to a displayed slice. Both slice edges are
    /// excluded; a chosen line's terminal glyph is carried separately.
    pub fn slice(&self, range: Range<usize>) -> Vec<usize> {
        if range.start >= range.end {
            return Vec::new();
        }
        let start = self.positions.partition_point(|at| *at <= range.start);
        let end = self.positions.partition_point(|at| *at < range.end);
        self.positions[start..end]
            .iter()
            .map(|at| at - range.start)
            .collect()
    }

    /// The complete displayed word owning a selected automatic boundary.
    /// Includes inline references so denying its breaks cannot detach a marker.
    pub(crate) fn word_at(&self, at: usize) -> Option<Range<usize>> {
        let index = self.words.partition_point(|word| word.end <= at);
        self.words
            .get(index)
            .filter(|word| word.start < at && at < word.end)
            .cloned()
    }

    /// An eligible word still needs bounded placement after its last candidate
    /// or when all its candidates were withheld beside generated references.
    pub(crate) fn overlaps_word(&self, range: Range<usize>) -> bool {
        if range.start >= range.end {
            return false;
        }
        let index = self.words.partition_point(|word| word.end <= range.start);
        self.words
            .get(index)
            .is_some_and(|word| word.start < range.end)
    }
}
