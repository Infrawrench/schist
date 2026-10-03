//! Composition: fitting a story into frames, columns, and pages.
//!
//! This is where a story stops being text and becomes page furniture. A
//! frame is a box; a story is a flow; composition decides which lines of
//! the flow land in which box, in which order, and -- when it runs out of
//! room -- which box gets the remainder.
//!
//! # The threading model
//!
//! A thread is an ordered list of frames. The first frame takes what fits;
//! the rest carry the overflow, in order. Nothing is duplicated and
//! nothing is skipped: the union of all frames' contents is exactly the
//! story, so editing one frame changes all of them. That invariant is
//! what makes "one article, three pages" behave like one document rather
//! than three copies.
//!
//! # Why this is not the text engine
//!
//! [`schist_text_engine`] measures and shapes a *single* run of text with
//! a single set of attributes. Composition is about the geometry around
//! that: column boxes, insets, forced breaks, and where the overflow
//! goes. It asks the text engine to measure a proposed piece of text, and
//! decides what to propose next. The text engine remains the authority on
//! where a glyph lands.

use schist_text_engine::{line_spans, TextSpec};

use crate::geometry::{Pt, Rect};
use crate::grid::GridMode;
use crate::ink::Ink;
use crate::model::{FrameOverflow, LayoutDocument, ObjectId, PlacedObject};
use crate::story::{Point, Story};
use crate::styles::{
    Align, ParagraphDirection, ResolvedCharacter, ResolvedParagraph, StyleSet, WritingMode,
};
mod break_flow;
mod footnote_flow;
mod keep_flow;
mod split_footnotes;

/// One laid-out line, positioned in page space.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedLine {
    pub projected: Option<crate::inline_text::RenderedLine>,
    /// Generated list ink owns no story bytes and no editable caret positions.
    pub generated: Option<crate::list_composition::GeneratedText>,
    /// A baseline relative to `bounds.origin()`. Its offset already includes
    /// paragraph alignment; line_spec disables a second rectangular alignment.
    pub text_path: Option<schist_text_engine::TextPath>,
    /// Logical inline start relative to a path's bracket ruler. Arc distances
    /// are distinct from page x coordinates. None for rectangular lines.
    pub path_inline_start: Option<Pt>,
    /// Byte range within the story's concatenated text.
    pub start: usize,
    pub end: usize,
    /// Where the line's box sits, in page space.
    pub bounds: Rect,
    /// Leading edge of this column along the inline axis, before paragraph
    /// indents: right for horizontal RTL and left/top otherwise.
    pub inline_origin: Pt,
    /// Baseline in the same local page coordinates as the line box.
    pub baseline: Pt,
    /// Requested distance from the preceding baseline or vertical column center.
    /// Independent of the nominal line-cell dimensions in `bounds`.
    pub advance: Pt,
    /// Paragraph style, resolved, for the caller to render with.
    pub paragraph: ResolvedParagraph,
    /// The paragraph style's *name*, so a renderer can re-measure this
    /// line on its own. `paragraph` is resolved and cannot be traced
    /// back to the document.
    pub paragraph_style: String,
    /// Character styles covering this line, in the order they were found.
    pub characters: Vec<ResolvedCharacter>,
    /// True for the last line of a paragraph. A justified line that is
    /// not the last one is stretched; this one is left ragged.
    ///
    /// This means the *paragraph's* last line, not the last line placed
    /// in this column. A paragraph split across two columns is justified
    /// right up to the break, because more text follows it.
    pub is_paragraph_end: bool,
    /// The measuring engine selected the final source U+00AD for display.
    /// This is independent of paragraph ends: explicit newlines never select it.
    pub discretionary_hyphen: bool,
    /// A selected generated break glyph; it owns no editable source bytes.
    pub generated_hyphen: bool,
    /// The line's width before justification, in points.
    pub natural_width: Pt,
    /// Extra points to add to each word space to bring this line flush to
    /// the measure. `None` when the line is not stretched: ragged, the
    /// last line of a paragraph, or a single word with no space to
    /// stretch.
    ///
    /// The text engine measures natural widths, so the stretch is
    /// computed here and applied by whoever draws. Keeping it as data
    /// rather than a baked coordinate is what lets a renderer decide
    /// whether to honour it.
    pub word_space: Option<Pt>,
    /// True for an empty paragraph or standalone line break. It reserves
    /// a line of space without painting glyphs. Column, frame and page
    /// breaks instead change the destination in the composed thread.
    pub forced_break: bool,
    /// True for a body line inset around an opening initial.
    pub drop_cap: bool,
    /// Enlarged opening text is a separate source range, painted exactly once.
    pub initial: Option<Initial>,
}

impl ComposedLine {
    pub fn is_generated(&self) -> bool {
        self.generated.is_some()
            || self
                .projected
                .as_ref()
                .is_some_and(|p| p.positions.is_none())
    }

    pub fn source_byte(&self, visual: usize) -> usize {
        self.projected
            .as_ref()
            .and_then(|p| p.positions.as_ref())
            .map_or(self.start + visual, |p| p.source(visual))
    }

    pub fn visual_byte(&self, source: usize) -> usize {
        self.projected
            .as_ref()
            .and_then(|p| p.positions.as_ref())
            .map_or(source.saturating_sub(self.start), |p| p.visual(source))
    }
}

/// A shaped opening initial and its actual page-local glyph extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Initial {
    pub scale: Pt,
    pub ink: Rect,
}

/// What a frame ended up holding.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedFrame {
    pub object: ObjectId,
    pub lines: Vec<ComposedLine>,
    pub footnotes: Vec<crate::footnote_composition::NoteArea>,
    /// Retained source structures unsupported by this composition path.
    pub unrendered_structures: usize,
    /// Glyph extent of the first opening initial in this frame. Every initial
    /// is also retained on its own line, including later paragraphs.
    pub drop_cap: Option<Rect>,
    /// Main-story bytes that fit. Footnote bodies have independent cursors.
    pub consumed_to: usize,
    /// This frame ran out of room and passed remaining main or note text to the next
    /// frame of the thread. False when the frame is the thread's last,
    /// because there is nowhere to pass it to.
    pub passed_on: bool,
    /// Main or footnote text did not fit anywhere and is not displayed.
    ///
    /// This is what preflight reports. It is distinct from
    /// [`ComposedFrame::passed_on`]: a long article threaded across
    /// three frames overflows twice and loses nothing.
    pub lost: bool,
}

impl ComposedFrame {
    pub fn all_lines(&self) -> impl Iterator<Item = &ComposedLine> {
        self.lines
            .iter()
            .chain(self.footnotes.iter().flat_map(|n| &n.lines))
    }
}

/// A thread of frames holding one story.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedThread {
    pub story: crate::model::StoryId,
    pub frames: Vec<ComposedFrame>,
}

impl ComposedThread {
    /// Every line in the thread, in reading order.
    pub fn lines(&self) -> impl Iterator<Item = &ComposedLine> {
        self.frames.iter().flat_map(|f| f.lines.iter())
    }

    /// Whether main or footnote text failed to fit.
    pub fn has_overflow(&self) -> bool {
        self.frames.iter().any(|f| f.lost)
    }

    /// Where the story ran out of room, for a preflight message.
    pub fn lost_at(&self) -> Option<ObjectId> {
        self.frames.iter().find(|f| f.lost).map(|f| f.object)
    }
}

/// The column boxes inside a frame, after insets and gutters.
pub fn columns(bounds: Rect, count: u16, gutter: Pt) -> Vec<Rect> {
    let count = count.max(1) as usize;
    if count == 1 {
        return vec![bounds];
    }
    let total_gutter = gutter * (count - 1) as Pt;
    let width = ((bounds.width - total_gutter) / count as Pt).max(0.0);
    (0..count)
        .map(|i| {
            let x = bounds.x + (width + gutter) * i as Pt;
            Rect::new(x, bounds.y, width, bounds.height)
        })
        .collect()
}

/// A text specification built from a paragraph style and a slice of text.
///
/// The defaults here are the last resort for anything a style chain left
/// unset, and they are the values a page layout tool uses when a user
/// types into an empty frame.
///
/// # Width
///
/// `width` is the column the text is to be measured in. It is required,
/// not optional: with no wrap width the text engine returns the whole
/// slice as one line of unbounded width, which would make every frame
/// look like it had room for the entire story.
///
/// # Leading and alignment
///
/// Fixed leading is an absolute baseline distance in points. Auto uses the
/// paragraph percentage of each run's nominal type size. Both reach the engine
/// as absolute spacing; font ascent and descent still determine cell geometry.
/// An absent value retains the engine's natural font spacing.
///
/// Justification is not something this hands to the text engine, which
/// has no notion of it. The engine left-aligns and the renderer stretches
/// the non-final lines; [`ComposedLine::is_paragraph_end`] is what tells
/// it which ones to leave ragged.
pub fn spec_for(
    story: &Story,
    start: usize,
    end: usize,
    styles: &StyleSet,
    paragraph_style: &str,
    character_style: &str,
    width: Pt,
) -> TextSpec {
    let paragraph = styles.resolve_paragraph(paragraph_style);
    let character = paragraph.character(styles.resolve_character(character_style));
    spec_with_character(story, (start, end), styles, &paragraph, character, width)
}

