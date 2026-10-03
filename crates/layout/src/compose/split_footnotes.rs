//! Main-story and note cursors advance independently. Trial layouts own both;
//! a rejected footer must not consume a reference, a break, or note text.
use super::*;
use crate::footnote_composition::{NoteArea, PreparedNote, PreparedStory};
use crate::footnotes::{FootnoteFirstBaseline, FootnoteOptions};

pub(super) struct Flow<'a> {
    prepared: &'a PreparedStory,
    notes: Vec<NoteFlow>,
    offsets: Vec<NoteCursor>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct NoteCursor {
    offset: usize,
    hyphens: hyphenation_flow::History,
}

struct NoteFlow {
    plan: crate::hyphenation::BreakPlan,
    doc: LayoutDocument,
    markers: crate::list_composition::MarkerPlans,
    context: footnote_flow::ProjectionContext,
    end: usize,
}

pub(super) struct Frame<'a> {
    pub area: Rect,
    pub columns: &'a [Rect],
    pub end: usize,
    pub breaks: &'a [(break_flow::Event, usize)],
    pub location: break_flow::Location,
    pub grid: Option<BaselineGrid>,
    pub options: &'a FootnoteOptions,
    pub spanning: bool,
    pub balance: bool,
}

impl<'a> Flow<'a> {
    pub(super) fn new(doc: &LayoutDocument, prepared: &'a PreparedStory) -> Self {
        let notes = prepared.notes.iter().map(|note| {
            let mut scratch = LayoutDocument::new(Vec::new());
            scratch.styles = doc.styles.clone();
            scratch.default_paragraph_style.clone_from(&doc.default_paragraph_style);
            scratch.default_character_style.clone_from(&doc.default_character_style);
            scratch.stories.push(note.body.story.clone());
            let story = &scratch.stories[0];
            let end = story.text_len() + usize::from(matches!(story.points.last(), Some(Point::Paragraph { text, .. }) if text.is_empty()));
            NoteFlow {
                plan: note.hyphenation.clone(),
                markers: note.markers.clone(),
                context: footnote_flow::ProjectionContext::new(&scratch, story),
                doc: scratch,
                end,
            }
        }).collect();
        Self {
            prepared,
            notes,
            offsets: vec![NoteCursor::default(); prepared.notes.len()],
        }
    }

    pub(super) fn checkpoint(&self) -> Vec<NoteCursor> {
        self.offsets.clone()
    }
    pub(super) fn restore(&mut self, saved: &[NoteCursor]) {
        self.offsets.copy_from_slice(saved);
    }

    pub(super) fn pending(&self, main: usize) -> bool {
        self.prepared.notes.iter().enumerate().any(|(index, note)| {
            note.reference.start < main && self.offsets[index].offset < self.notes[index].end
        })
    }

    pub(super) fn fill_frame(
        &mut self,
        source: &FlowSource<'_>,
        start: FlowCursor,
        frame: Frame<'_>,
    ) -> ColumnFlow {
        let run = |height: Pt| {
            let mut offsets = self.offsets.clone();
            let mut body = empty(start);
            let areas: Vec<_> = if frame.spanning {
                vec![frame.area]
            } else {
                frame.columns.to_vec()
            };
            for (area_index, area) in areas.into_iter().enumerate() {
                let columns = if frame.spanning {
                    frame.columns
                } else {
                    std::slice::from_ref(&area)
                };
                let before = body.next;
                let part = self.fill_area(source, &mut offsets, area, frame.options, |limit| {
                    fill_columns(
                        source.story,
                        before,
                        frame.end,
                        columns,
                        frame.breaks,
                        break_flow::Location {
                            column: area_index,
                            ..frame.location
                        },
                        |from, to, column, history| {
                            let (lines, next) = fill_column_with_rules(
                                &source.with_hyphens(history),
                                from,
                                to,
                                Rect {
                                    height: limit.min(height),
                                    ..column
                                },
                                frame.grid,
                            );
                            (lines, next, Vec::new())
                        },
                    )
                });
                body.next = part.next;
                body.stop_frame = part.stop_frame;
                body.stop_page = part.stop_page;
                body.lines.extend(part.lines);
                body.footnotes.extend(part.footnotes);
                if body.stop_frame {
                    break;
                }
            }
            (body, offsets)
        };
        let mut high = frame.area.height;
        let mut best = run(high);
        let complete = |result: &(ColumnFlow, Vec<NoteCursor>)| {
            result.0.next.offset >= frame.end
                && result
                    .1
                    .iter()
                    .zip(&self.notes)
                    .all(|(offset, note)| offset.offset >= note.end)
        };
        if frame.balance && complete(&best) {
            let mut low = 0.0;
            for _ in 0..16 {
                if high - low <= 0.05 {
                    break;
                }
                let middle = (low + high) * 0.5;
                let trial = run(middle);
                if complete(&trial) {
                    high = middle;
                    best = trial;
                } else {
                    low = middle;
                }
            }
        }
        self.offsets = best.1;
        best.0
    }

