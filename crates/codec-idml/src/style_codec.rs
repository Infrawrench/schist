//! Named style properties from the public IDML XML structure. Missing
//! attributes stay absent so inheritance is not flattened on save.
use crate::xml::{self, Element};
use schist_layout::styles::Align;
use schist_layout::{CharacterStyle, ParagraphStyle, StyleSet};

pub(crate) fn name(value: &str) -> String {
    value
        .strip_prefix("ParagraphStyle/")
        .or_else(|| value.strip_prefix("CharacterStyle/"))
        .or_else(|| value.strip_prefix("ObjectStyle/"))
        .unwrap_or(value)
        .trim_start_matches("$ID/")
        .to_owned()
}
pub(crate) fn property<'a>(element: &'a Element, key: &str) -> Option<&'a str> {
    element
        .child("Properties")
        .and_then(|p| p.child(key))
        .map(Element::trimmed)
        .or_else(|| element.attr(key))
}
fn boolean(element: &Element, key: &str) -> Option<bool> {
    match element.attr(key)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}
fn base(element: &Element) -> Option<String> {
    property(element, "BasedOn")
        .filter(|v| !v.is_empty() && *v != "n")
        .map(name)
}
fn align(value: &str) -> Option<Align> {
    match value {
        "LeftAlign" => Some(Align::Left),
        "CenterAlign" => Some(Align::Center),
        "RightAlign" => Some(Align::Right),
        "LeftJustified" | "JustifyAlign" => Some(Align::Justify),
        "FullyJustified" | "JustifyAllAlign" => Some(Align::JustifyAll),
        _ => None,
    }
}

/// IDs are opaque; display names need not match Self or be unique across groups.
#[derive(Default)]
pub(crate) struct References {
    pub(crate) languages: Vec<schist_layout::language::LanguageResource>,
    pub(crate) strokes:
        std::collections::BTreeMap<String, schist_layout::decorations::DecorationStroke>,
    paragraphs: std::collections::BTreeMap<String, String>,
    characters: std::collections::BTreeMap<String, String>,
    objects: std::collections::BTreeMap<String, String>,
}
impl References {
    pub(crate) fn new(roots: &[Element]) -> Self {
        let mut out = Self::default();
        for (kind, names) in [
            ("ParagraphStyle", &mut out.paragraphs),
            ("CharacterStyle", &mut out.characters),
            ("ObjectStyle", &mut out.objects),
        ] {
            for root in roots {
                for element in root.find_all(kind) {
                    let Some(id) = element.attr("Self") else {
                        continue;
                    };
                    let mut label = element
                        .attr("Name")
                        .map(|n| n.trim_start_matches("$ID/").to_owned())
                        .unwrap_or_else(|| name(id));
                    if names.values().any(|n| *n == label) {
                        label = format!("{label} [{id}]");
                    }
                    names.insert(id.to_owned(), label);
                }
            }
        }
        out
    }
    pub(crate) fn paragraph(&self, reference: &str) -> String {
        self.paragraphs
            .get(reference)
            .cloned()
            .unwrap_or_else(|| name(reference))
    }
    pub(crate) fn character(&self, reference: &str) -> String {
        self.characters
            .get(reference)
            .cloned()
            .unwrap_or_else(|| name(reference))
    }
    pub(crate) fn object(&self, reference: &str) -> String {
        self.objects
            .get(reference)
            .cloned()
            .unwrap_or_else(|| name(reference))
    }
}