pub(crate) fn spec_with_character(
    story: &Story,
    range: (usize, usize),
    styles: &StyleSet,
    paragraph: &ResolvedParagraph,
    character: ResolvedCharacter,
    width: Pt,
) -> TextSpec {
    let (start, end) = range;
    let text = story.slice(start, end);
    let size = paragraph
        .point_size
        .or(character.point_size)
        .unwrap_or(11.0);
    // A style that names no family gets the engine's default rather than
    // an empty string, which would fail to load any font and lay out
    // nothing at all.
    let family = character
        .family
        .clone()
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| TextSpec::default().family);
    let natural = natural_line_advance(&character, size, &family);
    // Paragraph bidi direction is independent of story column progression.
    // Resolve Auto from the complete paragraph before slicing a rendered line.
    let leading = paragraph
        .leading
        .or(character.leading)
        .and_then(|leading| leading.points(size, paragraph.auto_leading));
    let direction = paragraph
        .direction
        .map(engine_direction)
        .unwrap_or(schist_text_engine::ParagraphDirection::Auto);
    let direction = if direction == schist_text_engine::ParagraphDirection::Auto {
        let offsets = story.point_offsets();
        let context = story
            .points
            .iter()
            .zip(offsets)
            .find_map(|(point, from)| {
                let Point::Paragraph { text, .. } = point else {
                    return None;
                };
                (start >= from && start <= from + text.len()).then_some(text.as_str())
            })
            .unwrap_or(&text);
        schist_text_engine::base_direction(context)
    } else {
        direction
    };
    let base_position =
        crate::styles::TextPosition::resolved(character.position, character.baseline_shift);
    let base_leading = leading.unwrap_or(natural);
    let (base_scale, base_shift) = styles.text_preferences.script(base_position, base_leading);
    let base_script = base_position != crate::styles::TextPosition::Normal;
    let writing_mode =
        paragraph
            .writing_mode
            .map(engine_writing_mode)
            .unwrap_or(match story.prefs.orientation {
                crate::StoryOrientation::Horizontal => schist_text_engine::WritingMode::Horizontal,
                crate::StoryOrientation::Vertical => schist_text_engine::WritingMode::VerticalRl,
            });
    let reverse = direction == schist_text_engine::ParagraphDirection::RightToLeft
        && !writing_mode.is_vertical();
    let tabs = text
        .contains('\t')
        .then(|| crate::tabs::stops(paragraph, reverse));
    let mut spec = TextSpec {
        show_final_soft_hyphen: false,
        hyphenation_breaks: Vec::new(),
        hyphenation_policy: Default::default(),
        show_final_generated_hyphen: false,
        language: character
            .language
            .as_ref()
            .and_then(|v| styles.resolve_language(v))
            .unwrap_or_default(),
        text,
        family: family.clone(),
        font_style: character.font_style.clone(),
        bold: character.bold.unwrap_or(false),
        italic: character.italic.unwrap_or(false),
        size,
        align: engine_align(paragraph.align),
        // Paragraph direction controls bidi; writing mode may override the
        // story's default axis without changing its column progression.
        direction,
        writing_mode,
        line_height: 1.0,
        leading,
        tracking: paragraph.tracking.or(character.tracking).unwrap_or(0.0) * size / 1000.0,
        word_spacing: 0.0,
        wrap_width: (width > 0.0).then_some(width),
        tabs,
        runs: story
            .ranges
            .iter()
            .filter(|range| range.start < end && range.end > start)
            .map(|range| {
                let style = styles
                    .resolve_character(&range.style)
                    .with_paint_defaults(&character);
                let shift = style.baseline_shift.or(character.baseline_shift);
                let position = crate::styles::TextPosition::resolved(
                    style.position.or(character.position),
                    shift,
                );
                let scripted = position != crate::styles::TextPosition::Normal;
                let nominal = style.point_size.unwrap_or(size);
                let leading = style
                    .leading
                    .or(paragraph.leading)
                    .or(character.leading)
                    .and_then(|leading| leading.points(nominal, paragraph.auto_leading))
                    .or_else(|| {
                        scripted.then(|| {
                            let metrics_style = ResolvedCharacter {
                                font_style: if style.font_style.is_some()
                                    || style.bold.is_some()
                                    || style.italic.is_some()
                                {
                                    style.font_style.clone()
                                } else {
                                    character.font_style.clone()
                                },
                                bold: style.bold.or(character.bold),
                                italic: style.italic.or(character.italic),
                                ..Default::default()
                            };
                            natural_line_advance(
                                &metrics_style,
                                nominal,
                                style.family.as_deref().unwrap_or(&family),
                            )
                        })
                    });
                let (script_scale, script_shift) = styles
                    .text_preferences
                    .script(position, leading.unwrap_or(0.0));
                schist_text_engine::StyleRun {
                    language: style
                        .language
                        .as_ref()
                        .map(|v| styles.resolve_language(v).unwrap_or_default()),
                    start: range.start.max(start) - start,
                    end: range.end.min(end) - start,
                    no_break: style.no_break.or(character.no_break),
                    fill_disabled: Some(style.fill_disabled),
                    stroke: style.preview_stroke(),
                    underline_style: Some(style.underline_style.preview(&style)),
                    strike_style: Some(style.strike_style.preview(&style)),
                    family: style.family,
                    font_style: style.font_style,
                    bold: style.bold,
                    italic: style.italic,
                    size: if scripted {
                        Some(nominal * script_scale)
                    } else {
                        style.point_size
                    },
                    metric_size: scripted.then_some(nominal),
                    capitalization: Some(schist_text_engine::Capitalization::from_flags(
                        style.all_caps.or(character.all_caps).unwrap_or(false),
                        style.small_caps.or(character.small_caps).unwrap_or(false),
                    )),
                    small_cap_scale: Some(styles.text_preferences.small_cap_size / 100.0),
                    features: engine_features(
                        &style.features,
                        style.kerning,
                        style.directional_features,
                        writing_mode.is_vertical(),
                    ),
                    tracking: style
                        .tracking
                        .or(paragraph.tracking)
                        .or(character.tracking)
                        .map(|tracking| tracking * nominal * script_scale / 1000.0),
                    leading,
                    underline: style.underline.or(character.underline),
                    strikethrough: style.strikethrough.or(character.strikethrough),
                    baseline_shift: Some(
                        shift
                            .and_then(crate::styles::BaselineShift::explicit_offset)
                            .unwrap_or(0.0)
                            + script_shift,
                    ),
                    color: Some(
                        crate::styles::inherited_paint(
                            &style.fill,
                            style.fill_tint,
                            character.fill.as_ref(),
                        )
                        .unwrap_or_else(crate::Ink::black),
                    )
                    .map(|ink| {
                        let rgb = ink.preview_at_tint(
                            style.fill_tint.or(character.fill_tint).unwrap_or(1.0),
                        );
                        [
                            (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                            (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                            (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                            (style
                                .opacity
                                .or(character.opacity)
                                .unwrap_or(1.0)
                                .clamp(0.0, 1.0)
                                * 255.0)
                                .round() as u8,
                        ]
                    }),
                }
            })
            .collect(),
        features: engine_features(
            &character.features,
            paragraph.kerning.or(character.kerning),
            character.directional_features,
            writing_mode.is_vertical(),
        ),
        path: None,
    };
    let stroke = character.preview_stroke();
    let underline_style = Some(character.underline_style.preview(&character));
    let strike_style = Some(character.strike_style.preview(&character));
    let ink = character.fill.unwrap_or_else(crate::Ink::black);
    let rgb = ink.preview_at_tint(character.fill_tint.unwrap_or(1.0));
    spec.runs.push(schist_text_engine::StyleRun {
        start: 0,
        end: spec.text.len(),
        no_break: character.no_break,
        fill_disabled: Some(character.fill_disabled),
        stroke,
        underline_style,
        strike_style,
        underline: character.underline,
        strikethrough: character.strikethrough,
        size: base_script.then_some(size * base_scale),
        metric_size: base_script.then_some(size),
        capitalization: Some(schist_text_engine::Capitalization::from_flags(
            character.all_caps.unwrap_or(false),
            character.small_caps.unwrap_or(false),
        )),
        small_cap_scale: Some(styles.text_preferences.small_cap_size / 100.0),
        leading: base_script.then_some(base_leading),
        tracking: base_script.then_some(spec.tracking * base_scale),
        baseline_shift: Some(
            character
                .baseline_shift
                .and_then(crate::styles::BaselineShift::explicit_offset)
                .unwrap_or(0.0)
                + base_shift,
        ),
        color: Some([
            (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            (character.opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8,
        ]),
        ..Default::default()
    });
    spec
}

fn engine_features(
    features: &[(String, bool)],
    kerning: Option<bool>,
    directional: crate::directional_features::DirectionalFeatures,
    vertical: bool,
) -> Vec<schist_text_engine::OpenTypeFeature> {
    let features = directional.selected(features, vertical);
    let mut result: Vec<_> = features
        .iter()
        .map(|(tag, enabled)| schist_text_engine::OpenTypeFeature {
            tag: tag.clone(),
            value: u32::from(*enabled),
        })
        .collect();
    if let Some(enabled) = kerning.filter(|_| !features.iter().any(|(tag, _)| tag == "kern")) {
        result.push(schist_text_engine::OpenTypeFeature {
            tag: "kern".into(),
            value: u32::from(enabled),
        });
    }
    result
}

/// The engine's base direction for a page-layout direction.
pub fn engine_direction(direction: ParagraphDirection) -> schist_text_engine::ParagraphDirection {
    match direction {
        ParagraphDirection::LeftToRight => schist_text_engine::ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft => schist_text_engine::ParagraphDirection::RightToLeft,
        ParagraphDirection::Auto => schist_text_engine::ParagraphDirection::Auto,
    }
}

/// The engine's writing mode for a page-layout one.
pub fn engine_writing_mode(mode: WritingMode) -> schist_text_engine::WritingMode {
    match mode {
        WritingMode::Horizontal => schist_text_engine::WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft => schist_text_engine::WritingMode::VerticalRl,
        WritingMode::VerticalLeftToRight => schist_text_engine::WritingMode::VerticalLr,
    }
}

/// Force a spec onto an exact baseline-to-baseline distance.
///
/// A grid may increase the composed step. Carry that resolved step into
/// rendering and editing specifications without changing nominal cell metrics.
pub fn with_leading(spec: TextSpec, leading: Pt) -> TextSpec {
    let mut spec = spec;
    spec.leading = Some(leading);
    for run in &mut spec.runs {
        run.leading = Some(leading);
    }
    spec
}

/// The engine's own alignment for a page-layout alignment.
///
/// Justified text left-aligns during measurement. The widths the engine
/// reports are the natural ones, which is what a justification pass
/// needs as its starting point.
pub fn engine_align(align: Option<Align>) -> schist_text_engine::Align {
    match align {
        Some(Align::Center) => schist_text_engine::Align::Center,
        Some(Align::Right) => schist_text_engine::Align::Right,
        _ => schist_text_engine::Align::Left,
    }
}

/// The line advance the font would use on its own, in points, at `size`.
///
/// Falls back to a conventional 1.2em when the font database cannot
/// answer, so a document with a missing font still lays out with
/// plausible metrics rather than collapsing to zero-height lines.
pub fn natural_line_advance(character: &ResolvedCharacter, size: Pt, family: &str) -> Pt {
    let spec = TextSpec {
        // An empty logical line reads font metrics without shaping glyphs.
        text: String::new(),
        family: family.to_string(),
        font_style: character.font_style.clone(),
        bold: character.bold.unwrap_or(false),
        italic: character.italic.unwrap_or(false),
        size,
        line_height: 1.0,
        ..TextSpec::default()
    };
    schist_text_engine::measure(&spec)
        .map(|m| m.line_advance)
        .filter(|a| *a > 0.0)
        .unwrap_or(size * 1.2)
}

/// Measure how tall a slice of story is at a given width.
///
/// Returns the height in points, or `None` when the font is unavailable,
/// so a caller can distinguish "no text" from "cannot measure yet". The
/// height is the extent of the engine's positioned cells, including explicit
/// leading, rather than a guessed multiple of the point size.
pub fn measure_slice(
    story: &Story,
    start: usize,
    end: usize,
    styles: &StyleSet,
    paragraph_style: &str,
    character_style: &str,
    width: Pt,
) -> Option<Pt> {
    let spec = spec_for(
        story,
        start,
        end,
        styles,
        paragraph_style,
        character_style,
        width,
    );
    let lines = line_spans(&spec);
    if lines.is_empty() {
        return Some(0.0);
    }
    Some(
        lines
            .iter()
            .map(|line| line.top + line.height)
            .fold(0.0, f32::max),
    )
}

/// Fit a story into a thread of frames.
///
/// `frame_bounds` gives each frame's page-space box in thread order. The
/// first frame that clips does so at the point its text runs out of
/// height; the next frame resumes at that byte offset. Stored frames resolve
/// their own balancing policy; geometry-only IDs retain legacy balancing.
pub fn compose_thread(
    doc: &LayoutDocument,
    story_id: crate::model::StoryId,
    frames: &[(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)],
) -> ComposedThread {
    compose_thread_on_page(doc, story_id, frames, None)
}

struct FlowSource<'a> {
    doc: &'a LayoutDocument,
    story: &'a Story,
    markers: &'a crate::list_composition::MarkerPlans,
    notes: Option<&'a crate::footnote_composition::PreparedStory>,
}

#[derive(Clone, Copy)]
struct FlowCursor {
    offset: usize,
    break_index: usize,
    last: Option<break_flow::Location>,
    /// Page on which the pending explicit numbered-page break was encountered.
    /// Each consecutive zero-width break starts its own transition.
    break_origin: Option<break_flow::Location>,
}

struct ColumnFlow {
    lines: Vec<ComposedLine>,
    footnotes: Vec<crate::footnote_composition::NoteArea>,
    next: FlowCursor,
    stop_frame: bool,
    stop_page: bool,
}

/// Trial layouts own their break cursor. Reserving a shared note area must not
/// consume a forced break before the final body layout has been chosen.
fn fill_columns(
    story: &Story,
    start: FlowCursor,
    end: usize,
    columns: &[Rect],
    breaks: &[(break_flow::Event, usize)],
    location: break_flow::Location,
    mut fill: impl FnMut(
        usize,
        usize,
        Rect,
    ) -> (
        Vec<ComposedLine>,
        usize,
        Vec<crate::footnote_composition::NoteArea>,
    ),
) -> ColumnFlow {
    let mut out = ColumnFlow {
        lines: Vec::new(),
        footnotes: Vec::new(),
        next: start,
        stop_frame: false,
        stop_page: false,
    };
    for (column_index, column) in columns.iter().enumerate() {
        let here = break_flow::Location {
            column: location.column + column_index,
            ..location
        };
        loop {
            if out.next.offset >= end {
                return out;
            }
            if let Some((event, _)) = breaks
                .get(out.next.break_index)
                .filter(|(_, at)| *at == out.next.offset)
            {
                let previous = if event.numbered_page() {
                    Some(*out.next.break_origin.get_or_insert(here))
                } else {
                    out.next.last
                };
                let advance = event.advance(previous, here);
                if event.explicit() {
                    // A leading explicit break also leaves the initial
                    // container, even before the first source character.
                    out.next.last.get_or_insert(here);
                }
                if event.unconditional() || advance.is_none() {
                    out.next.break_index += 1;
                    out.next.break_origin = None;
                }
                if let Some(boundary) = advance {
                    out.stop_page = boundary == break_flow::Boundary::Page;
                    out.stop_frame = boundary != break_flow::Boundary::Column;
                    break;
                }
                // A satisfied paragraph constraint consumes no space. Its
                // first line still belongs at the top of this very column.
                continue;
            }
            let stop = breaks
                .get(out.next.break_index)
                .map_or(end, |(_, at)| *at)
                .min(end);
            let (part, consumed, notes) = fill(out.next.offset, stop, *column);
            out.lines.extend(part);
            out.footnotes.extend(notes);
            let previous = out.next.offset;
            out.next.offset = consumed;
            if consumed + 1 == stop && story.slice(consumed, stop) == "\n" {
                out.next.offset = stop;
            }
            if out.next.offset <= previous {
                return out;
            }
            out.next.last = Some(here);
            if out.next.offset < stop {
                break;
            }
        }
        if out.stop_frame {
            break;
        }
    }
    out
}

fn compose_thread_on_page(
    doc: &LayoutDocument,
    story_id: crate::model::StoryId,
    frames: &[(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)],
    parent_page: Option<usize>,
) -> ComposedThread {
    if footnote_flow::supported(doc, story_id, frames) {
        if let Some(prepared) = crate::footnote_composition::prepare(doc, story_id) {
            // Assets are Arc-backed. The temporary model is scoped to this one
            // thread pass; no projected bytes or generated styles enter history.
            let mut projected = doc.clone();
            projected.styles = prepared.styles.clone();
            projected.stories[story_id.0 as usize] = prepared.main.story.clone();
            let mut out =
                compose_thread_plain(&projected, story_id, frames, parent_page, Some(&prepared));
            let text = prepared.main.story.text();
            let projected_story = &projected.stories[story_id.0 as usize];
            let context = footnote_flow::ProjectionContext::new(&projected, projected_story);
            for frame in &mut out.frames {
                frame.unrendered_structures = frame
                    .unrendered_structures
                    .saturating_sub(prepared.notes.len());
                frame.consumed_to = prepared.main.positions.source(frame.consumed_to);
                for line in &mut frame.lines {
                    let positions = prepared.main.positions.line(&text, line.start, line.end);
                    if line.generated.is_none() {
                        footnote_flow::capture(
                            line,
                            projected_story,
                            &projected,
                            positions,
                            &context,
                        );
                    }
                    line.start = prepared.main.positions.source(line.start);
                    line.end = prepared.main.positions.source(line.end);
                }
            }
            return out;
        }
    }
    compose_thread_plain(doc, story_id, frames, parent_page, None)
}

fn compose_thread_plain(
    doc: &LayoutDocument,
    story_id: crate::model::StoryId,
    frames: &[(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)],
    parent_page: Option<usize>,
    notes: Option<&crate::footnote_composition::PreparedStory>,
) -> ComposedThread {
    let mut out = ComposedThread {
        story: story_id,
        frames: Vec::new(),
    };
    let Some(story) = doc.story(story_id) else {
        // A frame whose story is missing still gets an entry, so a caller
        // iterating the thread sees one result per frame and can report
        // the missing link rather than a short thread.
        out.frames = frames
            .iter()
            .map(|(object, _, _, _, _, _)| ComposedFrame {
                object: *object,
                lines: Vec::new(),
                footnotes: Vec::new(),
                unrendered_structures: 0,
                drop_cap: None,
                consumed_to: 0,
                passed_on: false,
                lost: false,
            })
            .collect();
        return out;
    };
    let markers = crate::list_composition::MarkerPlans::new(doc, story);
    let source = FlowSource {
        doc,
        story,
        markers: &markers,
        notes,
    };
    let mut split_notes = notes
        // IDML Appendix C: absent NoSplitting defaults to false. Preserve the
        // authored Option; only composition resolves that native default.
        .filter(|_| doc.footnotes.no_splitting != Some(true))
        .map(|notes| split_footnotes::Flow::new(doc, notes));
    let text_end = story.text_len();
    // Markers, explicit destination breaks and constrained blank paragraphs
    // still need a destination without source bytes. Ordinary untouched blank
    // stories retain an insertion point without reporting overset text.
    let has_content = text_end > 0
        || story.points.iter().any(|point| match point {
            Point::Paragraph { style, .. } => {
                let style = doc.styles.resolve_paragraph(style);
                style.list.active()
                    || !matches!(
                        style.start_paragraph,
                        None | Some(crate::styles::ParagraphStart::Anywhere)
                    )
            }
            Point::ColumnBreak
            | Point::FrameBreak
            | Point::PageBreak
            | Point::OddPageBreak
            | Point::EvenPageBreak => true,
            _ => false,
        });
    // A terminal empty paragraph has no source bytes, but still owns a line.
    // One internal flow position represents that line; all public text ranges
    // and consumed offsets are clamped to the actual story coordinate system.
    let terminal_blank =
        matches!(story.points.last(), Some(Point::Paragraph { text, .. }) if text.is_empty());
    let total = text_end + usize::from(terminal_blank);
    let mut cursor = 0usize;
    // Breaks have zero width in Story's text coordinate system. Track the
    // event index separately so a break never consumes the next character.
    let breaks = break_flow::events(doc, story);
    let mut break_index = 0;
    let mut break_origin = None;
    let mut last_location = None;
    let mut skip_page = None;

    for (index, (object, bounds, overflow, column_count, gutter, insets)) in
        frames.iter().enumerate()
    {
        let is_last = index + 1 == frames.len();
        let page_key = doc
            .object(*object)
            .map(|o| (0, o.page))
            .or_else(|| {
                doc.parents.iter().enumerate().find_map(|(index, parent)| {
                    parent
                        .objects
                        .iter()
                        .find(|entry| entry.object.id == *object)
                        .map(|entry| (index + 1, entry.object.page))
                })
            })
            .unwrap_or((0, 0));
        let page = parent_page.unwrap_or(page_key.1);
        let location = break_flow::Location {
            frame: index,
            column: 0,
            page: page_key,
            number: doc.page_number_value(page),
        };
        let grid = BaselineGrid::for_frame(doc, *object, page);
        if skip_page == Some(page_key) {
            let pending = has_content && cursor < total
                || split_notes
                    .as_ref()
                    .is_some_and(|notes| notes.pending(cursor));
            let threads = *overflow == FrameOverflow::Thread;
            out.frames.push(ComposedFrame {
                object: *object,
                lines: Vec::new(),
                footnotes: Vec::new(),
                unrendered_structures: story.retained_structures(),
                drop_cap: None,
                consumed_to: cursor.min(text_end),
                passed_on: pending && !is_last && threads,
                lost: pending && (is_last || !threads),
            });
            // A destination request cannot bypass a frame that terminates the
            // thread. Its empty tail ports stay addressable below as usual.
            if !pending || !threads {
                break;
            }
            continue;
        }
        skip_page = None;
        let text_path = doc
            .object(*object)
            .or_else(|| {
                doc.parents
                    .iter()
                    .flat_map(|p| &p.objects)
                    .find(|p| p.object.id == *object)
                    .map(|p| &p.object)
            })
            .and_then(|placed| match &placed.object {
                crate::LayoutObject::TextFrame { text_path, .. } => text_path.as_ref(),
                _ => None,
            });
        if let Some(path) = text_path {
            let flow = fill_columns(
                story,
                FlowCursor {
                    offset: cursor,
                    break_index,
                    last: last_location,
                    break_origin,
                },
                total,
                std::slice::from_ref(bounds),
                &breaks,
                location,
                |from, to, _| {
                    let (lines, next) = compose_path(story, from, to, *bounds, path, doc);
                    (lines, next, Vec::new())
                },
            );
            cursor = flow.next.offset;
            break_index = flow.next.break_index;
            break_origin = flow.next.break_origin;
            last_location = flow.next.last;
            if flow.stop_page {
                skip_page = Some(page_key);
            }
            let lines = flow.lines;
            let overflowed = has_content && cursor < total
                || split_notes
                    .as_ref()
                    .is_some_and(|notes| notes.pending(cursor));
            let threads = *overflow == FrameOverflow::Thread;
            out.frames.push(ComposedFrame {
                object: *object,
                lines,
                footnotes: Vec::new(),
                unrendered_structures: story.retained_structures(),
                drop_cap: None,
                consumed_to: cursor.min(text_end),
                passed_on: overflowed && !is_last && threads,
                lost: overflowed && (is_last || !threads),
            });
            if !overflowed || !threads {
                break;
            }
            continue;
        }
        let mut available = bounds.inset(insets.resolve());
        let note_options = crate::footnotes::frame_options(doc, *object);
        let mut lines = Vec::new();
        let mut footnotes = Vec::new();
        let frame_start = cursor;
        while cursor < total
            || split_notes
                .as_ref()
                .is_some_and(|notes| notes.pending(cursor))
        {
            if available.width <= 0.0 || available.height <= 0.0 {
                break;
            }
            let section_start = cursor;
            let axes = FlowAxes {
                bounds: available,
                writing: writing_mode_at(story, cursor, doc),
            };
            // A writing-mode change starts a new region in the room left by
            // the preceding region. Never shape with one orientation and
            // position its glyphs using another paragraph's axes.
            let (section_end, before_next) = story
                .points
                .iter()
                .zip(story.point_offsets())
                .find_map(|(point, offset)| {
                    let Point::Paragraph { style, .. } = point else {
                        return None;
                    };
                    (offset > cursor && writing_mode_at(story, offset, doc) != axes.writing).then(
                        || {
                            (
                                offset,
                                doc.styles
                                    .resolve_paragraph(style)
                                    .space_before
                                    .unwrap_or(0.0)
                                    .max(0.0),
                            )
                        },
                    )
                })
                .unwrap_or((total, 0.0));
            let mut columns = columns(axes.logical_bounds(), *column_count, *gutter);
            if story.prefs.direction == crate::StoryDirection::RightToLeft {
                columns.reverse();
            }
            // A newly reached frame can already satisfy its pending numbered
            // break or opening start constraint. Resolve it before deciding whether the remaining
            // text can balance; the policy must not disable final balancing.
            while let Some((event, _)) = breaks.get(break_index).filter(|(_, at)| *at == cursor) {
                let previous = if event.numbered_page() {
                    if break_origin.is_none() {
                        break;
                    }
                    break_origin
                } else {
                    last_location
                };
                if event.unconditional() || event.advance(previous, location).is_some() {
                    break;
                }
                break_index += 1;
                break_origin = None;
            }
            let spanning =
                source.notes.is_some() && columns.len() > 1 && note_options.straddle == Some(true);
            // Only enabled frames balance, and only when all remaining text fits.
            // Shared footers reserve space before balancing the body.
            let balance = crate::frame_text::balanced(doc, *object)
                && columns.len() > 1
                && section_end == total
                && break_index == breaks.len();
            if balance && !spanning && split_notes.is_none() {
                let (mut balanced, consumed, balanced_notes) =
                    balance_columns(&source, cursor, total, &columns, grid, &note_options);
                if consumed >= total {
                    cursor = consumed;
                    axes.place_lines(&mut balanced);
                    lines.extend(balanced);
                    footnotes.extend(balanced_notes);
                    break;
                }
            }
            let start = FlowCursor {
                offset: cursor,
                break_index,
                last: last_location,
                break_origin,
            };
            let flow = if let Some(notes) = &mut split_notes {
                notes.fill_frame(
                    &source,
                    start,
                    split_footnotes::Frame {
                        area: available,
                        columns: &columns,
                        end: section_end,
                        breaks: &breaks,
                        location,
                        grid,
                        options: &note_options,
                        spanning,
                        balance,
                    },
                )
            } else if spanning {
                footnote_flow::fill_spanning(
                    &source,
                    cursor,
                    available,
                    &note_options,
                    balance,
                    |height| {
                        fill_columns(
                            story,
                            start,
                            section_end,
                            &columns,
                            &breaks,
                            location,
                            |from, to, column| {
                                let (lines, next) = fill_column_with_rules(
                                    &source,
                                    from,
                                    to,
                                    Rect { height, ..column },
                                    grid,
                                );
                                (lines, next, Vec::new())
                            },
                        )
                    },
                )
            } else {
                fill_columns(
                    story,
                    start,
                    section_end,
                    &columns,
                    &breaks,
                    location,
                    |from, to, column| {
                        footnote_flow::fill(
                            &source,
                            from,
                            to,
                            column,
                            grid,
                            &note_options,
                            footnote_flow::FillPolicy::default(),
                        )
                    },
                )
            };
            cursor = flow.next.offset;
            break_index = flow.next.break_index;
            break_origin = flow.next.break_origin;
            last_location = flow.next.last;
            if flow.stop_page {
                skip_page = Some(page_key);
            }
            let stop_frame = flow.stop_frame;
            let mut placed = flow.lines;
            footnotes.extend(flow.footnotes);
            if placed.is_empty() {
                break;
            }
            axes.place_lines(&mut placed);
            available = axes.remaining(&placed, before_next);
            lines.extend(placed);
            // Split-note flow already owns every horizontal column and its
            // footer. Pending note text resumes in the next frame, never a
            // second region overlapping the footer just placed here.
            if split_notes.is_some() || cursor < section_end || stop_frame {
                break;
            }
            if cursor <= section_start {
                break;
            }
        }

        // Column flow enforces its own keeps. This also checks a binding that
        // crosses writing-mode regions within the same frame.
        cursor = keep_flow::enforce(&source, frame_start, total, &mut lines, cursor);

        let consumed_to = cursor.min(text_end);
        // Empty list paragraphs still have a generated marker to place.
        let overflowed = has_content && cursor < total
            || split_notes
                .as_ref()
                .is_some_and(|notes| notes.pending(cursor));
        // Main text or a pending note is lost when there is no next frame
        // to take it, or when the frame clips rather than threads.
        let threads = *overflow == FrameOverflow::Thread;
        out.frames.push(ComposedFrame {
            object: *object,
            drop_cap: first_drop_cap(&lines),
            lines,
            footnotes,
            unrendered_structures: story.retained_structures(),
            consumed_to,
            passed_on: overflowed && !is_last && threads,
            lost: overflowed && (is_last || !threads),
        });
        if !overflowed || !threads {
            break;
        }
    }
    // Empty tail frames remain addressable for caret and port controls.
    for (object, _, _, _, _, _) in &frames[out.frames.len()..] {
        out.frames.push(ComposedFrame {
            object: *object,
            lines: Vec::new(),
            footnotes: Vec::new(),
            unrendered_structures: story.retained_structures(),
            drop_cap: None,
            consumed_to: cursor.min(text_end),
            passed_on: false,
            lost: false,
        });
    }
    crate::list_composition::insert_markers(source.markers, &mut out);
    out
}

/// One baseline accepts one shaped line. Paragraph/soft breaks advance to the
/// next container. Box-only vertical spacing, grids, multi-line keeps and
/// enlarged initials cannot reserve additional rows on a path.
fn compose_path(
    story: &Story,
    start: usize,
    end: usize,
    bounds: Rect,
    path: &crate::text_path::PathText,
    doc: &LayoutDocument,
) -> (Vec<ComposedLine>, usize) {
    let empty = || (Vec::new(), start);
    let Some(mut guide) = path.engine_path() else {
        return empty();
    };
    let Some(mut block) = blocks(story, start, end).into_iter().next() else {
        return empty();
    };
    if block.style.is_empty() {
        block.style.clone_from(&doc.default_paragraph_style);
    }
    let paragraph = doc.styles.resolve_paragraph(&block.style);
    let mut spec = spec_for(
        story,
        block.start,
        if block.is_paragraph {
            block.end
        } else {
            block.start
        },
        &doc.styles,
        &block.style,
        &doc.default_character_style,
        0.0,
    );
    if spec.writing_mode.is_vertical() {
        return empty();
    }
    let interval = guide.span.unwrap_or(0.0);
    let reverse = reverse_ruler(&spec);
    let measure = line_measure(
        &paragraph,
        Rect::new(0.0, 0.0, interval, 0.0),
        block.paragraph_start == Some(block.start),
        None,
        reverse,
    );
    if measure.width <= 0.0 {
        return empty();
    }
    let inline_start = if reverse {
        interval - measure.right()
    } else {
        measure.x
    };
    spec.wrap_width = Some(measure.width);
    if let Some(tabs) = &mut spec.tabs {
        tabs.origin = inline_start;
    }
    let spans = line_spans(&spec);
    let Some(span) = spans.first() else {
        return empty();
    };
    // The shared engine can return an overlong word or protected range. A
    // bounded path keeps it overset instead of painting past its bracket.
    if span.width > measure.width + 0.0001 {
        return empty();
    }
    let paragraph_end = spans.len() == 1;
    let mut line = if block.is_paragraph {
        line_at(
            story,
            doc,
            &block,
            span,
            LinePlacement {
                bounds: Rect::new(bounds.x, bounds.y, measure.width, span.height),
                advance: span.advance,
                inline_origin: bounds.x,
                is_paragraph_end: paragraph_end,
                drop_cap: false,
            },
        )
    } else {
        break_line(
            story,
            doc,
            &bounds,
            &block,
            bounds.y,
            LineFlow::from_span(span, &spec),
        )
    };
    let width = span.width
        + line.word_space.unwrap_or(0.0) * count_spaces(story, line.start, line.end) as f32;
    guide.offset += measure.x
        + match spec.align {
            schist_text_engine::Align::Left => 0.0,
            schist_text_engine::Align::Center => (measure.width - width) / 2.0,
            schist_text_engine::Align::Right => measure.width - width,
        };
    guide.span = Some(measure.width);
    line.text_path = Some(guide);
    line.path_inline_start = Some(inline_start);
    let mut consumed = spans
        .get(1)
        .map_or(block.end, |next| block.start + next.start);
    // Paragraph separators consume no second baseline. Explicit LineBreak
    // points still own a blank line when they are encountered on their own.
    if consumed == block.end && consumed < end && story.slice(consumed, consumed + 1) == "\n" {
        consumed += 1;
    }
    (vec![line], consumed)
}

/// The paragraph's writing mode, falling back to native story orientation.
pub fn writing_mode_at(
    story: &Story,
    at: usize,
    doc: &LayoutDocument,
) -> schist_text_engine::WritingMode {
    let style = story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, offset)| match point {
            Point::Paragraph { text, style } if offset <= at && at <= offset + text.len() => {
                Some(style.as_str())
            }
            _ => None,
        })
        .unwrap_or(&doc.default_paragraph_style);
    doc.styles
        .resolve_paragraph(style)
        .writing_mode
        .map(engine_writing_mode)
        .unwrap_or(match story.prefs.orientation {
            crate::StoryOrientation::Horizontal => schist_text_engine::WritingMode::Horizontal,
            crate::StoryOrientation::Vertical => schist_text_engine::WritingMode::VerticalRl,
        })
}

/// Composition uses inline/block coordinates; only finished lines become page
/// rectangles. Glyph shaping itself keeps the requested writing mode, so CJK
/// glyphs stay upright and Western glyphs retain the engine's vertical rotation.
struct FlowAxes {
    bounds: Rect,
    writing: schist_text_engine::WritingMode,
}
impl FlowAxes {
    fn logical_bounds(&self) -> Rect {
        if self.writing == schist_text_engine::WritingMode::Horizontal {
            self.bounds
        } else {
            Rect::new(
                self.bounds.y,
                self.bounds.x,
                self.bounds.height,
                self.bounds.width,
            )
        }
    }
    fn remaining(&self, lines: &[ComposedLine], before_next: Pt) -> Rect {
        let gap = lines.last().map_or(0.0, |line| {
            line.paragraph.space_after.unwrap_or(0.0).max(0.0)
        }) + before_next;
        let mut bounds = self.bounds;
        match self.writing {
            schist_text_engine::WritingMode::Horizontal => {
                let bottom = lines
                    .iter()
                    .map(|line| line.bounds.bottom())
                    .fold(bounds.y, f32::max)
                    + gap;
                bounds.height = (bounds.bottom() - bottom).max(0.0);
                bounds.y = bottom;
            }
            schist_text_engine::WritingMode::VerticalRl => {
                let left = lines
                    .iter()
                    .map(|line| line.bounds.x)
                    .fold(bounds.right(), f32::min)
                    - gap;
                bounds.width = (left - bounds.x).max(0.0);
            }
            schist_text_engine::WritingMode::VerticalLr => {
                let right = lines
                    .iter()
                    .map(|line| line.bounds.right())
                    .fold(bounds.x, f32::max)
                    + gap;
                bounds.width = (bounds.right() - right).max(0.0);
                bounds.x = right;
            }
        }
        bounds
    }
    fn place_lines(&self, lines: &mut [ComposedLine]) {
        if self.writing == schist_text_engine::WritingMode::Horizontal {
            return;
        }
        for line in lines {
            let logical = line.bounds;
            let x = if self.writing == schist_text_engine::WritingMode::VerticalRl {
                self.bounds.right() - (logical.y - self.bounds.x) - logical.height
            } else {
                logical.y
            };
            line.bounds = Rect::new(x, logical.x, logical.height, logical.width);
            line.baseline = x + logical.height * 0.5;
        }
    }
}

/// Alignment moves the origin along the inline axis, including vertical text.
/// Preview, carets and print use the same rule with their own scaled rectangles.
pub fn aligned_origin(
    rect: Rect,
    inline_width: Pt,
    align: schist_text_engine::Align,
    writing: schist_text_engine::WritingMode,
) -> crate::Point {
    let vertical = writing != schist_text_engine::WritingMode::Horizontal;
    let measure = if vertical { rect.height } else { rect.width };
    let offset = match align {
        schist_text_engine::Align::Left => 0.0,
        schist_text_engine::Align::Center => (measure - inline_width) * 0.5,
        schist_text_engine::Align::Right => measure - inline_width,
    };
    crate::Point::new(
        rect.x + if vertical { 0.0 } else { offset },
        rect.y + if vertical { offset } else { 0.0 },
    )
}

/// Text insets, kept as a plain type so the engine does not depend on the
/// frame variant to read them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InsetsLike {
    pub top: Pt,
    pub right: Pt,
    pub bottom: Pt,
    pub left: Pt,
}

