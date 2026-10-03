//! Shared text-variable definitions. Instances refer to opaque identities rather
//! than names or vector positions. The kernel never interprets retained XML.
use serde::{Deserialize, Serialize};

/// A custom text definition shared by every referring story instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub id: String,
    pub name: String,
    /// Literal custom contents, not an instance's cached ResultText.
    pub contents: String,
}

/// Effective instance formatting, also used by package font inventories. An
/// instance inherits its paragraph, without footnote superscript preferences.
pub fn instance_character(
    doc: &crate::LayoutDocument,
    story: &crate::Story,
    at: usize,
    name: &str,
) -> Option<crate::ResolvedCharacter> {
    let (_, _, style) = paragraph(story, at)?;
    let paragraph = doc.styles.resolve_paragraph(if style.is_empty() {
        &doc.default_paragraph_style
    } else {
        style
    });
    let mut base = paragraph.character(doc.styles.resolve_character(&doc.default_character_style));
    base.point_size = paragraph.point_size.or(base.point_size);
    base.leading = paragraph.leading.or(base.leading);
    base.tracking = paragraph.tracking.or(base.tracking);
    Some(doc.styles.resolve_character(name).over(&base))
}

fn paragraph(story: &crate::Story, at: usize) -> Option<(&str, usize, &str)> {
    story
        .points
        .iter()
        .zip(story.point_offsets())
        .find_map(|(point, start)| {
            let crate::StoryPoint::Paragraph { text, style } = point else {
                return None;
            };
            (start <= at && at <= start + text.len() && text.is_char_boundary(at - start))
                .then_some((text.as_str(), start, style.as_str()))
        })
}

pub(crate) struct Instance {
    pub structure: usize,
    pub at: usize,
    pub text: String,
    pub character: crate::ResolvedCharacter,
}

/// Only supported, unambiguous references become display objects. Others stay
/// on the existing unrendered-structure diagnostic path, with their XML intact.
pub(crate) fn instances(doc: &crate::LayoutDocument, story: &crate::Story) -> Vec<Instance> {
    story.structures.iter().enumerate().filter_map(|(index, structure)| {
        if structure.kind != "TextVariableInstance" || structure.footnote.is_some() {
            return None;
        }
        let Some(crate::story::InlineControl::TextVariable { variable, character_style, .. }) = &structure.control else {
            return None;
        };
        let at = structure.at?;
        let (text, start, style) = paragraph(story, at)?;
        if !schist_text_engine::grapheme_boundaries(text).any(|offset| start + offset == at) {
            return None;
        }
        let paragraph = doc.styles.resolve_paragraph(if style.is_empty() {
            &doc.default_paragraph_style
        } else { style });
        // Initial and nested rules count a variable as one logical object.
        // Their source-only rule projection does not implement that yet.
        if (paragraph.drop_caps_lines.unwrap_or(0) > 0 && paragraph.drop_caps_characters.unwrap_or(1) > 0)
            || paragraph.nested_styles.as_ref().is_some_and(|rules| !rules.is_empty())
        {
            return None;
        }
        let mut definitions = doc.text_variables.iter().filter(|d| !d.id.is_empty() && d.id == *variable);
        let definition = definitions.next()?;
        if definitions.next().is_some() || definition.contents.chars().any(|c| {
            c.is_control() || matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }) {
            return None;
        }
        Some(Instance {
            structure: index,
            at,
            text: format!("\u{2068}{}\u{2069}", definition.contents),
            character: instance_character(doc, story, at, character_style)?,
        })
    }).collect()
}

pub(crate) fn slice_objects(
    objects: &[std::ops::Range<usize>],
    range: std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    objects
        .iter()
        .filter(|span| span.start >= range.start && span.end <= range.end)
        .map(|span| span.start - range.start..span.end - range.start)
        .collect()
}
