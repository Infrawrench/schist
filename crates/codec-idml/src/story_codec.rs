//! Lower IDML local formatting to editable named styles based on the
//! original style. The model has named styles rather than local override
//! bags. Equivalent overrides share a style, and our writer emits only
//! references, so reopening a save never creates another generation.
use crate::{color_codec::Colors, import::Report, style_codec, xml::Element};
use schist_layout::{CharacterStyle, ParagraphStyle, Story, StoryPoint, StyleRange, StyleSet};

pub(crate) fn normalize(
    story: &Element,
    styles: &mut StyleSet,
    colors: &Colors,
    refs: &style_codec::References,
    report: &mut Report,
) -> Element {
    let mut story = story.clone();
    crate::auto_direction::restore(&mut story, styles, refs);
    visit(&mut story, styles, colors, refs, report);
    story
}

fn visit(
    element: &mut Element,
    styles: &mut StyleSet,
    colors: &Colors,
    refs: &style_codec::References,
    report: &mut Report,
) {
    match element.name.as_str() {
        "Table" | "Footnote" | "TextFrame" | "Rectangle" | "Polygon" => {
            let message = schist_i18n::t("design.idml_story_structure").to_string();
            if !report.skipped.contains(&message) {
                report.skip(message);
            }
            return;
        }
        "ParagraphStyleRange" => {
            let base = refs.paragraph(element.attr("AppliedParagraphStyle").unwrap_or_default());
            let mut local = style_codec::paragraph_properties(element, colors, refs, report);
            let name = if local == ParagraphStyle::default() {
                base
            } else {
                local.based_on = present(&base).then_some(base);
                report_conversion(report);
                paragraph_style(styles, local)
            };
            set_attr(
                element,
                "AppliedParagraphStyle",
                format!("ParagraphStyle/$ID/{name}"),
            );
        }
        "CharacterStyleRange" => {
            if matches!(
                element
                    .attr("ParagraphBreakType")
                    .or_else(|| element.attr("GoToNextX")),
                Some("NextOddPage" | "NextEvenPage")
            ) {
                let message = schist_i18n::t("design.idml_page_parity").to_string();
                if !report.skipped.contains(&message) {
                    report.skip(message);
                }
            }
            let base = refs.character(element.attr("AppliedCharacterStyle").unwrap_or_default());
            let mut local = style_codec::character_properties(element, colors, refs, report);
            let name = if local == CharacterStyle::default() {
                base
            } else {
                local.based_on = present(&base).then_some(base);
                report_conversion(report);
                character_style(styles, local)
            };
            set_attr(
                element,
                "AppliedCharacterStyle",
                format!("CharacterStyle/$ID/{name}"),
            );
        }
        _ => {}
    }
    for child in &mut element.children {
        visit(child, styles, colors, refs, report);
    }
}

/// Style ranges are formatting boundaries, not paragraph boundaries.
/// IDML's Br ends a paragraph; literal LF in Content is a soft line break.
/// See the public specification, examples 48–50. Style offsets are derived
/// from the finished Story so its inter-paragraph separators count once.
pub(crate) fn decode(story: &Element) -> Story {
    let mut builder = StoryBuilder::default();
    if let Some(preference) = story.child("StoryPreference") {
        builder.out.prefs.direction = match preference.attr("StoryDirection") {
            Some("RightToLeftDirection") => schist_layout::StoryDirection::RightToLeft,
            _ => schist_layout::StoryDirection::LeftToRight,
        };
        builder.out.prefs.orientation = match preference.attr("StoryOrientation") {
            Some("Vertical") => schist_layout::StoryOrientation::Vertical,
            _ => schist_layout::StoryOrientation::Horizontal,
        };
    }
    builder.walk(story, "", "", "Anywhere");
    if !builder.text.is_empty() || builder.out.points.is_empty() || builder.trailing_paragraph {
        builder.flush();
    }
    let offsets = builder.out.point_offsets();
    for (point, mut range) in builder.ranges {
        range.start += offsets[point];
        range.end += offsets[point];
        builder.out.ranges.push(range);
    }
    builder.out
}

#[derive(Default)]
struct StoryBuilder {
    out: Story,
    text: String,
    style: String,
    local: Vec<StyleRange>,
    ranges: Vec<(usize, StyleRange)>,
    trailing_paragraph: bool,
}