impl InsetsLike {
    pub fn resolve(self) -> crate::geometry::Insets {
        crate::geometry::Insets::new(self.top, self.right, self.bottom, self.left)
    }
}

impl From<crate::geometry::Insets> for InsetsLike {
    fn from(i: crate::geometry::Insets) -> InsetsLike {
        InsetsLike {
            top: i.top,
            right: i.right,
            bottom: i.bottom,
            left: i.left,
        }
    }
}

/// A story block with its style and internal flow range. A terminal blank's
/// flow end is one beyond the text; emitted text offsets stay within the story.
#[derive(Clone)]
struct Block {
    start: usize,
    end: usize,
    paragraph_start: Option<usize>,
    style: String,
    is_paragraph: bool,
}

/// The first opening initial in this frame, for UI decoration.
fn first_drop_cap(lines: &[ComposedLine]) -> Option<Rect> {
    lines
        .iter()
        .find_map(|line| line.initial.map(|initial| initial.ink))
}

/// Spread a story across a frame's columns so none is much shorter than
/// the others.
///
/// This only applies when the whole story fits. Text that overflows has
/// to keep going, so its columns fill in order and the last is short on
/// purpose; rebalancing that would leave a gap mid-article and read as a
/// mistake.
///
/// Find the smallest column height that fits the remaining text using the same
/// paragraph splitting, widow, keep-with-next and grid rules as ordinary flow.
/// Balancing must not impose an extra keep-together rule on every paragraph.
fn balance_columns(
    source: &FlowSource<'_>,
    start: usize,
    end: usize,
    columns: &[Rect],
    grid: Option<BaselineGrid>,
    options: &crate::footnotes::FootnoteOptions,
) -> (
    Vec<ComposedLine>,
    usize,
    Vec<crate::footnote_composition::NoteArea>,
) {
    let story = source.story;
    let Some(first) = columns.first() else {
        return (Vec::new(), start, Vec::new());
    };
    let fill = |height: f32| {
        let mut out = Vec::new();
        let mut notes = Vec::new();
        let mut cursor = start;
        for column in columns {
            let (lines, next, areas) = footnote_flow::fill(
                source,
                cursor,
                end,
                *column,
                grid,
                options,
                footnote_flow::FillPolicy {
                    body_height: Some(height),
                },
            );
            out.extend(lines);
            notes.extend(areas);
            cursor = next;
            if cursor + 1 == end && story.slice(cursor, end) == "\n" {
                cursor = end;
            }
            if cursor >= end {
                break;
            }
        }
        (out, cursor, notes)
    };
    let mut high = first.height;
    let mut best = fill(high);
    if best.1 < end {
        return (Vec::new(), start, Vec::new());
    }
    let mut low = 0.0;
    // Bounded work and sub-point precision; retain a proven fitting layout.
    for _ in 0..16 {
        if high - low <= 0.05 {
            break;
        }
        let middle = (low + high) * 0.5;
        let attempt = fill(middle);
        if attempt.1 >= end {
            high = middle;
            best = attempt;
        } else {
            low = middle;
        }
    }
    best
}

