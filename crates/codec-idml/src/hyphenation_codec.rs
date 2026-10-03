//! Native paragraph hyphenation policy. No private metadata or guessed defaults.
use crate::{
    import::Report,
    xml::{self, Element},
};
use schist_layout::hyphenation::HyphenationOptions;

fn invalid(property: &str, value: &str) -> String {
    schist_i18n::tf!(
        "design.idml_text_preference_invalid",
        property = property,
        value = value
    )
}

pub(crate) fn boolean(element: &Element, key: &str, report: &mut Report) -> Option<bool> {
    let raw = element.attr(key)?;
    xml::parse_boolean(raw).or_else(|| {
        report.skip(invalid(key, raw));
        None
    })
}

fn count(element: &Element, key: &str, min: u8, max: u8, report: &mut Report) -> Option<u8> {
    let raw = element.attr(key)?;
    raw.trim_matches([' ', '\t', '\r', '\n'])
        .parse::<i16>()
        .ok()
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| (min..=max).contains(value))
        .or_else(|| {
            report.skip(invalid(key, raw));
            None
        })
}

pub(crate) fn read(element: &Element, report: &mut Report) -> HyphenationOptions {
    HyphenationOptions {
        capitalized_words: boolean(element, "HyphenateCapitalizedWords", report),
        last_word: boolean(element, "HyphenateLastWord", report),
        across_columns: boolean(element, "HyphenateAcrossColumns", report),
        after_first: count(element, "HyphenateAfterFirst", 1, 15, report),
        before_last: count(element, "HyphenateBeforeLast", 1, 15, report),
        words_longer_than: count(element, "HyphenateWordsLongerThan", 3, 25, report),
        ladder_limit: count(element, "HyphenateLadderLimit", 0, 25, report),
        weight: count(element, "HyphenWeight", 0, 100, report),
        zone: element.attr("HyphenationZone").and_then(|raw| {
            raw.trim_matches([' ', '\t', '\r', '\n'])
                .parse::<f32>()
                .ok()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .or_else(|| {
                    report.skip(invalid("HyphenationZone", raw));
                    None
                })
        }),
    }
}

fn counts(policy: &HyphenationOptions) -> [(&'static str, Option<u8>, u8, u8); 5] {
    [
        ("HyphenateAfterFirst", policy.after_first, 1, 15),
        ("HyphenateBeforeLast", policy.before_last, 1, 15),
        ("HyphenateWordsLongerThan", policy.words_longer_than, 3, 25),
        ("HyphenateLadderLimit", policy.ladder_limit, 0, 25),
        ("HyphenWeight", policy.weight, 0, 100),
    ]
}

pub(crate) fn warn(policy: &HyphenationOptions, warnings: &mut Vec<String>) {
    for (key, value, min, max) in counts(policy) {
        if let Some(value) = value.filter(|value| !(min..=max).contains(value)) {
            warnings.push(invalid(key, &value.to_string()));
        }
    }
    if let Some(value) = policy
        .zone
        .filter(|value| !value.is_finite() || *value < 0.0)
    {
        warnings.push(invalid("HyphenationZone", &value.to_string()));
    }
}

pub(crate) fn attributes(out: &mut String, policy: &HyphenationOptions) {
    for (key, value) in [
        ("HyphenateCapitalizedWords", policy.capitalized_words),
        ("HyphenateLastWord", policy.last_word),
        ("HyphenateAcrossColumns", policy.across_columns),
    ] {
        if let Some(value) = value {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
    }
    for (key, value, min, max) in counts(policy) {
        if let Some(value) = value.filter(|value| (min..=max).contains(value)) {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
    }
    if let Some(value) = policy
        .zone
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        out.push_str(&format!(" HyphenationZone=\"{value}\""));
    }
}
