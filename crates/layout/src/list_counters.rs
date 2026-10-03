//! Immutable source-order counters, with evidenced frame chronology for
//! same-page, unthreaded cross-story sequences. Balance trials never mutate them.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    list_numbering::CounterFormat,
    lists::{ListKind, ListStyle, NumberingFormat},
    LayoutDocument, LayoutObject, Story, StoryId, StoryPoint,
};

type Issue = &'static str;
type Marker = Result<(String, bool), Issue>;

/// Immutable, call-local numbering results. Rebuild after editing the story or
/// its styles or frame membership/chronology. Column balancing does not affect
/// the sequence. All source paragraphs, including overset, participate; native
/// overset agreement has not been established.
#[derive(Default)]
pub struct StoryCounters {
    markers: BTreeMap<usize, Marker>,
}

#[derive(Clone)]
struct Counter {
    event: usize,
    value: Result<u64, Issue>,
    format: Option<CounterFormat>,
}

impl StoryCounters {
    pub fn new(doc: &LayoutDocument, story: &Story) -> Self {
        let mut out = Self::pass(doc, &[story], None, story);
        let sequences: BTreeSet<_> = story
            .points
            .iter()
            .filter_map(|point| {
                let StoryPoint::Paragraph { style, .. } = point else {
                    return None;
                };
                let list = doc.styles.resolve_paragraph(style).list;
                (list.kind == Some(ListKind::Numbered)
                    && doc
                        .styles
                        .numbering_lists
                        .iter()
                        .any(|r| r.id == sequence_id(&list) && r.across_stories))
                .then(|| sequence_id(&list).to_owned())
            })
            .collect();
        for sequence in sequences {
            match continuation_stories(doc, story, &sequence) {
                Ok(stories) => out
                    .markers
                    .extend(Self::pass(doc, &stories, Some(&sequence), story).markers),
                Err(issue) => {
                    for (point, at) in story.points.iter().zip(story.point_offsets()) {
                        let StoryPoint::Paragraph { style, .. } = point else {
                            continue;
                        };
                        let list = doc.styles.resolve_paragraph(style).list;
                        if list.kind == Some(ListKind::Numbered) && sequence_id(&list) == sequence {
                            out.markers.insert(at, Err(issue));
                        }
                    }
                }
            }
        }
        out
    }

    fn pass(
        doc: &LayoutDocument,
        stories: &[&Story],
        sequence: Option<&str>,
        target: &Story,
    ) -> Self {
        let mut out = Self::default();
        let mut lists = BTreeMap::<String, [Option<Counter>; 9]>::new();
        // A monotonic event ordinal crosses story boundaries. Byte offsets
        // restart at zero in each story and cannot order ancestor restarts.
        let paragraphs = stories.iter().flat_map(|story| {
            story
                .points
                .iter()
                .zip(story.point_offsets())
                .filter_map(move |(point, at)| {
                    if let StoryPoint::Paragraph { style, .. } = point {
                        Some((*story, style, at))
                    } else {
                        None
                    }
                })
        });
        for (event, (story, style, at)) in paragraphs.enumerate() {
            let retain = std::ptr::eq(story, target);
            let list = doc.styles.resolve_paragraph(style).list;
            if list.kind != Some(ListKind::Numbered)
                || sequence.is_some_and(|id| sequence_id(&list) != id)
            {
                continue;
            }
            let level = list.level.unwrap_or(1);
            if !(1..=9).contains(&level) {
                if retain {
                    out.markers.insert(at, Err("NumberingLevel"));
                }
                continue;
            }
            let index = level as usize - 1;
            let levels = lists.entry(sequence_id(&list).into()).or_default();
            let previous = levels[index].as_ref();
            // Compare source events, not rendered numbers: restarting a parent
            // at the same value must still restart a child.
            let higher = levels[..index].iter().flatten().map(|c| c.event).max();
            let restart = list.apply_restart_policy != Some(false)
                && previous.is_some_and(|p| higher.is_some_and(|h| h > p.event));
            let value = if !supported_restart(&list) {
                Err("NumberingRestartPolicies")
            } else if list.start == Some(0) {
                Err("NumberingStartAt")
            } else if list.continue_numbering == Some(false) {
                Ok(u64::from(list.start.unwrap_or(1)))
            } else {
                match previous {
                    Some(_) if restart => Ok(1),
                    Some(previous) => previous
                        .value
                        .and_then(|v| v.checked_add(1).ok_or("NumberingCounterOverflow")),
                    None => Ok(u64::from(list.start.unwrap_or(1))),
                }
            };
            levels[index] = Some(Counter {
                event,
                value,
                format: counter_format(&list),
            });
            let marker = expand(list.expression.as_deref().unwrap_or("^#.^t"), |code| {
                let referenced = match code {
                    '#' => index,
                    '1'..='9' if (code as usize - '1' as usize) < index => {
                        code as usize - '1' as usize
                    }
                    _ => return Err("NumberingExpression"),
                };
                let counter = levels[referenced]
                    .as_ref()
                    .ok_or("NumberingExpression.MissingLevel")?;
                // A previously used ancestor from an earlier branch does not
                // supply a guessed value for a skipped level in this branch.
                if levels[..referenced]
                    .iter()
                    .flatten()
                    .any(|c| c.event > counter.event)
                {
                    return Err("NumberingExpression.MissingLevel");
                }
                counter
                    .format
                    .ok_or("NumberingFormat")?
                    .render(counter.value?)
            });
            // Even an expression without a substitution must diagnose a bad
            // sequence or out-of-range format instead of hiding the failure.
            let marker = value
                .and_then(|v| counter_format(&list).ok_or("NumberingFormat")?.render(v))
                .and(marker);
            if retain {
                out.markers.insert(at, marker);
            }
        }
        out
    }