/// The line that stands in for a forced break.
fn break_line(
    story: &Story,
    doc: &LayoutDocument,
    column: &Rect,
    block: &Block,
    top: Pt,
    metrics: LineFlow,
) -> ComposedLine {
    let style = if block.style.is_empty() {
        &doc.default_paragraph_style
    } else {
        &block.style
    };
    let paragraph = doc.styles.resolve_paragraph(style);
    let spec = spec_for(
        story,
        block.start,
        block.start,
        &doc.styles,
        style,
        &doc.default_character_style,
        column.width,
    );
    let reverse = reverse_ruler(&spec);
    let measure = line_measure(
        &paragraph,
        *column,
        block.paragraph_start == Some(block.start),
        None,
        reverse,
    );
    ComposedLine {
        projected: None,
        generated: None,
        text_path: None,
        path_inline_start: None,
        start: block.start,
        end: block.end.min(story.text_len()),
        bounds: Rect::new(measure.x, top, measure.width, metrics.height),
        inline_origin: ruler_origin(*column, reverse),
        baseline: top + metrics.ascent,
        advance: metrics.advance,
        paragraph,
        paragraph_style: style.clone(),
        characters: Vec::new(),
        is_paragraph_end: true,
        discretionary_hyphen: false,
        generated_hyphen: false,
        forced_break: true,
        natural_width: 0.0,
        word_space: None,
        drop_cap: false,
        initial: None,
    }
}

/// Fill one column with a baseline grid in force.
///
/// Placement is tracked as a running height rather than a line index,
/// because paragraph spacing, drop caps and a grid leading all move a
/// line without adding a line. A line index cannot express "this starts
/// 18pt lower because the paragraph above it has space after it", and
/// every one of those features needs to.
fn fill_column_with_rules(
    source: &FlowSource<'_>,
    start: usize,
    end: usize,
    column: Rect,
    grid: Option<BaselineGrid>,
) -> (Vec<ComposedLine>, usize) {
    let story = source.story;
    let doc = source.doc;
    let mut lines: Vec<ComposedLine> = Vec::new();
    let mut cursor = start;
    let mut used = 0.0;
    let blocks = blocks(story, start, end);
    for block in &blocks {
        let paragraph = doc.styles.resolve_paragraph(if block.style.is_empty() {
            &doc.default_paragraph_style
        } else {
            &block.style
        });
        let gap = if lines.is_empty() {
            0.0
        } else {
            paragraph.space_before.unwrap_or(0.0).max(0.0)
        };
        used += gap;
        let placed = if block.is_paragraph {
            place_block(
                source,
                block,
                column,
                PlacementSpace {
                    top: column.y + used,
                    height: column.height - used,
                    previous: lines.last().map(PreviousLine::from_line),
                },
                grid,
            )
        } else {
            let spec = spec_for(
                story,
                block.start,
                block.start,
                &doc.styles,
                if block.style.is_empty() {
                    &doc.default_paragraph_style
                } else {
                    &block.style
                },
                &doc.default_character_style,
                column.width,
            );
            let Some(metrics) = schist_text_engine::measure(&spec) else {
                break;
            };
            let mut flow = LineFlow {
                ascent: metrics.first_baseline,
                height: metrics.height,
                advance: metrics.line_advance,
                writing: spec.writing_mode,
                absolute: spec.has_absolute_leading(),
            };
            let (top, advance) = grid_position(
                grid,
                column.y + used,
                flow,
                lines.last().map(PreviousLine::from_line),
            );
            if top + flow.height > column.bottom() {
                break;
            }
            flow.advance = advance;
            let mut line = break_line(story, doc, &column, block, top, flow);
            if paragraph.list.active() && block.paragraph_start == Some(block.start) {
                if let Some(marker) = source.markers.get(block.start) {
                    let body = marker.body_start(column.x, &paragraph);
                    let width = line.bounds.right() - body;
                    if width <= 0.0 {
                        break;
                    }
                    line.bounds.x = body;
                    line.bounds.width = width;
                }
            }
            Placed {
                lines: vec![line],
                consumed_to: block.end,
            }
        };
        if placed.lines.is_empty() {
            break;
        }
        cursor = placed.consumed_to;
        lines.extend(placed.lines);
        // Tight leading can put a smaller last cell inside an earlier large
        // cell. Continue from the last baseline, without inventing a gap from
        // the union of all preceding bounds; each cell was checked for fit.
        used = lines.last().unwrap().bounds.bottom() - column.y;
        if cursor < block.end {
            // A partial paragraph owns the rest of this column. Never skip
            // ahead to a smaller paragraph just because it could fit the gap.
            break;
        }
        if cursor < end {
            used += paragraph.space_after.unwrap_or(0.0).max(0.0);
        }
    }
    cursor = keep_flow::enforce(source, start, end, &mut lines, cursor);
    (lines, cursor.max(start))
}

/// The outcome of trying to place one paragraph into the room left.
struct Placed {
    lines: Vec<ComposedLine>,
    consumed_to: usize,
}

/// The active frame's page-relative baseline grid. Named frame grids use the
/// same page origin; a later page must never borrow the first page's margins.
#[derive(Clone, Copy)]
struct BaselineGrid {
    first: Pt,
    interval: Pt,
    mode: GridMode,
}

impl BaselineGrid {
    fn for_frame(doc: &LayoutDocument, object: ObjectId, page: usize) -> Option<Self> {
        let settings = doc.object(object).map_or(&doc.grids.document, |frame| {
            doc.grids.for_frame(&frame.name)
        });
        if settings.mode == GridMode::None {
            return None;
        }
        let page = doc.pages.get(page)?;
        Some(Self {
            first: settings.first_baseline(page)?,
            interval: settings.baseline_interval()?,
            mode: settings.mode,
        })
    }
}

/// Snap forward, accounting for the measured ascent and the previous baseline.
/// Explicit leading controls baseline/column-center spacing. Relative legacy
/// spacing reserves line boxes. Vertical columns do not use horizontal guides.
#[derive(Clone, Copy)]
struct LineFlow {
    ascent: Pt,
    height: Pt,
    advance: Pt,
    writing: schist_text_engine::WritingMode,
    absolute: bool,
}
impl LineFlow {
    fn from_span(span: &schist_text_engine::LineSpan, spec: &TextSpec) -> Self {
        Self {
            ascent: span.baseline - span.top,
            height: span.height,
            advance: span.advance,
            writing: spec.writing_mode,
            absolute: spec.has_absolute_leading(),
        }
    }
}
fn grid_position(
    grid: Option<BaselineGrid>,
    top: Pt,
    flow: LineFlow,
    previous: Option<PreviousLine>,
) -> (Pt, Pt) {
    let LineFlow {
        ascent,
        height,
        advance,
        writing,
        absolute,
    } = flow;
    let advance = advance.max(0.0);
    let top = previous.filter(|_| absolute).map_or(top, |previous| {
        let gap = (top - previous.bottom).max(0.0);
        if writing.is_vertical() {
            previous.bottom - previous.height / 2.0 + advance - height / 2.0 + gap
        } else {
            previous.baseline + advance - ascent + gap
        }
    });
    let Some(grid) = grid.filter(|_| writing == schist_text_engine::WritingMode::Horizontal) else {
        return (top, advance);
    };
    let minimum = previous.map_or(top + ascent, |previous| {
        (top + ascent).max(previous.baseline + if absolute { advance } else { previous.advance })
    });
    // Tolerance keeps a baseline already on a guide from jumping an interval.
    let steps = (((minimum - grid.first) / grid.interval) - 0.00001)
        .ceil()
        .max(0.0);
    let baseline = grid.first + steps * grid.interval;
    let advance = if grid.mode == GridMode::LinesPerGrid {
        ((advance / grid.interval - 0.00001).ceil().max(1.0)) * grid.interval
    } else {
        advance
    };
    ((baseline - ascent).max(top), advance)
}

#[derive(Clone, Copy)]
struct PreviousLine {
    baseline: Pt,
    advance: Pt,
    height: Pt,
    bottom: Pt,
}
impl PreviousLine {
    fn from_line(line: &ComposedLine) -> Self {
        Self {
            baseline: line.baseline,
            advance: line.advance,
            height: line.bounds.height,
            bottom: line.bounds.bottom(),
        }
    }
}

struct PlacementSpace {
    top: Pt,
    height: Pt,
    previous: Option<PreviousLine>,
}

