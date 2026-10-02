//! Whole text-only notes in horizontal column areas. The shrinking
//! body ceiling makes reference eviction monotonic, avoiding fit/evict loops.
use super::*;
use crate::{
    footnote_composition::{NoteArea, PreparedNote, Rule},
    footnotes::{FootnoteFirstBaseline, FootnoteReference},
    inline_text::{LinePositions, RenderedLine},
};

pub(super) fn supported(
    doc: &LayoutDocument,
    story: crate::StoryId,
    frames: &[(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)],
) -> bool {
    let Some(story) = doc.story(story) else {
        return false;
    };
    let options = &doc.footnotes;
    let horizontal = |story: &Story| {
        story.prefs.orientation == crate::StoryOrientation::Horizontal
            && story.points.iter().all(|point| match point {
                Point::Paragraph { style, .. } => matches!(
                    doc.styles.resolve_paragraph(style).writing_mode,
                    None | Some(WritingMode::Horizontal)
                ),
                _ => true,
            })
    };
    // Public IDML defaults resolve absent first-baseline/rule preferences.
    // Unknown spanning defaults remain gated below; other unsupported settings
    // stay retained and reported instead of being silently approximated.
    matches!(
        options.first_baseline,
        None | Some(FootnoteFirstBaseline::Ascent | FootnoteFirstBaseline::Leading)
    ) && rule_supported(&options.rule)
        && (options.no_splitting == Some(true) || rule_supported(&options.continuing_rule))
        && horizontal(story)
        && story
            .structures
            .iter()
            .filter_map(|s| s.footnote.as_ref())
            .all(|n| horizontal(&n.story))
        && frames.iter().all(|(id, _, _, count, _, _)| {
            let options = crate::footnotes::frame_options(doc, *id);
            (*count <= 1 || options.straddle.is_some())
                && options.valid()
                && doc
                    .object(*id)
                    .or_else(|| {
                        doc.parents
                            .iter()
                            .flat_map(|p| &p.objects)
                            .find(|p| p.object.id == *id)
                            .map(|p| &p.object)
                    })
                    .is_none_or(|o| {
                        matches!(
                            o.object,
                            crate::LayoutObject::TextFrame {
                                text_path: None,
                                ..
                            }
                        )
                    })
        })
}

fn rule_supported(rule: &crate::footnotes::FootnoteRule) -> bool {
    !rule.on.unwrap_or(true)
        || (!matches!(rule.paint, Some(FootnoteReference::Unresolved(_)))
            && match &rule.stroke {
                None => true,
                Some(FootnoteReference::None) => false,
                Some(FootnoteReference::Resolved(stroke)) => matches!(
                    stroke.pattern,
                    schist_text_engine::TextDecorationPattern::Solid
                ),
                Some(FootnoteReference::Unresolved(_)) => false,
            })
}

pub(super) struct ProjectionContext {
    paragraphs: std::collections::BTreeMap<usize, std::sync::Arc<str>>,
    counters: crate::list_counters::StoryCounters,
}

impl ProjectionContext {
    pub(super) fn new(doc: &LayoutDocument, story: &Story) -> Self {
        Self {
            paragraphs: story
                .points
                .iter()
                .zip(story.point_offsets())
                .filter_map(|(point, at)| match point {
                    Point::Paragraph { text, .. } => Some((at, text.as_str().into())),
                    _ => None,
                })
                .collect(),
            counters: crate::list_counters::StoryCounters::new(doc, story),
        }
    }
}

pub(super) fn capture(
    line: &mut ComposedLine,
    story: &Story,
    doc: &LayoutDocument,
    positions: Option<LinePositions>,
    context: &ProjectionContext,
) {
    let paragraph = context.paragraphs.range(..=line.start).next_back();
    line.projected = Some(RenderedLine {
        spec: line_spec(line, story, doc),
        paints: line_paint_styles(line, story, doc),
        positions,
        context: paragraph.map_or_else(|| "".into(), |(_, text)| text.clone()),
        counter_issue: paragraph.and_then(|(at, _)| context.counters.issue(*at)),
    });
}

