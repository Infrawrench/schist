//! Ordered native paragraph rules, independent of authored character ranges.
use serde::{Deserialize, Serialize};
mod composition;
pub use composition::initial_style;
pub(crate) use composition::{materialize, runs};

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

impl NestedStyle {
    /// The native initial record's shape, independently of its character-style
    /// reference or the paragraph counts that activate its formatting.
    pub fn is_initial(&self) -> bool {
        matches!(&self.delimiter, Delimiter::Enumeration(value) if value == "Dropcap")
            && self.repetition == 1
            && self.inclusive
    }
}

/// Only the canonical leading Dropcap rule composes so far. Other rules
/// requesting a character style remain explicitly unsupported.
/// Empty lists and sequences made entirely of no-style rules cannot change
/// appearance, including when they reset an inherited formatted sequence.
pub fn unsupported(paragraph: &crate::ResolvedParagraph) -> Option<&'static str> {
    paragraph
        .nested_styles
        .as_ref()
        .filter(|rules| {
            rules.iter().enumerate().any(|(index, rule)| {
                !matches!(rule.character_style, CharacterStyle::None)
                    && !(index == 0 && composition::canonical_initial(rule))
            })
        })
        .map(|_| "AllNestedStyles")
}