/// Shape with the actual per-line measures, then select complete lines.
/// Measuring arbitrary prefixes can split a word at a frame boundary.
fn place_block(
    source: &FlowSource<'_>,
    block: &Block,
    column: Rect,
    space: PlacementSpace,
    grid: Option<BaselineGrid>,
) -> Placed {
    let story = source.story;
    let doc = source.doc;
    let paragraph = doc.styles.resolve_paragraph(&block.style);
    let first = block.paragraph_start == Some(block.start);
    // Resolve direction from the complete paragraph before assigning physical
    // first-line indents or taking a slice that starts with neutral text.
    let mut full_spec = spec_for(
        story,
        block.start,
        block.end,
        &doc.styles,
        &block.style,
        &doc.default_character_style,
        column.width,
    );
    let reverse = reverse_ruler(&full_spec);
    let normal = line_measure(&paragraph, column, false, None, reverse);
    let mut initial = line_measure(&paragraph, column, first, None, reverse);
    full_spec.wrap_width = Some(normal.width);
    let marker = (first && paragraph.list.active())
        .then(|| source.markers.get(block.start))
        .flatten();
    if let Some(marker) = &marker {
        let body = marker.body_start(column.x, &paragraph);
        initial.width = (initial.right() - body).max(0.0);
        initial.x = body;
        if initial.width <= 0.0 {
            return Placed {
                lines: Vec::new(),
                consumed_to: block.start,
            };
        }
    }
    let opening = if first && marker.is_none() {
        opening(&full_spec, &paragraph)
    } else {
        None
    };
    let prefix = opening.as_ref().map_or(0, |cap| cap.bytes);
    let spec = slice_spec(&full_spec, prefix, full_spec.text.len());
    let body = Block {
        start: block.start + prefix,
        ..block.clone()
    };
    let mut all = schist_text_engine::line_spans_with_measures(
        &spec,
        &[
            inline_measure(initial, column, reverse),
            inline_measure(normal, column, reverse),
        ],
    );
    let mut cap = None;
    if let Some(opening) = &opening {
        // Bound work for malformed imported counts. Preserve the text as
        // overset rather than silently changing the requested drop-cap size.
        if opening.lines > 4096 {
            return Placed {
                lines: Vec::new(),
                consumed_to: block.start,
            };
        }
        // Width affects wrapping, which affects mixed-font baselines. Refit the
        // initial to those baselines until the actual reserved geometry settles.
        let mut settled = false;
        for _ in 0..8 {
            let planned = plan_initial(opening, &spec, &all, initial, &space, grid);
            if !planned.area.bounds.width.is_finite() || planned.area.bounds.width >= normal.width {
                return Placed {
                    lines: Vec::new(),
                    consumed_to: block.start,
                };
            }
            if cap.as_ref().is_some_and(|old: &InitialPlan| {
                (old.area.bounds.width - planned.area.bounds.width).abs() < 0.001
                    && (old.ink.y - planned.ink.y).abs() < 0.001
                    && (old.ink.height - planned.ink.height).abs() < 0.001
            }) {
                settled = true;
                cap = Some(planned);
                break;
            }
            let measures: Vec<_> = (0..=opening.lines)
                .map(|i| {
                    inline_measure(
                        line_measure(
                            &paragraph,
                            column,
                            first && i == 0,
                            (i < opening.lines).then_some(planned.area),
                            reverse,
                        ),
                        column,
                        reverse,
                    )
                })
                .collect();
            all = schist_text_engine::line_spans_with_measures(&spec, &measures);
            cap = Some(planned);
        }
        if !settled {
            return Placed {
                lines: Vec::new(),
                consumed_to: block.start,
            };
        }
    }
    let measure_at = |index| {
        if index == 0 && marker.is_some() {
            initial
        } else {
            let area = cap
                .as_ref()
                .filter(|cap| index < cap.lines)
                .map(|cap| cap.area);
            line_measure(&paragraph, column, first && index == 0, area, reverse)
        }
    };
    let mut top = space.top;
    let mut previous = space.previous;
    let mut positions = Vec::new();
    for (index, span) in all.iter().enumerate() {
        if span.width > measure_at(index).width + 0.001
            && (spec.text[span.start..span.end].contains('\t')
                || spec.text[span.start..span.end]
                    .char_indices()
                    .any(|(at, _)| spec.no_break_at(span.start + at)))
        {
            break;
        }
        let flow = LineFlow::from_span(span, &spec);
        let (placed_top, advance) = grid_position(grid, top, flow, previous);
        if placed_top + span.height > space.top + space.height {
            break;
        }
        positions.push((placed_top, advance));
        previous = Some(PreviousLine {
            baseline: placed_top + flow.ascent,
            advance,
            height: span.height,
            bottom: placed_top + span.height,
        });
        top = placed_top + span.height;
    }
    let mut count = paragraph.keeps.fitting_lines(positions.len(), all.len());
    if cap.as_ref().is_some_and(|cap| {
        count < cap.lines.min(all.len()) || cap.bounds.bottom() > space.top + space.height
    }) {
        count = 0;
    }
    if count == 0 {
        return Placed {
            lines: Vec::new(),
            consumed_to: block.start,
        };
    }
    let mut lines: Vec<_> = all[..count]
        .iter()
        .enumerate()
        .map(|(i, span)| {
            let (top, advance) = positions[i];
            let area = cap.as_ref().filter(|cap| i < cap.lines).map(|cap| cap.area);
            let measure = measure_at(i);
            line_at(
                story,
                doc,
                &body,
                span,
                LinePlacement {
                    bounds: Rect::new(measure.x, top, measure.width, span.height),
                    advance,
                    inline_origin: ruler_origin(column, reverse),
                    is_paragraph_end: i + 1 == all.len(),
                    drop_cap: area.is_some(),
                },
            )
        })
        .collect();
    if let Some(cap) = cap {
        lines.insert(
            0,
            ComposedLine {
                projected: None,
                generated: None,
                text_path: None,
                path_inline_start: None,
                start: block.start,
                end: body.start,
                bounds: cap.bounds,
                inline_origin: ruler_origin(column, reverse),
                baseline: cap.baseline,
                advance: cap.bounds.height,
                paragraph: paragraph.clone(),
                paragraph_style: block.style.clone(),
                characters: character_styles(story, doc, block.start, body.start),
                is_paragraph_end: body.start == block.end,
                discretionary_hyphen: false,
                generated_hyphen: false,
                natural_width: cap.bounds.width,
                word_space: None,
                forced_break: false,
                drop_cap: false,
                initial: Some(Initial {
                    scale: cap.scale,
                    ink: cap.ink,
                }),
            },
        );
    }
    let consumed_to = all
        .get(count)
        .map_or(block.end, |next| body.start + next.start);
    Placed { lines, consumed_to }
}

#[derive(Clone, Copy)]
struct CapArea {
    bounds: Rect,
    right: bool,
}

struct Opening {
    spec: TextSpec,
    bytes: usize,
    lines: usize,
    metrics: schist_text_engine::TextMetrics,
    ink: [f32; 4],
    right: bool,
}

struct InitialPlan {
    lines: usize,
    scale: Pt,
    bounds: Rect,
    ink: Rect,
    baseline: Pt,
    area: CapArea,
}

/// Slice styled text without splitting a character range or changing its
/// already-resolved paragraph direction.
fn slice_spec(spec: &TextSpec, start: usize, end: usize) -> TextSpec {
    let mut out = spec.clone();
    out.text = spec.text[start..end].into();
    out.runs = spec
        .runs
        .iter()
        .filter(|r| r.start < end && r.end > start)
        .map(|r| {
            let mut run = r.clone();
            run.start = r.start.max(start) - start;
            run.end = r.end.min(end) - start;
            run
        })
        .collect();
    out
}

fn opening(spec: &TextSpec, paragraph: &ResolvedParagraph) -> Option<Opening> {
    let lines = paragraph.drop_caps_lines?;
    let characters = paragraph.drop_caps_characters.unwrap_or(1);
    if lines < 2
        || characters == 0
        || spec.writing_mode != schist_text_engine::WritingMode::Horizontal
    {
        return None;
    }
    let bytes = schist_text_engine::grapheme_boundaries(&spec.text)
        .nth(characters)
        .unwrap_or(spec.text.len());
    // An initial containing a source tab is retained and diagnosed, but its
    // native reservation/scaling semantics are not established. Fall back to
    // ordinary source flow; enlarging its ruler gap would invent geometry and
    // could push otherwise fitting text into overset.
    if spec.text[..bytes].contains('\t') {
        return None;
    }
    let mut initial = slice_spec(spec, 0, bytes);
    initial.wrap_width = None;
    initial.align = schist_text_engine::Align::Left;
    let painted = initial.clone();
    // Baseline offsets move ink, not the initial reservation or body leading.
    for run in &mut initial.runs {
        run.baseline_shift = None;
    }
    let metrics = schist_text_engine::measure(&initial)?;
    Some(Opening {
        spec: painted,
        bytes,
        lines,
        ink: metrics.ink_bounds?,
        metrics,
        right: spec.direction == schist_text_engine::ParagraphDirection::RightToLeft,
    })
}

fn plan_initial(
    opening: &Opening,
    body: &TextSpec,
    spans: &[schist_text_engine::LineSpan],
    measure: Rect,
    space: &PlacementSpace,
    grid: Option<BaselineGrid>,
) -> InitialPlan {
    // A capital H measures the body font's actual capital height without a
    // bitmap or a guessed width-to-height ratio.
    let mut probe = slice_spec(body, 0, body.text.chars().next().map_or(0, char::len_utf8));
    probe.text = "H".into();
    for run in &mut probe.runs {
        run.start = 0;
        run.end = 1;
        run.baseline_shift = None;
    }
    probe.wrap_width = None;
    let body_metrics = schist_text_engine::measure(&probe).unwrap();
    let cap_height = body_metrics
        .ink_bounds
        .map_or(body_metrics.first_baseline, |ink| {
            body_metrics.first_baseline - ink[1]
        });
    let mut top = space.top;
    let mut previous = space.previous;
    let mut first_ink_top = top;
    let mut baseline = top;
    for i in 0..opening.lines {
        let span = spans.get(i).or_else(|| spans.last());
        let ascent = span.map_or(body_metrics.first_baseline, |s| s.baseline - s.top);
        let advance = span.map_or(body_metrics.line_advance, |s| s.advance);
        let height = span.map_or(body_metrics.height, |s| s.height);
        let flow = LineFlow {
            ascent,
            height,
            advance,
            writing: body.writing_mode,
            absolute: body.has_absolute_leading(),
        };
        let (placed_top, advance) = grid_position(grid, top, flow, previous);
        baseline = placed_top + ascent;
        if i == 0 {
            first_ink_top = baseline - cap_height;
        }
        previous = Some(PreviousLine {
            baseline,
            advance,
            height,
            bottom: placed_top + height,
        });
        top = placed_top + height;
    }
    let scale = (baseline - first_ink_top) / (opening.ink[3] - opening.ink[1]);
    let width = (opening.ink[2] - opening.ink[0]) * scale;
    let ink = Rect::new(
        if opening.right {
            measure.right() - width
        } else {
            measure.x
        },
        first_ink_top,
        width,
        baseline - first_ink_top,
    );
    let bounds = Rect::new(
        ink.x - opening.ink[0] * scale,
        ink.y - opening.ink[1] * scale,
        opening.metrics.width * scale,
        opening.metrics.height * scale,
    );
    // Preserve the fixed point offsets when enlarging the initial's font.
    // Its painted extent moves, while the body-text reservation stays put.
    let mut painted = scale_initial_spec(opening.spec.clone(), scale);
    painted.wrap_width = None;
    let painted_ink = schist_text_engine::measure(&painted)
        .and_then(|m| m.ink_bounds)
        .map_or(ink, |b| {
            Rect::new(bounds.x + b[0], bounds.y + b[1], b[2] - b[0], b[3] - b[1])
        });
    // A small gap belongs to the reservation, not to the glyph width.
    let gap = body.size * 0.15;
    InitialPlan {
        lines: opening.lines,
        scale,
        bounds,
        ink: painted_ink,
        baseline: bounds.y + opening.metrics.first_baseline * scale,
        area: CapArea {
            bounds: Rect::new(
                if opening.right { ink.x - gap } else { ink.x },
                ink.y,
                width + gap,
                ink.height,
            ),
            right: opening.right,
        },
    }
}

fn reverse_ruler(spec: &TextSpec) -> bool {
    !spec.writing_mode.is_vertical()
        && spec.direction == schist_text_engine::ParagraphDirection::RightToLeft
}

fn ruler_origin(column: Rect, reverse: bool) -> Pt {
    if reverse {
        column.right()
    } else {
        column.x
    }
}

fn inline_measure(rect: Rect, column: Rect, reverse: bool) -> schist_text_engine::InlineMeasure {
    schist_text_engine::InlineMeasure {
        width: rect.width,
        start: if reverse {
            column.right() - rect.right()
        } else {
            rect.x - column.x
        },
    }
}

/// Horizontal measure used by both shaping and placement.
fn line_measure(
    paragraph: &ResolvedParagraph,
    column: Rect,
    first: bool,
    drop_cap: Option<CapArea>,
    reverse: bool,
) -> Rect {
    let indent = paragraph.left_indent.unwrap_or(0.0);
    let right_indent = paragraph.right_indent.unwrap_or(0.0);
    let first_indent = if first {
        paragraph.first_line_indent.unwrap_or(0.0)
    } else {
        0.0
    };
    let mut left = column.x + indent + if reverse { 0.0 } else { first_indent };
    let mut right = column.right() - right_indent - if reverse { first_indent } else { 0.0 };
    if let Some(cap) = drop_cap {
        if cap.right {
            right = right.min(cap.bounds.x);
        } else {
            left = left.max(cap.bounds.right());
        }
    }
    Rect::new(left, column.y, (right - left).max(0.0), column.height)
}

struct LinePlacement {
    bounds: Rect,
    advance: Pt,
    inline_origin: Pt,
    is_paragraph_end: bool,
    drop_cap: bool,
}

/// Build a composed line, carrying the character styles that cover it.
fn line_at(
    story: &Story,
    doc: &LayoutDocument,
    block: &Block,
    span: &schist_text_engine::LineSpan,
    placement: LinePlacement,
) -> ComposedLine {
    let start = block.start + span.start;
    let end = block.start + span.end;
    let paragraph = doc.styles.resolve_paragraph(&block.style);
    let is_paragraph_end = placement.is_paragraph_end;
    let word_space = word_space(
        story,
        &paragraph,
        start,
        end,
        span.width,
        placement.bounds.width,
        is_paragraph_end,
    );
    ComposedLine {
        projected: None,
        generated: None,
        text_path: None,
        path_inline_start: None,
        start,
        end,
        bounds: placement.bounds,
        inline_origin: placement.inline_origin,
        baseline: placement.bounds.y + span.baseline - span.top,
        advance: placement.advance,
        paragraph,
        paragraph_style: block.style.clone(),
        characters: character_styles(story, doc, start, end),
        is_paragraph_end,
        natural_width: span.width,
        discretionary_hyphen: span.discretionary_hyphen,
        generated_hyphen: span.generated_hyphen,
        word_space,
        forced_break: false,
        drop_cap: placement.drop_cap,
        initial: None,
    }
}

/// The extra points to add to each word space to justify a line flush.
///
/// Returns `None` when the line should not be stretched: it is not
/// justified, it is the last line of its paragraph, it is already at or
/// past the measure, or it is a single word with nothing to stretch. A
/// lone word left short is the one case where justification genuinely
/// cannot help, and letting it stretch would look worse than leaving it.
pub fn word_space(
    story: &Story,
    paragraph: &ResolvedParagraph,
    start: usize,
    end: usize,
    natural_width: Pt,
    measure: Pt,
    is_paragraph_end: bool,
) -> Option<Pt> {
    let align = paragraph.align?;
    if !align.is_justified() {
        return None;
    }
    // Expanding an aligned field could move its start behind the preceding
    // text or select another stop. Until native justification of these tabs
    // is implemented, keep natural spacing and diagnose this combination.
    if crate::tabs::has_aligned_stops(paragraph) && story.slice(start, end).contains('\t') {
        return None;
    }
    if is_paragraph_end && !matches!(align, Align::JustifyAll) {
        return None;
    }
    let spaces = count_spaces(story, start, end);
    if spaces == 0 {
        return None;
    }
    let slack = measure - natural_width;
    if slack <= 0.0 {
        // Already at or beyond the measure. Negative tracking is a
        // legitimate way to fit a line, and justification should not
        // then squeeze it further.
        return None;
    }
    Some(slack / spaces as Pt)
}

/// Count the ordinary spaces the engine expands. Tabbed lines justify their
/// final field; earlier fields remain anchored to the paragraph's stops.
pub fn count_spaces(story: &Story, start: usize, end: usize) -> usize {
    story
        .slice(start, end)
        .rsplit('\t')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| *c == ' ')
        .count()
}

/// The resolved character styles covering a byte range, nearest the start
/// of the line first so a renderer applies them left to right.
fn character_styles(
    story: &Story,
    doc: &LayoutDocument,
    start: usize,
    end: usize,
) -> Vec<ResolvedCharacter> {
    story
        .ranges
        .iter()
        .filter(|r| r.start < end && r.end > start)
        .map(|r| doc.styles.resolve_character(&r.style))
        .collect()
}

