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
    Paragraph(ParagraphStart),
}

impl Event {
    pub fn explicit(self) -> bool {
        matches!(self, Self::Explicit(_))
    }

    /// The first paragraph can start in its initial container. Odd/even
    /// policies still require the document's actual numbered-page parity.
    pub fn advance(self, previous: Option<Location>, here: Location) -> Option<Boundary> {
        let policy = match self {
            Self::Explicit(boundary) => return Some(boundary),
            Self::Paragraph(policy) => policy,
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