pub(crate) fn paragraph_properties(
    element: &Element,
    colors: &crate::color_codec::Colors,
    refs: &References,
    report: &mut crate::import::Report,
) -> ParagraphStyle {
    let character = character_properties(element, colors, refs, report);
    let (list, bullet) =
        crate::list_codec::restore(element, crate::list_codec::read(element, refs, report));
    ParagraphStyle {
        family: character.family,
        font_style: character.font_style,
        bold: character.bold,
        italic: character.italic,
        underline: character.underline,
        strikethrough: character.strikethrough,
        underline_style: character.underline_style,
        strike_style: character.strike_style,
        baseline_shift: character.baseline_shift,
        position: character.position,
        all_caps: character.all_caps,
        small_caps: character.small_caps,
        fill_tint: character.fill_tint,
        stroke_tint: character.stroke_tint,
        fill: character.fill,
        stroke: character.stroke,
        fill_disabled: character.fill_disabled,
        stroke_disabled: character.stroke_disabled,
        stroke_weight: character.stroke_weight,
        stroke_outside: character.stroke_outside,
        stroke_join: character.stroke_join,
        stroke_miter_limit: character.stroke_miter_limit,
        overprint_fill: character.overprint_fill,
        overprint_stroke: character.overprint_stroke,
        point_size: character.point_size,
        leading: character.leading,
        auto_leading: auto_leading(element, report),
        tracking: character.tracking,
        kerning: character.kerning,
        align: element.attr("Justification").and_then(align),
        left_indent: element.number("LeftIndent"),
        right_indent: element.number("RightIndent"),
        first_line_indent: element.number("FirstLineIndent"),
        space_before: element.number("SpaceBefore"),
        space_after: element.number("SpaceAfter"),
        keep_with_next: element.number("KeepWithNext").map(|v| v > 0.0),
        keep_lines: element
            .number("KeepFirstLines")
            .map(|v| v.max(0.0) as usize),
        drop_caps_lines: element.number("DropCapLines").map(|v| v.max(0.0) as usize),
        drop_caps_characters: element
            .number("DropCapCharacters")
            .map(|v| v.max(0.0) as usize),
        direction: crate::auto_direction::style_direction(element),
        list,
        bullet,
        hyphenate: boolean(element, "Hyphenation"),
        language: character.language,
        features: character.features,
        directional_features: character.directional_features,
        ..ParagraphStyle::default()
    }
}

pub(crate) fn character_properties(
    element: &Element,
    colors: &crate::color_codec::Colors,
    refs: &References,
    report: &mut crate::import::Report,
) -> CharacterStyle {
    let (font_style, bold, italic) = read_font_choice(element);
    let (all_caps, small_caps) = crate::capitalization_codec::read(element, report);
    let stroke_weight = element
        .attr("StrokeWeight")
        .and_then(|raw| match raw.parse::<f32>() {
            Ok(value) if value.is_finite() && value >= 0.0 => Some(value),
            _ => {
                report.skip(schist_i18n::tf!(
                    "design.idml_text_stroke_invalid",
                    value = raw
                ));
                None
            }
        });
    use schist_text_engine::TextStrokeJoin;
    let stroke_join = element.attr("EndJoin").and_then(|value| match value {
        "MiterEndJoin" => Some(TextStrokeJoin::Miter),
        "RoundEndJoin" => Some(TextStrokeJoin::Round),
        "BevelEndJoin" => Some(TextStrokeJoin::Bevel),
        _ => {
            report.skip(schist_i18n::tf!(
                "design.idml_text_stroke_option",
                property = "EndJoin"
            ));
            None
        }
    });
    let stroke_miter_limit = element
        .attr("MiterLimit")
        .and_then(|raw| match raw.parse::<f32>() {
            Ok(value) if value.is_finite() && value >= 0.0 => Some(value),
            _ => {
                report.skip(schist_i18n::tf!(
                    "design.idml_text_stroke_option",
                    property = "MiterLimit"
                ));
                None
            }
        });
    use schist_layout::styles::TextPosition;
    let position = element.attr("Position").and_then(|value| match value {
        "Normal" => Some(TextPosition::Normal),
        "Superscript" => Some(TextPosition::Superscript),
        "Subscript" => Some(TextPosition::Subscript),
        _ => {
            report.skip(schist_i18n::tf!(
                "design.idml_position_unsupported",
                value = value
            ));
            None
        }
    });
    let baseline_shift = element
        .attr("BaselineShift")
        .and_then(|raw| match raw.parse::<f32>() {
            Ok(value) if value.is_finite() => {
                Some(schist_layout::styles::BaselineShift::Offset(value))
            }
            _ => {
                report.skip(schist_i18n::tf!(
                    "design.idml_baseline_invalid",
                    value = raw
                ));
                None
            }
        });
    CharacterStyle {
        all_caps,
        small_caps,
        family: property(element, "AppliedFont").map(str::to_owned),
        fill_tint: crate::color_codec::tint(element, "FillTint", report),
        stroke_tint: crate::color_codec::tint(element, "StrokeTint", report),
        fill: crate::color_codec::resolve(element, "FillColor", colors, report),
        stroke: crate::color_codec::resolve(element, "StrokeColor", colors, report),
        fill_disabled: matches!(element.attr("FillColor"), Some("Swatch/None" | "n")),
        stroke_disabled: matches!(element.attr("StrokeColor"), Some("Swatch/None" | "n")),
        stroke_weight,
        stroke_join,
        stroke_miter_limit,
        stroke_outside: element.attr("StrokeAlignment").and_then(|v| match v {
            "CenterAlignment" => Some(false),
            "OutsideAlignment" => Some(true),
            _ => {
                report.skip(schist_i18n::tf!(
                    "design.idml_text_stroke_option",
                    property = "StrokeAlignment"
                ));
                None
            }
        }),
        overprint_fill: boolean(element, "OverprintFill"),
        overprint_stroke: boolean(element, "OverprintStroke"),
        point_size: element.number("PointSize"),
        leading: leading(element, report),
        tracking: element.number("Tracking"),
        kerning: element.attr("KerningMethod").map(|v| v != "$ID/None"),
        font_style,
        bold,
        italic,
        underline: boolean(element, "Underline"),
        strikethrough: boolean(element, "StrikeThru"),
        underline_style: crate::decoration_codec::read(
            element,
            "Underline",
            colors,
            &refs.strokes,
            report,
        ),
        strike_style: crate::decoration_codec::read(
            element,
            "StrikeThrough",
            colors,
            &refs.strokes,
            report,
        ),
        baseline_shift,
        position,
        features: crate::opentype_codec::read(element, report),
        directional_features: crate::opentype_codec::directional(element, report),
        language: crate::language_codec::applied(element, &refs.languages, report),
        ..CharacterStyle::default()
    }
}

