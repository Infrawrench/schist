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
    // Stroke and corner options; a value the specification does not name is
    // reported and read as InDesign's default.
    let mut option = |key: &str| {
        let value = element.attr(key)?;
        let known = match key {
            "EndCap" => matches!(value, "ButtEndCap" | "RoundEndCap" | "ProjectingEndCap"),
            "EndJoin" => matches!(value, "MiterEndJoin" | "RoundEndJoin" | "BevelEndJoin"),
            _ => matches!(
                value,
                "CenterAlignment" | "InsideAlignment" | "OutsideAlignment"
            ),
        };
        if !known {
            report.skip(schist_i18n::tf!(
                "design.idml_object_paint_invalid",
                property = key
            ));
        }
        known.then_some(value)
    };
    let stroke_cap = option("EndCap").map(|v| match v {
        "RoundEndCap" => schist_layout::StrokeCap::Round,
        "ProjectingEndCap" => schist_layout::StrokeCap::Projecting,
        _ => schist_layout::StrokeCap::Butt,
    });
    let stroke_join = option("EndJoin").map(|v| match v {
        "RoundEndJoin" => schist_layout::StrokeJoin::Round,
        "BevelEndJoin" => schist_layout::StrokeJoin::Bevel,
        _ => schist_layout::StrokeJoin::Miter,
    });
    let stroke_alignment = option("StrokeAlignment").map(|v| match v {
        "InsideAlignment" => schist_layout::StrokeAlignment::Inside,
        "OutsideAlignment" => schist_layout::StrokeAlignment::Outside,
        _ => schist_layout::StrokeAlignment::Center,
    });
    let miter_limit = element.attr("MiterLimit").and_then(|raw| {
        let value = xml::parse_number(raw).filter(|v| v.is_finite() && (1.0..=500.0).contains(v));
        if value.is_none() {
            report.skip(schist_i18n::tf!(
                "design.idml_object_paint_invalid",
                property = "MiterLimit"
            ));
        }
        value
    });
    // A gradient fill's offset highlight is not drawn.
    if element
        .attr("FillColor")
        .is_some_and(|r| colors.gradient(r).is_some())
        && element
            .number("GradientFillHiliteLength")
            .is_some_and(|v| v.is_finite() && v != 0.0)
    {
        report.skip(schist_i18n::tf!(
            "design.idml_gradient_highlight",
            name = element
                .attr("Name")
                .filter(|n| !n.is_empty() && *n != "$ID/")
                .or_else(|| element.attr("Self"))
                .unwrap_or_default()
        ));
    }
    let mut paint = |key| match element.attr(key) {
        Some("Swatch/None" | "n") => Some(Paint::None),
        Some(_) => color_codec::resolve(element, key, colors, report).map(Paint::Ink),
        None => None,
    };
    // A gradient fill runs where the item says, in its own coordinates.
    let fill = match element.attr("FillColor") {
        Some(reference) if colors.gradient(reference).is_some() => {
            let gradient = colors.gradient(reference).cloned().unwrap();
            let start = element
                .attr("GradientFillStart")
                .map(xml::numbers)
                .and_then(|v| match v.as_slice() {
                    [x, y] if x.is_finite() && y.is_finite() => {
                        Some(schist_layout::Point::new(*x, *y))
                    }
                    _ => None,
                });
            Some(Paint::Gradient(Box::new(
                schist_layout::gradients::GradientFill {
                    gradient,
                    start,
                    length: element
                        .number("GradientFillLength")
                        .filter(|v| v.is_finite() && *v > 0.0),
                    angle: element
                        .number("GradientFillAngle")
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0),
                },
            )))
        }
        _ => paint("FillColor"),
    };
    ObjectPaint {
        stroke_cap,
        stroke_join,
        miter_limit,
        stroke_alignment,
        fill,
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
                && !at_defaults(element, key)
        }) || ([
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
        }) && !effects_at_defaults(element))
            || (style.enable_stroke_options != Some(false) && unsupported_outline(element))
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
            let reference = match value {
                Paint::Ink(ink) => color_codec::reference(ink),
                Paint::Gradient(fill) => color_codec::gradient_reference(&fill.gradient),
                Paint::None => "Swatch/None".into(),
            };
            attr(&mut out, key, reference);
        }
    }
    for (key, value) in [
        (
            "EndCap",
            paint.stroke_cap.map(|v| match v {
                schist_layout::StrokeCap::Butt => "ButtEndCap",
                schist_layout::StrokeCap::Round => "RoundEndCap",
                schist_layout::StrokeCap::Projecting => "ProjectingEndCap",
            }),
        ),
        (
            "EndJoin",
            paint.stroke_join.map(|v| match v {
                schist_layout::StrokeJoin::Miter => "MiterEndJoin",
                schist_layout::StrokeJoin::Round => "RoundEndJoin",
                schist_layout::StrokeJoin::Bevel => "BevelEndJoin",
            }),
        ),
        (
            "StrokeAlignment",
            paint.stroke_alignment.map(|v| match v {
                schist_layout::StrokeAlignment::Center => "CenterAlignment",
                schist_layout::StrokeAlignment::Inside => "InsideAlignment",
                schist_layout::StrokeAlignment::Outside => "OutsideAlignment",
            }),
        ),
    ] {
        if let Some(value) = value {
            attr(&mut out, key, value);
        }
    }
    if let Some(limit) = paint.miter_limit {
        attr(&mut out, "MiterLimit", limit);
    }
    if let Some(fill) = paint.fill_gradient() {
        if let Some(start) = fill.start {
            attr(
                &mut out,
                "GradientFillStart",
                format!("{} {}", start.x, start.y),
            );
        }
        if let Some(length) = fill.length {
            attr(&mut out, "GradientFillLength", length);
        }
        attr(&mut out, "GradientFillAngle", fill.angle);
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
    let mut paint = read_paint(element, colors, report);
    // A gradient's start is in the item's own coordinates; paths here are
    // relative to their bounds' corner.
    if let Some(Paint::Gradient(fill)) = &mut paint.fill {
        if let Some(start) = &mut fill.start {
            start.x -= local.x;
            start.y -= local.y;
        }
    }
    if object.appearance.style.is_some()
        || !matches!(object.object, LayoutObject::Shape { .. })
        || paint.fill_gradient().is_some()
    {
        object.appearance.paint = paint;
        if let LayoutObject::Shape { path, .. } = &object.object {
            object.object = ObjectPaint::default().shape(path.clone());
        }
    } else if let LayoutObject::Shape { path, .. } = &object.object {
        object.object = paint.shape(path.clone());
        // Stroke options have no place on the shape itself.
        object.appearance.paint = paint.stroke_options();
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

/// Whether an enabled category's settings are InDesign's defaults, as its
/// own exports write them for the default object styles: one top-justified
/// column without insets, the first baseline at the ascent, no auto-sizing,
/// no column rules, horizontal stories without optical margins. Frames
/// already have those, so the category changes nothing.
fn at_defaults(element: &Element, category: &str) -> bool {
    let preference = element.child("TextFramePreference");
    let is = |name: &str, defaults: &[&str]| {
        preference
            .and_then(|p| p.attr(name))
            .is_none_or(|v| defaults.contains(&v))
    };
    match category {
        "EnableTextFrameGeneralOptions" => {
            is("TextColumnCount", &["1"])
                && is("UseFixedColumnWidth", &["false"])
                && is("UseFlexibleColumnWidth", &["false"])
                && is("VerticalJustification", &["TopAlign"])
                && is("IgnoreWrap", &["false"])
                && is("VerticalBalanceColumns", &["false"])
                && preference
                    .and_then(|p| p.child("Properties"))
                    .and_then(|p| p.child("InsetSpacing"))
                    .is_none_or(|insets| {
                        insets
                            .children
                            .iter()
                            .all(|item| xml::parse_number(item.trimmed()) == Some(0.0))
                    })
        }
        "EnableTextFrameBaselineOptions" => {
            is("FirstBaselineOffset", &["AscentOffset"])
                && is("MinimumFirstBaselineOffset", &["0"])
                && element
                    .child("BaselineFrameGridOption")
                    .and_then(|g| g.attr("UseCustomBaselineFrameGrid"))
                    .is_none_or(|v| v == "false")
        }
        "EnableTextFrameAutoSizingOptions" => is("AutoSizingType", &["Off"]),
        "EnableTextFrameColumnRuleOptions" => is("ColumnRuleOverride", &["false"]),
        "EnableStoryOptions" => element.child("StoryPreference").is_none_or(|story| {
            story
                .attr("OpticalMarginAlignment")
                .is_none_or(|v| v == "false")
                && story
                    .attr("StoryOrientation")
                    .is_none_or(|v| matches!(v, "Horizontal" | "Unknown"))
                && story
                    .attr("StoryDirection")
                    .is_none_or(|v| matches!(v, "LeftToRightDirection" | "UnknownDirection"))
        }),
        _ => false,
    }
}

/// Whether every effect an object style's transparency settings give is
/// off: normal blending at full opacity, no shadows, feathers, glows,
/// bevels or satin. InDesign's default object styles enable the effects
/// categories with these settings, which change nothing.
fn effects_at_defaults(element: &Element) -> bool {
    [
        "TransparencySetting",
        "StrokeTransparencySetting",
        "FillTransparencySetting",
        "ContentTransparencySetting",
    ]
    .iter()
    .filter_map(|name| element.child(name))
    .flat_map(|setting| &setting.children)
    .all(|effect| match effect.name.as_str() {
        "BlendingSetting" => {
            effect.attr("BlendMode").is_none_or(|v| v == "Normal")
                && effect.number("Opacity").is_none_or(|v| v == 100.0)
                && effect.boolean("KnockoutGroup") != Some(true)
                && effect.boolean("IsolateBlending") != Some(true)
        }
        "DropShadowSetting" | "FeatherSetting" => effect.attr("Mode").is_none_or(|v| v == "None"),
        _ => effect.boolean("Applied") != Some(true),
    })
}

/// Stroke and corner options not drawn: arrowheads, non-solid stroke types,
/// and corner effects. Caps, joins, mitre limits and alignment are.
fn unsupported_outline(element: &Element) -> bool {
    [("LeftLineEnd", "None"), ("RightLineEnd", "None")]
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
}