impl StoryBuilder {
    fn flush(&mut self) {
        let index = self.out.points.len();
        self.out.points.push(StoryPoint::Paragraph {
            text: std::mem::take(&mut self.text),
            style: self.style.clone(),
        });
        self.ranges.extend(self.local.drain(..).map(|r| (index, r)));
    }

    fn append(&mut self, text: &str, paragraph: &str, character: &str) {
        if self.text.is_empty() {
            self.style = paragraph.to_owned();
        }
        let start = self.text.len();
        self.text.push_str(text);
        if self.text.len() > start && present(character) {
            if let Some(last) = self
                .local
                .last_mut()
                .filter(|r| r.end == start && r.style == character)
            {
                last.end = self.text.len();
            } else {
                self.local
                    .push(StyleRange::new(start, self.text.len(), character));
            }
        }
        if !text.is_empty() {
            self.trailing_paragraph = false;
        }
    }

    fn walk(&mut self, element: &Element, paragraph: &str, character: &str, next: &str) {
        let paragraph_name;
        let character_name;
        let mut paragraph = paragraph;
        let mut character = character;
        let mut next = next;
        match element.name.as_str() {
            "ParagraphStyleRange" => {
                paragraph_name =
                    style_codec::name(element.attr("AppliedParagraphStyle").unwrap_or_default());
                paragraph = &paragraph_name;
                if self.text.is_empty() {
                    self.style = paragraph.to_owned();
                }
            }
            "CharacterStyleRange" => {
                character_name =
                    style_codec::name(element.attr("AppliedCharacterStyle").unwrap_or_default());
                character = &character_name;
                next = element
                    .attr("ParagraphBreakType")
                    .or_else(|| element.attr("GoToNextX"))
                    .unwrap_or(next);
            }
            "Content" => {
                self.append(&element.text, paragraph, character);
                return;
            }
            "Tab" => {
                self.append("\t", paragraph, character);
                return;
            }
            "Br" | "br" => {
                let forced = match next {
                    "NextColumn" => Some(StoryPoint::ColumnBreak),
                    "NextFrame" => Some(StoryPoint::FrameBreak),
                    "NextPage" | "NextOddPage" | "NextEvenPage" => Some(StoryPoint::PageBreak),
                    _ => None,
                };
                if forced.is_none() || !self.text.is_empty() {
                    self.flush();
                }
                self.trailing_paragraph = forced.is_none();
                if let Some(point) = forced {
                    self.out.points.push(point);
                }
                return;
            }
            "Table" | "Footnote" | "TextFrame" | "Rectangle" | "Polygon" | "Properties" => return,
            _ => {}
        }
        for child in &element.children {
            self.walk(child, paragraph, character, next);
        }
    }
}

fn present(name: &str) -> bool {
    !matches!(
        name,
        "" | "[None]" | "[No character style]" | "[No paragraph style]"
    )
}

fn set_attr(element: &mut Element, key: &str, value: String) {
    if let Some((_, old)) = element.attributes.iter_mut().find(|(name, _)| name == key) {
        *old = value;
    } else {
        element.attributes.push((key.to_owned(), value));
    }
}

fn report_conversion(report: &mut Report) {
    let message = schist_i18n::t("design.idml_local_styles").to_string();
    if !report.skipped.contains(&message) {
        report.skip(message);
    }
}

fn paragraph_style(styles: &mut StyleSet, mut local: ParagraphStyle) -> String {
    if let Some(existing) = styles.paragraphs.iter().find(|style| {
        let mut definition = (*style).clone();
        definition.name.clear();
        definition == local
    }) {
        return existing.name.clone();
    }
    local.name = unique_name(|name| styles.paragraph(name).is_some());
    let name = local.name.clone();
    styles.add_paragraph(local);
    name
}

fn character_style(styles: &mut StyleSet, mut local: CharacterStyle) -> String {
    if let Some(existing) = styles.characters.iter().find(|style| {
        let mut definition = (*style).clone();
        definition.name.clear();
        definition == local
    }) {
        return existing.name.clone();
    }
    local.name = unique_name(|name| styles.character(name).is_some());
    let name = local.name.clone();
    styles.add_character(local);
    name
}

fn unique_name(exists: impl Fn(&str) -> bool) -> String {
    let label = schist_i18n::t("design.imported_style");
    let mut index = 1;
    loop {
        let name = format!("{label} {index}");
        if !exists(&name) {
            return name;
        }
        index += 1;
    }
}