    /// Reserve whole notes where possible. A note splits only when its legal
    /// prefix and the reference line exhaust the available area. Every newly
    /// referenced note must start here; minimum legal prefixes are reserved
    /// before distributing remaining room in source order.
    fn fill_area(
        &self,
        source: &FlowSource<'_>,
        offsets: &mut [NoteCursor],
        area: Rect,
        options: &FootnoteOptions,
        fill_body: impl Fn(Pt) -> ColumnFlow,
    ) -> ColumnFlow {
        let mut ceiling = area.height;
        let mut minima = vec![None; self.notes.len()];
        for _ in 0..self.notes.len() + 2 {
            let body = fill_body(ceiling);
            let selected: Vec<_> = self
                .prepared
                .notes
                .iter()
                .enumerate()
                .filter(|(index, note)| {
                    note.reference.start < body.next.offset
                        && offsets[*index].offset < self.notes[*index].end
                })
                .map(|(index, _)| index)
                .collect();
            if selected.is_empty() {
                return body;
            }
            // A long affixed reference can wrap across several body lines.
            // Its note may start only after the entire generated range fits;
            // otherwise a later frame would inherit half a reference number.
            if let Some(note) = selected
                .iter()
                .map(|index| &self.prepared.notes[*index])
                .find(|note| note.reference.end > body.next.offset)
            {
                if let Some(line) = footnote_flow::reference_line(&body.lines, note) {
                    ceiling = ceiling.min(line.bounds.bottom() - area.y - 0.001).max(0.0);
                    continue;
                }
                return fill_body(0.0);
            }
            let required = selected
                .iter()
                .filter(|index| offsets[**index].offset == 0)
                .map(|index| self.prepared.notes[*index].reference.end)
                .max();
            // The minimum body height retaining these references. Full paragraph
            // shaping and keep rules remain authoritative in every trial.
            let mut minimum = if required.is_some() {
                body
            } else {
                fill_body(0.0)
            };
            if let Some(required) = required {
                let mut low = 0.0;
                let mut high = ceiling;
                for _ in 0..20 {
                    if high - low <= 0.001 {
                        break;
                    }
                    let middle = (low + high) * 0.5;
                    let trial = fill_body(middle);
                    if trial.next.offset >= required {
                        high = middle;
                        minimum = trial;
                    } else {
                        low = middle;
                    }
                }
            }
            let body_height = minimum
                .lines
                .iter()
                .map(|line| line.bounds.bottom() - area.y)
                .fold(0.0, Pt::max);
            let gap = if minimum.lines.is_empty() {
                0.0
            } else {
                options.spacer.unwrap_or(0.0)
            };
            let between = options.space_between.unwrap_or(0.0);
            let room = (area.height - body_height - gap).max(0.0);
            for index in &selected {
                minima[*index].get_or_insert_with(|| {
                    self.notes[*index].minimum(offsets[*index], area, options)
                });
            }
            let minimum_height: Option<Pt> =
                selected.iter().map(|index| minima[*index].unwrap()).sum();
            let fits = minimum_height.is_some_and(|height| {
                height + between * selected.len().saturating_sub(1) as Pt <= room + 0.001
            });
            if !fits {
                // Evict the last new reference line, preserving every earlier
                // trial cursor. If none can start, a later larger frame may fit.
                let last = selected
                    .iter()
                    .rev()
                    .find(|index| offsets[**index].offset == 0);
                if let Some(last) = last {
                    if let Some(line) =
                        footnote_flow::reference_line(&minimum.lines, &self.prepared.notes[*last])
                    {
                        ceiling = ceiling.min(line.bounds.bottom() - area.y - 0.001).max(0.0);
                        continue;
                    }
                }
                return fill_body(0.0);
            }
            let mut remaining = room;
            let mut reserve =
                minimum_height.unwrap() + between * selected.len().saturating_sub(1) as Pt;
            let mut areas = Vec::new();
            let mut next = offsets.to_vec();
            for index in &selected {
                reserve -= minima[*index].unwrap().unwrap();
                let Some((fragment, consumed)) = self.notes[*index].fragment(
                    &self.prepared.notes[*index],
                    offsets[*index],
                    Rect {
                        height: (remaining - reserve).max(0.0) + 0.001,
                        ..area
                    },
                    options,
                ) else {
                    return fill_body(0.0);
                };
                remaining -= fragment.bounds.height + between;
                reserve -= between;
                next[*index] = consumed;
                areas.push(fragment);
            }
            let height = areas.iter().map(|note| note.bounds.height).sum::<Pt>()
                + between * areas.len().saturating_sub(1) as Pt;
            let mut body = fill_body(
                ceiling.min((area.height - height - options.spacer.unwrap_or(0.0)).max(0.0)),
            );
            if body.next.offset < minimum.next.offset {
                body = minimum;
            }
            let mut placement = options.clone();
            if offsets[selected[0]].offset > 0 {
                placement.rule = options.continuing_rule.clone();
                placement.rule.width.get_or_insert(288.0);
            }
            if selected
                .iter()
                .any(|index| next[*index].offset < self.notes[*index].end)
            {
                placement.end_of_story = Some(false);
            }
            if body.lines.is_empty() {
                placement.spacer = Some(0.0);
            }
            footnote_flow::place_areas(
                source,
                &body.lines,
                body.next.offset,
                area,
                &placement,
                &mut areas,
            );
            offsets.copy_from_slice(&next);
            body.footnotes = areas;
            return body;
        }
        fill_body(0.0)
    }
}

