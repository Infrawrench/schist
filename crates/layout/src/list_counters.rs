//! One story-order counter pass, independent of frames and balance trials.
use std::collections::BTreeMap;

use crate::{
    list_numbering::CounterFormat,
    lists::{ListKind, ListStyle, NumberingFormat},
    LayoutDocument, Story, StoryPoint,
};

type Issue = &'static str;
type Marker = Result<(String, bool), Issue>;

/// Immutable, call-local numbering results. Rebuild after editing the story or
/// its styles; frame geometry and column balancing do not affect the sequence.
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
        let mut out = Self::default();
        let mut lists = BTreeMap::<String, [Option<Counter>; 9]>::new();
        for (point, at) in story.points.iter().zip(story.point_offsets()) {
            let StoryPoint::Paragraph { style, .. } = point else {
                continue;
            };
            let list = doc.styles.resolve_paragraph(style).list;
            if list.kind != Some(ListKind::Numbered) {
                continue;
            }
            let level = list.level.unwrap_or(1);
            if !(1..=9).contains(&level) {
                out.markers.insert(at, Err("NumberingLevel"));
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
                event: at,
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
            out.markers.insert(at, marker);
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
