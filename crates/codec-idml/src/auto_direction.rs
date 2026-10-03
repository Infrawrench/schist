//! Native IDML has two paragraph directions, while Schist also has Auto.
//! Emit a real direction for every automatic paragraph. Standard Labels retain
//! the editing intent only while the native text, style and direction agree.
use crate::{style_codec, xml::Element};
use schist_layout::{ParagraphDirection, Story, StoryPoint, StyleSet};
use serde::{Deserialize, Serialize};

pub(crate) const STYLE_LABEL: &str = "Schist.ParagraphDirection.v1";
const STORY_LABEL: &str = "Schist.AutomaticParagraphs.v1";

pub(crate) fn label<'a>(element: &'a Element, key: &str) -> Option<&'a str> {
    element
        .child("Properties")?
        .child("Label")?
        .children_named("KeyValuePair")
        .find(|entry| entry.attr("Key") == Some(key))?
        .attr("Value")
}

pub(crate) fn style_direction(element: &Element) -> Option<ParagraphDirection> {
    match element.attr("ParagraphDirection") {
        Some("LeftToRightDirection")
            if element.name == "ParagraphStyle" && label(element, STYLE_LABEL) == Some("Auto") =>
        {
            Some(ParagraphDirection::Auto)
        }
        Some("LeftToRightDirection") => Some(ParagraphDirection::LeftToRight),
        Some("RightToLeftDirection") => Some(ParagraphDirection::RightToLeft),
        None if element.name == "ParagraphStyle"
            && label(element, STYLE_LABEL) != Some("AutoDefault")
            && element
                .child("Properties")
                .and_then(|p| p.child("BasedOn"))
                .map(Element::trimmed)
                .or_else(|| element.attr("BasedOn"))
                .is_none_or(|base| matches!(base, "" | "n")) =>
        {
            Some(ParagraphDirection::LeftToRight)
        }
        _ => None,
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct AutomaticParagraph {
    pub index: usize,
    style: String,
    text: String,
    pub direction: String,
    break_after: bool,
}

fn automatic(styles: &StyleSet, name: &str) -> bool {
    matches!(
        styles.resolve_paragraph(name).direction,
        None | Some(ParagraphDirection::Auto)
    )
}

pub(crate) fn lower(story: &Story, styles: &StyleSet) -> Vec<AutomaticParagraph> {
    let points: Vec<_> = story
        .points
        .iter()
        .filter(|p| !matches!(p, StoryPoint::Other { .. }))
        .collect();
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| {
            let StoryPoint::Paragraph { text, style } = point else {
                return None;
            };
            automatic(styles, style).then(|| AutomaticParagraph {
                index,
                style: format!("ParagraphStyle/$ID/{style}"),
                text: text.clone(),
                direction: match schist_text_engine::base_direction(text) {
                    schist_text_engine::ParagraphDirection::RightToLeft => "RightToLeftDirection",
                    _ => "LeftToRightDirection",
                }
                .into(),
                break_after: matches!(
                    points.get(index + 1),
                    Some(StoryPoint::Paragraph { .. } | StoryPoint::LineBreak)
                ),
            })
        })
        .collect()
}

pub(crate) fn properties(paragraphs: &[AutomaticParagraph]) -> String {
    if paragraphs.is_empty() {
        return String::new();
    }
    // All members are strings, integers and booleans: serialization is total.
    let value =
        crate::export::escape(&serde_json::to_string(paragraphs).expect("paragraph metadata"));
    format!(
        r#"<Properties><Label><KeyValuePair Key="{STORY_LABEL}" Value="{value}"/></Label></Properties>"#
    )
}

pub(crate) fn restore(story: &mut Element, styles: &StyleSet, refs: &style_codec::References) {
    let Some(records) = label(story, STORY_LABEL)
        .and_then(|json| serde_json::from_str::<Vec<AutomaticParagraph>>(json).ok())
    else {
        return;
    };
    for (index, range) in story
        .children
        .iter_mut()
        .filter(|e| e.name == "ParagraphStyleRange")
        .enumerate()
    {
        let Some(record) = records.iter().find(|r| r.index == index) else {
            continue;
        };
        let Some(reference) = range.attr("AppliedParagraphStyle") else {
            continue;
        };
        if reference != record.style
            || !automatic(styles, &refs.paragraph(reference))
            || range.attr("ParagraphDirection") != Some(record.direction.as_str())
            || paragraph_nodes(range, "Br").len() != usize::from(record.break_after)
            || paragraph_nodes(range, "Content")
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                != record.text
        {
            continue;
        }
        range
            .attributes
            .retain(|(key, _)| key != "ParagraphDirection");
    }
}

/// Footnotes, tables and anchored objects own independent text. Their words
/// and paragraph breaks must not invalidate the containing paragraph's guard.
fn paragraph_nodes<'a>(element: &'a Element, name: &str) -> Vec<&'a Element> {
    if crate::xml::story_structure(&element.name) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if element.name == name {
        out.push(element);
    }
    for child in &element.children {
        out.extend(paragraph_nodes(child, name));
    }
    out
}
