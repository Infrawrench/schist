//! Native keep policies and guarded retention of older Schist style fields.
use crate::{import::Report, style_codec, xml::Element};
use schist_layout::{paragraph_keeps::ParagraphKeeps, ParagraphStyle};

const LABEL: &str = "Schist.ParagraphKeeps.v1";

fn invalid(report: &mut Report, property: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_text_preference_invalid",
        property = property,
        value = value
    ));
}

pub(crate) fn read(element: &Element, report: &mut Report) -> ParagraphKeeps {
    let mut boolean = |key| {
        element
            .attr(key)
            .and_then(|value| match crate::xml::parse_boolean(value) {
                Some(value) => Some(value),
                None => {
                    invalid(report, key, value);
                    None
                }
            })
    };
    let enabled = boolean("KeepLinesTogether");
    let all = boolean("KeepAllLinesTogether");
    let previous = boolean("KeepWithPrevious");
    let mut count = |key, min, max| {
        element.attr(key).and_then(|value| {
            value
                .parse::<usize>()
                .ok()
                .filter(|n| (min..=max).contains(n))
                .or_else(|| {
                    invalid(report, key, value);
                    None
                })
        })
    };
    ParagraphKeeps {
        enabled,
        all,
        first: count("KeepFirstLines", 1, 50),
        last: count("KeepLastLines", 1, 50),
        next: count("KeepWithNext", 0, 5),
        previous,
    }
}

fn authored(style: &ParagraphStyle) -> ParagraphKeeps {
    style.keeps.over(&ParagraphKeeps::from_legacy(
        style.keep_with_next,
        style.keep_lines,
    ))
}

pub(crate) fn native(style: &ParagraphStyle) -> ParagraphKeeps {
    let mut keeps = authored(style);
    keeps.first = keeps.first.filter(|n| (1..=50).contains(n));
    keeps.last = keeps.last.filter(|n| (1..=50).contains(n));
    keeps.next = keeps.next.filter(|n| *n <= 5);
    keeps
}

pub(crate) fn warn(style: &ParagraphStyle, warnings: &mut Vec<String>) {
    let keeps = authored(style);
    for (property, value, valid) in [
        (
            "KeepFirstLines",
            keeps.first,
            keeps.first.is_none_or(|n| (1..=50).contains(&n)),
        ),
        (
            "KeepLastLines",
            keeps.last,
            keeps.last.is_none_or(|n| (1..=50).contains(&n)),
        ),
        (
            "KeepWithNext",
            keeps.next,
            keeps.next.is_none_or(|n| n <= 5),
        ),
    ] {
        if !valid {
            warnings.push(schist_i18n::tf!(
                "design.idml_text_preference_invalid",
                property = property,
                value = value.unwrap().to_string()
            ));
        }
    }
}

pub(crate) fn attributes(out: &mut String, keeps: &ParagraphKeeps) {
    for (key, value) in [
        ("KeepLinesTogether", keeps.enabled),
        ("KeepAllLinesTogether", keeps.all),
        ("KeepWithPrevious", keeps.previous),
    ] {
        if let Some(value) = value {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
    }
    for (key, value) in [
        ("KeepFirstLines", keeps.first),
        ("KeepLastLines", keeps.last),
        ("KeepWithNext", keeps.next),
    ] {
        if let Some(value) = value {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Authored {
    keeps: ParagraphKeeps,
    keep_lines: Option<usize>,
    keep_with_next: Option<bool>,
    native: ParagraphKeeps,
}

pub(crate) fn restore(
    element: &Element,
    keeps: ParagraphKeeps,
) -> (ParagraphKeeps, Option<usize>, Option<bool>) {
    // Malformed native edits must not accidentally match an absent saved
    // property and resurrect an older authored value.
    let mut checked = Report::default();
    read(element, &mut checked);
    if let Some(saved) = crate::auto_direction::label(element, LABEL)
        .and_then(|value| serde_json::from_str::<Authored>(value).ok())
        .filter(|saved| checked.is_complete() && saved.native == keeps)
    {
        return (saved.keeps, saved.keep_lines, saved.keep_with_next);
    }
    (keeps, None, None)
}

pub(crate) fn label(out: &mut String, style: &ParagraphStyle) {
    if style.keep_lines.is_none()
        && style.keep_with_next.is_none()
        && native(style) == authored(style)
    {
        return;
    }
    let saved = Authored {
        keeps: style.keeps.clone(),
        keep_lines: style.keep_lines,
        keep_with_next: style.keep_with_next,
        native: native(style),
    };
    let value = serde_json::to_string(&saved).expect("integer and boolean keep settings");
    let pair = format!(
        "<KeyValuePair Key=\"{LABEL}\" Value=\"{}\"/>",
        style_codec::escape(&value)
    );
    if let Some(at) = out.find("</Label>") {
        out.insert_str(at, &pair);
    } else if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &format!("<Label>{pair}</Label>"));
    }
}
