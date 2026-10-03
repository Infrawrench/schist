//! Generate automatic markers without changing source text or flow positions.
use crate::{
    compose::{ComposedLine, ComposedThread},
    list_counters::{counter_format, expand, sequence_id, supported_restart, StoryCounters},
    lists::{ListKind, ListStyle, MarkerAlignment},
    styles::{ResolvedCharacter, ResolvedParagraph},
    LayoutDocument, Rect, Story, StoryPoint,
};
use schist_text_engine::{TextSpec, WritingMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedRole {
    Marker,
    Leader,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedText {
    pub role: GeneratedRole,
    pub spec: TextSpec,
    pub character: ResolvedCharacter,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkerPlan {
    generated: GeneratedText,
    width: f32,
    height: f32,
    ascent: f32,
    tab: bool,
    list: ListStyle,
}
impl MarkerPlan {
    fn explicit_stop(
        &self,
        column: f32,
        paragraph: &ResolvedParagraph,
    ) -> Option<&crate::lists::ListTab> {
        let end = self.left(column, paragraph) + self.width;
        let stop = self
            .list
            .tabs
            .iter()
            .flatten()
            .filter(|t| t.position.is_finite() && column + t.position > end + 0.001)
            .min_by(|a, b| a.position.total_cmp(&b.position))?;
        // A native hanging indent acts as an earlier marker/body stop. An
        // equal explicit stop still owns its configured leader; a later one
        // cannot borrow the virtual stop's gap.
        let indent = column + paragraph.left_indent.unwrap_or(0.0);
        (indent <= end + 0.001 || column + stop.position <= indent).then_some(stop)
    }
    fn left(&self, column: f32, paragraph: &ResolvedParagraph) -> f32 {
        let anchor = column
            + paragraph.left_indent.unwrap_or(0.0)
            + paragraph.first_line_indent.unwrap_or(0.0);
        let align = if self.list.kind == Some(ListKind::Bullet) {
            self.list.bullet_alignment
        } else {
            self.list.numbering_alignment
        };
        anchor
            - match align.unwrap_or(MarkerAlignment::Left) {
                MarkerAlignment::Left => 0.0,
                MarkerAlignment::Center => self.width / 2.0,
                MarkerAlignment::Right => self.width,
            }
    }
    pub(crate) fn body_start(&self, column: f32, paragraph: &ResolvedParagraph) -> f32 {
        let end = self.left(column, paragraph) + self.width;
        if let Some(gap) = self.list.legacy_gap.filter(|v| v.is_finite()) {
            return end + gap.max(0.0);
        }
        if !self.tab {
            return end;
        }
        if let Some(stop) = self.explicit_stop(column, paragraph) {
            return column + stop.position;
        }
        let indent = column + paragraph.left_indent.unwrap_or(0.0);
        if indent > end + 0.001 {
            return indent;
        }
        // Schist's implicit marker tab interval is half an inch. Native ruler-
        // dependent implicit stops are not available in the current model.
        column + (((end - column) / 36.0).floor() + 1.0) * 36.0
    }
}

/// Measured marker plans shared by all columns and balance trials for one flow.
/// No document-global cache: changing text, styles or font availability requires
/// a fresh value, which each composition and resource-inventory call constructs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkerPlans {
    plans: std::collections::BTreeMap<usize, MarkerPlan>,
}
impl MarkerPlans {
    pub fn new(doc: &LayoutDocument, story: &Story) -> Self {
        let counters = StoryCounters::new(doc, story);
        let mut out = Self::default();
        for (point, at) in story.points.iter().zip(story.point_offsets()) {
            let StoryPoint::Paragraph { style, .. } = point else {
                continue;
            };
            if let Some(plan) = plan(doc, story, at, style, &counters) {
                out.plans.insert(at, plan);
            }
        }
        out
    }

    /// Inline references move paragraph anchors, but do not supply the source
    /// character context or alter source-order list counters. Keep the measured
    /// marker and place it before any generated text at the paragraph start.
    pub(crate) fn projected(self, positions: &crate::inline_text::SourceMap) -> Self {
        Self {
            plans: self
                .plans
                .into_iter()
                .map(|(at, plan)| (positions.before(at), plan))
                .collect(),
        }
    }

    pub(crate) fn get(&self, at: usize) -> Option<&MarkerPlan> {
        self.plans.get(&at)
    }

    /// Includes overset paragraphs for resource inventories.
    pub fn spec(&self, at: usize) -> Option<&TextSpec> {
        Some(&self.get(at)?.generated.spec)
    }
}

fn plan(
    doc: &LayoutDocument,
    story: &Story,
    at: usize,
    style: &str,
    counters: &StoryCounters,
) -> Option<MarkerPlan> {
    let mut paragraph = doc.styles.resolve_paragraph(style);
    let list = paragraph.list.clone();
    let context = story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, offset)| (offset == at).then_some(point.text()))
        .unwrap_or_default();
    if !unsupported(&list).is_empty()
        || !unsupported_paragraph(&paragraph, context).is_empty()
        || paragraph.drop_caps_lines.unwrap_or(0) > 1
    {
        return None;
    }
    if list.kind == Some(ListKind::Numbered)
        && doc
            .styles
            .numbering_lists
            .iter()
            .any(|r| r.id == sequence_id(&list) && r.across_documents)
    {
        return None;
    }
    let (text, tab, explicit) = match list.kind? {
        ListKind::None => return None,
        ListKind::Bullet => {
            let c = list.bullet.as_ref().map_or(Some('•'), |b| b.character())?;
            let (after, tab) = expand(list.text_after.as_deref().unwrap_or("^t"), |_| {
                Err("BulletsTextAfter")
            })
            .ok()?;
            (format!("{c}{after}"), tab, &list.bullet_character_style)
        }
        ListKind::Numbered => {
            let (text, tab) = counters.marker(at)?;
            (text.clone(), *tab, &list.numbering_character_style)
        }
    };
    let mut base = paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
    base.point_size = paragraph.point_size.or(base.point_size);
    base.leading = paragraph.leading.or(base.leading);
    base.tracking = paragraph.tracking.or(base.tracking);
    let mut character =
        crate::nested_styles::runs(story, &doc.styles, at..at.saturating_add(1), &paragraph)
            .into_iter()
            .next()
            .map_or_else(|| base.clone(), |r| r.character.over(&base));
    if let Some(explicit) = explicit {
        character = doc.styles.resolve_character(explicit).over(&character);
    }
    if list.kind == Some(ListKind::Bullet) {
        if let Some(font) = list
            .bullet_font
            .as_deref()
            .filter(|v| !matches!(*v, "" | "$ID/" | "Auto"))
        {
            character.family = Some(font.into());
        }
        if let Some(face) = list
            .bullet_font_style
            .as_deref()
            .filter(|v| !matches!(*v, "" | "Nothing" | "Auto"))
        {
            character.font_style = Some(face.into());
            let (bold, italic) = schist_text_engine::font_style_hints(face);
            character.bold = Some(bold);
            character.italic = Some(italic);
        }
    }
    paragraph.point_size = character.point_size;
    paragraph.leading = character.leading;
    paragraph.tracking = character.tracking;
    paragraph.align = Some(crate::styles::Align::Left);
    // The marker inherits the source context above, never its own new prefix.
    paragraph.nested_styles = None;
    let mut generated_story = Story::from_text(&text, style);
    generated_story.prefs = story.prefs;
    let mut spec = crate::compose::spec_with_character(
        &generated_story,
        (0, text.len()),
        &doc.styles,
        &paragraph,
        character.clone(),
        0.0,
    );
    if spec.writing_mode != WritingMode::Horizontal {
        return None;
    }
    spec.wrap_width = None;
    let metrics = schist_text_engine::measure(&spec)?;
    Some(MarkerPlan {
        generated: GeneratedText {
            role: GeneratedRole::Marker,
            spec,
            character,
        },
        width: metrics.width,
        height: metrics.height,
        ascent: metrics.first_baseline,
        tab,
        list,
    })
}

