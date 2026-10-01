//! Language-specific shaping and display casing keep original source clusters.

/// Normalize spelling after RFC 5646 syntax checks (not registry validation).
/// Unicode casing permits underscores as separators. Empty/malformed tags
/// select default shaping. Deprecated tags are retained, not replaced.
pub fn normalize_language(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    let value = value.trim().replace('_', "-").to_ascii_lowercase();
    if matches!(
        value.as_str(),
        "en-gb-oed"
            | "i-ami"
            | "i-bnn"
            | "i-default"
            | "i-enochian"
            | "i-hak"
            | "i-klingon"
            | "i-lux"
            | "i-mingo"
            | "i-navajo"
            | "i-pwn"
            | "i-tao"
            | "i-tay"
            | "i-tsu"
            | "sgn-be-fr"
            | "sgn-be-nl"
            | "sgn-ch-de"
    ) {
        return Some(value);
    }
    let parts: Vec<_> = value.split('-').collect();
    if parts
        .iter()
        .any(|s| s.is_empty() || s.len() > 8 || !s.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return None;
    }
    if parts[0] == "x" {
        return (parts.len() > 1).then_some(value);
    }
    let alpha = |s: &str| s.bytes().all(|b| b.is_ascii_alphabetic());
    if parts[0].len() < 2 || !alpha(parts[0]) {
        return None;
    }
    let mut i = 1;
    if parts[0].len() <= 3 {
        for _ in 0..3 {
            if parts.get(i).is_some_and(|s| s.len() == 3 && alpha(s)) {
                i += 1;
            } else {
                break;
            }
        }
    }
    if parts.get(i).is_some_and(|s| s.len() == 4 && alpha(s)) {
        i += 1;
    }
    if parts.get(i).is_some_and(|s| {
        (s.len() == 2 && alpha(s)) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
    }) {
        i += 1;
    }
    let mut variants = std::collections::HashSet::new();
    while let Some(s) = parts
        .get(i)
        .filter(|s| s.len() >= 5 || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit()))
    {
        if !variants.insert(*s) {
            return None;
        }
        i += 1;
    }
    let mut extensions = [false; 128];
    while let Some(s) = parts.get(i).filter(|s| s.len() == 1 && **s != "x") {
        let key = s.as_bytes()[0] as usize;
        if extensions[key] {
            return None;
        }
        extensions[key] = true;
        i += 1;
        let start = i;
        while parts.get(i).is_some_and(|s| s.len() >= 2) {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if parts.get(i) == Some(&"x") {
        i += 1;
        if i == parts.len() {
            return None;
        }
        i = parts.len();
    }
    (i == parts.len()).then_some(value)
}

// "und" and an empty language both request default shaping. Avoid introducing
// a shaping boundary (and losing kerning) for an equivalent explicit reset.
pub(super) fn effective(value: &str) -> String {
    normalize_language(value)
        .filter(|s| s != "und")
        .unwrap_or_default()
}

/// Unicode 17 SpecialCasing: Turkic dotted capitals and Lithuanian dot removal.
/// Context is original source text, including across style/item boundaries.
pub(super) fn uppercase(text: &str, byte: usize, language: &str) -> impl Iterator<Item = char> {
    let c = text[byte..]
        .chars()
        .next()
        .expect("source character boundary");
    let primary = language.split('-').next();
    let dotted = c == 'i' && matches!(primary, Some("tr" | "az"));
    let remove = c == '\u{307}' && primary == Some("lt") && after_soft_dotted(&text[..byte]);
    c.to_uppercase()
        .filter(move |_| !remove)
        .map(move |upper| if dotted { 'İ' } else { upper })
}

fn after_soft_dotted(before: &str) -> bool {
    for c in before.chars().rev() {
        if soft_dotted(c) {
            return true;
        }
        if matches!(
            unicode_normalization::char::canonical_combining_class(c),
            0 | 230
        ) {
            return false;
        }
    }
    false
}

// Unicode 17 PropList.txt, Soft_Dotted (50 scalars). Keep in step with the
// combining classes in unicode-normalization. https://www.unicode.org/license.txt
fn soft_dotted(c: char) -> bool {
    matches!(c as u32,
        0x69..=0x6a | 0x12f | 0x249 | 0x268 | 0x29d | 0x2b2 | 0x3f3 | 0x456 | 0x458 |
        0x1d62 | 0x1d96 | 0x1da4 | 0x1da8 | 0x1e2d | 0x1ecb | 0x2071 | 0x2148..=0x2149 | 0x2c7c |
        0x1d422..=0x1d423 | 0x1d456..=0x1d457 | 0x1d48a..=0x1d48b | 0x1d4be..=0x1d4bf |
        0x1d4f2..=0x1d4f3 | 0x1d526..=0x1d527 | 0x1d55a..=0x1d55b | 0x1d58e..=0x1d58f |
        0x1d5c2..=0x1d5c3 | 0x1d5f6..=0x1d5f7 | 0x1d62a..=0x1d62b | 0x1d65e..=0x1d65f |
        0x1d692..=0x1d693 | 0x1df1a | 0x1e04c..=0x1e04d | 0x1e068)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lithuanian_casing_uses_original_context_and_stops_at_above_marks_or_starters() {
        for base in ['i', 'j', 'į', 'і', '\u{1d422}'] {
            for intervening in ["", "\u{328}", "\u{323}\u{328}"] {
                let text = format!("{base}{intervening}\u{307}");
                assert_eq!(uppercase(&text, text.len() - 2, "lt-lt").count(), 0);
                assert_eq!(
                    uppercase(&text, text.len() - 2, "en").collect::<String>(),
                    "\u{307}"
                );
            }
            for blocker in ["a", " ", "\u{301}", "\u{307}", "\u{34f}"] {
                let text = format!("{base}{blocker}\u{307}");
                assert_eq!(
                    uppercase(&text, text.len() - 2, "lt").collect::<String>(),
                    "\u{307}"
                );
            }
        }
    }
    #[test]
    fn tags_normalize_without_losing_script_region_or_private_subtags() {
        for (input, expected) in [
            ("TR_tr", "tr-tr"),
            ("sr-Latn-RS", "sr-latn-rs"),
            ("x-custom", "x-custom"),
            ("zh-cmn-Hans-CN", "zh-cmn-hans-cn"),
            ("de-CH-1901", "de-ch-1901"),
            ("sgn-BE-FR", "sgn-be-fr"),
            ("en-x-a-b", "en-x-a-b"),
            ("en-US-u-co-phonebk", "en-us-u-co-phonebk"),
        ] {
            assert_eq!(normalize_language(input).as_deref(), Some(expected));
        }
        for input in [
            "",
            " ",
            "en--US",
            "-tr",
            "tr-",
            "Language/$ID/Turkish",
            "Turkish: test",
            "é",
            "123",
            "en-123456789",
            "en-u",
            "en-x",
            "i-invented",
            "en-US-US",
            "en-abc-def-ghi-jkl",
            "sl-rozaj-rozaj",
            "en-u-ca-u-nu",
            "en-Latn-Latn",
        ] {
            assert!(normalize_language(input).is_none());
        }
    }
}
