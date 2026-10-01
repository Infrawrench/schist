//! Paragraph tabs use the same native TabList record as list-marker spacing.
use crate::styles::{Align, ParagraphDirection, ResolvedParagraph};

pub(crate) fn stops(paragraph: &ResolvedParagraph) -> schist_text_engine::TabStops {
    schist_text_engine::TabStops {
        positions: paragraph
            .list
            .tabs
            .iter()
            .flatten()
            .map(|tab| tab.position)
            .collect(),
        alignments: paragraph
            .list
            .tabs
            .iter()
            .flatten()
            .map(|tab| tab.text_alignment().unwrap_or_default())
            .collect(),
        ..Default::default()
    }
}

pub(crate) fn has_aligned_stops(paragraph: &ResolvedParagraph) -> bool {
    paragraph
        .list
        .tabs
        .iter()
        .flatten()
        .any(|tab| tab.alignment != "LeftAlign")
}

/// Retained options whose native layout is not implemented. Diagnose only
/// paragraphs containing actual source tabs, independently of their list kind.
pub fn unsupported(paragraph: &ResolvedParagraph, text: &str, path: bool) -> Vec<&'static str> {
    if !text.contains('\t') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if paragraph.list.tabs.iter().flatten().any(|tab| {
        !tab.position.is_finite() || tab.text_alignment().is_none() || !tab.leader.is_empty()
    }) {
        out.push("TabList");
    }
    let rtl = match paragraph.direction.unwrap_or(ParagraphDirection::Auto) {
        ParagraphDirection::RightToLeft => true,
        ParagraphDirection::LeftToRight => false,
        ParagraphDirection::Auto => {
            schist_text_engine::base_direction(text)
                == schist_text_engine::ParagraphDirection::RightToLeft
        }
    };
    if rtl {
        out.push("ParagraphDirection + TabList");
    }
    if matches!(paragraph.align, Some(Align::Center | Align::Right))
        || paragraph.align.is_some_and(|align| align.is_justified()) && has_aligned_stops(paragraph)
    {
        out.push("Justification + TabList");
    }
    if path {
        out.push("TextPath + TabList");
    }
    if paragraph.drop_caps_lines.unwrap_or(0) > 1 {
        let end = schist_text_engine::grapheme_boundaries(text)
            .nth(paragraph.drop_caps_characters.unwrap_or(1))
            .unwrap_or(text.len());
        if text[..end].contains('\t') {
            out.push("DropCapCharacters + TabList");
        }
    }
    out
}