pub(crate) fn insert_markers(plans: &MarkerPlans, thread: &mut ComposedThread) {
    for frame in &mut thread.frames {
        let mut lines = Vec::new();
        for line in std::mem::take(&mut frame.lines) {
            if line.paragraph.list.active() && line.text_path.is_none() && line.initial.is_none() {
                if let Some(marker) = plans.get(line.start) {
                    let mut generated: ComposedLine = line.clone();
                    generated.end = generated.start;
                    generated.bounds = Rect::new(
                        marker.left(line.inline_origin, &line.paragraph),
                        line.baseline - marker.ascent,
                        marker.width,
                        marker.height,
                    );
                    generated.natural_width = marker.width;
                    generated.word_space = None;
                    generated.forced_break = false;
                    generated.drop_cap = false;
                    generated.characters = vec![marker.generated.character.clone()];
                    generated.generated = Some(marker.generated.clone());
                    let mut leader_line = None;
                    if marker.tab && marker.list.legacy_gap.is_none() {
                        if let Some(stop) = marker
                            .explicit_stop(line.inline_origin, &line.paragraph)
                            .filter(|stop| !stop.leader.is_empty())
                        {
                            let mut leader = generated.clone();
                            let paint = leader.generated.as_mut().unwrap();
                            paint.role = GeneratedRole::Leader;
                            let spec = &mut paint.spec;
                            let offset = generated.bounds.x + marker.width - line.inline_origin;
                            spec.text = "\u{200b}\t".into();
                            let mut tab_style = spec.runs.first().cloned().unwrap_or_default();
                            tab_style.start = 3;
                            tab_style.end = 4;
                            let mut padding = tab_style.clone();
                            padding.start = 0;
                            padding.end = 3;
                            padding.tracking = Some(offset);
                            spec.runs = vec![padding, tab_style];
                            // One counter has separate marker/leader paint.
                            // Keep the original marker's glyph placement and
                            // source positions unchanged when adding leaders.
                            spec.tabs = Some(schist_text_engine::TabStops {
                                positions: vec![stop.position],
                                leaders: vec![stop.leader.clone()],
                                ..Default::default()
                            });
                            leader.bounds.x = line.inline_origin;
                            leader.bounds.width = stop.position;
                            leader.natural_width = stop.position;
                            leader_line = Some(leader);
                        }
                    }
                    lines.push(generated);
                    lines.extend(leader_line);
                }
            }
            lines.push(line);
        }
        frame.lines = lines;
    }
}

