//! Public IDML integer ranges and native drop-cap flags; no guessed defaults.
use crate::{import::Report, xml::Element};
use schist_layout::{ParagraphStyle, ResolvedParagraph};

fn invalid(key: &str, value: &str) -> String {
    schist_i18n::tf!(
        "design.idml_text_preference_invalid",
        property = key,
        value = value
    )
}

pub(crate) fn count(
    element: &Element,
    key: &str,
    maximum: usize,
    report: &mut Report,
) -> Option<usize> {
    let raw = element.attr(key)?;
    raw.trim_matches([' ', '\t', '\r', '\n'])
        .parse::<i16>()
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= maximum)
        .or_else(|| {
            report.skip(invalid(key, raw));
            None
        })
}

pub(crate) fn detail(element: &Element, report: &mut Report) -> Option<i32> {
    let raw = element.attr("DropcapDetail")?;
    raw.trim_matches([' ', '\t', '\r', '\n'])
        .parse::<i32>()
        .ok()
        .or_else(|| {
            report.skip(invalid("DropcapDetail", raw));
            None
        })
}

fn counts(style: &ParagraphStyle) -> [(&'static str, Option<usize>, usize); 2] {
    [
        ("DropCapLines", style.drop_caps_lines, 25),
        ("DropCapCharacters", style.drop_caps_characters, 150),
    ]
}

pub(crate) fn warn(style: &ParagraphStyle, warnings: &mut Vec<String>) {
    for (key, value, max) in counts(style) {
        if let Some(value) = value.filter(|value| *value > max) {
            warnings.push(invalid(key, &value.to_string()));
        }
    }
}

pub(crate) fn warn_composition(style: &ResolvedParagraph, warnings: &mut Vec<String>) {
    if let Some(property) = schist_layout::drop_caps::unsupported_detail(style) {
        let message = schist_i18n::tf!("design.idml_drop_cap_unsupported", value = property);
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }
}

pub(crate) fn attributes(out: &mut String, style: &ParagraphStyle) {
    for (key, value, max) in counts(style) {
        if let Some(value) = value.filter(|value| *value <= max) {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
    }
    if let Some(value) = style.drop_caps_detail {
        out.push_str(&format!(" DropcapDetail=\"{value}\""));
    }
}
