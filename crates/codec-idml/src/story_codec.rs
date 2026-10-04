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
    // Content PIs may carry document characters; do not ignore them as metadata.
    // Typed note/end-style controls also keep their original recovery data;
    // unknown instructions remain inert and are reported as unrendered.
    if element.name == "Content" && !element.instructions.is_empty() {
        let message = schist_i18n::t("design.idml_story_structure").to_string();
        if !report.skipped.contains(&message) {
            report.skip(message);
        }
    }
    match element.name.as_str() {
        name if crate::xml::story_structure(name) => {
            let message = schist_i18n::t("design.idml_story_structure").to_string();
            if !report.skipped.contains(&message) {
                report.skip(message);
            }
            if name != "Footnote" || !text_only_footnote(element) {
                return;
            }
            crate::auto_direction::restore(element, styles, refs);
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
    decode_with_markers(story, false).0
}

/// Older guarded records retained supported instructions only as contextual
/// recovery XML. Upgrade after native agreement, never by parsing XML in the
/// layout kernel.
pub(crate) fn upgrade_controls(story: &mut Story, refs: &style_codec::References) {
    for structure in &mut story.structures {
        if structure.kind != "ProcessingInstruction"
            || structure.control.is_some()
            || structure.footnote.is_some()
        {
            continue;
        }
        let Ok(root) = crate::xml::parse(&structure.payload) else {
            continue;
        };
        if root.name != "ParagraphStyleRange" || root.children.len() != 1 {
            continue;
        }
        let character = &root.children[0];
        if character.name != "CharacterStyleRange" || character.children.len() != 1 {
            continue;
        }
        let content = &character.children[0];
        if content.name != "Content"
            || !content.text.is_empty()
            || !content.children.is_empty()
            || content.instructions.len() != 1
            || content.instructions[0].0 != 0
        {
            continue;
        }
        let name = refs.character(character.attr("AppliedCharacterStyle").unwrap_or_default());
        structure.control = control(
            &content.instructions[0].1,
            &name,
            character.attr("PageNumberType"),
        );
    }
}

/// Supported zero-width Content instructions. ACE 3 ends a nested style;
/// ACE 18 is a page number whose range PageNumberType names current, next or
/// previous; ACE 19 is a section marker. Other instructions and page-number
/// modes remain recovery data.
pub(crate) fn control(
    instruction: &str,
    character_style: &str,
    page_number_type: Option<&str>,
) -> Option<schist_layout::story::InlineControl> {
    use schist_layout::story::{InlineControl, PageNumberKind};
    let words: Vec<_> = instruction.split_whitespace().collect();
    let character_style = character_style.to_owned();
    match (words.as_slice(), page_number_type) {
        (["ACE", "3"], _) => Some(InlineControl::EndNestedStyle { character_style }),
        (["ACE", "18"], None | Some("AutoPageNumber")) => Some(InlineControl::PageNumber {
            kind: PageNumberKind::Current,
            character_style,
        }),
        (["ACE", "18"], Some("NextPageNumber")) => Some(InlineControl::PageNumber {
            kind: PageNumberKind::Next,
            character_style,
        }),
        (["ACE", "18"], Some("PreviousPageNumber")) => Some(InlineControl::PageNumber {
            kind: PageNumberKind::Previous,
            character_style,
        }),
        (["ACE", "19"], None | Some("AutoPageNumber")) => {
            Some(InlineControl::SectionMarker { character_style })
        }
        _ => None,
    }
}

fn decode_with_markers(
    story: &Element,
    footnote_markers: bool,
) -> (Story, Vec<schist_layout::footnotes::FootnoteMarker>) {
    let mut builder = StoryBuilder {
        footnote_markers,
        ..Default::default()
    };
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
    builder.walk(story, "", "", "Anywhere", None);
    if !builder.text.is_empty()
        || builder.out.points.is_empty()
        || builder.trailing_paragraph
        || builder
            .structures
            .last()
            .is_some_and(|(point, _)| *point == builder.out.points.len())
        || builder
            .markers
            .last()
            .is_some_and(|(point, _)| *point == builder.out.points.len())
    {
        builder.flush();
    }
    let offsets = builder.out.point_offsets();
    for (point, mut range) in builder.ranges {
        range.start += offsets[point];
        range.end += offsets[point];
        builder.out.ranges.push(range);
    }
    for (point, mut structure) in builder.structures {
        structure.at = structure.at.map(|at| offsets[point] + at);
        builder.out.structures.push(structure);
    }
    let markers = builder
        .markers
        .into_iter()
        .map(|(point, mut marker)| {
            marker.at += offsets[point];
            marker
        })
        .collect();
    (builder.out, markers)
}

// The native XML remains the fallback for tables, embedded objects, variables,
// unknown ACE instructions and mixed content. Lowering must not silently flatten
// those into a text-only note. IDML example 54 supplies the ACE 4 marker form.
fn text_only_footnote(element: &Element) -> bool {
    if element.name == "Properties" {
        return true;
    }
    if !matches!(
        element.name.as_str(),
        "Footnote"
            | "ParagraphStyleRange"
            | "CharacterStyleRange"
            | "Content"
            | "Tab"
            | "Br"
            | "br"
    ) {
        return false;
    }
    if element.name != "Content" && !element.text.trim().is_empty() {
        return false;
    }
    if element.name == "Content" && !element.children.is_empty() {
        return false;
    }
    element.instructions.iter().all(|(_, instruction)| {
        element.name == "Content" && instruction.split_whitespace().eq(["ACE", "4"])
    }) && element
        .children
        .iter()
        .all(|child| child.name != "Footnote" && text_only_footnote(child))
}

fn footnote(
    element: &Element,
    paragraph: &str,
    character: &str,
) -> Option<schist_layout::footnotes::FootnoteBody> {
    if element.name != "Footnote" || !text_only_footnote(element) {
        return None;
    }
    let mut body = element.clone();
    body.name = "Story".into();
    let (story, markers) = decode_with_markers(&body, true);
    let note = schist_layout::footnotes::FootnoteBody {
        story,
        markers,
        reference_paragraph_style: paragraph.into(),
        reference_character_style: character.into(),
    };
    note.valid().then_some(note)
}

#[derive(Default)]
struct StoryBuilder {
    out: Story,
    text: String,
    style: String,
    local: Vec<StyleRange>,
    ranges: Vec<(usize, StyleRange)>,
    trailing_paragraph: bool,
    structures: Vec<(usize, schist_layout::StoryStructure)>,
    markers: Vec<(usize, schist_layout::footnotes::FootnoteMarker)>,
    footnote_markers: bool,
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

    fn walk(
        &mut self,
        element: &Element,
        paragraph: &str,
        character: &str,
        next: &str,
        page_number_type: Option<&str>,
    ) {
        let paragraph_name;
        let character_name;
        let mut paragraph = paragraph;
        let mut character = character;
        let mut next = next;
        let mut page_number_type = page_number_type;
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
                page_number_type = element.attr("PageNumberType").or(page_number_type);
                next = element
                    .attr("ParagraphBreakType")
                    .or_else(|| element.attr("GoToNextX"))
                    .unwrap_or(next);
            }
            "Content" => {
                for (at, instruction) in &element.instructions {
                    if self.footnote_markers && instruction.split_whitespace().eq(["ACE", "4"]) {
                        self.markers.push((
                            self.out.points.len(),
                            schist_layout::footnotes::FootnoteMarker {
                                at: self.text.len() + at,
                                character_style: character.into(),
                            },
                        ));
                    } else {
                        self.structures.push((
                            self.out.points.len(),
                            schist_layout::StoryStructure {
                                control: control(instruction, character, page_number_type),
                                at: Some(self.text.len() + at),
                                kind: "ProcessingInstruction".into(),
                                payload: inline_payload(
                                    &format!("<Content><?{instruction}?></Content>"),
                                    paragraph,
                                    character,
                                    page_number_type,
                                ),
                                footnote: None,
                            },
                        ));
                    }
                }
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
                    "NextPage" => Some(StoryPoint::PageBreak),
                    "NextOddPage" => Some(StoryPoint::OddPageBreak),
                    "NextEvenPage" => Some(StoryPoint::EvenPageBreak),
                    _ => None,
                };
                if forced.is_none()
                    || !self.text.is_empty()
                    || self
                        .structures
                        .last()
                        .is_some_and(|(point, _)| *point == self.out.points.len())
                {
                    self.flush();
                }
                self.trailing_paragraph = forced.is_none();
                if let Some(point) = forced {
                    self.out.points.push(point);
                }
                return;
            }
            name if crate::xml::story_structure(name) => {
                if let Some(raw) = &element.raw {
                    self.structures.push((
                        self.out.points.len(),
                        schist_layout::StoryStructure {
                            control: crate::custom_text_codec::instance(element, character),
                            at: Some(self.text.len()),
                            kind: name.into(),
                            payload: if name == "TextVariableInstance" {
                                inline_payload(raw, paragraph, character, page_number_type)
                            } else {
                                raw.to_string()
                            },
                            footnote: footnote(element, paragraph, character),
                        },
                    ));
                }
                return;
            }
            "Properties" => return,
            _ => {}
        }
        for child in &element.children {
            self.walk(child, paragraph, character, next, page_number_type);
        }
    }
}

// Recovery XML retains the exact inline XML plus effective named formatting and
// native page-number mode. The wrappers describe its context; they are not a
// claim that the unsupported marker has been emitted as native story content.
fn inline_payload(
    content: &str,
    paragraph: &str,
    character: &str,
    page_number_type: Option<&str>,
) -> String {
    let page_number_type = page_number_type
        .map(|value| format!(r#" PageNumberType="{}""#, crate::export::escape(value)))
        .unwrap_or_default();
    format!(
        r#"<ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/{}"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/{}"{}>{}</CharacterStyleRange></ParagraphStyleRange>"#,
        crate::export::escape(paragraph),
        crate::export::escape(character),
        page_number_type,
        content
    )
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
