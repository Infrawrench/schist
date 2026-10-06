//! Public Graphics.xml stripe/dash/dot resources; references are opaque IDs.
use crate::{designmap::DesignPackage, import::Report, xml};
use schist_layout::{decorations::DecorationStroke, StrokeType};
use schist_text_engine::{DecorationCap, DecorationDashes, DecorationFit, TextDecorationPattern};
use std::collections::BTreeMap;

pub(crate) fn read(
    opened: &DesignPackage<'_>,
    report: &mut Report,
) -> BTreeMap<String, DecorationStroke> {
    let mut result = BTreeMap::new();
    for part in opened.listed.iter().filter(|p| p.role == "Graphic") {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            continue;
        };
        for element in root.find_all("StrokeStyle") {
            if element.attr("Name") == Some("$ID/Solid") {
                if let Some(id) = element.attr("Self") {
                    result.insert(id.into(), DecorationStroke::solid());
                }
            }
        }
        for (tag, array) in [
            ("StripedStrokeStyle", "StripeArray"),
            ("DashedStrokeStyle", "DashArray"),
            ("DottedStrokeStyle", "DotArray"),
        ] {
            let dashed = tag == "DashedStrokeStyle";
            let dotted = tag == "DottedStrokeStyle";
            for element in root.find_all(tag) {
                let Some(id) = element.attr("Self") else {
                    continue;
                };
                let mut cap = DecorationCap::Butt;
                if dashed {
                    let mut supported = true;
                    cap = match element.attr("EndCap").unwrap_or("ButtEndCap") {
                        "ButtEndCap" => DecorationCap::Butt,
                        "RoundEndCap" => DecorationCap::Round,
                        "ProjectingEndCap" => DecorationCap::Projecting,
                        _ => {
                            report.skip(schist_i18n::tf!(
                                "design.idml_decoration_unsupported",
                                property = "EndCap"
                            ));
                            supported = false;
                            DecorationCap::Butt
                        }
                    };
                    if !supported {
                        continue;
                    }
                }
                let fitting = match element.attr("StrokeCornerAdjustment").unwrap_or("None") {
                    "None" => DecorationFit::None,
                    "Dashes" if dashed => DecorationFit::Dashes,
                    "Gaps" if dashed || dotted => DecorationFit::Gaps,
                    "DashesAndGaps" if dashed || dotted => DecorationFit::DashesAndGaps,
                    _ => {
                        report.skip(schist_i18n::tf!(
                            "design.idml_decoration_unsupported",
                            property = "StrokeCornerAdjustment"
                        ));
                        continue;
                    }
                };
                let raw = element.attr(array).unwrap_or_default();
                let pattern = raw
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<Vec<f32>, _>>()
                    .ok()
                    .map(|values| {
                        if dashed {
                            TextDecorationPattern::Dashes(DecorationDashes {
                                lengths: values,
                                cap,
                            })
                        } else if dotted {
                            TextDecorationPattern::Dots(values)
                        } else {
                            TextDecorationPattern::Stripes(values)
                        }
                    })
                    .filter(TextDecorationPattern::valid);
                if let Some(pattern) = pattern {
                    result.insert(
                        id.into(),
                        DecorationStroke {
                            fitting,
                            name: element.attr("Name").unwrap_or(id).into(),
                            pattern,
                        },
                    );
                } else {
                    report.skip(schist_i18n::tf!(
                        "design.idml_decoration_invalid",
                        property = array,
                        value = raw
                    ));
                }
            }
        }
    }
    result
}

/// The stroke types an item's StrokeType can name, by Self: every
/// resource the package declares. The built-in StrokeStyle elements name
/// themselves (`$ID/Dashed`); Solid and Dashed are drawn, the others kept by
/// name. A reference the package does not declare is not here: InDesign
/// strokes it solid, as its PDF of the public paged-media `strokes-fills`
/// sample draws `StrokeStyle/$ID/Dashed`, `$ID/Dotted`, `$ID/Canned Dotted`
/// and `$ID/Japanese Dots` exactly as `$ID/Solid` when that package's
/// Graphic.xml declares no stroke style: a 6 pt `re` stroked with no dash
/// array.
pub(crate) fn item_types(
    opened: &DesignPackage<'_>,
    strokes: &BTreeMap<String, DecorationStroke>,
) -> Vec<(String, StrokeType)> {
    let mut out: Vec<(String, StrokeType)> = strokes
        .iter()
        .map(|(id, stroke)| (id.clone(), StrokeType::Style(stroke.clone())))
        .collect();
    for part in opened.listed.iter().filter(|p| p.role == "Graphic") {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            continue;
        };
        for element in root.find_all("StrokeStyle") {
            let (Some(id), Some(name)) = (element.attr("Self"), element.attr("Name")) else {
                continue;
            };
            let stroke = match name {
                "$ID/Solid" => StrokeType::Style(DecorationStroke::solid()),
                "$ID/Dashed" => StrokeType::Dashed,
                _ => StrokeType::Builtin(name.into()),
            };
            if !out.iter().any(|(known, _)| known == id) {
                out.push((id.into(), stroke));
            }
        }
    }
    out
}