fn leading(
    element: &Element,
    report: &mut crate::import::Report,
) -> Option<schist_layout::styles::Leading> {
    let raw = property(element, "Leading")?;
    if raw == "Auto" {
        return Some(schist_layout::styles::Leading::Auto);
    }
    if let Some(value) = xml::parse_number(raw).filter(|v| v.is_finite() && *v >= 0.0) {
        return Some(schist_layout::styles::Leading::Points(value));
    }
    report.skip(schist_i18n::tf!(
        "design.idml_leading_invalid",
        property = "Leading",
        value = raw
    ));
    None
}
fn auto_leading(element: &Element, report: &mut crate::import::Report) -> Option<f32> {
    let raw = element.attr("AutoLeading")?;
    if let Some(value) =
        xml::parse_number(raw).filter(|v| v.is_finite() && (0.0..=500.0).contains(v))
    {
        return Some(value);
    }
    report.skip(schist_i18n::tf!(
        "design.idml_leading_invalid",
        property = "AutoLeading",
        value = raw
    ));
    None
}

pub(crate) fn warn_leading(
    leading: Option<schist_layout::styles::Leading>,
    automatic: Option<f32>,
    warnings: &mut Vec<String>,
) {
    let points = match leading {
        Some(schist_layout::styles::Leading::Points(v)) => Some(v),
        _ => None,
    };
    for (property, value, valid) in [
        (
            "Leading",
            points,
            points.is_none_or(|v| v.is_finite() && v >= 0.0),
        ),
        (
            "AutoLeading",
            automatic,
            automatic.is_none_or(|v| v.is_finite() && (0.0..=500.0).contains(&v)),
        ),
    ] {
        if !valid {
            warnings.push(schist_i18n::tf!(
                "design.idml_leading_invalid",
                property = property,
                value = value.unwrap().to_string()
            ));
        }
    }
}

