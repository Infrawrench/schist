//! Tables breaking across columns and frames. Each table of a story is cut
//! into parts between whole rows, every part set on a line of its own. Where
//! the cuts fall depends on where composition puts each part, so a story
//! holding tables is composed again until its parts settle.
//!
//! InDesign's PDF of the public paged-media `tables-rows` sample shows the
//! rules: a table breaks between whole rows, the next part starting at the
//! top of the next column with the header rows repeated; a break never falls
//! after a row kept with the next one, unless nothing else fits a whole
//! column; and a row no column can hold leaves it and the rows after it
//! overset. Where header and footer rows repeat and the first and last
//! skips follow the public IDML specification. A break never falls inside a
//! cell spanning rows. A body row whose StartRow asks for the next column,
//! frame or page ends the part before it, and its part starts there, as the
//! specification describes and as a paragraph's start does; composition
//! moves the part on with a row event at its line (see `break_flow`).
use crate::compose::{ComposedLine, ComposedThread, InsetsLike};
use crate::styles::ParagraphStart;
use crate::tables::{Part, Parts, RepeatRows, Table, TableLayout};
use crate::{FrameOverflow, LayoutDocument, ObjectId, Pt, Rect, StoryId};
use std::collections::BTreeMap;

/// A column of the thread a part can be set in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Room {
    frame: usize,
    page: (usize, usize),
    /// The page's number, for odd and even page starts.
    number: u32,
    rect: Rect,
    last_in_frame: bool,
    last_on_page: bool,
}

/// Whether room `here` meets a part's StartRow after content in room
/// `previous`, as a paragraph's start does: a later column, another frame,
/// another page, another page with an odd or even number.
fn begins(policy: ParagraphStart, rooms: &[Room], previous: usize, here: usize) -> bool {
    let (before, room) = (&rooms[previous], &rooms[here]);
    let page = room.page != before.page;
    match policy {
        ParagraphStart::Anywhere => true,
        ParagraphStart::NextColumn => here > previous,
        ParagraphStart::NextFrame => room.frame != before.frame,
        ParagraphStart::NextPage => page,
        ParagraphStart::NextOddPage => page && room.number % 2 == 1,
        ParagraphStart::NextEvenPage => page && room.number % 2 == 0,
    }
}

/// What earlier passes learned: the room each part was planned for, and the
/// rooms a part did not fit as planned.
#[derive(Debug, Default)]
pub(crate) struct Flow {
    planned: BTreeMap<usize, Vec<usize>>,
    /// (structure, room): the tallest part the room may take, and how often
    /// a planned part did not fit there.
    caps: BTreeMap<(usize, usize), (Pt, u8)>,
}

/// The room of a part no column holds.
const NOWHERE: usize = usize::MAX;

/// The thread's columns in flow order, or None when the story has no typed
/// table.
pub(crate) fn rooms(
    doc: &LayoutDocument,
    story: StoryId,
    frames: &[(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)],
) -> Option<Vec<Room>> {
    let source = doc.story(story)?;
    if !source.structures.iter().any(|s| s.table.is_some()) {
        return None;
    }
    let mut out = Vec::new();
    for (index, (object, bounds, _, count, gutter, insets)) in frames.iter().enumerate() {
        let page = page_of(doc, *object);
        let mut columns = crate::compose::columns(bounds.inset(insets.resolve()), *count, *gutter);
        if source.prefs.direction == crate::StoryDirection::RightToLeft {
            columns.reverse();
        }
        let last = columns.len().saturating_sub(1);
        for (column, rect) in columns.into_iter().enumerate() {
            out.push(Room {
                frame: index,
                page,
                number: doc.page_number_value(page.1),
                rect,
                last_in_frame: column == last,
                last_on_page: false,
            });
        }
    }
    for index in 0..out.len() {
        let page = out[index].page;
        out[index].last_on_page = out.get(index + 1).is_none_or(|next| next.page != page);
    }
    Some(out)
}

/// The page an object is on, keyed as composition keys it: parent pages
/// after the document's.
fn page_of(doc: &LayoutDocument, object: ObjectId) -> (usize, usize) {
    doc.object(object)
        .map(|o| (0, o.page))
        .or_else(|| {
            doc.parents.iter().enumerate().find_map(|(index, parent)| {
                parent
                    .objects
                    .iter()
                    .find(|entry| entry.object.id == object)
                    .map(|entry| (index + 1, entry.object.page))
            })
        })
        .unwrap_or((0, 0))
}

