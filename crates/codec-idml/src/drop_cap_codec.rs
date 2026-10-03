//! Public IDML integer ranges and native drop-cap flags; no guessed defaults.
use crate::{import::Report, xml::Element};
use schist_layout::{ParagraphStyle, ResolvedParagraph, StyleSet};

fn needs_implicit_count(style: &ResolvedParagraph) -> bool {
    style.drop_caps_characters.is_none()
        && (style.drop_caps_lines.is_some_and(|lines| lines > 1)
            || schist_layout::nested_styles::initial_style(style).is_some())
}

/// Schist's legacy unset count means one; the native default is zero. Emit
/// the necessary one at the first active style, preserving descendant count
/// inheritance. Inactive ancestors stay unchanged. Broken or cyclic chains
/// keep the explicit fallback rather than assuming another member supplies it.
pub(crate) fn needs_native_default(
    styles: &StyleSet,
    own: &ParagraphStyle,
    resolved: &ResolvedParagraph,
) -> bool {
    if !needs_implicit_count(resolved) {
        return false;
    }
    let mut seen = vec![own.name.as_str()];
    let mut parent = own.based_on.as_deref();
    let mut inherited_default = false;
    while let Some(name) = parent {
        if seen.contains(&name) {
            return true;
        }
        let Some(style) = styles.paragraph(name) else {
            return true;
        };
        seen.push(name);
        inherited_default |= needs_implicit_count(&styles.resolve_paragraph(name));
        parent = style.based_on.as_deref();
    }
    !inherited_default
}

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