pub fn read(
    root: &Element,
    styles: &mut StyleSet,
    colors: &crate::color_codec::Colors,
    report: &mut crate::import::Report,
    refs: &References,
) {
    for element in root.find_all("ParagraphStyle") {
        let Some(raw) = element.attr("Self").or_else(|| element.attr("Name")) else {
            continue;
        };
        let mut style = paragraph_properties(element, colors, refs, report);
        style.name = refs.paragraph(raw);
        style.based_on =
            base(element).map(|base| refs.paragraph(property(element, "BasedOn").unwrap_or(&base)));
        style.next = element.attr("NextStyle").map(|v| refs.paragraph(v));
        styles.add_paragraph(style);
    }
    for element in root.find_all("CharacterStyle") {
        let Some(raw) = element.attr("Self").or_else(|| element.attr("Name")) else {
            continue;
        };
        let mut style = character_properties(element, colors, refs, report);
        style.name = refs.character(raw);
        style.based_on =
            base(element).map(|base| refs.character(property(element, "BasedOn").unwrap_or(&base)));
        styles.add_character(style);
    }
}

pub(crate) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn attr(out: &mut String, key: &str, value: impl ToString) {
    out.push_str(&format!(" {key}=\"{}\"", escape(&value.to_string())));
}
fn optional(out: &mut String, key: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        attr(out, key, value);
    }
}
fn props(
    out: &mut String,
    kind: &str,
    based_on: &Option<String>,
    leading: Option<schist_layout::styles::Leading>,
    family: Option<&str>,
    automatic_direction: Option<&str>,
    features: &[(String, bool)],
) {
    out.push_str("><Properties>");
    if let Some(base) = based_on {
        out.push_str(&format!(
            "<BasedOn type=\"object\">{kind}/$ID/{}</BasedOn>",
            escape(base)
        ));
    }
    if let Some(value) = leading {
        match value {
            schist_layout::styles::Leading::Auto => {
                out.push_str("<Leading type=\"enumeration\">Auto</Leading>")
            }
            schist_layout::styles::Leading::Points(value) if value.is_finite() && value >= 0.0 => {
                out.push_str(&format!("<Leading type=\"unit\">{value}</Leading>"))
            }
            _ => {}
        }
    }
    if let Some(family) = family {
        out.push_str(&format!(
            "<AppliedFont type=\"string\">{}</AppliedFont>",
            escape(family)
        ));
    }
    let feature_label = crate::opentype_codec::label(features);
    if automatic_direction.is_some() || !feature_label.is_empty() {
        out.push_str("<Label>");
        if let Some(value) = automatic_direction {
            out.push_str(&format!(
                r#"<KeyValuePair Key="{}" Value="{value}"/>"#,
                crate::auto_direction::STYLE_LABEL
            ));
        }
        out.push_str(&feature_label);
        out.push_str("</Label>");
    }
    out.push_str(&format!("</Properties></{kind}>"));
}

const FONT_CHOICE_LABEL: &str = "schist.font-choice";

#[derive(serde::Serialize, serde::Deserialize)]
struct FontChoice {
    native: String,
    name: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
}

fn read_font_choice(element: &Element) -> (Option<String>, Option<bool>, Option<bool>) {
    let native = element.attr("FontStyle");
    if let Some(choice) = element
        .child("Properties")
        .and_then(|p| p.child("Label"))
        .and_then(|label| {
            label
                .children
                .iter()
                .find(|e| e.attr("Key") == Some(FONT_CHOICE_LABEL))
        })
        .and_then(|e| e.attr("Value"))
        .and_then(|v| serde_json::from_str::<FontChoice>(v).ok())
        .filter(|choice| Some(choice.native.as_str()) == native)
    {
        // An external native edit takes precedence over stale extension data.
        return (choice.name, choice.bold, choice.italic);
    }
    (
        native.map(str::to_owned),
        native.map(|n| n.contains("Bold")),
        native.map(|n| n.contains("Italic") || n.contains("Oblique")),
    )
}

