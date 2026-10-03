//! Ordered native paragraph rules, independent of authored character ranges.
use serde::{Deserialize, Serialize};

/// Native string and enumeration delimiters have different semantics, even
/// when their text is identical. Literal strings keep their whitespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delimiter {
    Text(String),
    Enumeration(String),
}

/// Keep unresolved opaque references unchanged until a matching resource exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharacterStyle {
    None,
    Named(String),
    Unresolved(String),
}

/// One AllNestedStyles record. The public schema uses a signed 32-bit repeat
/// count; retention must not clamp values whose composition is unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NestedStyle {
    pub character_style: CharacterStyle,
    pub delimiter: Delimiter,
    pub repetition: i32,
    pub inclusive: bool,
}

/// Rules requesting a character style are retained but not yet composed.
/// Empty lists and sequences made entirely of no-style rules cannot change
/// appearance, including when they reset an inherited formatted sequence.
pub fn unsupported(paragraph: &crate::ResolvedParagraph) -> Option<&'static str> {
    paragraph
        .nested_styles
        .as_ref()
        .filter(|rules| {
            rules
                .iter()
                .any(|rule| !matches!(rule.character_style, CharacterStyle::None))
        })
        .map(|_| "AllNestedStyles")
}
