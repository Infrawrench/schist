//! Explicit breaks advance unconditionally. Paragraph starts are constraints:
//! an earlier break or natural flow can already have reached their boundary.
use super::{LayoutDocument, Point, Story};
use crate::styles::ParagraphStart;

#[derive(Clone, Copy)]
pub(super) struct Location {
    pub frame: usize,
    pub column: usize,
    pub page: (usize, usize),
    pub number: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Boundary {
    Column,
    Frame,
    Page,
}

#[derive(Clone, Copy)]
pub(super) enum Event {
    Explicit(Boundary),
    /// Unlike a paragraph constraint this must leave the event's own page,
    /// even at the beginning of a story or after another zero-width break.
    NumberedPage {
        odd: bool,
    },
    Paragraph(ParagraphStart),
    /// A table part whose first row starts as its StartRow asks: a
    /// constraint like a paragraph's, at the line break before the part,
    /// which is passed over once the constraint is met.
    Row(ParagraphStart),
}

impl Event {
    pub fn explicit(self) -> bool {
        !matches!(self, Self::Paragraph(_) | Self::Row(_))
    }

    /// Whether a line break at the event's position is passed over once the
    /// event is met, so a part moved on starts its column without an empty
    /// line above it.
    pub fn passes_break(self) -> bool {
        matches!(self, Self::Row(_))
    }

    pub fn unconditional(self) -> bool {
        matches!(self, Self::Explicit(_))
    }

    pub fn numbered_page(self) -> bool {
        matches!(self, Self::NumberedPage { .. })
    }

    /// The first paragraph can start in its initial container. Odd/even
    /// policies still require the document's actual numbered-page parity.
    pub fn advance(self, previous: Option<Location>, here: Location) -> Option<Boundary> {
        let policy = match self {
            Self::Explicit(boundary) => return Some(boundary),
            Self::NumberedPage { odd } => {
                return (previous.is_some_and(|p| p.page == here.page)
                    || here.number.is_multiple_of(2) == odd)
                    .then_some(Boundary::Page)
            }
            Self::Paragraph(policy) | Self::Row(policy) => policy,
        };
        let same_frame = previous.is_some_and(|p| p.frame == here.frame);
        let same_column = same_frame && previous.is_some_and(|p| p.column == here.column);
        let same_page = previous.is_some_and(|p| p.page == here.page);
        match policy {
            ParagraphStart::Anywhere => None,
            ParagraphStart::NextColumn => same_column.then_some(Boundary::Column),
            ParagraphStart::NextFrame => same_frame.then_some(Boundary::Frame),
            ParagraphStart::NextPage => same_page.then_some(Boundary::Page),
            ParagraphStart::NextOddPage => {
                (same_page || here.number.is_multiple_of(2)).then_some(Boundary::Page)
            }
            ParagraphStart::NextEvenPage => {
                (same_page || !here.number.is_multiple_of(2)).then_some(Boundary::Page)
            }
        }
    }
}

/// The line break a table part's line follows, passed over once its event
/// is met.
pub(super) const LINE_BREAK: &str = "\u{2028}";

/// The table parts of projected `story` whose first row asks to start in a
/// later column, frame or page, each at the start of its line: the line
/// break before its box when one ends the line above, else the box's
/// isolate.
pub(super) fn rows(
    story: &Story,
    boxes: &[crate::inline_text::ProjectedBox],
) -> Vec<(Event, usize)> {
    let starts: Vec<_> = boxes
        .iter()
        .filter_map(|set| {
            let (_, part) = set.part?;
            let table = story.structures.get(set.structure)?.table.as_deref()?;
            Some((table.start(part.start)?, set.at))
        })
        .collect();
    if starts.is_empty() {
        return Vec::new();
    }
    let text = story.text();
    starts
        .into_iter()
        .filter_map(|(policy, at)| {
            let lead = at.checked_sub('\u{2068}'.len_utf8())?;
            let at = match text.get(..lead) {
                Some(before) if before.ends_with(LINE_BREAK) => lead - LINE_BREAK.len(),
                _ => lead,
            };
            Some((Event::Row(policy), at))
        })
        .collect()
}

pub(super) fn events(doc: &LayoutDocument, story: &Story) -> Vec<(Event, usize)> {
    story
        .points
        .iter()
        .zip(story.point_offsets())
        .filter_map(|(point, at)| {
            let event = match point {
                Point::ColumnBreak => Event::Explicit(Boundary::Column),
                Point::FrameBreak => Event::Explicit(Boundary::Frame),
                Point::PageBreak => Event::Explicit(Boundary::Page),
                Point::OddPageBreak => Event::NumberedPage { odd: true },
                Point::EvenPageBreak => Event::NumberedPage { odd: false },
                Point::Paragraph { style, .. } => {
                    let policy = doc.styles.resolve_paragraph(style).start_paragraph?;
                    if policy == ParagraphStart::Anywhere {
                        return None;
                    }
                    Event::Paragraph(policy)
                }
                _ => return None,
            };
            Some((event, at))
        })
        .collect()
}