/// What an item's StrokeType names: a resource's reference, or a built-in
/// style's, which saving declares in Graphic.xml (see [`builtin_resource`]).
pub(crate) fn type_reference(stroke: &StrokeType) -> String {
    match stroke {
        StrokeType::Style(stroke) => reference(stroke),
        StrokeType::Dashed => "StrokeStyle/$ID/Dashed".into(),
        StrokeType::Builtin(name) => format!("StrokeStyle/{name}"),
    }
}

/// The StrokeStyle element declaring a built-in style an item names. An
/// undeclared reference strokes solid in InDesign.
pub(crate) fn builtin_resource(stroke: &StrokeType) -> Option<String> {
    let name = match stroke {
        StrokeType::Dashed => "$ID/Dashed",
        StrokeType::Builtin(name) => name,
        StrokeType::Style(_) => return None,
    };
    let escape = crate::export::escape;
    Some(format!(
        r#"<StrokeStyle Self="{}" Name="{}"/>"#,
        escape(&type_reference(stroke)),
        escape(name)
    ))
}

pub(crate) fn array_name(pattern: &TextDecorationPattern) -> &'static str {
    match pattern {
        TextDecorationPattern::Solid => "StrokeStyle",
        TextDecorationPattern::Stripes(_) => "StripeArray",
        TextDecorationPattern::Dashes(_) => "DashArray",
        TextDecorationPattern::Dots(_) => "DotArray",
    }
}

fn values(pattern: &TextDecorationPattern) -> &[f32] {
    match pattern {
        TextDecorationPattern::Solid => &[],
        TextDecorationPattern::Stripes(values) | TextDecorationPattern::Dots(values) => values,
        TextDecorationPattern::Dashes(dashes) => &dashes.lengths,
    }
}

fn tag(pattern: &TextDecorationPattern) -> &'static str {
    match pattern {
        TextDecorationPattern::Solid => "StrokeStyle",
        TextDecorationPattern::Stripes(_) => "StripedStrokeStyle",
        TextDecorationPattern::Dashes(_) => "DashedStrokeStyle",
        TextDecorationPattern::Dots(_) => "DottedStrokeStyle",
    }
}

fn cap_name(cap: DecorationCap) -> &'static str {
    match cap {
        DecorationCap::Butt => "ButtEndCap",
        DecorationCap::Round => "RoundEndCap",
        DecorationCap::Projecting => "ProjectingEndCap",
    }
}

fn fit_name(fitting: DecorationFit) -> &'static str {
    match fitting {
        DecorationFit::None => "None",
        DecorationFit::Dashes => "Dashes",
        DecorationFit::Gaps => "Gaps",
        DecorationFit::DashesAndGaps => "DashesAndGaps",
    }
}

pub(crate) fn reference(stroke: &DecorationStroke) -> String {
    if matches!(stroke.pattern, TextDecorationPattern::Solid) {
        return "StrokeStyle/$ID/Solid".into();
    }
    let cap = match &stroke.pattern {
        TextDecorationPattern::Dashes(dashes) => format!("-{}", cap_name(dashes.cap)),
        _ => String::new(),
    };
    format!(
        "{}/Schist-{}-{}{cap}-{}",
        tag(&stroke.pattern),
        stroke.name,
        values(&stroke.pattern)
            .iter()
            .map(|v| format!("{:08x}", v.to_bits()))
            .collect::<Vec<_>>()
            .join("-"),
        fit_name(stroke.fitting)
    )
}

pub(crate) fn resource(stroke: &DecorationStroke) -> String {
    if !stroke.valid() {
        return String::new();
    }
    let escape = crate::export::escape;
    if matches!(stroke.pattern, TextDecorationPattern::Solid) {
        return r#"<StrokeStyle Self="StrokeStyle/$ID/Solid" Name="$ID/Solid"/>"#.into();
    }
    let attributes = match &stroke.pattern {
        TextDecorationPattern::Dashes(dashes) => format!(
            r#" EndCap="{}" StrokeCornerAdjustment="{}""#,
            cap_name(dashes.cap),
            fit_name(stroke.fitting)
        ),
        TextDecorationPattern::Dots(_) => {
            format!(r#" StrokeCornerAdjustment="{}""#, fit_name(stroke.fitting))
        }
        _ => String::new(),
    };
    format!(
        r#"<{} Self="{}" Name="{}" {}="{}"{attributes}/>"#,
        tag(&stroke.pattern),
        escape(&reference(stroke)),
        escape(&stroke.name),
        array_name(&stroke.pattern),
        values(&stroke.pattern)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    )
}
