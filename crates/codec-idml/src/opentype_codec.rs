//! Native IDML feature switches from the public XML specification. Atomic
//! figure/set attributes expand to explicit per-tag values, including false.
//! Features with no faithful native representation use standard Label entries
//! and an export notice; they must not masquerade as native support.
use crate::{import::Report, xml::Element};

const LABEL: &str = "Schist.OpenTypeFeatures.v1";
const SWITCHES: &[(&str, &str)] = &[
    ("Ligatures", "liga"),
    ("OTFDiscretionaryLigature", "dlig"),
    ("OTFContextualAlternate", "calt"),
    ("OTFFraction", "frac"),
    ("OTFOrdinal", "ordn"),
    ("OTFTitling", "titl"),
    ("OTFSwash", "swsh"),
    ("OTFSlashedZero", "zero"),
    ("OTFHistorical", "hist"),
    ("OTFMark", "mark"),
    ("OTFLocale", "locl"),
    ("OTFStylisticAlternate", "salt"),
    ("OTFJustificationAlternate", "jalt"),
    ("OTFStretchedAlternate", "stch"),
    ("OTFOverlapSwash", "cswh"),
    ("OTFRomanItalics", "ital"),
];
const FIGURES: &[(&str, [bool; 4])] = &[
    ("Default", [false, false, false, false]),
    ("TabularLining", [true, false, true, false]),
    ("ProportionalLining", [false, true, true, false]),
    ("TabularOldstyle", [true, false, false, true]),
    ("ProportionalOldstyle", [false, true, false, true]),
];
const FIGURE_TAGS: [&str; 4] = ["tnum", "pnum", "lnum", "onum"];
const CONDITIONAL: &[(&str, [&str; 2])] = &[
    ("OTFHVKana", ["hkna", "vkna"]),
    ("OTFProportionalMetrics", ["palt", "vpal"]),
];

fn invalid(report: &mut Report, property: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_feature_invalid",
        property = property,
        value = value
    ));
}
fn valid(features: &[(String, bool)]) -> bool {
    features.iter().enumerate().all(|(index, (tag, _))| {
        tag.len() == 4
            && tag.bytes().all(|b| b.is_ascii_graphic())
            && !features[..index].iter().any(|(other, _)| other == tag)
    })
}
pub(crate) fn read(element: &Element, report: &mut Report) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    for (property, tag) in SWITCHES {
        if let Some(raw) = element.attr(property) {
            match raw {
                "true" | "1" => result.push(((*tag).into(), true)),
                "false" | "0" => result.push(((*tag).into(), false)),
                _ => invalid(report, property, raw),
            }
        }
    }
    if let Some(raw) = element.attr("OTFFigureStyle") {
        if let Some((_, values)) = FIGURES.iter().find(|(name, _)| *name == raw) {
            result.extend(
                FIGURE_TAGS
                    .into_iter()
                    .zip(values)
                    .map(|(tag, value)| (tag.into(), *value)),
            );
        } else {
            invalid(report, "OTFFigureStyle", raw);
        }
    }
    if let Some(raw) = element.attr("OTFStylisticSets") {
        if let Some(mask) = raw.parse::<u32>().ok().filter(|mask| *mask < (1 << 20)) {
            result.extend((1..=20).map(|i| (format!("ss{i:02}"), mask & (1 << (i - 1)) != 0)));
        } else {
            invalid(report, "OTFStylisticSets", raw);
        }
    }
    // These native switches choose different tags in horizontal/vertical text.
    // Their conditional behavior is not represented by independent tag switches.
    for (property, tags) in CONDITIONAL {
        if let Some(raw) = element.attr(property) {
            if matches!(raw, "false" | "0") {
                // Disabling both modes is representable and must reset an
                // inherited per-tag override rather than silently inheriting.
                result.extend(tags.iter().map(|tag| ((*tag).into(), false)));
            } else {
                report.skip(schist_i18n::tf!(
                    "design.idml_feature_unsupported",
                    property = property,
                    value = raw
                ));
            }
        }
    }
    if let Some(raw) = crate::auto_direction::label(element, LABEL) {
        match serde_json::from_str::<Vec<(String, bool)>>(raw) {
            Ok(extra) if valid(&extra) => {
                // Native edits take precedence if another application changed
                // an attribute while retaining our extension label.
                result = schist_layout::styles::inherited_features(&result, &extra);
            }
            _ => invalid(report, LABEL, raw),
        }
    }
    schist_layout::styles::inherited_features(&result, &[])
}

fn value(features: &[(String, bool)], tag: &str) -> Option<bool> {
    features
        .iter()
        .find(|(name, _)| name == tag)
        .map(|(_, enabled)| *enabled)
}
fn figure(features: &[(String, bool)]) -> Option<&'static str> {
    let values: Vec<_> = FIGURE_TAGS.iter().map(|tag| value(features, tag)).collect();
    FIGURES
        .iter()
        .find(|(_, candidate)| values.iter().zip(candidate).all(|(a, b)| *a == Some(*b)))
        .map(|(name, _)| *name)
}
fn sets(features: &[(String, bool)]) -> Option<u32> {
    (1..=20).try_fold(0, |mask, i| {
        Some(mask | (u32::from(value(features, &format!("ss{i:02}"))?) << (i - 1)))
    })
}
fn extras(features: &[(String, bool)]) -> Vec<(String, bool)> {
    let figure = figure(features);
    let sets = sets(features);
    features
        .iter()
        .filter(|(tag, _)| {
            !SWITCHES.iter().any(|(_, native)| native == tag)
                && !(figure.is_some() && FIGURE_TAGS.contains(&tag.as_str()))
                && !(sets.is_some() && (1..=20).any(|i| *tag == format!("ss{i:02}")))
                && !CONDITIONAL.iter().any(|(_, tags)| {
                    tags.contains(&tag.as_str())
                        && tags.iter().all(|t| value(features, t) == Some(false))
                })
        })
        .cloned()
        .collect()
}

pub(crate) fn attributes(out: &mut String, features: &[(String, bool)]) {
    for (property, tag) in SWITCHES {
        if let Some(value) = value(features, tag) {
            out.push_str(&format!(" {property}=\"{value}\""));
        }
    }
    if let Some(figure) = figure(features) {
        out.push_str(&format!(" OTFFigureStyle=\"{figure}\""));
    }
    if let Some(mask) = sets(features) {
        out.push_str(&format!(" OTFStylisticSets=\"{mask}\""));
    }
    for (property, tags) in CONDITIONAL {
        if tags.iter().all(|tag| value(features, tag) == Some(false)) {
            out.push_str(&format!(" {property}=\"false\""));
        }
    }
}
/// Contents of a Properties/Label, shared with the paragraph direction label.
pub(crate) fn label(features: &[(String, bool)]) -> String {
    let extra = extras(features);
    if extra.is_empty() {
        return String::new();
    }
    let json = serde_json::to_string(&extra).expect("feature tuples serialize");
    format!(
        "<KeyValuePair Key=\"{LABEL}\" Value=\"{}\"/>",
        crate::export::escape(&json)
    )
}
pub(crate) fn warn(features: &[(String, bool)], warnings: &mut Vec<String>) {
    let extra = extras(features);
    if !extra.is_empty() {
        let tags = extra
            .iter()
            .map(|(tag, _)| tag.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let message = schist_i18n::tf!("design.idml_features_private", features = tags);
        if !warnings.contains(&message) {
            warnings.push(message);
        }
    }
}
