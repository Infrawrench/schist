//! Trial layouts carry their own consecutive-line history and forbidden words.
//! A column-end prohibition is applied after paragraph keeps, never by hiding
//! a selected glyph or editing source text.
use super::*;
use std::ops::Range;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct History {
    pub at: usize,
    pub consecutive: usize,
}

impl History {
    pub fn advance(self, story: &Story, lines: &[ComposedLine], consumed: usize) -> Self {
        let text = story.text();
        let mut consecutive = self.consecutive;
        for line in lines
            .iter()
            .filter(|line| line.initial.is_none() && line.generated.is_none())
        {
            let literal = text
                .get(line.start..line.end)
                .is_some_and(|text| text.trim_end().ends_with(['-', '\u{2010}']));
            consecutive = if line.is_paragraph_end
                || line.forced_break
                || text
                    .get(line.end..)
                    .is_some_and(|rest| rest.starts_with('\n'))
            {
                0
            } else if line.generated_hyphen || line.discretionary_hyphen || literal {
                consecutive.saturating_add(1)
            } else {
                0
            };
        }
        Self {
            at: consumed,
            consecutive,
        }
    }
}

/// The source word that a forbidden column/frame tail would split. Generated
/// break positions always lie strictly inside one of these complete words.
pub(super) fn forbidden_word(
    source: &FlowSource<'_>,
    lines: &[ComposedLine],
) -> Option<Range<usize>> {
    let last = lines.last()?;
    if !last.generated_hyphen || last.paragraph.hyphenation.across_columns != Some(false) {
        return None;
    }
    // Generated boundaries came from this same plan. Complete source-word
    // ownership remains available even if inline references split display words.
    source.plan.word_at(last.end)
}

pub(super) fn column(
    source: &FlowSource<'_>,
    start: usize,
    end: usize,
    column: Rect,
    grid: Option<BaselineGrid>,
) -> (Vec<ComposedLine>, usize) {
    let mut denied = source.denied_hyphen_words.to_vec();
    loop {
        let attempt = FlowSource {
            denied_hyphen_words: &denied,
            ..*source
        };
        let (lines, next) = fill_column(&attempt, start, end, column, grid);
        let Some(word) = forbidden_word(source, &lines) else {
            return (lines, next);
        };
        // Each retry removes every automatic break in a distinct source word.
        // It may move that word to the next column; it cannot cross this edge.
        // `place_block` keeps an overlong denied word overset instead of
        // accepting an overwide line after its opportunities are removed.
        if denied.contains(&word) {
            // A broken source mapping must remain visibly overset, not loop or
            // paint a prohibited edge. Regression tests require this never to
            // occur for supported dictionary words and projections.
            log::error!("generated hyphen escaped a denied source word");
            return (Vec::new(), start);
        }
        denied.push(word);
    }
}