/// The room of frame `frame` that holds `line`.
fn room_of(rooms: &[Room], frame: usize, line: &ComposedLine) -> Option<usize> {
    let middle = line.bounds.x + line.bounds.width / 2.0;
    let mut first = None;
    for (index, room) in rooms.iter().enumerate().filter(|(_, r)| r.frame == frame) {
        if middle >= room.rect.x - 0.01 && middle <= room.rect.right() + 0.01 {
            return Some(index);
        }
        first.get_or_insert(index);
    }
    first
}

/// Whether `line` holds part `number` of the table at `structure`.
fn holds(line: &ComposedLine, structure: usize, number: Option<usize>) -> bool {
    line.projected.as_ref().is_some_and(|p| {
        p.tables
            .iter()
            .any(|set| set.structure == structure && number.is_none_or(|n| set.index == n))
    })
}

/// The parts each table of `story` breaks into, from where `thread` set the
/// parts it was composed with. Tables set whole are left out.
pub(crate) fn plan(
    doc: &LayoutDocument,
    story: StoryId,
    rooms: &[Room],
    thread: &ComposedThread,
    current: &Parts,
    flow: &mut Flow,
) -> Parts {
    let mut out = Parts::new();
    let Some(source) = doc.story(story) else {
        return out;
    };
    if rooms.is_empty() {
        return out;
    }
    // The lines in flow order, each with its room.
    let mut lines = Vec::new();
    for (frame, composed) in thread.frames.iter().enumerate() {
        for line in &composed.lines {
            if let Some(room) = room_of(rooms, frame, line) {
                lines.push((room, line));
            }
        }
    }
    // How far a line holding a part reaches below its box, which is set on
    // the baseline.
    let extra = lines
        .iter()
        .filter(|(_, line)| {
            line.projected
                .as_ref()
                .is_some_and(|p| !p.tables.is_empty())
        })
        .map(|(_, line)| (line.bounds.bottom() - line.baseline).max(0.0))
        .fold(0.0, Pt::max);
    for (index, structure) in source.structures.iter().enumerate() {
        let Some(table) = structure.table.as_deref() else {
            continue;
        };
        let Some(layout) = crate::tables::layout(doc, table, 0) else {
            continue;
        };
        let parts = current
            .get(&index)
            .cloned()
            .unwrap_or_else(|| vec![table.whole()]);
        // Where each part was set: its room and line.
        let set: Vec<Option<(usize, usize)>> = (0..parts.len())
            .map(|number| {
                let at = lines
                    .iter()
                    .position(|(_, line)| holds(line, index, Some(number)))?;
                Some((lines[at].0, at))
            })
            .collect();
        learn(flow, index, table, &layout, &parts, &set);
        // The line before the table, outside it.
        let first = set.first().copied().flatten();
        let before = lines[..first.map_or(lines.len(), |(_, at)| at)]
            .iter()
            .rev()
            .find(|(_, line)| !line.is_generated() && !holds(line, index, None));
        let expected = before.map_or(0, |(room, _)| *room);
        let start = match first {
            // Set just after the line before it: exactly the room left there.
            Some((room, at)) if room == expected => {
                let rect = rooms[room].rect;
                let top = lines[at].1.baseline - layout.part_height(table, &parts[0]);
                (room, rect.bottom() - top, (top - rect.y).abs() < 0.01)
            }
            // Moved on, or overset: the room below the line before it and
            // the table's space before it.
            _ => match before {
                Some((room, line)) => {
                    let rect = rooms[*room].rect;
                    let mut top = line.bounds.bottom() + table.space_before.max(0.0);
                    if line.is_paragraph_end {
                        top += line.paragraph.space_after.unwrap_or(0.0).max(0.0);
                    }
                    (*room, rect.bottom() - top, false)
                }
                None => (0, rooms[0].rect.height, true),
            },
        };
        // A table whose first row asks to start later moves on from the
        // line before it, as a paragraph's start does; with nothing before
        // it, it starts where it is.
        let start = match (table.start(table.body().start), before) {
            (Some(policy), Some((room, _))) => {
                match (*room..rooms.len()).find(|&k| begins(policy, rooms, *room, k)) {
                    Some(k) => (k, rooms[k].rect.height, true),
                    None => (rooms.len(), 0.0, true),
                }
            }
            _ => start,
        };
        let caps = &flow.caps;
        let cap = |room: usize| caps.get(&(index, room)).map_or(Pt::INFINITY, |c| c.0);
        let planned = split(table, &layout, rooms, start, cap, extra);
        flow.planned
            .insert(index, planned.iter().map(|(_, room)| *room).collect());
        let parts: Vec<Part> = planned.into_iter().map(|(part, _)| part).collect();
        if parts != [table.whole()] {
            out.insert(index, parts);
        }
    }
    out
}