/// The story's points as composable blocks, clipped to `start..end`.
///
/// A paragraph already fully consumed is dropped: a paragraph ending
/// exactly at `start` is *not* left behind as a zero-length block, which
/// would place nothing and still let the caller think it made progress.
fn blocks(story: &Story, start: usize, end: usize) -> Vec<Block> {
    let offsets = story.point_offsets();
    let mut out = Vec::new();
    for (index, point) in story.points.iter().enumerate() {
        let from = offsets[index];
        let blank = matches!(point, Point::LineBreak)
            || matches!(point, Point::Paragraph { text, .. } if text.is_empty());
        let to = from + if blank { 1 } else { point.text().len() };
        if to <= start || from >= end {
            continue;
        }
        match point {
            Point::Paragraph { style, .. } => out.push(Block {
                start: from.max(start),
                end: to.min(end),
                paragraph_start: Some(from),
                style: style.clone(),
                is_paragraph: !blank,
            }),
            Point::LineBreak => out.push(Block {
                start: from.max(start),
                end: to.min(end),
                paragraph_start: None,
                style: String::new(),
                is_paragraph: false,
            }),
            // Structural destinations are processed by compose_thread;
            // opaque anchors have no measured content in this composer.
            _ => {}
        }
    }
    out
}

/// Compose an entire ordinary story in its explicit text-flow order.
pub fn compose_story(doc: &LayoutDocument, story: crate::StoryId) -> ComposedThread {
    let frames: Vec<_> = doc
        .story_frames(story)
        .into_iter()
        .filter_map(frame_input)
        .collect();
    compose_thread(doc, story, &frames)
}

fn frame_input(
    placed: &PlacedObject,
) -> Option<(ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike)> {
    let crate::LayoutObject::TextFrame {
        columns,
        gutter,
        insets,
        overflow,
        ..
    } = &placed.object
    else {
        return None;
    };
    Some((
        placed.id,
        placed.bounds,
        *overflow,
        *columns,
        *gutter,
        (*insets).into(),
    ))
}

/// A frame's portion of the whole story, never a fresh start at byte zero.
/// Parent-page content composes separately from ordinary document frames.
pub fn compose_object(doc: &LayoutDocument, placed: &PlacedObject) -> Option<ComposedFrame> {
    object_thread(doc, placed)?
        .frames
        .into_iter()
        .find(|frame| frame.object == placed.id)
}

fn object_thread(doc: &LayoutDocument, placed: &PlacedObject) -> Option<ComposedThread> {
    let crate::LayoutObject::TextFrame { story, .. } = placed.object else {
        return None;
    };
    let thread = if doc.object(placed.id).is_some() {
        compose_story(doc, story)
    } else {
        let mut frames: Vec<_> = doc
            .frame_thread(placed)
            .into_iter()
            .filter_map(frame_input)
            .collect();
        if frames.is_empty() {
            frames.push(frame_input(placed)?);
        }
        compose_thread_on_page(doc, story, &frames, Some(placed.page))
    };
    Some(thread)
}

/// A single immutable document pass composes each ordinary story once. Parent
/// instances additionally depend on their destination page's baseline grid.
/// The borrow prevents edits while entries are live; no revision guess can
/// leave stale text after an edit. Unknown standalone frames are not cached.
pub(crate) struct CompositionCache<'a> {
    doc: &'a LayoutDocument,
    threads: std::collections::HashMap<(crate::StoryId, Option<usize>), ComposedThread>,
}
impl<'a> CompositionCache<'a> {
    pub fn new(doc: &'a LayoutDocument) -> Self {
        Self {
            doc,
            threads: Default::default(),
        }
    }
    pub fn frame(&mut self, placed: &PlacedObject) -> Option<ComposedFrame> {
        let crate::LayoutObject::TextFrame { story, .. } = placed.object else {
            return None;
        };
        let page = if self.doc.object(placed.id).is_some() {
            None
        } else {
            if !self.doc.parents.iter().any(|parent| {
                parent
                    .objects
                    .iter()
                    .any(|entry| entry.object.id == placed.id)
            }) {
                return compose_object(self.doc, placed);
            }
            Some(placed.page)
        };
        let key = (story, page);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.threads.entry(key) {
            entry.insert(object_thread(self.doc, placed)?);
        }
        self.threads
            .get(&key)?
            .frames
            .iter()
            .find(|frame| frame.object == placed.id)
            .cloned()
    }
}

// Enlarge the font and spacing, retaining authored offsets in absolute points.
fn scale_initial_spec(mut spec: TextSpec, scale: Pt) -> TextSpec {
    if let Some(tabs) = &mut spec.tabs {
        tabs.scaled(scale);
    }
    spec.size *= scale;
    spec.leading = spec.leading.map(|v| v * scale);
    spec.tracking *= scale;
    spec.align = schist_text_engine::Align::Left;
    for run in &mut spec.runs {
        run.size = run.size.map(|v| v * scale);
        run.metric_size = run.metric_size.map(|v| v * scale);
        run.tracking = run.tracking.map(|v| v * scale);
        run.leading = run.leading.map(|v| v * scale);
    }
    spec
}

/// Render/edit one composed line without wrapping it again. A reserved blank
/// line carries an empty spec for its caret, never a newline creating two rows.
pub fn line_spec(line: &ComposedLine, story: &Story, doc: &LayoutDocument) -> TextSpec {
    if let Some(projected) = &line.projected {
        return projected.spec.clone();
    }
    if let Some(generated) = &line.generated {
        return generated.spec.clone();
    }
    let mut spec = spec_for(
        story,
        line.start,
        line.end,
        &doc.styles,
        &line.paragraph_style,
        &doc.default_character_style,
        0.0,
    );
    if let Some(initial) = line.initial {
        spec = scale_initial_spec(spec, initial.scale);
    }
    spec = with_leading(spec, line.advance);
    spec.show_final_soft_hyphen = line.discretionary_hyphen;
    spec.show_final_generated_hyphen = line.generated_hyphen;
    spec.word_spacing = line.word_space.unwrap_or(0.0);
    let reverse = reverse_ruler(&spec);
    if let Some(tabs) = &mut spec.tabs {
        let width = if spec.writing_mode.is_vertical() {
            line.bounds.height
        } else {
            line.bounds.width
        };
        tabs.line_width = (width > 0.0).then_some(width);
        tabs.origin = line.path_inline_start.unwrap_or_else(|| {
            if reverse {
                line.inline_origin - line.bounds.right()
            } else if spec.writing_mode.is_vertical() {
                line.bounds.y - line.inline_origin
            } else {
                line.bounds.x - line.inline_origin
            }
        });
    }
    if let Some(path) = &line.text_path {
        spec.path = Some(path.clone());
        spec.align = schist_text_engine::Align::Left;
    }
    if line.forced_break {
        spec.text.clear();
        spec.runs.clear();
    }
    spec
}

/// Character paint for each engine run, in exactly the same precedence order
/// as line_spec. Shared by ordinary and projected text and native separation.
pub fn line_paint_styles(
    line: &ComposedLine,
    story: &Story,
    doc: &LayoutDocument,
) -> Vec<ResolvedCharacter> {
    if let Some(projected) = &line.projected {
        return projected.paints.clone();
    }
    let spec = line_spec(line, story, doc);
    let base = line
        .paragraph
        .character(doc.styles.resolve_character(&doc.default_character_style));
    let ranges: Vec<_> = story
        .ranges
        .iter()
        .filter(|r| r.start < line.end && r.end > line.start)
        .collect();
    (0..spec.runs.len())
        .map(|index| {
            line.generated
                .as_ref()
                .map(|g| g.character.clone())
                .unwrap_or_else(|| {
                    ranges.get(index).map_or_else(
                        || base.clone(),
                        |range| {
                            doc.styles
                                .resolve_character(&range.style)
                                .with_paint_defaults(&base)
                        },
                    )
                })
        })
        .collect()
}

