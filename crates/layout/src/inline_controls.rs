//! Zero-width source controls. The kernel consumes typed data, never recovery XML.
use crate::{story::InlineControl, ResolvedParagraph, Story, StoryPoint, StyleSet};

fn supported(paragraph: &ResolvedParagraph) -> bool {
    // The native interaction with enlarged/one-line initials is not established.
    // Preserve and diagnose those controls instead of guessing initial counts.
    paragraph.drop_caps_lines.unwrap_or(0) == 0 || paragraph.drop_caps_characters.unwrap_or(1) == 0
}

pub(crate) fn end_markers(
    story: &Story,
    start: usize,
    text: &str,
    paragraph: &ResolvedParagraph,
) -> Vec<usize> {
    if !supported(paragraph) {
        return Vec::new();
    }
    let mut anchors: Vec<_> = story
        .structures
        .iter()
        .filter_map(|structure| {
            if structure.kind != "ProcessingInstruction"
                || structure.footnote.is_some()
                || !matches!(
                    structure.control,
                    Some(InlineControl::EndNestedStyle { .. })
                )
            {
                return None;
            }
            let at = structure.at?.checked_sub(start)?;
            (at <= text.len()).then_some(at)
        })
        .collect();
    if anchors.is_empty() {
        return anchors;
    }
    let boundaries: std::collections::BTreeSet<_> =
        schist_text_engine::grapheme_boundaries(text).collect();
    anchors.retain(|at| boundaries.contains(at));
    anchors.sort();
    anchors
}

/// Controls outside a paragraph, inside a grapheme, or using unsupported initial
/// geometry remain unrendered. Unknown locations are never assigned a new one.
pub(crate) fn unrendered(story: &Story, styles: &StyleSet) -> usize {
    if !story.structures.iter().any(|s| s.control.is_some()) {
        return story.retained_structures();
    }
    let handled = story
        .points
        .iter()
        .zip(story.point_offsets())
        .map(|(point, start)| {
            let StoryPoint::Paragraph { text, style } = point else {
                return 0;
            };
            end_markers(story, start, text, &styles.resolve_paragraph(style)).len()
        })
        .sum::<usize>();
    story.retained_structures().saturating_sub(handled)
}
