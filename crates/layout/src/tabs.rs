//! Paragraph tabs use the same native TabList record as list-marker spacing.
use crate::styles::{Align, ParagraphDirection, ResolvedParagraph};

pub(crate) fn stops(paragraph: &ResolvedParagraph, reverse: bool) -> schist_text_engine::TabStops {
    schist_text_engine::TabStops {
        hanging_indent: if reverse {
            paragraph.right_indent
        } else {
            paragraph.left_indent
        },
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
            .map(|tab| {
                use schist_text_engine::TabAlignment;
                // Native names describe physical left/right field edges. The
                // engine anchors along the paragraph's logical inline ruler.
                match (tab.text_alignment().unwrap_or_default(), reverse) {
                    (TabAlignment::Leading, true) => TabAlignment::Trailing,
                    (TabAlignment::Trailing, true) => TabAlignment::Leading,
                    (alignment, _) => alignment,
                }
            })
            .collect(),
        leaders: paragraph
            .list
            .tabs
            .iter()
            .flatten()
            .map(|tab| tab.leader.clone())
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
    unsupported_in_mode(
        paragraph,
        text,
        path,
        paragraph
            .writing_mode
            .map(crate::compose::engine_writing_mode)
            .unwrap_or_default(),
    )
}

/// Story orientation can supply the axis when the paragraph inherits it.
/// Diagnostics use that same resolved axis as composition and paint.
pub fn unsupported_in_mode(
    paragraph: &ResolvedParagraph,
    text: &str,
    path: bool,
    mode: schist_text_engine::WritingMode,
) -> Vec<&'static str> {
    if !text.contains('\t') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if paragraph.list.tabs.iter().flatten().any(|tab| {
        !tab.position.is_finite()
            || tab.text_alignment().is_none()
            || !schist_text_engine::valid_tab_leader(&tab.leader)
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
    let reverse = rtl && !mode.is_vertical();
    if reverse && paragraph.align != Some(Align::Right) {
        out.push("ParagraphDirection + TabList");
    }
    if paragraph.align == Some(Align::Center)
        || paragraph.align == Some(Align::Right) && !reverse
        || paragraph.align.is_some_and(|align| align.is_justified()) && has_aligned_stops(paragraph)
    {
        out.push("Justification + TabList");
    }
    if path && mode.is_vertical() {
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