/// Native list settings retained by the model but not composed yet.
pub fn unsupported_paragraph(paragraph: &ResolvedParagraph, text: &str) -> Vec<&'static str> {
    if !paragraph.list.active() {
        return Vec::new();
    }
    let rtl = match paragraph
        .direction
        .unwrap_or(crate::styles::ParagraphDirection::Auto)
    {
        crate::styles::ParagraphDirection::RightToLeft => true,
        crate::styles::ParagraphDirection::LeftToRight => false,
        crate::styles::ParagraphDirection::Auto => {
            schist_text_engine::base_direction(text)
                == schist_text_engine::ParagraphDirection::RightToLeft
        }
    };
    if rtl {
        vec!["ParagraphDirection + BulletsAndNumberingListType"]
    } else {
        Vec::new()
    }
}

/// Unsupported options within the list record, independently of paragraph flow.
pub fn unsupported(list: &ListStyle) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !matches!(list.kind, Some(ListKind::Bullet | ListKind::Numbered)) {
        return out;
    }
    if list.kind == Some(ListKind::Numbered) && !(1..=9).contains(&list.level.unwrap_or(1)) {
        out.push("NumberingLevel");
    }
    if list.tabs.iter().flatten().any(|t| {
        !t.position.is_finite()
            || t.alignment != "LeftAlign"
            || !schist_text_engine::valid_tab_leader(&t.leader)
    }) {
        out.push("TabList");
    }
    if list.kind == Some(ListKind::Bullet) {
        if list
            .bullet
            .as_ref()
            .is_some_and(|b| b.character().is_none())
        {
            out.push("BulletChar");
        }
        if expand(list.text_after.as_deref().unwrap_or("^t"), |_| {
            Err("BulletsTextAfter")
        })
        .is_err()
        {
            out.push("BulletsTextAfter");
        }
    } else {
        if list.start == Some(0) {
            out.push("NumberingStartAt");
        }
        if counter_format(list).is_none() {
            out.push("NumberingFormat");
        }
        if !supported_restart(list) {
            out.push("NumberingRestartPolicies");
        }
        if expand(list.expression.as_deref().unwrap_or("^#.^t"), |code| {
            if code == '#'
                || (('1'..='9').contains(&code)
                    && u32::from(code) - u32::from('0') < list.level.unwrap_or(1))
            {
                Ok("1".into())
            } else {
                Err("NumberingExpression")
            }
        })
        .is_err()
        {
            out.push("NumberingExpression");
        }
    }
    out
}

/// Context-dependent counter diagnostics. Reuse StoryCounters for batch queries.
pub fn numbering_issue(doc: &LayoutDocument, story: &Story, at: usize) -> Option<&'static str> {
    StoryCounters::new(doc, story).issue(at)
}

/// Typography requested by a generated marker, including overset paragraphs.
/// Reuse MarkerPlans when enumerating all paragraphs in a story.
pub fn marker_spec(doc: &LayoutDocument, story: &Story, at: usize) -> Option<TextSpec> {
    MarkerPlans::new(doc, story).spec(at).cloned()
}
