//! Retained native initial options with explicit composition limits.
use crate::ResolvedParagraph;

/// Native outline/grid flags are kept for interchange. The current initial
/// reservation uses Schist's legacy bounding-box policy; it does not implement
/// these native switches. Report active explicit settings until that changes.
pub fn unsupported_detail(paragraph: &ResolvedParagraph) -> Option<&'static str> {
    (paragraph.drop_caps_lines.unwrap_or(0) > 1
        && paragraph.drop_caps_characters.unwrap_or(1) > 0
        && paragraph.drop_caps_detail.is_some())
    .then_some("DropcapDetail")
}
