//! Unsupported native story containers are inert data, not flattened body text.
//! Standard Labels retain the model until native body/formatting edits invalidate
//! its coordinates. A stale Label never replaces those edits; its payloads survive
//! with unknown locations. This is Schist retention, not native table export.
use crate::{
    export,
    import::Report,
    style_codec::References,
    xml::{self, Element},
};
use schist_layout::{Story, StoryPoint, StoryStructure, StyleSet};
use serde::{Deserialize, Serialize};

const LABEL: &str = "Schist.StructuredStory.v1";

#[derive(Serialize, Deserialize)]
struct Record {
    native: String,
    story: Story,
    /// Older v1 records emitted only main-story text. Keep their guard exact
    /// without resurrecting a footnote deleted from a newer native export.
    #[serde(default)]
    native_footnotes: bool,
}

pub(crate) fn retain(
    native: String,
    id: &str,
    story: &Story,
    warnings: &mut Vec<String>,
) -> String {
    if story.retained_structures() == 0 {
        return native;
    }
    notice(warnings, "design.idml_story_structure");
    if story.structures.iter().any(|s| s.at.is_none()) {
        notice(warnings, "design.idml_structure_location");
    }
    let record = Record {
        native: id.into(),
        story: story.clone(),
        native_footnotes: true,
    };
    let value = export::escape(&serde_json::to_string(&record).expect("story metadata"));
    let entry = format!(r#"<KeyValuePair Key="{LABEL}" Value="{value}"/>"#);
    // Native writer places its own optional Properties/Label first. Merge there
    // so the Story has one Properties node even when Auto direction is present.
    if native.contains("</Label>") {
        native.replacen("</Label>", &format!("{entry}</Label>"), 1)
    } else {
        let at = native
            .find("<StoryPreference ")
            .expect("native story preference");
        let mut out = native;
        out.insert_str(
            at,
            &format!("<Properties><Label>{entry}</Label></Properties>"),
        );
        out
    }
}

pub(crate) fn restore(
    native: &Element,
    mut decoded: Story,
    styles: &StyleSet,
    refs: &References,
    report: &mut Report,
) -> Story {
    let entries: Vec<_> = native
        .children_named("Properties")
        .flat_map(|p| p.children_named("Label"))
        .flat_map(|label| label.children_named("KeyValuePair"))
        .filter(|entry| entry.attr("Key") == Some(LABEL))
        .collect();
    if entries.is_empty() {
        return decoded;
    }
    notice(&mut report.skipped, "design.idml_story_structure");
    let record = (entries.len() == 1)
        .then(|| entries[0].attr("Value"))
        .flatten()
        .and_then(|json| serde_json::from_str::<Record>(json).ok());
    if let Some(mut record) = record {
        if agrees(native, &record, styles, refs) {
            if record.story.structures.iter().any(|s| s.at.is_none()) {
                notice(&mut report.skipped, "design.idml_structure_location");
            }
            crate::story_codec::upgrade_controls(&mut record.story, refs);
            return record.story;
        }
        decoded
            .structures
            .extend(record.story.structures.into_iter().map(|mut s| {
                s.at = None;
                s
            }));
        decoded
            .structures
            .extend(record.story.points.into_iter().filter_map(|p| {
                if let StoryPoint::Other { kind, payload } = p {
                    Some(StoryStructure {
                        control: None,
                        at: None,
                        kind,
                        payload,
                        footnote: None,
                    })
                } else {
                    None
                }
            }));
    } else {
        // Even malformed or duplicate retention records remain recoverable data.
        // Never execute or recursively interpret their embedded XML/metadata.
        decoded
            .structures
            .extend(entries.iter().map(|entry| StoryStructure {
                control: None,
                at: None,
                kind: LABEL.into(),
                payload: entry.attr("Value").unwrap_or_default().into(),
                footnote: None,
            }));
    }
    notice(&mut report.skipped, "design.idml_structure_location");
    decoded
}

fn agrees(native: &Element, record: &Record, styles: &StyleSet, refs: &References) -> bool {
    if native.attr("Self") != Some(record.native.as_str()) {
        return false;
    }
    let text = record.story.text();
    if record
        .story
        .structures
        .iter()
        .filter_map(|s| s.at)
        .any(|at| at > text.len() || !text.is_char_boundary(at))
    {
        return false;
    }
    for note in record
        .story
        .structures
        .iter()
        .filter_map(|s| s.footnote.as_ref())
    {
        if !note.valid() {
            return false;
        }
        let paragraph_agrees =
            |name: &str| refs.paragraph(&format!("ParagraphStyle/$ID/{name}")) == name;
        let character_agrees =
            |name: &str| refs.character(&format!("CharacterStyle/$ID/{name}")) == name;
        if !paragraph_agrees(&note.reference_paragraph_style)
            || !character_agrees(&note.reference_character_style)
            || note.story.points.iter().any(
                |p| matches!(p, StoryPoint::Paragraph { style, .. } if !paragraph_agrees(style)),
            )
            || note
                .story
                .ranges
                .iter()
                .any(|r| !character_agrees(&r.style))
            || note
                .markers
                .iter()
                .any(|m| !character_agrees(&m.character_style))
        {
            return false;
        }
    }
    let mut legacy;
    let expected_story = if record.native_footnotes {
        &record.story
    } else {
        legacy = record.story.clone();
        for structure in &mut legacy.structures {
            structure.footnote = None;
        }
        &legacy
    };
    let expected =
        export::story_native_xml(&record.native, expected_story, styles, &mut Vec::new());
    let Ok(root) = xml::parse(&expected) else {
        return false;
    };
    let Some(expected) = root.find("Story") else {
        return false;
    };
    // A resource rename can leave its native Self unchanged. Restoring old model
    // names would silently undo that edit even though the range XML still matches.
    for paragraph in expected.find_all("ParagraphStyleRange") {
        let name = paragraph.attr("AppliedParagraphStyle").unwrap_or_default();
        if refs.paragraph(name) != crate::style_codec::name(name) {
            return false;
        }
    }
    for character in expected.find_all("CharacterStyleRange") {
        let name = character.attr("AppliedCharacterStyle").unwrap_or_default();
        if refs.character(name) != crate::style_codec::name(name) {
            return false;
        }
    }
    canonical(native.clone()) == canonical(expected.clone())
}

fn canonical(mut element: Element) -> Element {
    element.raw = None;
    element.attributes.sort();
    if element.name != "Content" && element.text.trim().is_empty() {
        element.text.clear();
    }
    element.children = element
        .children
        .into_iter()
        .filter(|child| !(child.name == "KeyValuePair" && child.attr("Key") == Some(LABEL)))
        .map(canonical)
        .filter(|child| {
            !(matches!(child.name.as_str(), "Properties" | "Label")
                && child.attributes.is_empty()
                && child.children.is_empty()
                && child.text.is_empty())
        })
        .collect();
    element
}

fn notice(warnings: &mut Vec<String>, key: &str) {
    let message = schist_i18n::t(key).to_string();
    if !warnings.contains(&message) {
        warnings.push(message);
    }
}