    pub fn issue(&self, at: usize) -> Option<Issue> {
        self.markers.get(&at)?.as_ref().err().copied()
    }

    pub(crate) fn marker(&self, at: usize) -> Option<&(String, bool)> {
        self.markers.get(&at)?.as_ref().ok()
    }
}

/// The public native rule establishes creation order only for unthreaded
/// frames on one page. Missing chronology, multiple frame/template instances,
/// pages and book sequences stay explicit issues rather than guessed order.
fn continuation_stories<'a>(
    doc: &'a LayoutDocument,
    target: &Story,
    sequence: &str,
) -> Result<Vec<&'a Story>, Issue> {
    let resources: Vec<_> = doc
        .styles
        .numbering_lists
        .iter()
        .filter(|r| r.id == sequence)
        .collect();
    if resources.len() != 1 {
        return Err("ContinueNumbersAcrossStories.AmbiguousList");
    }
    if resources[0].across_documents {
        return Err("ContinueNumbersAcrossStories + ContinueNumbersAcrossDocuments");
    }
    if !doc.stories.iter().any(|story| std::ptr::eq(story, target)) {
        return Err("ContinueNumbersAcrossStories.UnknownStory");
    }
    let mut ordered = Vec::new();
    let mut page = None;
    let creation = doc.creation_ranks();
    let mut frames = BTreeMap::<StoryId, Vec<&crate::PlacedObject>>::new();
    for object in &doc.objects {
        if let LayoutObject::TextFrame { story, .. } = object.object {
            frames.entry(story).or_default().push(object);
        }
    }
    let parent_stories: BTreeSet<_> = doc
        .parents
        .iter()
        .flat_map(|p| &p.objects)
        .filter_map(|o| {
            if let LayoutObject::TextFrame { story, .. } = o.object.object {
                Some(story)
            } else {
                None
            }
        })
        .collect();
    let mut threads = BTreeMap::<StoryId, Vec<&[crate::ObjectId]>>::new();
    for (id, order) in &doc.thread_order {
        threads.entry(*id).or_default().push(order);
    }
    for (index, story) in doc.stories.iter().enumerate() {
        let uses_sequence = story.points.iter().any(|point| {
            let StoryPoint::Paragraph { style, .. } = point else {
                return false;
            };
            let list = doc.styles.resolve_paragraph(style).list;
            list.kind == Some(ListKind::Numbered) && sequence_id(&list) == sequence
        });
        if !uses_sequence {
            continue;
        }
        let id = StoryId(index as u32);
        if parent_stories.contains(&id) {
            return Err("ContinueNumbersAcrossStories.ParentFrames");
        }
        let story_frames = frames.get(&id).map_or(&[][..], Vec::as_slice);
        let Some(frame) = story_frames.first() else {
            // Unplaced stories do not appear in this document's list. Deleting
            // the last frame leaves its story available to undo; that tombstone
            // must not permanently prevent live frames from being numbered.
            if std::ptr::eq(story, target) {
                return Err("ContinueNumbersAcrossStories.MissingFrame");
            }
            continue;
        };
        if story_frames.len() != 1
            || threads
                .get(&id)
                .is_some_and(|orders| orders.iter().any(|order| *order != [frame.id]))
        {
            return Err("ContinueNumbersAcrossStories.ThreadedFrames");
        }
        if frame.page >= doc.pages.len() || page.is_some_and(|p| p != frame.page) {
            return Err("ContinueNumbersAcrossStories.PageOrder");
        }
        page = Some(frame.page);
        let rank = creation
            .get(&frame.id)
            .copied()
            .ok_or("ContinueNumbersAcrossStories.UnknownCreationOrder")?;
        ordered.push((rank, story));
    }
    ordered.sort_by_key(|entry| entry.0);
    Ok(ordered.into_iter().map(|(_, story)| story).collect())
}

/// Native sequence identity, including the default when AppliedNumberingList
/// is absent. Display names never merge distinct explicit sequence identities.
pub fn sequence_id(list: &ListStyle) -> &str {
    list.list
        .as_deref()
        .unwrap_or("NumberingList/$ID/[Default]")
}

pub(crate) fn counter_format(list: &ListStyle) -> Option<CounterFormat> {
    list.format.as_ref().map_or(
        Some(CounterFormat::Decimal),
        NumberingFormat::counter_format,
    )
}

pub(crate) fn supported_restart(list: &ListStyle) -> bool {
    list.apply_restart_policy == Some(false)
        || list
            .restart_policy
            .as_ref()
            .is_none_or(|p| p.policy == "AnyPreviousLevel" && p.lower == 0 && p.upper == 0)
}

/// Expand supported native escapes. A tab is the final marker/body separator.
pub(crate) fn expand(value: &str, mut number: impl FnMut(char) -> Result<String, Issue>) -> Marker {
    let mut out = String::new();
    let mut chars = value.chars();
    let mut tab = false;
    while let Some(c) = chars.next() {
        if tab {
            return Err("NumberingExpression");
        }
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next().ok_or("NumberingExpression")? {
            '^' => out.push('^'),
            't' => tab = true,
            code => out.push_str(&number(code)?),
        }
    }
    Ok((out, tab))
}