/// The first part set later than planned did not fit its room: cap that
/// room just below the part, and close it the second time.
fn learn(
    flow: &mut Flow,
    structure: usize,
    table: &Table,
    layout: &TableLayout,
    parts: &[Part],
    set: &[Option<(usize, usize)>],
) {
    let Some(planned) = flow.planned.get(&structure) else {
        return;
    };
    for (number, room) in planned.iter().enumerate() {
        let (Some(part), true) = (parts.get(number), *room != NOWHERE) else {
            return;
        };
        match set.get(number).copied().flatten() {
            Some((landed, _)) if landed == *room => continue,
            // Set earlier than planned: the next plan measures it.
            Some((landed, _)) if landed < *room => return,
            _ => {}
        }
        let height = layout.part_height(table, part);
        let cap = flow
            .caps
            .entry((structure, *room))
            .or_insert((Pt::INFINITY, 0));
        cap.1 += 1;
        cap.0 = if cap.1 >= 2 {
            0.0
        } else {
            cap.0.min(height - 0.01)
        };
        return;
    }
}

/// Whether rows repeating by `rule` show again in `here` after a part in
/// `previous`.
fn repeats(rule: RepeatRows, previous: &Room, here: &Room) -> bool {
    match rule {
        RepeatRows::EveryColumn => true,
        RepeatRows::OncePerFrame => previous.frame != here.frame,
        RepeatRows::OncePerPage => previous.page != here.page,
    }
}

/// Cut the body rows into parts filling the rooms from `start` (its room,
/// the height left there, and whether that is the column's top), each with
/// the room it is planned for. What no room holds is one last part, overset.
fn split(
    table: &Table,
    layout: &TableLayout,
    rooms: &[Room],
    (start, first, top): (usize, Pt, bool),
    cap: impl Fn(usize) -> Pt,
    extra: Pt,
) -> Vec<(Part, usize)> {
    let body = table.body();
    if body.is_empty() {
        return vec![(table.whole(), start)];
    }
    let mut out = Vec::new();
    let mut next = body.start;
    let mut previous: Option<usize> = None;
    let (mut room, mut height, mut fresh) = (start, first, top);
    while next < body.end {
        let Some(here) = rooms.get(room) else {
            break;
        };
        let header = table.header_rows > 0
            && match previous {
                None => !table.skip_first_header,
                Some(p) => repeats(table.header_repeat, &rooms[p], here),
            };
        let footer = |end: usize| {
            table.footer_rows > 0
                && if end == body.end {
                    !table.skip_last_footer
                } else {
                    match table.footer_repeat {
                        RepeatRows::EveryColumn => true,
                        RepeatRows::OncePerFrame => here.last_in_frame,
                        RepeatRows::OncePerPage => here.last_on_page,
                    }
                }
        };
        let part = |end: usize| Part {
            header,
            start: next,
            end,
            footer: footer(end),
        };
        let room_height = height.min(cap(room)) - extra;
        let fits = |end: usize| layout.part_height(table, &part(end)) <= room_height + 0.001;
        // A row asking to start later ends the part before it, kept rows
        // above it included.
        let limit = (next + 1..body.end)
            .find(|&row| table.start(row).is_some())
            .unwrap_or(body.end);
        let breaks = |end: usize, keeps: bool| {
            end == limit || (!table.joined(end) && !(keeps && table.rows[end - 1].keep_with_next))
        };
        let best = |keeps: bool| {
            (next + 1..=limit)
                .rev()
                .find(|&end| breaks(end, keeps) && fits(end))
        };
        // Keeps give way only where nothing else would fill a whole column.
        if let Some(end) = best(true).or_else(|| if fresh { best(false) } else { None }) {
            out.push((part(end), room));
            next = end;
            previous = Some(room);
        }
        room += 1;
        // The next part starts as its first row asks, after the last part.
        if let (Some(policy), Some(last)) =
            (table.start(next).filter(|_| next < body.end), previous)
        {
            while room < rooms.len() && !begins(policy, rooms, last, room) {
                room += 1;
            }
        }
        height = rooms.get(room).map_or(0.0, |r| r.rect.height);
        fresh = true;
    }
    if next < body.end {
        out.push((
            Part {
                header: table.header_rows > 0 && (previous.is_some() || !table.skip_first_header),
                start: next,
                end: body.end,
                footer: table.footer_rows > 0 && !table.skip_last_footer,
            },
            NOWHERE,
        ));
    }
    out
}
