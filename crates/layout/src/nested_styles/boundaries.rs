//! Source boundaries for the supported ordered native rules.
use super::{CharacterStyle, Delimiter, NestedStyle};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

enum Kind<'a> {
    Characters(&'a str),
    Digits,
    Letters,
    Character(char),
    AnyCharacter,
    Word,
    EndMarker,
}

fn kind(delimiter: &Delimiter) -> Option<Kind<'_>> {
    match delimiter {
        // Native strings are sets of alternative terminating characters.
        Delimiter::Text(value) => (!value.is_empty()).then_some(Kind::Characters(value)),
        Delimiter::Enumeration(value) => Some(match value.as_str() {
            "AnyCharacter" => Kind::AnyCharacter,
            "AnyWord" => Kind::Word,
            "EndNestedStyle" => Kind::EndMarker,
            "Digits" => Kind::Digits,
            "Letters" => Kind::Letters,
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
            || (rule.repetition > 0
                && kind(&rule.delimiter).is_some()
                && (!matches!(kind(&rule.delimiter), Some(Kind::EndMarker))
                    || rule.repetition == 1)))
}

/// A valid Repeat loops only the preceding ordinary rules; later records stay
/// retained but never participate. Unknown bounds stop the supported prefix.
pub(super) struct Plan {
    pub prefix: usize,
    pub repeat_from: Option<usize>,
    pub complete: bool,
}

pub(super) fn plan(rules: &[NestedStyle]) -> Plan {
    for (index, rule) in rules.iter().enumerate() {
        if matches!(&rule.delimiter, Delimiter::Enumeration(value) if value == "Repeat") {
            let count = usize::try_from(rule.repetition).ok().filter(|n| *n > 0);
            let repeat_from = count.and_then(|n| index.checked_sub(n)).filter(|start| {
                matches!(rule.character_style, CharacterStyle::None)
                    && !rules[*start..index].iter().any(NestedStyle::is_initial)
            });
            return Plan {
                prefix: index,
                complete: repeat_from.is_some(),
                repeat_from,
            };
        }
        if !supported(index, rule) {
            return Plan {
                prefix: index,
                repeat_from: None,
                complete: false,
            };
        }
    }
    Plan {
        prefix: rules.len(),
        repeat_from: None,
        complete: true,
    }
}

// Only ordinary rules produce source spans; Repeat is a terminal control.
pub(super) fn prefix(rules: &[NestedStyle]) -> usize {
    plan(rules).prefix
}

/// Through includes the final delimiter; up-to leaves it to the next rule.
/// Missing delimiters extend to paragraph end. All cuts stay on graphemes,
/// including a literal base character followed by combining marks.
pub(super) struct Boundary {
    pub at: usize,
    /// A through-bound ends before a zero-width control at its next boundary;
    /// an excluded delimiter or missing condition has not consumed that point.
    pub found: bool,
}

pub(super) fn end(text: &str, rule: &NestedStyle) -> Boundary {
    let Some(kind) = kind(&rule.delimiter) else {
        return Boundary {
            at: text.len(),
            found: false,
        };
    };
    let mut remaining = rule.repetition as usize;
    let mut boundaries = schist_text_engine::grapheme_boundaries(text);
    let mut start = boundaries.next().unwrap_or(0);
    let mut word_content = false;
    for end in boundaries {
        let instances = match kind {
            Kind::EndMarker => 0,
            Kind::AnyCharacter => 1,
            Kind::Word => {
                let separator = text[start..end].chars().any(word_separator);
                let count = usize::from(separator && word_content);
                word_content = !separator;
                count
            }
            Kind::Characters(values) => text[start..end]
                .chars()
                .filter(|c| values.contains(*c))
                .count(),
            Kind::Digits => text[start..end]
                .chars()
                .filter(char::is_ascii_digit)
                .count(),
            // Count Letter scalars, excluding NumberLetter and combining marks.
            // The enclosing grapheme still determines the only legal cut.
            Kind::Letters => text[start..end]
                .chars()
                .filter(|c| c.general_category_group() == GeneralCategoryGroup::Letter)
                .count(),
            Kind::Character(value) => text[start..end].chars().filter(|c| *c == value).count(),
        };
        if instances >= remaining {
            return Boundary {
                at: if rule.inclusive { end } else { start },
                found: true,
            };
        }
        remaining -= instances;
        start = end;
    }
    Boundary {
        at: text.len(),
        found: false,
    }
}

// Whitespace terminates a nonempty word. Nonbreaking spaces join terms;
// punctuation and script changes alone do not introduce a word boundary.
// This is a bounded Unicode policy, not language-dependent segmentation.
fn word_separator(value: char) -> bool {
    value.is_whitespace() && !matches!(value, '\u{a0}' | '\u{2007}' | '\u{202f}')
}