fn measure_note(
    source: &FlowSource<'_>,
    note: &PreparedNote,
    column: Rect,
    options: &crate::footnotes::FootnoteOptions,
) -> Option<NoteArea> {
    let mut scratch = LayoutDocument::new(Vec::new());
    scratch.styles = source.doc.styles.clone();
    scratch
        .default_paragraph_style
        .clone_from(&source.doc.default_paragraph_style);
    scratch
        .default_character_style
        .clone_from(&source.doc.default_character_style);
    scratch.stories.push(note.body.story.clone());
    let frames = [(
        ObjectId(0),
        Rect::new(0.0, 0.0, column.width, column.height),
        FrameOverflow::Clip,
        1,
        0.0,
        crate::Insets::default().into(),
    )];
    let mut composed = compose_thread_plain(&scratch, crate::StoryId(0), &frames, None, None);
    let frame = composed.frames.first_mut()?;
    if frame.lost {
        return None;
    }
    let first = frame.lines.first()?;
    let offset = match options.first_baseline {
        None | Some(FootnoteFirstBaseline::Leading) => first.advance,
        _ => first.baseline,
    }
    .max(options.minimum_first_baseline.unwrap_or(0.0));
    let delta = offset - first.baseline;
    // A first baseline above the measured cell needs an independent ink-fit
    // policy. Keep it overset instead of allowing it to overlap body text.
    if delta < 0.0 {
        return None;
    }
    let height = frame
        .lines
        .iter()
        .map(|l| l.bounds.bottom())
        .fold(0.0f32, f32::max)
        + delta;
    if height > column.height {
        return None;
    }
    let context = ProjectionContext::new(&scratch, &scratch.stories[0]);
    for line in &mut frame.lines {
        capture(line, &scratch.stories[0], &scratch, None, &context);
        translate_line(line, crate::Point::new(0.0, delta));
        line.start = note.anchor;
        line.end = note.anchor;
    }
    Some(NoteArea {
        structure: note.structure,
        anchor: note.anchor,
        bounds: Rect::new(0.0, 0.0, column.width, height),
        lines: std::mem::take(&mut frame.lines),
        rule: None,
    })
}

pub(super) fn translate_line(line: &mut ComposedLine, by: crate::Point) {
    line.bounds = line.bounds.translated(by);
    line.inline_origin += by.x;
    line.baseline += by.y;
    if let Some(initial) = &mut line.initial {
        initial.ink = initial.ink.translated(by);
    }
}

fn references(
    prepared: &crate::footnote_composition::PreparedStory,
    start: usize,
    consumed: usize,
) -> Vec<&PreparedNote> {
    prepared
        .notes
        .iter()
        .filter(|n| start <= n.reference.start && n.reference.start < consumed)
        .collect()
}

pub(super) fn reference_line<'a>(
    lines: &'a [ComposedLine],
    note: &PreparedNote,
) -> Option<&'a ComposedLine> {
    lines
        .iter()
        .find(|l| l.start <= note.reference.start && note.reference.start < l.end)
}

#[derive(Default)]
pub(super) struct FillPolicy {
    /// Balancing limits body flow without moving bottom-aligned note areas.
    pub body_height: Option<Pt>,
}

pub(super) fn fill(
    source: &FlowSource<'_>,
    start: usize,
    end: usize,
    column: Rect,
    grid: Option<BaselineGrid>,
    options: &crate::footnotes::FootnoteOptions,
    policy: FillPolicy,
) -> (Vec<ComposedLine>, usize, Vec<NoteArea>) {
    let body_fill = |bounds| fill_column_with_rules(source, start, end, bounds, grid);
    let mut ceiling = policy
        .body_height
        .unwrap_or(column.height)
        .min(column.height);
    let Some(prepared) = source.notes else {
        let (lines, next) = body_fill(Rect {
            height: ceiling,
            ..column
        });
        return (lines, next, Vec::new());
    };
    for _ in 0..prepared.notes.len() + 2 {
        let room = Rect {
            height: ceiling,
            ..column
        };
        let (lines, next) = body_fill(room);
        let refs = references(prepared, start, next);
        if refs.is_empty() {
            return (lines, next, Vec::new());
        }
        let mut areas = Vec::new();
        let mut failed = None;
        for note in &refs {
            let area = reference_line(&lines, note)
                .filter(|l| note.reference.end <= l.end)
                .and_then(|_| measure_note(source, note, column, options));
            if let Some(area) = area {
                areas.push(area);
            } else {
                failed = Some(*note);
                break;
            }
        }
        if let Some(note) = failed {
            let Some(line) = reference_line(&lines, note) else {
                break;
            };
            ceiling = ceiling.min(line.bounds.bottom() - column.y - 0.001);
            continue;
        }
        let between = options.space_between.unwrap_or(0.0);
        let height =
            areas.iter().map(|a| a.bounds.height).sum::<f32>() + between * (areas.len() - 1) as f32;
        let gap = options.spacer.unwrap_or(0.0);
        let body_height = ceiling.min((column.height - height - gap).max(0.0));
        let (body, consumed) = body_fill(Rect {
            height: body_height,
            ..column
        });
        let kept = references(prepared, start, consumed);
        if kept.len() != refs.len()
            || refs
                .iter()
                .any(|n| reference_line(&body, n).is_none_or(|l| l.end < n.reference.end))
        {
            let Some(evicted) = refs.iter().find(|n| {
                !kept.iter().any(|k| k.structure == n.structure)
                    || reference_line(&body, n).is_none_or(|l| l.end < n.reference.end)
            }) else {
                break;
            };
            let Some(line) = reference_line(&lines, evicted) else {
                break;
            };
            ceiling = ceiling.min(line.bounds.bottom() - column.y - 0.001);
            continue;
        }
        place_areas(source, &body, consumed, column, options, &mut areas);
        return (body, consumed, areas);
    }
    (Vec::new(), start, Vec::new())
}