/// Native FontStyle is an atomic face name. The label retains Schist's
/// independent legacy bold/italic inheritance without changing that name.
fn font_choice(
    name: Option<&str>,
    bold: Option<bool>,
    italic: Option<bool>,
    resolved: (bool, bool),
) -> Option<FontChoice> {
    if name.is_none() && bold.is_none() && italic.is_none() {
        return None;
    }
    let native = name
        .unwrap_or(match resolved {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        })
        .to_owned();
    Some(FontChoice {
        native,
        name: name.map(str::to_owned),
        bold,
        italic,
    })
}

fn font_choice_label(out: &mut String, choice: Option<&FontChoice>) {
    let Some(choice) = choice else {
        return;
    };
    let value = serde_json::to_string(choice).expect("font choice contains strings and booleans");
    let pair = format!(
        r#"<KeyValuePair Key="{FONT_CHOICE_LABEL}" Value="{}"/>"#,
        escape(&value)
    );
    if let Some(at) = out.find("</Label>") {
        out.insert_str(at, &pair);
    } else if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &format!("<Label>{pair}</Label>"));
    }
}

fn position(
    out: &mut String,
    value: Option<schist_layout::styles::TextPosition>,
    shift: Option<schist_layout::styles::BaselineShift>,
) {
    use schist_layout::styles::{BaselineShift, TextPosition};
    if value.is_some()
        || matches!(
            shift,
            Some(BaselineShift::Superscript | BaselineShift::Subscript)
        )
    {
        attr(
            out,
            "Position",
            match TextPosition::resolved(value, shift) {
                TextPosition::Normal => "Normal",
                TextPosition::Superscript => "Superscript",
                TextPosition::Subscript => "Subscript",
            },
        );
    }
}

#[cfg(test)]
pub fn paragraph(style: &ParagraphStyle) -> String {
    paragraph_resolved(
        style,
        (style.bold.unwrap_or(false), style.italic.unwrap_or(false)),
    )
}

