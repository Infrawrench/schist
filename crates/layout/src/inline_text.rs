//! Compose generated inline text without inserting it into an editable story.
//!
//! A footnote number occupies real typographic space but owns no source bytes.
//! This projection keeps the two coordinate systems explicit. It is disposable
//! composition data: neither the projected story nor its positions are saved.
use crate::{Story, StoryPoint, StyleRange};

/// A composed projection retains its resolved runs after temporary styles go
/// away. Paint definitions stay typed so native spot inks survive printing.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub spec: schist_text_engine::TextSpec,
    pub paints: Vec<crate::ResolvedCharacter>,
    /// Full projected paragraph, shared by its lines for contextual preflight.
    pub context: std::sync::Arc<str>,
    /// Numbering diagnostics belong to this projection, not its source anchor.
    pub counter_issue: Option<&'static str>,
    /// None identifies generated content outside the editable main story.
    pub positions: Option<LinePositions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    pub at: usize,
    pub text: String,
    pub style: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSpan {
    pub source: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub story: Story,
    pub positions: SourceMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    source: String,
    pub generated: Vec<GeneratedSpan>,
}

/// A rendered line's UTF-8 boundaries mapped to absolute source positions.
/// Generated glyph boundaries all map to the insertion's source anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePositions {
    boundaries: Vec<(usize, usize)>,
}

impl LinePositions {
    /// Hit tests can return any byte. Always return a source character boundary.
    pub fn source(&self, visual: usize) -> usize {
        let index = self.boundaries.partition_point(|(at, _)| *at <= visual);
        self.boundaries[index.saturating_sub(1)].1
    }

    /// A source insertion is before generated text at that boundary, matching
    /// Story::replace_text's right-affinity structure anchors.
    pub fn visual(&self, source: usize) -> usize {
        let index = self.boundaries.partition_point(|(_, at)| *at < source);
        self.boundaries[index.min(self.boundaries.len() - 1)].0
    }
}

impl SourceMap {
    /// Immutable source behind the disposable display projection.
    pub fn original_text(&self) -> &str {
        &self.source
    }

    pub fn source(&self, visual: usize) -> usize {
        let mut added = 0;
        for span in &self.generated {
            if visual < span.start {
                break;
            }
            if visual <= span.end {
                return span.source;
            }
            added += span.end - span.start;
        }
        crate::story::floor_char_boundary(
            &self.source,
            visual.saturating_sub(added).min(self.source.len()),
        )
    }

    /// Original caret positions precede generated text at the same anchor.
    pub fn before(&self, source: usize) -> usize {
        self.visual(source, false)
    }

    /// Source glyphs and right-affinity structures follow generated text.
    pub fn after(&self, source: usize) -> usize {
        self.visual(source, true)
    }

    fn visual(&self, source: usize, after: bool) -> usize {
        let source = crate::story::floor_char_boundary(&self.source, source.min(self.source.len()));
        source
            + self
                .generated
                .iter()
                .take_while(|span| span.source < source || (after && span.source == source))
                .map(|span| span.end - span.start)
                .sum::<usize>()
    }

    pub fn line(&self, text: &str, start: usize, end: usize) -> Option<LinePositions> {
        let slice = text.get(start..end)?;
        Some(LinePositions {
            boundaries: slice
                .char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(slice.len()))
                .map(|at| (at, self.source(start + at)))
                .collect(),
        })
    }
}

impl Projection {
    /// Preserve source paragraph boundaries, forced breaks, style precedence
    /// and structure order. Unknown/interior anchors are errors, never guessed.
    /// Coincident insertions retain the caller's order. Newlines are not inline
    /// content: callers must model a new paragraph or forced break explicitly.
    pub fn new(source: &Story, mut insertions: Vec<Insertion>) -> Option<Self> {
        let text = source.text();
        let offsets = source.point_offsets();
        if insertions
            .iter()
            .any(|i| !text.is_char_boundary(i.at) || i.text.contains(['\r', '\n']))
            || source.ranges.iter().any(|r| {
                r.start > r.end || !text.is_char_boundary(r.start) || !text.is_char_boundary(r.end)
            })
            || source
                .structures
                .iter()
                .filter_map(|s| s.at)
                .any(|at| !text.is_char_boundary(at))
        {
            return None;
        }
        insertions.sort_by_key(|i| i.at);
        let mut owners = Vec::new();
        let mut spans = Vec::new();
        let mut added = 0;
        for insertion in &insertions {
            let owner = source.points.iter().zip(&offsets).position(|(point, at)| {
                matches!(point, StoryPoint::Paragraph { .. })
                    && *at <= insertion.at
                    && insertion.at <= *at + point.text().len()
            })?;
            owners.push(owner);
            spans.push(GeneratedSpan {
                source: insertion.at,
                start: insertion.at + added,
                end: insertion.at + added + insertion.text.len(),
            });
            added += insertion.text.len();
        }
        let positions = SourceMap {
            source: text,
            generated: spans,
        };
        let mut story = source.clone();
        for (index, point) in story.points.iter_mut().enumerate() {
            let StoryPoint::Paragraph { text, .. } = point else {
                continue;
            };
            let original = std::mem::take(text);
            let mut cursor = 0;
            for (insertion, owner) in insertions.iter().zip(&owners) {
                if *owner != index {
                    continue;
                }
                let at = insertion.at - offsets[index];
                text.push_str(&original[cursor..at]);
                text.push_str(&insertion.text);
                cursor = at;
            }
            text.push_str(&original[cursor..]);
        }
        story.ranges = positions
            .generated
            .iter()
            .zip(&insertions)
            .filter(|(span, _)| span.start < span.end)
            .map(|(span, insertion)| StyleRange::new(span.start, span.end, &insertion.style))
            .collect();
        for range in &source.ranges {
            let mut cursor = range.start;
            for at in insertions
                .iter()
                .map(|i| i.at)
                .filter(|at| *at > range.start && *at < range.end)
                .chain(std::iter::once(range.end))
            {
                if cursor < at {
                    story.ranges.push(StyleRange::new(
                        positions.after(cursor),
                        positions.before(at),
                        &range.style,
                    ));
                }
                cursor = at;
            }
        }
        for structure in &mut story.structures {
            structure.at = structure.at.map(|at| positions.after(at));
        }
        Some(Self { story, positions })
    }
}