/// The ink a composed line's text prints in, defaulting to the document
/// black when no character style names one.
pub fn line_ink(line: &ComposedLine, doc: &LayoutDocument) -> Option<Ink> {
    line.characters
        .iter()
        .rev()
        .find_map(|c| c.fill.clone())
        .or_else(|| doc.ink(&doc.default_character_style).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{mm, Rect};
    use crate::model::{blank_a4, StoryId};
    use crate::story::Story;
    use crate::styles::{CharacterStyle, ParagraphStyle};

    #[test]
    fn a_document_pass_composes_each_thread_once_without_changing_frame_results() {
        for count in [1, 2, 7] {
            let mut doc = blank_a4();
            let mut frames = Vec::new();
            for _ in 0..count {
                frames.push(
                    crate::authoring::text_frame(
                        &mut doc,
                        &mut crate::History::default(),
                        0,
                        Rect::new(20.0, 20.0, 120.0, 45.0),
                    )
                    .unwrap(),
                );
            }
            let story = frames[0].story;
            doc.stories[story.0 as usize] =
                Story::from_text("Aé body with words. ".repeat(20), "Body");
            for frame in &mut doc.objects {
                if let crate::LayoutObject::TextFrame {
                    story: id,
                    overflow,
                    ..
                } = &mut frame.object
                {
                    *id = story;
                    *overflow = FrameOverflow::Thread;
                }
            }
            let mut cache = CompositionCache::new(&doc);
            for _ in 0..3 {
                for frame in doc.objects.iter().rev() {
                    assert_eq!(cache.frame(frame), compose_object(&doc, frame));
                    assert_eq!(cache.threads.len(), 1);
                }
            }
        }
    }

    #[test]
    fn cached_parent_threads_keep_destination_page_grids_separate() {
        let mut doc = blank_a4();
        doc.pages.push(doc.pages[0].clone());
        doc.pages[0].margins.top = 3.0;
        doc.pages[1].margins.top = 9.0;
        doc.grids.document.mode = crate::GridMode::SnapToGrid;
        doc.grids.document.baseline_count = 72.0 / 14.0;
        let frame = crate::authoring::text_frame(
            &mut doc,
            &mut crate::History::default(),
            0,
            Rect::new(30.0, 40.0, 130.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] =
            Story::from_text("Parent text on each destination grid.", "Body");
        let object = doc.objects.remove(0);
        doc.parents.push(crate::ParentPage {
            name: "A".into(),
            applied_to: vec![0, 1],
            based_on: None,
            hidden: false,
            sheets: Vec::new(),
            placements: Vec::new(),
            objects: vec![crate::ParentObject {
                object,
                overridden_on: Vec::new(),
            }],
        });
        let mut cache = CompositionCache::new(&doc);
        let mut baselines = Vec::new();
        for page in [0, 1, 0, 1] {
            let object = doc.page_objects(page).remove(0);
            let cached = cache.frame(&object).unwrap();
            assert_eq!(Some(cached.clone()), compose_object(&doc, &object));
            baselines.push(cached.lines[0].baseline);
        }
        assert_ne!(baselines[0], baselines[1]);
        assert_eq!(baselines[..2], baselines[2..]);
        assert_eq!(cache.threads.len(), 2);
    }

    fn doc_with(story_text: &str) -> (LayoutDocument, StoryId) {
        let mut doc = blank_a4();
        let id = doc.add_story(Story::from_text(story_text, "Body"));
        (doc, id)
    }

    fn frame(id: ObjectId, bounds: Rect) -> (ObjectId, Rect, FrameOverflow, u16, Pt, InsetsLike) {
        (
            id,
            bounds,
            FrameOverflow::Thread,
            1,
            0.0,
            InsetsLike::default(),
        )
    }

    #[test]
    fn columns_split_evenly_and_leave_the_gutter() {
        let rect = Rect::new(0.0, 0.0, 100.0, 200.0);
        let cols = columns(rect, 3, 10.0);
        assert_eq!(cols.len(), 3);
        // 100 - 2*10 gutter = 80, /3 each
        assert!((cols[0].width - (80.0 / 3.0)).abs() < 1e-4);
        assert!((cols[1].x - (80.0 / 3.0 + 10.0)).abs() < 1e-4);
        // All three together plus gutters span the frame.
        let total = cols[2].right() - cols[0].x;
        assert!((total - 100.0).abs() < 1e-4);
    }

    #[test]
    fn a_single_column_frame_has_no_gutter_math() {
        let rect = Rect::new(5.0, 5.0, 50.0, 50.0);
        assert_eq!(columns(rect, 1, 20.0), vec![rect]);
        // Zero columns is treated as one rather than producing nothing.
        assert_eq!(columns(rect, 0, 20.0), vec![rect]);
    }

    #[test]
    fn a_tall_frame_holds_the_whole_story() {
        let (doc, story) = doc_with("The quick brown fox jumps over the lazy dog.");
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        assert!(!thread.has_overflow());
        assert!(thread.lines().count() > 0);
    }

    #[test]
    fn a_short_frame_overflows_and_a_second_frame_takes_the_rest() {
        let text = "The quick brown fox jumps over the lazy dog. \
             Pack my box with five dozen liquor jugs. \
             How vexingly quick daft zebras jump! \
             Sphinx of black quartz, judge my vow. \
             Jackdaws love my big sphinx of quartz. \
             The five boxing wizards jump quickly at dawn. "
            .repeat(3);
        let (doc, story) = doc_with(&text);
        let spans = line_spans(&spec_for(
            doc.story(story).unwrap(),
            0,
            text.len(),
            &doc.styles,
            "Body",
            &doc.default_character_style,
            mm(170.0),
        ));
        assert!(
            spans.len() >= 4,
            "two-line keeps need four or more lines to split"
        );
        let height = spans[..2]
            .iter()
            .map(|line| line.top + line.height)
            .fold(0.0, f32::max)
            + 0.01;
        // A frame two lines tall: the body style keeps two lines
        // together, so a one-line frame could hold nothing at all.
        let thread = compose_thread(
            &doc,
            story,
            &[
                frame(ObjectId::next(), Rect::new(0.0, 0.0, mm(170.0), height)),
                frame(
                    ObjectId::next(),
                    Rect::new(0.0, mm(100.0), mm(170.0), mm(200.0)),
                ),
            ],
        );
        assert_eq!(thread.frames.len(), 2);
        // The first frame overflowed and passed the rest on...
        assert!(thread.frames[0].passed_on);
        // ...without losing any of it.
        assert!(!thread.frames[0].lost);
        assert!(!thread.has_overflow());
        assert_eq!(thread.frames[0].lines.len(), 2);
        // ...and the second picked it up, continuing where the first
        // stopped.
        assert!(!thread.frames[1].lines.is_empty());
        assert_eq!(
            thread.frames[0].consumed_to,
            thread.frames[1].lines[0].start
        );
    }

    #[test]
    fn lines_do_not_overlap_across_frames() {
        let (doc, story) = doc_with(
            "One two three four five six seven eight nine ten. \
             Eleven twelve thirteen fourteen fifteen sixteen. \
             Seventeen eighteen nineteen twenty twenty-one.",
        );
        let thread = compose_thread(
            &doc,
            story,
            &[
                frame(ObjectId::next(), Rect::new(0.0, 0.0, mm(170.0), mm(10.0))),
                frame(
                    ObjectId::next(),
                    Rect::new(0.0, mm(50.0), mm(170.0), mm(10.0)),
                ),
                frame(
                    ObjectId::next(),
                    Rect::new(0.0, mm(100.0), mm(170.0), mm(200.0)),
                ),
            ],
        );
        // Every line is within its own frame's vertical extent.
        for frame in &thread.frames {
            for line in &frame.lines {
                assert!(line.bounds.y >= 0.0);
            }
        }
        // And the byte ranges do not go backwards.
        let mut last = 0;
        for line in thread.lines() {
            assert!(line.start >= last);
            last = line.start;
        }
    }

    #[test]
    fn a_story_shorter_than_one_line_still_places_one_line() {
        let (doc, story) = doc_with("Short.");
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        assert_eq!(thread.lines().count(), 1);
        assert!(!thread.has_overflow());
    }

    #[test]
    fn a_frame_too_short_for_any_line_holds_nothing_rather_than_overflowing_its_bounds() {
        let (doc, story) = doc_with("Some text that needs a line of height.");
        let thread = compose_thread(
            &doc,
            story,
            &[frame(ObjectId::next(), Rect::new(0.0, 0.0, mm(170.0), 1.0))],
        );
        // Nothing is placed outside the frame.
        assert!(thread.lines().count() == 0);
    }

    #[test]
    fn a_missing_story_composes_to_nothing_rather_than_panicking() {
        let doc = blank_a4();
        let thread = compose_thread(
            &doc,
            StoryId(99),
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        assert_eq!(thread.frames.len(), 1);
        assert_eq!(thread.lines().count(), 0);
    }

    #[test]
    fn insets_shrink_the_column_and_therefore_the_text_area() {
        let (doc, story) = doc_with("Some words to lay out here.");
        let wide = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        let insets = InsetsLike {
            top: 0.0,
            bottom: 0.0,
            left: 40.0,
            right: 40.0,
        };
        let narrow = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
                FrameOverflow::Clip,
                1,
                0.0,
                insets,
            )],
        );
        // The narrow column starts further right.
        assert!(narrow.lines().next().unwrap().bounds.x > 0.0);
        assert_eq!(wide.lines().count(), narrow.lines().count());
    }

    #[test]
    fn a_last_frame_reports_no_threadable_overflow() {
        let (doc, story) = doc_with("A story too long for its single short frame.");
        let thread = compose_thread(
            &doc,
            story,
            &[frame(ObjectId::next(), Rect::new(0.0, 0.0, mm(170.0), 4.0))],
        );
        // The text is lost, but the frame is not claiming to hand it on.
        assert!(!thread.frames[0].passed_on);
        assert!(thread.frames[0].lost);
    }

    #[test]
    fn text_wraps_to_the_column_width() {
        // Without a wrap width the engine returns the whole story as one
        // unbounded line, and every frame would look able to hold it.
        let (doc, story) = doc_with(
            "Pack my box with five dozen liquor jugs, and how vexingly quick daft zebras jump.",
        );
        let narrow = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(60.0), mm(200.0)),
            )],
        );
        assert!(narrow.lines().count() > 1);
    }

    #[test]
    fn a_narrower_column_needs_more_lines() {
        let text =
            "Pack my box with five dozen liquor jugs, and how vexingly quick daft zebras jump.";
        let (doc, story) = doc_with(text);
        let count = |w: Pt| {
            compose_thread(
                &doc,
                story,
                &[frame(ObjectId::next(), Rect::new(0.0, 0.0, w, mm(400.0)))],
            )
            .lines()
            .count()
        };
        assert!(count(mm(40.0)) > count(mm(150.0)));
    }

    #[test]
    fn lines_never_exceed_their_frames_height() {
        // The budget has to hold across every pass fill_column makes,
        // not just the first measurement.
        let (doc, story) = doc_with(
            "The quick brown fox jumps over the lazy dog. Pack my box with five dozen \
             liquor jugs. How vexingly quick daft zebras jump! Sphinx of black quartz, \
             judge my vow. Jackdaws love my big sphinx of quartz.",
        );
        let bounds = Rect::new(0.0, 0.0, mm(170.0), mm(10.0));
        let thread = compose_thread(&doc, story, &[frame(ObjectId::next(), bounds)]);
        for line in thread.lines() {
            assert!(
                line.bounds.bottom() <= bounds.bottom() + 0.01,
                "line at {:?} escapes the frame",
                line.bounds
            );
        }
    }

    #[test]
    fn a_story_splits_across_a_thread_without_gaps_or_repeats() {
        let text = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen \
                    liquor jugs. How vexingly quick daft zebras jump!";
        let (doc, story) = doc_with(text);
        let thread = compose_thread(
            &doc,
            story,
            &[
                frame(ObjectId::next(), Rect::new(0.0, 0.0, mm(170.0), mm(12.0))),
                frame(
                    ObjectId::next(),
                    Rect::new(0.0, mm(50.0), mm(170.0), mm(12.0)),
                ),
                frame(
                    ObjectId::next(),
                    Rect::new(0.0, mm(100.0), mm(170.0), mm(400.0)),
                ),
            ],
        );
        // Each frame resumes exactly where the previous one stopped.
        for pair in thread.frames.windows(2) {
            if let Some(line) = pair[1].lines.first() {
                assert_eq!(pair[0].consumed_to, line.start);
            } else {
                assert_eq!(pair[0].consumed_to, pair[1].consumed_to);
            }
        }
        // The last frame consumed the whole story.
        assert_eq!(thread.frames.last().unwrap().consumed_to, text.len());
        // And the byte ranges cover it once, in order.
        let mut covered = 0usize;
        for line in thread.lines() {
            assert!(line.start >= covered);
            covered = line.end;
        }
    }

    #[test]
    fn justification_left_aligns_while_measuring() {
        // The engine cannot justify, so it must report natural widths and
        // the renderer does the stretching.
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Just".into(),
            based_on: Some("Default".into()),
            align: Some(Align::Justify),
            point_size: Some(11.0),
            ..Default::default()
        });
        doc.default_paragraph_style = "Just".into();
        let story = doc.add_story(Story::from_text(
            "Pack my box with five dozen liquor jugs.",
            "Just",
        ));
        let s = doc.story(story).unwrap();
        let spec = spec_for(
            s,
            0,
            s.text_len(),
            &doc.styles,
            &doc.default_paragraph_style,
            &doc.default_character_style,
            mm(170.0),
        );
        assert_eq!(spec.align, schist_text_engine::Align::Left);
    }

    #[test]
    fn a_slice_lands_on_char_boundaries() {
        // "é" is two bytes, so a range can land inside one.
        let mut story = Story::new();
        story.push_paragraph("café latte", "Default");
        let s = story.slice(3, 6);
        // Byte 3..6 would split the é if boundaries were not respected.
        assert!(s.chars().all(|c| c != '\u{fffd}'));
        assert!(std::str::from_utf8(s.as_bytes()).is_ok());
    }

    #[test]
    fn absolute_leading_is_independent_of_font_metrics() {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Tight".into(),
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(13.5)),
            ..Default::default()
        });
        let story = doc.add_story(Story::from_text("Hxy", "Tight"));
        let s = doc.story(story).unwrap();
        let spec = spec_for(
            s,
            0,
            s.text_len(),
            &doc.styles,
            "Tight",
            &doc.default_character_style,
            mm(170.0),
        );
        // Whatever the font, the advance must come back as the 13.5pt
        // the style asked for.
        let spans = schist_text_engine::line_spans(&spec);
        assert!(
            (spans[0].advance - 13.5).abs() < 0.01,
            "advance {}",
            spans[0].advance
        );
    }

    #[test]
    fn a_style_naming_no_font_still_lays_out() {
        // An empty family would fail to load any face and produce
        // nothing at all.
        let (doc, story) = doc_with("Some text at all.");
        let s = doc.story(story).unwrap();
        let spec = spec_for(
            s,
            0,
            s.text_len(),
            &doc.styles,
            &doc.default_paragraph_style,
            &doc.default_character_style,
            mm(170.0),
        );
        assert!(!spec.family.is_empty());
        assert!(!schist_text_engine::line_spans(&spec).is_empty());
    }

    #[test]
    fn each_paragraph_keeps_its_own_style() {
        // Composition walks the story's points, so a style change
        // between paragraphs must survive it.
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Small".into(),
            based_on: Some("Default".into()),
            point_size: Some(7.0),
            ..Default::default()
        });
        let mut story = Story::new();
        story.push_paragraph("Big opening line of the story.", "Body");
        story.push_paragraph("Tiny second line of the story.", "Small");
        let id = doc.add_story(story);
        let thread = compose_thread(
            &doc,
            id,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        let sizes: Vec<f32> = thread
            .lines()
            .map(|l| l.paragraph.point_size.unwrap_or(0.0))
            .collect();
        assert!(sizes.contains(&11.0), "body size missing from {sizes:?}");
        assert!(sizes.contains(&7.0), "small size missing from {sizes:?}");
    }

    #[test]
    fn a_character_style_reaches_the_lines_it_covers() {
        let mut doc = blank_a4();
        doc.styles.add_character(CharacterStyle {
            name: "Em".into(),
            based_on: Some("Default".into()),
            italic: Some(true),
            ..Default::default()
        });
        let mut story = Story::new();
        story.push_paragraph(
            "Plain words at the start, then some emphasised words, then plain words again \
             to make sure the paragraph wraps over several lines in a narrow column.",
            "Body",
        );
        // "emphasised" sits after the first stretch of plain text.
        let text = story.points[0].text().len();
        let at = "Plain words at the start, then some ".len();
        story.apply_style(at, at + "emphasised".len(), "Em");
        let id = doc.add_story(story);
        let thread = compose_thread(
            &doc,
            id,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(50.0), mm(200.0)),
            )],
        );
        assert!(text > 0);
        let styled: Vec<&ComposedLine> = thread
            .lines()
            .filter(|l| !l.characters.is_empty())
            .collect();
        assert!(!styled.is_empty(), "no line picked up the range");
        assert_eq!(styled[0].characters[0].italic, Some(true));
        // Lines outside the range carry none.
        assert!(
            thread.lines().any(|l| l.characters.is_empty()),
            "every line picked up the range"
        );
    }

    #[test]
    fn every_column_of_a_multi_column_frame_is_filled() {
        let mut story = Story::new();
        for i in 0..24 {
            story.push_paragraph(
                format!(
                    "Paragraph {i} of body copy, long enough to wrap more than once in a \
                         narrow measure and therefore to need the next column."
                ),
                "Body",
            );
        }
        let mut doc = blank_a4();
        let id = doc.add_story(story);
        // Enough room for some text, nowhere near all of it.
        let bounds = Rect::new(0.0, 0.0, mm(170.0), mm(60.0));
        let single = compose_thread(&doc, id, &[frame(ObjectId::next(), bounds)]);
        let two = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                bounds,
                FrameOverflow::Thread,
                2,
                mm(5.0),
                InsetsLike::default(),
            )],
        );
        assert!(single.has_overflow(), "the test needs more text than fits");
        // Two columns hold strictly more than one.
        assert!(two.lines().count() > single.lines().count());
        let cols = columns(bounds, 2, mm(5.0));
        let xs: Vec<f32> = two.lines().map(|l| l.bounds.x).collect();
        // The text is split across both columns rather than all landing
        // in the first.
        assert!(
            xs.iter().any(|x| (*x - cols[0].x).abs() < 0.01),
            "first column empty"
        );
        assert!(
            xs.iter().any(|x| (*x - cols[1].x).abs() < 0.01),
            "second column empty"
        );
    }

    #[test]
    fn a_column_break_ends_a_column_early() {
        let mut story = Story::new();
        story.push_paragraph("First column text that is long enough to wrap.", "Body");
        story.points.push(Point::ColumnBreak);
        story.push_paragraph("Second column text after the break.", "Body");
        let mut doc = blank_a4();
        let id = doc.add_story(story);
        let bounds = Rect::new(0.0, 0.0, mm(170.0), mm(200.0));
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                bounds,
                FrameOverflow::Thread,
                2,
                mm(5.0),
                InsetsLike::default(),
            )],
        );
        let xs: Vec<f32> = thread.lines().map(|l| l.bounds.x).collect();
        let cols = columns(bounds, 2, mm(5.0));
        // The text after the break is in the second column.
        assert!(xs.iter().any(|x| (*x - cols[1].x).abs() < 0.01));
    }

    fn justified_doc() -> (LayoutDocument, StoryId) {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Just".into(),
            based_on: Some("Default".into()),
            align: Some(Align::Justify),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            ..Default::default()
        });
        doc.default_paragraph_style = "Just".into();
        let story = doc.add_story(Story::from_text(
            "Pack my box with five dozen liquor jugs and then consider what else might be \
             needed to fill the column with a good deal of ordinary prose for a while yet.",
            "Just",
        ));
        (doc, story)
    }

    fn wrap_in(text: &str) -> Story {
        let mut story = Story::new();
        story.push_paragraph(text, "Body");
        story
    }

    #[test]
    fn a_justified_line_carries_the_slack_it_needs() {
        let (doc, story) = justified_doc();
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(200.0)),
            )],
        );
        let stretched: Vec<&ComposedLine> =
            thread.lines().filter(|l| l.word_space.is_some()).collect();
        assert!(!stretched.is_empty(), "no line was justified");
        for line in stretched {
            let extra = line.word_space.unwrap();
            assert!(extra > 0.0, "a justified line was given no slack");
            // The spaces must add up to exactly the slack, or the line
            // is either short of the measure or overshoots it.
            let spaces = count_spaces(doc.story(story).unwrap(), line.start, line.end);
            let total = line.natural_width + extra * spaces as Pt;
            assert!(
                (total - line.bounds.width).abs() < 0.5,
                "{total} vs {}",
                line.bounds.width
            );
        }
    }

    #[test]
    fn the_last_line_of_a_justified_paragraph_is_left_ragged() {
        let (doc, story) = justified_doc();
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(200.0)),
            )],
        );
        let last = thread.lines().last().expect("no lines");
        assert!(last.is_paragraph_end);
        assert_eq!(last.word_space, None, "the last line was stretched");
    }

    #[test]
    fn justify_all_stretches_the_last_line_too() {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "All".into(),
            based_on: Some("Default".into()),
            align: Some(Align::JustifyAll),
            point_size: Some(11.0),
            ..Default::default()
        });
        doc.default_paragraph_style = "All".into();
        let story = doc.add_story(Story::from_text("One short paragraph of text here.", "All"));
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(200.0)),
            )],
        );
        let last = thread.lines().last().unwrap();
        assert!(last.is_paragraph_end);
        assert!(
            last.word_space.is_some(),
            "JustifyAll left the last line ragged"
        );
    }

    #[test]
    fn a_ragged_line_carries_no_stretch() {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Ragged".into(),
            based_on: Some("Default".into()),
            align: Some(Align::Left),
            point_size: Some(11.0),
            ..Default::default()
        });
        doc.default_paragraph_style = "Ragged".into();
        let story = doc.add_story(Story::from_text(
            "Pack my box with five dozen liquor jugs and then consider what else might be needed.",
            "Ragged",
        ));
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(200.0)),
            )],
        );
        assert!(
            thread.lines().count() > 1,
            "the test needs more than one line"
        );
        assert!(thread.lines().all(|l| l.word_space.is_none()));
    }

    #[test]
    fn a_line_with_no_space_is_left_alone() {
        // A single long word cannot be justified by stretching a space.
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Just".into(),
            based_on: Some("Default".into()),
            align: Some(Align::Justify),
            point_size: Some(24.0),
            ..Default::default()
        });
        doc.default_paragraph_style = "Just".into();
        let story = doc.add_story(Story::from_text(
            "Donaudampfschiffahrtsgesellschaftskapitaen",
            "Just",
        ));
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(40.0), mm(200.0)),
            )],
        );
        assert!(thread.lines().all(|l| l.word_space.is_none()));
    }

    #[test]
    fn a_paragraph_split_across_columns_keeps_its_last_line_justified() {
        // The last line in a column is not the last line of the
        // paragraph, so it is stretched. Leaving it ragged puts a visible
        // notch at the foot of every column of a long article.
        let (doc, story) = justified_doc();
        // Two lines tall, so the paragraph cannot possibly fit.
        let thread = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(10.0)),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        let placed = thread.frames[0].lines.last().expect("no lines");
        assert!(
            !placed.is_paragraph_end,
            "a split paragraph reported its end at byte {} of {}",
            placed.end,
            doc.story(story).unwrap().text_len()
        );
        assert!(
            placed.word_space.is_some(),
            "the last line before a break was left ragged"
        );
    }

    #[test]
    fn columns_are_balanced_when_the_story_fits() {
        // Six short paragraphs in a three-column frame: a sequential
        // fill would put all six in the first column and leave two
        // empty, which reads as a mistake rather than a layout.
        let mut doc = blank_a4();
        let mut story = Story::new();
        for i in 0..6 {
            story.push_paragraph(format!("Paragraph number {i} of the body copy."), "Body");
        }
        let id = doc.add_story(story);
        let bounds = Rect::new(0.0, 0.0, mm(170.0), mm(120.0));
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                bounds,
                FrameOverflow::Clip,
                3,
                mm(5.0),
                InsetsLike::default(),
            )],
        );
        assert!(!thread.has_overflow(), "the test needs the story to fit");
        let cols = columns(bounds, 3, mm(5.0));
        let per_column: Vec<usize> = cols
            .iter()
            .map(|c| {
                thread
                    .lines()
                    .filter(|l| (l.bounds.x - c.x).abs() < 0.01)
                    .count()
            })
            .collect();
        assert!(
            per_column.iter().all(|n| *n > 0),
            "a column is empty: {per_column:?}"
        );
        let max = *per_column.iter().max().unwrap();
        let min = *per_column.iter().min().unwrap();
        // Evenly enough that no column is a whole paragraph longer.
        assert!(max - min <= 2, "columns are lopsided: {per_column:?}");
    }

    #[test]
    fn an_overflowing_multi_column_frame_is_not_balanced() {
        // Text that has to keep going fills in order; a gap in the middle
        // of an article reads as a mistake.
        let mut doc = blank_a4();
        let mut story = Story::new();
        for i in 0..24 {
            story.push_paragraph(
                format!("Paragraph {i} of body copy, long enough to wrap more than once."),
                "Body",
            );
        }
        let id = doc.add_story(story);
        let bounds = Rect::new(0.0, 0.0, mm(170.0), mm(60.0));
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                bounds,
                FrameOverflow::Thread,
                2,
                mm(5.0),
                InsetsLike::default(),
            )],
        );
        assert!(thread.has_overflow());
        let cols = columns(bounds, 2, mm(5.0));
        // The first column is full: the article is running past it.
        let in_first = thread
            .lines()
            .filter(|l| (l.bounds.x - cols[0].x).abs() < 0.01)
            .count();
        assert!(in_first > 1, "the first column is not full");
    }

    #[test]
    fn a_single_column_frame_is_never_balanced() {
        let mut doc = blank_a4();
        let id = doc.add_story(wrap_in(
            "One two three four five six seven eight nine ten eleven twelve.",
        ));
        let thread = compose_thread(
            &doc,
            id,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(80.0), mm(200.0)),
            )],
        );
        // Everything lands in the one column, whatever its height.
        assert!(thread.lines().count() > 0);
        assert!(!thread.has_overflow());
    }

    fn styled(paragraph: ParagraphStyle, text: &str) -> (LayoutDocument, StoryId) {
        let mut doc = blank_a4();
        let name = paragraph.name.clone();
        doc.styles.add_paragraph(ParagraphStyle {
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            ..paragraph
        });
        doc.default_paragraph_style = name.clone();
        let story = doc.add_story(Story::from_text(text, name));
        (doc, story)
    }

    const PROSE: &str = "The quick brown fox jumps over the lazy dog while the pack my box \
         with five dozen liquor jugs waits for how vexingly quick daft zebras jump along";

    /// Two one-line paragraphs, both in the document's default style.
    /// A document whose default style is `style`, holding two paragraphs.
    fn doc_with_style(style: ParagraphStyle) -> LayoutDocument {
        let mut doc = blank_a4();
        let name = style.name.clone();
        doc.styles.add_paragraph(ParagraphStyle {
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            keep_lines: Some(1),
            ..style
        });
        doc.default_paragraph_style = name;
        doc
    }

    /// The y of the second paragraph's first line.
    fn second_paragraph_y(mut doc: LayoutDocument) -> Pt {
        let id = doc.add_story(two_paragraphs_with(&doc));
        compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        )
        .lines()
        .nth(1)
        .map(|l| l.bounds.y)
        .expect("no second line")
    }

    #[test]
    fn space_before_pushes_a_paragraph_down() {
        // Measured on the second paragraph: the first one's space before
        // is dropped at the top of a column, so it cannot show a
        // difference.
        let plain = second_paragraph_y(doc_with_style(ParagraphStyle {
            name: "Plain".into(),
            ..Default::default()
        }));
        let spaced = second_paragraph_y(doc_with_style(ParagraphStyle {
            name: "Spaced".into(),
            space_before: Some(20.0),
            ..Default::default()
        }));
        assert!(
            (spaced - plain - 20.0).abs() < 0.01,
            "{spaced} against {plain}"
        );
    }

    fn two_paragraphs_with(doc: &LayoutDocument) -> Story {
        let mut story = Story::new();
        for text in [
            "First paragraph of the body copy, one line.",
            "Second paragraph of the body copy, one line.",
        ] {
            story.push_paragraph(text, doc.default_paragraph_style.clone());
        }
        story
    }

    #[test]
    fn space_before_is_dropped_at_the_top_of_a_column() {
        // A column that opens with a gap reads as a mistake, so the
        // space before the first paragraph is discarded.
        let (doc, story) = styled(
            ParagraphStyle {
                name: "Spaced".into(),
                space_before: Some(30.0),
                ..Default::default()
            },
            PROSE,
        );
        let thread = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        assert_eq!(thread.lines().next().unwrap().bounds.y, 0.0);
    }

    #[test]
    fn space_between_paragraphs_appears_once_not_twice() {
        let mut doc = blank_a4();
        let mut story = Story::new();
        story.push_paragraph("First paragraph of the body copy.", "Body");
        story.push_paragraph("Second paragraph of the body copy.", "Body");
        let id = doc.add_story(story);
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Gapped".into(),
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            space_after: Some(12.0),
            keep_lines: Some(1),
            ..Default::default()
        });
        for point in &mut doc.story_mut(id).points {
            if let crate::story::Point::Paragraph { style, .. } = point {
                *style = "Gapped".into();
            }
        }
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        let ys: Vec<Pt> = thread.lines().map(|l| l.bounds.y).collect();
        assert!(ys.len() >= 2, "expected two paragraphs");
        // The gap between them is one line plus the space after.
        let gap = ys[1] - ys[0];
        assert!((gap - (14.0 + 12.0)).abs() < 0.01, "gap of {gap}");
    }

    #[test]
    fn a_split_paragraph_keeps_enough_lines_on_each_side() {
        // `keep_lines` is the widow rule: neither side of a break may be
        // left with a scrap.
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Keep".into(),
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            keep_lines: Some(2),
            ..Default::default()
        });
        doc.default_paragraph_style = "Keep".into();
        let story = doc.add_story(Story::from_text(PROSE, "Keep"));
        // Two lines tall, and the paragraph is longer.
        let thread = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(60.0), mm(10.0)),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        let total = line_spans(&spec_for(
            doc.story(story).unwrap(),
            0,
            PROSE.len(),
            &doc.styles,
            "Keep",
            &doc.default_character_style,
            mm(60.0),
        ))
        .len();
        assert!(total > 4, "the test needs a paragraph of {total} lines");
        let first = thread.frames[0].lines.len();
        assert!(first >= 2, "only {first} lines before the break");
        assert!(
            total - first >= 2,
            "only {} lines after the break",
            total - first
        );
    }

    #[test]
    fn a_paragraph_that_cannot_be_split_keeps_whole() {
        // Three lines with a two-line minimum cannot be split without
        // leaving a one-line scrap, so the whole paragraph moves on.
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Keep".into(),
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            keep_lines: Some(2),
            ..Default::default()
        });
        doc.default_paragraph_style = "Keep".into();
        let story = doc.add_story(Story::from_text(
            "One two three four five six seven eight nine ten eleven twelve thirteen.",
            "Keep",
        ));
        let thread = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(10.0)),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        // Two lines tall, so a two-line paragraph could go in, but this
        // one is three lines and so it is passed on whole.
        let first = thread.frames[0].lines.len();
        assert!(
            first == 0 || first >= 2,
            "a one-line scrap was left behind: {first}"
        );
    }

    #[test]
    fn a_heading_is_not_left_at_the_foot_of_a_column() {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Heading".into(),
            based_on: Some("Default".into()),
            point_size: Some(18.0),
            leading: Some(crate::styles::Leading::Points(22.0)),
            keep_with_next: Some(true),
            space_after: Some(0.0),
            keep_lines: Some(1),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Plain".into(),
            based_on: Some("Default".into()),
            point_size: Some(11.0),
            leading: Some(crate::styles::Leading::Points(14.0)),
            keep_lines: Some(1),
            ..Default::default()
        });
        let mut story = Story::new();
        story.push_paragraph("Filler to push the heading down the column.", "Plain");
        story.push_paragraph("A heading that must not be stranded", "Heading");
        story.push_paragraph("Body copy under the heading here.", "Plain");
        let id = doc.add_story(story);
        // One line of body plus one line of heading is exactly full, so
        // without the rule the heading would end the column alone.
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(10.0)),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        let styles: Vec<&str> = thread.lines().map(|l| l.paragraph_style.as_str()).collect();
        assert!(
            !styles.contains(&"Heading"),
            "the heading was stranded: {styles:?}"
        );
    }

    #[test]
    fn indents_narrow_the_measure_a_line_is_set_in() {
        let (doc, story) = styled(
            ParagraphStyle {
                name: "Indented".into(),
                left_indent: Some(30.0),
                right_indent: Some(10.0),
                ..Default::default()
            },
            PROSE,
        );
        let column = Rect::new(0.0, 0.0, mm(120.0), mm(200.0));
        let thread = compose_thread(
            &doc,
            story,
            &[(
                ObjectId::next(),
                column,
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        let line = thread.lines().next().expect("no lines");
        assert!((line.bounds.x - (column.x + 30.0)).abs() < 0.01);
        assert!((line.bounds.width - (column.width - 40.0)).abs() < 0.01);
    }

    #[test]
    fn a_first_line_indent_applies_only_to_the_first_line() {
        let (doc, story) = styled(
            ParagraphStyle {
                name: "Indented".into(),
                first_line_indent: Some(24.0),
                ..Default::default()
            },
            PROSE,
        );
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(60.0), mm(200.0)),
            )],
        );
        let lines: Vec<&ComposedLine> = thread.lines().collect();
        assert!(lines.len() > 1, "the test needs more than one line");
        assert!(lines[0].bounds.x > lines[1].bounds.x);
    }

    #[test]
    fn a_drop_cap_insets_the_lines_it_covers() {
        let (doc, story) = styled(
            ParagraphStyle {
                name: "Dropped".into(),
                drop_caps_lines: Some(3),
                ..Default::default()
            },
            PROSE,
        );
        // A narrow measure, so the paragraph is longer than the capital.
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(60.0), mm(200.0)),
            )],
        );
        let lines: Vec<&ComposedLine> = thread.lines().collect();
        let dropped: Vec<&&ComposedLine> = lines.iter().filter(|l| l.drop_cap).collect();
        assert_eq!(
            dropped.len(),
            3,
            "expected three lines to clear the capital"
        );
        let clear: Vec<&&ComposedLine> = lines.iter().filter(|l| !l.drop_cap).collect();
        assert!(!clear.is_empty(), "the test needs a line past the capital");
        // The dropped lines start further right than the ones after them.
        assert!(dropped[0].bounds.x > clear[0].bounds.x);
        assert!(thread.frames[0].drop_cap.is_some());
    }

    #[test]
    fn a_one_line_drop_cap_reserves_nothing() {
        // Reserving space for a capital the height of one line would just
        // leave a hole where the letter should be.
        let (doc, story) = styled(
            ParagraphStyle {
                name: "Dropped".into(),
                drop_caps_lines: Some(1),
                ..Default::default()
            },
            PROSE,
        );
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
            )],
        );
        assert!(thread.lines().all(|l| !l.drop_cap));
        assert!(thread.frames[0].drop_cap.is_none());
    }

    #[test]
    fn baseline_snapping_preserves_minimum_leading_and_aligns_baselines() {
        let mut doc = blank_a4();
        doc.grids.document = crate::grid::GridSettings {
            mode: crate::grid::GridMode::SnapToGrid,
            baseline_count: 12.0,
            ..Default::default()
        };
        let story = doc.add_story(Story::from_text(PROSE, "Body"));
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
            )],
        );
        // A 6pt guide interval must not compress 13.5pt text into 6pt rows.
        for line in thread.lines() {
            assert!((line.advance - 13.5).abs() < 0.01);
            let phase = (line.baseline - doc.pages[0].margins.top) / 6.0;
            assert!((phase - phase.round()).abs() < 0.001);
        }
    }

    #[test]
    fn a_grid_off_leaves_the_style_leading_alone() {
        let (doc, story) = doc_with(PROSE);
        let thread = compose_thread(
            &doc,
            story,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(120.0), mm(200.0)),
            )],
        );
        for line in thread.lines() {
            assert!(
                (line.advance - 13.5).abs() < 0.01,
                "leading {}",
                line.advance
            );
        }
    }

    #[test]
    fn a_forced_break_becomes_a_line_with_no_glyphs() {
        let mut story = Story::new();
        story.push_paragraph("Before the break.", "Body");
        story.points.push(Point::LineBreak);
        story.push_paragraph("After the break.", "Body");
        let mut doc = blank_a4();
        let id = doc.add_story(story);
        let thread = compose_thread(
            &doc,
            id,
            &[frame(
                ObjectId::next(),
                Rect::new(0.0, 0.0, mm(170.0), mm(200.0)),
            )],
        );
        let breaks: Vec<&ComposedLine> = thread.lines().filter(|l| l.forced_break).collect();
        assert_eq!(breaks.len(), 1);
        assert!(breaks[0].bounds.height > 0.0);
        // The bytes either side are contiguous, with the break between.
        let first = thread.lines().next().unwrap();
        assert!(first.end <= breaks[0].start);
    }
}