pub fn paragraph_resolved(style: &ParagraphStyle, resolved: (bool, bool)) -> String {
    let choice = font_choice(
        style.font_style.as_deref(),
        style.bold,
        style.italic,
        resolved,
    );
    let mut out = String::from("<ParagraphStyle");
    crate::list_codec::attributes(&mut out, &crate::list_codec::native(style));
    crate::capitalization_codec::attributes(&mut out, style.all_caps, style.small_caps);
    crate::opentype_codec::attributes(&mut out, &style.features);
    crate::opentype_codec::directional_attributes(&mut out, style.directional_features);
    crate::decoration_codec::attributes(&mut out, [&style.underline_style, &style.strike_style]);
    optional(
        &mut out,
        "AutoLeading",
        style
            .auto_leading
            .filter(|v| v.is_finite() && (0.0..=500.0).contains(v)),
    );
    attr(
        &mut out,
        "Self",
        format!("ParagraphStyle/$ID/{}", style.name),
    );
    attr(&mut out, "Name", &style.name);
    optional(&mut out, "FontStyle", choice.as_ref().map(|c| &c.native));
    optional(&mut out, "Underline", style.underline);
    optional(&mut out, "StrikeThru", style.strikethrough);
    position(&mut out, style.position, style.baseline_shift);
    optional(
        &mut out,
        "BaselineShift",
        style
            .baseline_shift
            .and_then(schist_layout::styles::BaselineShift::explicit_offset),
    );
    optional(
        &mut out,
        "FillColor",
        if style.fill_disabled {
            Some("Swatch/None".into())
        } else {
            style.fill.as_ref().map(crate::color_codec::reference)
        },
    );
    optional(
        &mut out,
        "StrokeColor",
        if style.stroke_disabled {
            Some("Swatch/None".into())
        } else {
            style.stroke.as_ref().map(crate::color_codec::reference)
        },
    );
    optional(
        &mut out,
        "FillTint",
        crate::color_codec::paint_tint(style.fill.as_ref(), style.fill_tint),
    );
    optional(
        &mut out,
        "StrokeTint",
        crate::color_codec::paint_tint(style.stroke.as_ref(), style.stroke_tint),
    );
    optional(&mut out, "OverprintFill", style.overprint_fill);
    optional(&mut out, "OverprintStroke", style.overprint_stroke);
    optional(
        &mut out,
        "StrokeAlignment",
        style.stroke_outside.map(|v| {
            if v {
                "OutsideAlignment"
            } else {
                "CenterAlignment"
            }
        }),
    );
    optional(
        &mut out,
        "StrokeWeight",
        style.stroke_weight.filter(|v| v.is_finite() && *v >= 0.0),
    );
    optional(
        &mut out,
        "EndJoin",
        style.stroke_join.map(|join| match join {
            schist_text_engine::TextStrokeJoin::Miter => "MiterEndJoin",
            schist_text_engine::TextStrokeJoin::Round => "RoundEndJoin",
            schist_text_engine::TextStrokeJoin::Bevel => "BevelEndJoin",
        }),
    );
    optional(
        &mut out,
        "MiterLimit",
        style
            .stroke_miter_limit
            .filter(|v| v.is_finite() && *v >= 0.0),
    );
    optional(
        &mut out,
        "NextStyle",
        style
            .next
            .as_ref()
            .map(|n| format!("ParagraphStyle/$ID/{n}")),
    );
    for (key, value) in [
        ("PointSize", style.point_size),
        ("Tracking", style.tracking),
        ("LeftIndent", style.left_indent),
        ("RightIndent", style.right_indent),
        ("FirstLineIndent", style.first_line_indent),
        ("SpaceBefore", style.space_before),
        ("SpaceAfter", style.space_after),
    ] {
        optional(&mut out, key, value);
    }
    optional(
        &mut out,
        "Justification",
        style.align.map(|a| match a {
            Align::Left => "LeftAlign",
            Align::Center => "CenterAlign",
            Align::Right => "RightAlign",
            Align::Justify => "LeftJustified",
            Align::JustifyAll => "FullyJustified",
        }),
    );
    optional(&mut out, "KeepWithNext", style.keep_with_next.map(u8::from));
    optional(&mut out, "KeepFirstLines", style.keep_lines);
    optional(&mut out, "DropCapLines", style.drop_caps_lines);
    optional(&mut out, "DropCapCharacters", style.drop_caps_characters);
    optional(
        &mut out,
        "ParagraphDirection",
        style.direction.map(|direction| match direction {
            schist_layout::ParagraphDirection::LeftToRight
            | schist_layout::ParagraphDirection::Auto => "LeftToRightDirection",
            schist_layout::ParagraphDirection::RightToLeft => "RightToLeftDirection",
        }),
    );
    optional(&mut out, "Hyphenation", style.hyphenate);
    optional(&mut out, "AppliedLanguage", style.language.as_ref());
    optional(
        &mut out,
        "KerningMethod",
        style
            .kerning
            .map(|v| if v { "$ID/Metrics" } else { "$ID/None" }),
    );
    props(
        &mut out,
        "ParagraphStyle",
        &style.based_on,
        style.leading,
        style.family.as_deref(),
        match style.direction {
            Some(schist_layout::ParagraphDirection::Auto) => Some("Auto"),
            None if style.based_on.is_none() => Some("AutoDefault"),
            _ => None,
        },
        &style.features,
    );
    font_choice_label(&mut out, choice.as_ref());
    crate::list_codec::properties(&mut out, &crate::list_codec::native(style));
    crate::list_codec::label(&mut out, style);
    crate::decoration_codec::properties(&mut out, [&style.underline_style, &style.strike_style]);
    crate::opentype_codec::directional_label(&mut out, style.directional_features, &style.features);
    crate::capitalization_codec::label(&mut out, style.all_caps, style.small_caps);
    out
}

