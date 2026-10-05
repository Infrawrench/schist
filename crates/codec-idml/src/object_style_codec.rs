//! Native object-style paint categories and page-item local overrides.
use crate::{
    color_codec,
    import::Report,
    style_codec::References,
    xml::{self, Element},
};
use schist_layout::{LayoutDocument, LayoutObject, ObjectPaint, ObjectStyle, Paint, PlacedObject};

pub(crate) fn read_paint(
    element: &Element,
    colors: &color_codec::Colors,
    report: &mut Report,
) -> ObjectPaint {
    if element.attr("StrokeWeight").is_some()
        && !element
            .number("StrokeWeight")
            .is_some_and(|v| v.is_finite() && v >= 0.0)
    {
        report.skip(schist_i18n::tf!(
            "design.idml_object_paint_invalid",
            property = "StrokeWeight"
        ));
    }
    let mut paint = |key| match element.attr(key) {
        Some("Swatch/None" | "n") => Some(Paint::None),
        Some(_) => color_codec::resolve(element, key, colors, report).map(Paint::Ink),
        None => None,
    };
    ObjectPaint {
        fill: paint("FillColor"),
        stroke: paint("StrokeColor"),
        stroke_width: element
            .number("StrokeWeight")
            .filter(|v| v.is_finite() && *v >= 0.0),
        fill_tint: color_codec::tint(element, "FillTint", report),
        stroke_tint: color_codec::tint(element, "StrokeTint", report),
        overprint_fill: element.boolean("OverprintFill"),
        overprint_stroke: element.boolean("OverprintStroke"),
    }
}

pub(crate) fn read_styles(
    root: &Element,
    doc: &mut LayoutDocument,
    colors: &color_codec::Colors,
    refs: &References,
    report: &mut Report,
) {
    for element in root.find_all("ObjectStyle") {
        let Some(id) = element.attr("Self").or_else(|| element.attr("Name")) else {
            continue;
        };
        let based_on = element
            .child("Properties")
            .and_then(|p| p.child("BasedOn"))
            .map(Element::trimmed)
            .or_else(|| element.attr("BasedOn"))
            .filter(|v| !v.is_empty() && *v != "n")
            .map(|v| refs.object(v));
        let style = ObjectStyle {
            name: refs.object(id),
            based_on,
            enable_fill: element.boolean("EnableFill"),
            enable_stroke: element.boolean("EnableStroke"),
            enable_stroke_options: element.boolean("EnableStrokeAndCornerOptions"),
            enable_footnotes: element.boolean("EnableTextFrameFootnoteOptions"),
            enable_text_frame_general: element.boolean("EnableTextFrameGeneralOptions"),
            balance_columns: crate::preferences_codec::frame_balance(element, report),
            footnotes: crate::footnote_codec::read_frame(element, report),
            paint: read_paint(element, colors, report),
            enable_text_wrap: element.boolean("EnableTextWrapAndOthers"),
            text_wrap: crate::text_wrap_codec::read(element, &refs.object(id), report),
        };
        if element.attributes.iter().any(|(key, value)| {
            key.starts_with("Enable")
                && !matches!(
                    key.as_str(),
                    "EnableFill"
                        | "EnableStroke"
                        | "EnableStrokeAndCornerOptions"
                        | "EnableTextFrameFootnoteOptions"
                        | "EnableTextWrapAndOthers"
                )
                && xml::parse_boolean(value) == Some(true)
        }) || [
            "ObjectStyleObjectEffectsCategorySettings",
            "ObjectStyleFillEffectsCategorySettings",
            "ObjectStyleStrokeEffectsCategorySettings",
            "ObjectStyleContentEffectsCategorySettings",
        ]
        .iter()
        .flat_map(|name| element.find_all(name))
        .any(|e| {
            e.attributes
                .iter()
                .any(|(k, v)| k.starts_with("Enable") && xml::parse_boolean(v) == Some(true))
        }) || (style.enable_stroke_options != Some(false) && unsupported_outline(element))
            // The category's other member is Nonprinting, which Schist ignores.
            || (style.enable_text_wrap == Some(true) && element.boolean("Nonprinting") == Some(true))
        {
            report.skip(schist_i18n::tf!(
                "design.idml_object_style_limits",
                name = style.name
            ));
        }
        if let Some(existing) = doc.styles.objects.iter_mut().find(|s| s.name == style.name) {
            *existing = style;
        } else {
            doc.styles.objects.push(style);
        }
    }
}

