//! Native atomic Capitalization and legacy independent-flag inheritance.
use crate::{import::Report, xml::Element};
use schist_text_engine::Capitalization;

const LABEL: &str = "Schist.CapitalizationFlags.v1";

fn name(value: Capitalization) -> &'static str {
    match value {
        Capitalization::Normal => "Normal",
        Capitalization::AllCaps => "AllCaps",
        Capitalization::SmallCaps => "SmallCaps",
        Capitalization::OpenTypeAllSmallCaps => "CapToSmallCap",
    }
}

pub(crate) fn read(element: &Element, report: &mut Report) -> (Option<bool>, Option<bool>) {
    if let Some(raw) = element.attr("Capitalization") {
        let value = match raw {
            "Normal" => Some(Capitalization::Normal),
            "AllCaps" => Some(Capitalization::AllCaps),
            "SmallCaps" => Some(Capitalization::SmallCaps),
            "CapToSmallCap" => Some(Capitalization::OpenTypeAllSmallCaps),
            _ => None,
        };
        if let Some(value) = value {
            let (all, small) = value.flags();
            return (Some(all), Some(small));
        }
        report.skip(schist_i18n::tf!("design.idml_caps_invalid", value = raw));
        // A native edit takes precedence, even if unsupported, over stale labels.
        return (None, None);
    }
    if let Some(raw) = crate::auto_direction::label(element, LABEL) {
        match serde_json::from_str::<(Option<bool>, Option<bool>)>(raw) {
            Ok(value) => return value,
            Err(_) => report.skip(schist_i18n::tf!("design.idml_caps_invalid", value = raw)),
        }
    }
    (None, None)
}

pub(crate) fn attributes(out: &mut String, all: Option<bool>, small: Option<bool>) {
    if let (Some(all), Some(small)) = (all, small) {
        out.push_str(&format!(
            " Capitalization=\"{}\"",
            name(Capitalization::from_flags(all, small))
        ));
    }
}

/// Native Capitalization cannot encode one independently inherited flag.
/// Retain that uncommon legacy case in a reported standard extension label.
pub(crate) fn label(out: &mut String, all: Option<bool>, small: Option<bool>) {
    if all.is_some() == small.is_some() {
        return;
    }
    let json = serde_json::to_string(&(all, small)).expect("optional booleans serialize");
    let pair = format!(
        "<KeyValuePair Key=\"{LABEL}\" Value=\"{}\"/>",
        crate::export::escape(&json)
    );
    if let Some(at) = out.find("</Label>") {
        out.insert_str(at, &pair);
    } else if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &format!("<Label>{pair}</Label>"));
    }
}

pub(crate) fn warn(all: Option<bool>, small: Option<bool>, warnings: &mut Vec<String>) {
    if all.is_some() != small.is_some() {
        let message = schist_i18n::t("design.idml_caps_private").to_string();
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }
}
