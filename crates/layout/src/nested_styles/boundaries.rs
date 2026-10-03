//! Source boundaries for the supported ordered native rules.
use super::{CharacterStyle, Delimiter, NestedStyle};

enum Kind<'a> {
    Characters(&'a str),
    Digits,
    Character(char),
    AnyCharacter,
}

fn kind(delimiter: &Delimiter) -> Option<Kind<'_>> {
    match delimiter {
        // Native strings are sets of alternative terminating characters.
        Delimiter::Text(value) => (!value.is_empty()).then_some(Kind::Characters(value)),
        Delimiter::Enumeration(value) => Some(match value.as_str() {
            "AnyCharacter" => Kind::AnyCharacter,
            "Digits" => Kind::Digits,
            "Tabs" => Kind::Character('\t'),
            "ForcedLineBreak" => Kind::Character('\u{2028}'),
            "EmSpace" => Kind::Character('\u{2003}'),
            "EnSpace" => Kind::Character('\u{2002}'),
            "NonbreakingSpace" => Kind::Character('\u{a0}'),
            _ => return None,
        }),
    }
}

pub(super) fn supported(index: usize, rule: &NestedStyle) -> bool {
    !matches!(rule.character_style, CharacterStyle::Unresolved(_))
        && ((index == 0 && rule.is_initial())
            || (rule.repetition > 0 && kind(&rule.delimiter).is_some()))
}

/// Unknown bounds stop the sequence, including no-style rules: guessing their
/// extent would start a later named style at the wrong source character.
pub(super) fn prefix(rules: &[NestedStyle]) -> usize {
    rules
        .iter()
        .enumerate()
        .take_while(|(index, rule)| supported(*index, rule))
        .count()
}

/// Through includes the final delimiter; up-to leaves it to the next rule.
/// Missing delimiters extend to paragraph end. All cuts stay on graphemes,
/// including a literal base character followed by combining marks.
pub(super) fn end(text: &str, rule: &NestedStyle) -> usize {
    let Some(kind) = kind(&rule.delimiter) else {
        return text.len();
    };
    let mut remaining = rule.repetition as usize;
    let mut boundaries = schist_text_engine::grapheme_boundaries(text);
    let mut start = boundaries.next().unwrap_or(0);
    for end in boundaries {
        let instances = match kind {
            Kind::AnyCharacter => 1,
            Kind::Characters(values) => text[start..end]
                .chars()
                .filter(|c| values.contains(*c))
                .count(),
            Kind::Digits => text[start..end]
                .chars()
                .filter(char::is_ascii_digit)
                .count(),
            Kind::Character(value) => text[start..end].chars().filter(|c| *c == value).count(),
        };
        if instances >= remaining {
            return if rule.inclusive { end } else { start };
        }
        remaining -= instances;
        start = end;
    }
    text.len()
}
