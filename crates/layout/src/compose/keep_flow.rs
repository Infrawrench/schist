//! Bind adjacent paragraphs after shaping. Only complete line suffixes move;
//! rollback also respects the preceding paragraph's widow/orphan policy.
use super::{blocks, ComposedLine, FlowSource, Point};

pub(super) fn enforce(
    source: &FlowSource<'_>,
    start: usize,
    end: usize,
    lines: &mut Vec<ComposedLine>,
    mut cursor: usize,
) -> usize {
    let blocks = blocks(source.story, start, end);
    let breaks: Vec<_> = source
        .story
        .points
        .iter()
        .zip(source.story.point_offsets())
        .filter_map(|(point, at)| {
            matches!(
                point,
                Point::ColumnBreak
                    | Point::FrameBreak
                    | Point::PageBreak
                    | Point::OddPageBreak
                    | Point::EvenPageBreak
            )
            .then_some(at)
        })
        .collect();
    // A later rollback can invalidate an earlier heading's binding. Visiting
    // boundaries backwards propagates that change through an arbitrary chain.
    for pair in blocks.windows(2).rev() {
        let (before, after) = (&pair[0], &pair[1]);
        if cursor < before.end
            || breaks
                .iter()
                .any(|at| before.end <= *at && *at <= after.start)
        {
            continue;
        }
        let previous = source.doc.styles.resolve_paragraph(&before.style).keeps;
        let after_style = source.doc.styles.resolve_paragraph(&after.style);
        if !matches!(
            after_style.start_paragraph,
            None | Some(crate::styles::ParagraphStart::Anywhere)
        ) {
            continue;
        }
        let following = after_style.keeps;
        let required = previous
            .next
            .unwrap_or(0)
            .max(usize::from(following.previous == Some(true)));
        if required == 0 || cursor >= after.end {
            continue;
        }
        let boundary = lines.partition_point(|line| line.start < after.start);
        let count = lines[boundary..]
            .iter()
            .filter(|line| line.initial.is_none())
            .count();
        if count >= required {
            continue;
        }
        let first = lines.partition_point(|line| line.start < before.start);
        let body: Vec<_> = (first..boundary)
            .filter(|i| lines[*i].initial.is_none())
            .collect();
        if body.is_empty() {
            continue;
        }
        let retain = previous.fitting_lines(
            body.len() - 1,
            body.len(),
            super::holds_table(source, before),
        );
        let mut cut = if retain == 0 { first } else { body[retain] };
        // An opening initial reserves its complete inset body area. Moving
        // any of those lines moves the opening too, never a partial drop cap.
        if cut > first && lines[first].initial.is_some() && lines[cut].drop_cap {
            cut = first;
        }
        cursor = if cut == first {
            before.start
        } else {
            lines[cut].start
        };
        lines.truncate(cut);
    }
    cursor.max(start)
}