pub(crate) fn resolve_references(doc: &mut LayoutDocument, refs: &References, report: &mut Report) {
    for object in doc.objects.iter_mut().chain(
        doc.parents
            .iter_mut()
            .flat_map(|p| p.objects.iter_mut().map(|o| &mut o.object)),
    ) {
        if let Some(style) = &mut object.appearance.style {
            *style = refs.object(style);
            if doc.styles.object_style(style).is_none() {
                report.skip(schist_i18n::tf!(
                    "design.idml_object_style_missing",
                    name = style
                ));
            }
        }
    }
    for style in &doc.styles.objects {
        let mut seen = std::collections::HashSet::new();
        let mut next = Some(style.name.as_str());
        while let Some(name) = next {
            if !seen.insert(name) {
                report.skip(schist_i18n::tf!(
                    "design.idml_object_style_cycle",
                    name = style.name
                ));
                break;
            }
            let Some(base) = doc.styles.object_style(name) else {
                report.skip(schist_i18n::tf!(
                    "design.idml_object_style_missing",
                    name = name
                ));
                break;
            };
            next = base.based_on.as_deref();
        }
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn attr(out: &mut String, key: &str, value: impl ToString) {
    out.push_str(&format!(" {key}=\"{}\"", escape(&value.to_string())));
}

pub(crate) fn paint_attributes(paint: &ObjectPaint) -> String {
    let mut out = String::new();
    for (key, value) in [("FillColor", &paint.fill), ("StrokeColor", &paint.stroke)] {
        if let Some(value) = value {
            attr(
                &mut out,
                key,
                value
                    .ink()
                    .map(color_codec::reference)
                    .unwrap_or_else(|| "Swatch/None".into()),
            );
        }
    }
    for (key, value) in [
        ("StrokeWeight", paint.stroke_width),
        (
            "FillTint",
            color_codec::paint_tint(paint.fill_ink(), paint.fill_tint),
        ),
        (
            "StrokeTint",
            color_codec::paint_tint(paint.stroke_ink(), paint.stroke_tint),
        ),
    ] {
        if let Some(value) = value {
            attr(&mut out, key, value);
        }
    }
    for (key, value) in [
        ("OverprintFill", paint.overprint_fill),
        ("OverprintStroke", paint.overprint_stroke),
    ] {
        if let Some(value) = value {
            attr(&mut out, key, value);
        }
    }
    out
}

pub(crate) fn object_attributes(object: &PlacedObject) -> String {
    let mut paint = if object.appearance.style.is_some() {
        object.appearance.paint.clone()
    } else {
        object.appearance.paint.over(&object.legacy_paint())
    };
    if object.overprint {
        paint.overprint_fill = Some(true);
        paint.overprint_stroke = Some(true);
    }
    let mut out = paint_attributes(&paint);
    if let Some(style) = &object.appearance.style {
        attr(
            &mut out,
            "AppliedObjectStyle",
            format!("ObjectStyle/$ID/{style}"),
        );
    }
    out
}

pub(crate) fn styles_xml(doc: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let mut out = String::from("<RootObjectStyleGroup Self=\"SchistObjectStyles\">");
    for style in &doc.styles.objects {
        out.push_str("<ObjectStyle");
        attr(&mut out, "Self", format!("ObjectStyle/$ID/{}", style.name));
        attr(&mut out, "Name", &style.name);
        for (key, value) in [
            ("EnableFill", style.enable_fill),
            ("EnableStroke", style.enable_stroke),
            ("EnableStrokeAndCornerOptions", style.enable_stroke_options),
            ("EnableTextFrameFootnoteOptions", style.enable_footnotes),
            (
                "EnableTextFrameGeneralOptions",
                style.enable_text_frame_general,
            ),
            ("EnableTextWrapAndOthers", style.enable_text_wrap),
        ] {
            if let Some(value) = value {
                attr(&mut out, key, value);
            }
        }
        out.push_str(&paint_attributes(&style.paint));
        out.push_str("><Properties>");
        if let Some(base) = &style.based_on {
            out.push_str(&format!(
                "<BasedOn type=\"object\">ObjectStyle/$ID/{}</BasedOn>",
                escape(base)
            ));
        }
        out.push_str("</Properties>");
        out.push_str(&crate::footnote_codec::write_frame(
            &style.footnotes,
            warnings,
        ));
        if style.balance_columns.is_some() {
            out.push_str(&format!(
                "<TextFramePreference{} />",
                crate::preferences_codec::balance_attribute(style.balance_columns)
            ));
        }
        if let Some(wrap) = &style.text_wrap {
            out.push_str(&crate::text_wrap_codec::write(wrap));
        }
        out.push_str("</ObjectStyle>");
    }
    out.push_str("</RootObjectStyleGroup>");
    out
}

pub(crate) fn read_appearance(
    element: &Element,
    object: &mut PlacedObject,
    local: schist_layout::Rect,
    colors: &color_codec::Colors,
    report: &mut Report,
) {
    if unsupported_outline(element) {
        report.skip(schist_i18n::tf!(
            "design.idml_object_style_limits",
            name = object.name
        ));
    }
    object.appearance.style = element
        .attr("AppliedObjectStyle")
        .filter(|s| !s.is_empty() && *s != "n")
        .map(str::to_owned);
    let paint = read_paint(element, colors, report);
    if object.appearance.style.is_some() || !matches!(object.object, LayoutObject::Shape { .. }) {
        object.appearance.paint = paint;
        if let LayoutObject::Shape { path, .. } = &object.object {
            object.object = ObjectPaint::default().shape(path.clone());
        }
    } else if let LayoutObject::Shape { path, .. } = &object.object {
        object.object = paint.shape(path.clone());
    }
    if matches!(
        object.object,
        LayoutObject::TextFrame {
            text_path: None,
            ..
        }
    ) && local.width > 0.0
        && local.height > 0.0
    {
        object.appearance.outline = crate::import::path_of(element).and_then(|mut path| {
            path.map_points(|p| {
                schist_layout::Point::new(
                    (p.x - local.x) / local.width,
                    (p.y - local.y) / local.height,
                )
            });
            (!crate::graphic_codec::is_rectangle(&path)).then_some(path)
        });
    }
}

fn unsupported_outline(element: &Element) -> bool {
    [
        ("EndCap", "ButtEndCap"),
        ("EndJoin", "MiterEndJoin"),
        ("StrokeAlignment", "CenterAlignment"),
        ("LeftLineEnd", "None"),
        ("RightLineEnd", "None"),
    ]
    .iter()
    .any(|(key, default)| element.attr(key).is_some_and(|v| v != *default))
        || element
            .attr("StrokeType")
            .is_some_and(|v| !matches!(v, "Solid" | "$ID/Solid" | "StrokeStyle/$ID/Solid"))
        || [
            "TopLeftCornerOption",
            "TopRightCornerOption",
            "BottomLeftCornerOption",
            "BottomRightCornerOption",
            "CornerOption",
        ]
        .iter()
        .any(|key| element.attr(key).is_some_and(|v| v != "None"))
        || element.number("MiterLimit").is_some_and(|v| v != 4.0)
}