pub fn character_resolved(style: &CharacterStyle, resolved: (bool, bool)) -> String {
    let choice = font_choice(
        style.font_style.as_deref(),
        style.bold,
        style.italic,
        resolved,
    );
    let mut out = String::from("<CharacterStyle");
    crate::capitalization_codec::attributes(&mut out, style.all_caps, style.small_caps);
    crate::opentype_codec::attributes(&mut out, &style.features);
    crate::opentype_codec::directional_attributes(&mut out, style.directional_features);
    crate::decoration_codec::attributes(&mut out, [&style.underline_style, &style.strike_style]);
    attr(
        &mut out,
        "Self",
        format!("CharacterStyle/$ID/{}", style.name),
    );
    attr(&mut out, "Name", &style.name);
    optional(
        &mut out,
        "FillColor",
        if style.fill_disabled {
            Some("Swatch/None".into())
        } else {
            style.fill.as_ref().map(crate::color_codec::reference)
        },
    );
    optional(
        &mut out,
        "StrokeColor",
        if style.stroke_disabled {
            Some("Swatch/None".into())
        } else {
            style.stroke.as_ref().map(crate::color_codec::reference)
        },
    );
    optional(
        &mut out,
        "FillTint",
        crate::color_codec::paint_tint(style.fill.as_ref(), style.fill_tint),
    );
    optional(
        &mut out,
        "StrokeTint",
        crate::color_codec::paint_tint(style.stroke.as_ref(), style.stroke_tint),
    );
    optional(&mut out, "OverprintFill", style.overprint_fill);
    optional(&mut out, "OverprintStroke", style.overprint_stroke);
    optional(
        &mut out,
        "StrokeAlignment",
        style.stroke_outside.map(|v| {
            if v {
                "OutsideAlignment"
            } else {
                "CenterAlignment"
            }
        }),
    );
    optional(
        &mut out,
        "StrokeWeight",
        style.stroke_weight.filter(|v| v.is_finite() && *v >= 0.0),
    );
    optional(
        &mut out,
        "EndJoin",
        style.stroke_join.map(|join| match join {
            schist_text_engine::TextStrokeJoin::Miter => "MiterEndJoin",
            schist_text_engine::TextStrokeJoin::Round => "RoundEndJoin",
            schist_text_engine::TextStrokeJoin::Bevel => "BevelEndJoin",
        }),
    );
    optional(
        &mut out,
        "MiterLimit",
        style
            .stroke_miter_limit
            .filter(|v| v.is_finite() && *v >= 0.0),
    );
    optional(&mut out, "PointSize", style.point_size);
    optional(&mut out, "Tracking", style.tracking);
    optional(&mut out, "FontStyle", choice.as_ref().map(|c| &c.native));
    optional(&mut out, "Underline", style.underline);
    optional(&mut out, "StrikeThru", style.strikethrough);
    position(&mut out, style.position, style.baseline_shift);
    optional(
        &mut out,
        "BaselineShift",
        style
            .baseline_shift
            .and_then(schist_layout::styles::BaselineShift::explicit_offset),
    );
    optional(&mut out, "AppliedLanguage", style.language.as_ref());
    optional(
        &mut out,
        "KerningMethod",
        style
            .kerning
            .map(|v| if v { "$ID/Metrics" } else { "$ID/None" }),
    );
    props(
        &mut out,
        "CharacterStyle",
        &style.based_on,
        style.leading,
        style.family.as_deref(),
        None,
        &style.features,
    );
    font_choice_label(&mut out, choice.as_ref());
    crate::decoration_codec::properties(&mut out, [&style.underline_style, &style.strike_style]);
    crate::opentype_codec::directional_label(&mut out, style.directional_features, &style.features);
    crate::capitalization_codec::label(&mut out, style.all_caps, style.small_caps);
    out
}

pub(crate) fn warn_stroke(
    weight: Option<f32>,
    miter_limit: Option<f32>,
    warnings: &mut Vec<String>,
) {
    if miter_limit.is_some_and(|v| !v.is_finite() || v < 0.0) {
        warnings.push(schist_i18n::tf!(
            "design.idml_text_stroke_option",
            property = "MiterLimit"
        ));
    }
    if let Some(value) = weight.filter(|v| !v.is_finite() || *v < 0.0) {
        warnings.push(schist_i18n::tf!(
            "design.idml_text_stroke_invalid",
            value = value
        ));
    }
}