impl NoteFlow {
    fn lines(
        &self,
        start: NoteCursor,
        area: Rect,
        options: &FootnoteOptions,
    ) -> Option<(Vec<ComposedLine>, usize, Pt)> {
        let source = FlowSource {
            doc: &self.doc,
            story: &self.doc.stories[0],
            markers: &self.markers,
            notes: None,
            plan: &self.plan,
            hyphens: start.hyphens,
            denied_hyphen_words: &[],
        };
        let bounds = Rect::new(0.0, 0.0, area.width, area.height);
        let (mut lines, mut next) =
            fill_column_with_rules(&source, start.offset, self.end, bounds, None);
        let first = lines.first()?;
        let offset = match options.first_baseline {
            None | Some(FootnoteFirstBaseline::Leading) => first.advance,
            _ => first.baseline,
        }
        .max(options.minimum_first_baseline.unwrap_or(0.0));
        let delta = offset - first.baseline;
        if delta < 0.0 {
            return None;
        }
        if delta > 0.0 {
            (lines, next) = fill_column_with_rules(
                &source,
                start.offset,
                self.end,
                Rect {
                    height: (area.height - delta).max(0.0),
                    ..bounds
                },
                None,
            );
        }
        if lines.is_empty() || next <= start.offset {
            return None;
        }
        if next + 1 == self.end && source.story.slice(next, self.end) == "\n" {
            next = self.end;
        }
        for line in &mut lines {
            footnote_flow::translate_line(line, crate::Point::new(0.0, delta));
        }
        let height = lines
            .iter()
            .map(|line| line.bounds.bottom())
            .fold(0.0, Pt::max);
        Some((lines, next, height))
    }

    fn minimum(&self, start: NoteCursor, area: Rect, options: &FootnoteOptions) -> Option<Pt> {
        let mut high = area.height;
        self.lines(start, area, options)?;
        let mut low = 0.0;
        for _ in 0..20 {
            if high - low <= 0.001 {
                break;
            }
            let middle = (low + high) * 0.5;
            if self
                .lines(
                    start,
                    Rect {
                        height: middle,
                        ..area
                    },
                    options,
                )
                .is_some()
            {
                high = middle;
            } else {
                low = middle;
            }
        }
        Some(high)
    }

    fn fragment(
        &self,
        note: &PreparedNote,
        start: NoteCursor,
        area: Rect,
        options: &FootnoteOptions,
    ) -> Option<(NoteArea, NoteCursor)> {
        let (lines, consumed, height) = self.lines(start, area, options)?;
        let cursor = NoteCursor {
            offset: consumed,
            hyphens: start
                .hyphens
                .advance(&self.doc.stories[0], &lines, consumed),
        };
        let mut thread = ComposedThread {
            story: crate::StoryId(0),
            frames: vec![ComposedFrame {
                object: ObjectId(0),
                drop_cap: first_drop_cap(&lines),
                lines,
                footnotes: Vec::new(),
                unrendered_structures: 0,
                consumed_to: consumed,
                passed_on: false,
                lost: false,
            }],
        };
        crate::list_composition::insert_markers(&self.markers, &mut thread);
        let mut lines = std::mem::take(&mut thread.frames[0].lines);
        for line in &mut lines {
            footnote_flow::capture(line, &self.doc.stories[0], &self.doc, None, &self.context);
            line.start = note.anchor;
            line.end = note.anchor;
        }
        Some((
            NoteArea {
                structure: note.structure,
                anchor: note.anchor,
                bounds: Rect::new(0.0, 0.0, area.width, height),
                lines,
                rule: None,
            },
            cursor,
        ))
    }
}

fn empty(start: FlowCursor) -> ColumnFlow {
    ColumnFlow {
        lines: Vec::new(),
        footnotes: Vec::new(),
        next: start,
        stop_frame: false,
        stop_page: false,
    }
}
