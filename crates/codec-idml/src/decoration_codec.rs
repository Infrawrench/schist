//! Native text-decoration paints and supported stroke resources from public XML.
use crate::{
    color_codec,
    import::Report,
    style_codec::{escape, property},
    xml::Element,
};
use schist_layout::decorations::{DecorationMeasure, DecorationPaint, DecorationStyle};

fn invalid(report: &mut Report, key: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_decoration_invalid",
        property = key,
        value = value
    ));
}
fn measure(
    element: &Element,
    key: &str,
    weight: bool,
    report: &mut Report,
) -> Option<DecorationMeasure> {
    let raw = element.attr(key)?;
    match raw.parse::<f32>() {
        Ok(-9999.0) => Some(DecorationMeasure::Auto),
        Ok(value) if value.is_finite() && (!weight || value >= 0.0) => {
            Some(DecorationMeasure::Points(value))
        }
        _ => {
            invalid(report, key, raw);
            None
        }
    }
}
fn boolean(element: &Element, key: &str, report: &mut Report) -> Option<bool> {
    let raw = element.attr(key)?;
    match crate::xml::parse_boolean(raw) {
        Some(value) => Some(value),
        None => {
            invalid(report, key, raw);
            None
        }
    }
}

fn paint(
    element: &Element,
    key: &str,
    colors: &color_codec::Colors,
    report: &mut Report,
) -> Option<DecorationPaint> {
    match property(element, key) {
        Some("Text Color") => Some(DecorationPaint::Text),
        Some("Swatch/None" | "n") => Some(DecorationPaint::None),
        Some(reference) => {
            if let Some(ink) = colors.get(reference) {
                Some(DecorationPaint::Ink(ink.clone()))
            } else {
                report.skip(schist_i18n::tf!(
                    "design.idml_color_unread",
                    name = reference
                ));
                None
            }
        }
        None => None,
    }
}

pub(crate) fn read(
    element: &Element,
    prefix: &str,
    colors: &color_codec::Colors,
    strokes: &std::collections::BTreeMap<String, schist_layout::decorations::DecorationStroke>,
    report: &mut Report,
) -> DecorationStyle {
    let key = format!("{prefix}Type");
    let stroke = property(element, &key).and_then(|reference| {
        if reference == "StrokeStyle/$ID/Solid" {
            Some(schist_layout::decorations::DecorationStroke::solid())
        } else if let Some(stroke) = strokes.get(reference) {
            Some(stroke.clone())
        } else {
            report.skip(schist_i18n::tf!(
                "design.idml_decoration_unsupported",
                property = key
            ));
            None
        }
    });
    DecorationStyle {
        stroke,
        paint: paint(element, &format!("{prefix}Color"), colors, report),
        gap_paint: paint(element, &format!("{prefix}GapColor"), colors, report),
        gap_tint: color_codec::tint(element, &format!("{prefix}GapTint"), report),
        gap_overprint: boolean(element, &format!("{prefix}GapOverprint"), report),
        weight: measure(element, &format!("{prefix}Weight"), true, report),
        offset: measure(element, &format!("{prefix}Offset"), false, report),
        tint: color_codec::tint(element, &format!("{prefix}Tint"), report),
        overprint: boolean(element, &format!("{prefix}Overprint"), report),
    }
}

pub(crate) fn attributes(out: &mut String, styles: [&DecorationStyle; 2]) {
    for (prefix, style) in ["Underline", "StrikeThrough"].into_iter().zip(styles) {
        for (suffix, value, weight) in [
            ("Weight", style.weight, true),
            ("Offset", style.offset, false),
        ] {
            let value = value.and_then(|v| match v {
                DecorationMeasure::Auto => Some(-9999.0),
                DecorationMeasure::Points(v)
                    if v.is_finite() && v != -9999.0 && (!weight || v >= 0.0) =>
                {
                    Some(v)
                }
                _ => None,
            });
            if let Some(value) = value {
                out.push_str(&format!(r#" {prefix}{suffix}="{value}""#));
            }
        }
        if let Some(value) = color_codec::paint_tint(
            style.paint.as_ref().and_then(DecorationPaint::ink),
            style.tint,
        ) {
            out.push_str(&format!(r#" {prefix}Tint="{value}""#));
        }
        if let Some(value) = style.overprint {
            out.push_str(&format!(r#" {prefix}Overprint="{value}""#));
        }
        if let Some(value) = color_codec::paint_tint(
            style.gap_paint.as_ref().and_then(DecorationPaint::ink),
            style.gap_tint,
        ) {
            out.push_str(&format!(r#" {prefix}GapTint="{value}""#));
        }
        if let Some(value) = style.gap_overprint {
            out.push_str(&format!(r#" {prefix}GapOverprint="{value}""#));
        }
    }
}

pub(crate) fn properties(out: &mut String, styles: [&DecorationStyle; 2]) {
    let Some(at) = out.find("</Properties>") else {
        return;
    };
    let mut properties = String::new();
    for (prefix, style) in ["Underline", "StrikeThrough"].into_iter().zip(styles) {
        for (suffix, paint) in [("Color", &style.paint), ("GapColor", &style.gap_paint)] {
            if let Some(paint) = paint {
                let (kind, reference) = match paint {
                    DecorationPaint::Text => ("string", "Text Color".into()),
                    DecorationPaint::None => ("object", "Swatch/None".into()),
                    DecorationPaint::Ink(ink) => ("object", color_codec::reference(ink)),
                };
                properties.push_str(&format!(
                    r#"<{prefix}{suffix} type="{kind}">{}</{prefix}{suffix}>"#,
                    escape(&reference)
                ));
            }
        }
        if let Some(stroke) = &style.stroke {
            if stroke.valid() {
                properties.push_str(&format!(
                    r#"<{prefix}Type type="object">{}</{prefix}Type>"#,
                    escape(&crate::stroke_style_codec::reference(stroke))
                ));
            }
        }
    }
    out.insert_str(at, &properties);
}

pub(crate) fn warn(styles: [&DecorationStyle; 2], warnings: &mut Vec<String>) {
    for (prefix, style) in ["Underline", "StrikeThrough"].into_iter().zip(styles) {
        if let Some(stroke) = &style.stroke {
            if !stroke.valid() {
                warnings.push(schist_i18n::tf!(
                    "design.idml_decoration_invalid",
                    property = format!("{prefix}Type"),
                    value = &stroke.name
                ));
            }
        }
        for (suffix, value, weight) in [
            ("Weight", style.weight, true),
            ("Offset", style.offset, false),
        ] {
            if let Some(DecorationMeasure::Points(value)) = value {
                if !value.is_finite() || (weight && value < 0.0) || value == -9999.0 {
                    warnings.push(schist_i18n::tf!(
                        "design.idml_decoration_invalid",
                        property = format!("{prefix}{suffix}"),
                        value = value
                    ));
                }
            }
        }
    }
}