/// A spanning area shares the frame's inset width and reduces every body
/// column by the same amount. Search the largest feasible body height: adding
/// room can expose another reference, which in turn needs more footer room.
/// Trial fills use complete paragraphs and ordinary widow/keep/grid rules;
/// clipping a source prefix here would reshape the last line or lose a keep.
pub(super) fn fill_spanning(
    source: &FlowSource<'_>,
    start: usize,
    area: Rect,
    options: &crate::footnotes::FootnoteOptions,
    balance: bool,
    fill_body: impl Fn(Pt) -> ColumnFlow,
) -> ColumnFlow {
    let Some(prepared) = source.notes else {
        return fill_body(area.height);
    };
    // A note's width does not change during the height search. Measure it once
    // and retain failed fits too; large notes must stay with their references.
    let mut measured = std::collections::BTreeMap::new();
    let mut attempt = |height: Pt| {
        let body = fill_body(height);
        let mut notes = Vec::new();
        for note in references(prepared, start, body.next.offset) {
            if reference_line(&body.lines, note).is_none_or(|line| line.end < note.reference.end) {
                return (body, None);
            }
            let Some(measured) = measured
                .entry(note.structure)
                .or_insert_with(|| measure_note(source, note, area, options))
            else {
                return (body, None);
            };
            notes.push(measured.clone());
        }
        let reserved = area_height(&notes, options)
            + if notes.is_empty() {
                0.0
            } else {
                options.spacer.unwrap_or(0.0)
            };
        let fits = height + reserved <= area.height;
        (body, fits.then_some(notes))
    };
    let (full, notes) = attempt(area.height);
    let (mut best, mut notes, mut ceiling) = if let Some(notes) = notes {
        (full, notes, area.height)
    } else {
        let mut low = 0.0;
        let mut high = area.height;
        let mut best = fill_body(0.0);
        let mut notes = Vec::new();
        for _ in 0..20 {
            if high - low <= 0.001 {
                break;
            }
            let middle = (low + high) * 0.5;
            let (body, areas) = attempt(middle);
            if let Some(areas) = areas {
                low = middle;
                best = body;
                notes = areas;
            } else {
                high = middle;
            }
        }
        (best, notes, low)
    };
    let terminal_blank = matches!(source.story.points.last(), Some(Point::Paragraph { text, .. }) if text.is_empty());
    let end = source.story.text_len() + usize::from(terminal_blank);
    if balance && best.next.offset >= end {
        // Every remaining reference is already included, so the footer is now
        // fixed. Minimize the common body height while retaining a proven fit.
        let mut low = 0.0;
        for _ in 0..16 {
            if ceiling - low <= 0.05 {
                break;
            }
            let middle = (low + ceiling) * 0.5;
            let body = fill_body(middle);
            if body.next.offset >= end {
                ceiling = middle;
                best = body;
            } else {
                low = middle;
            }
        }
    }
    place_areas(
        source,
        &best.lines,
        best.next.offset,
        area,
        options,
        &mut notes,
    );
    best.footnotes = notes;
    best
}

fn area_height(areas: &[NoteArea], options: &crate::footnotes::FootnoteOptions) -> Pt {
    areas.iter().map(|area| area.bounds.height).sum::<Pt>()
        + options.space_between.unwrap_or(0.0) * areas.len().saturating_sub(1) as Pt
}

pub(super) fn place_areas(
    source: &FlowSource<'_>,
    body: &[ComposedLine],
    consumed: usize,
    column: Rect,
    options: &crate::footnotes::FootnoteOptions,
    areas: &mut [NoteArea],
) {
    let height = area_height(areas, options);
    let gap = options.spacer.unwrap_or(0.0);
    let between = options.space_between.unwrap_or(0.0);
    let bottom = body
        .iter()
        .map(|l| l.bounds.bottom())
        .fold(column.y, f32::max);
    let terminal_blank = matches!(source.story.points.last(), Some(Point::Paragraph { text, .. }) if text.is_empty());
    let at_story_end = consumed >= source.story.text_len() + usize::from(terminal_blank);
    let mut top = if options.end_of_story == Some(true) && at_story_end {
        bottom + gap
    } else {
        column.bottom() - height
    };
    for (index, area) in areas.iter_mut().enumerate() {
        let by = crate::Point::new(column.x, top);
        for line in &mut area.lines {
            translate_line(line, by);
        }
        area.bounds = area.bounds.translated(by);
        if index == 0
            && options.rule.on.unwrap_or(true)
            && !matches!(options.rule.paint, Some(FootnoteReference::None))
        {
            let width = options.rule.width.unwrap_or(72.0);
            let weight = options.rule.weight.unwrap_or(1.0);
            if width > 0.0 && weight > 0.0 {
                area.rule = Some(Rule {
                    bounds: Rect::new(
                        column.x + options.rule.left_indent.unwrap_or(0.0),
                        top + options.rule.offset.unwrap_or(0.0) - weight / 2.0,
                        width,
                        weight,
                    ),
                    ink: options
                        .rule
                        .paint
                        .as_ref()
                        .and_then(FootnoteReference::resolved)
                        .cloned()
                        .unwrap_or_else(Ink::black),
                    tint: options.rule.tint.unwrap_or(1.0),
                    overprint: options.rule.overprint.unwrap_or(false),
                });
            }
        }
        top += area.bounds.height + between;
    }
}
