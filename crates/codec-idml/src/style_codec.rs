//! Named style properties from the public IDML XML structure. Missing
//! attributes stay absent so inheritance is not flattened on save.
use crate::xml::{self, Element};
use schist_layout::styles::Align;
use schist_layout::{CharacterStyle, ParagraphStyle, StyleSet};

pub(crate) fn name(value: &str) -> String {
    value
        .strip_prefix("ParagraphStyle/")
        .or_else(|| value.strip_prefix("CharacterStyle/"))
        .unwrap_or(value)
        .trim_start_matches("$ID/")
        .to_owned()
}
fn property<'a>(element: &'a Element, key: &str) -> Option<&'a str> {
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
    paragraphs: std::collections::BTreeMap<String, String>,
    characters: std::collections::BTreeMap<String, String>,
}
impl References {
    pub(crate) fn new(roots: &[Element]) -> Self {
        let mut out = Self::default();
        for (kind, names) in [
            ("ParagraphStyle", &mut out.paragraphs),
            ("CharacterStyle", &mut out.characters),
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
}

pub(crate) fn paragraph_properties(
    element: &Element,
    colors: &crate::color_codec::Colors,
    report: &mut crate::import::Report,
) -> ParagraphStyle {
    let character = character_properties(element, colors, report);
    ParagraphStyle {
        family: character.family,
        bold: character.bold,
        italic: character.italic,
        underline: character.underline,
        strikethrough: character.strikethrough,
        fill_tint: character.fill_tint,
        stroke_tint: character.stroke_tint,
        fill: character.fill,
        stroke: character.stroke,
        overprint_fill: character.overprint_fill,
        overprint_stroke: character.overprint_stroke,
        point_size: character.point_size,
        leading: character.leading,
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
        hyphenate: boolean(element, "Hyphenation"),
        language: character.language,
        ..ParagraphStyle::default()
    }
}

pub(crate) fn character_properties(
    element: &Element,
    colors: &crate::color_codec::Colors,
    report: &mut crate::import::Report,
) -> CharacterStyle {
    let font_style = element.attr("FontStyle");
    CharacterStyle {
        family: property(element, "AppliedFont").map(str::to_owned),
        fill_tint: crate::color_codec::tint(element, "FillTint", report),
        stroke_tint: crate::color_codec::tint(element, "StrokeTint", report),
        fill: crate::color_codec::resolve(element, "FillColor", colors, report),
        stroke: crate::color_codec::resolve(element, "StrokeColor", colors, report),
        overprint_fill: boolean(element, "OverprintFill"),
        overprint_stroke: boolean(element, "OverprintStroke"),
        point_size: element.number("PointSize"),
        leading: property(element, "Leading").and_then(xml::parse_number),
        tracking: element.number("Tracking"),
        kerning: element.attr("KerningMethod").map(|v| v != "$ID/None"),
        bold: font_style.map(|v| v.contains("Bold")),
        italic: font_style.map(|v| v.contains("Italic") || v.contains("Oblique")),
        underline: boolean(element, "Underline"),
        strikethrough: boolean(element, "StrikeThru"),
        language: element.attr("AppliedLanguage").map(str::to_owned),
        ..CharacterStyle::default()
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
        let mut style = paragraph_properties(element, colors, report);
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
        let mut style = character_properties(element, colors, report);
        style.name = refs.character(raw);
        style.based_on =
            base(element).map(|base| refs.character(property(element, "BasedOn").unwrap_or(&base)));
        styles.add_character(style);
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
fn optional(out: &mut String, key: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        attr(out, key, value);
    }
}
fn props(
    out: &mut String,
    kind: &str,
    based_on: &Option<String>,
    leading: Option<f32>,
    family: Option<&str>,
    automatic_direction: Option<&str>,
) {
    out.push_str("><Properties>");
    if let Some(base) = based_on {
        out.push_str(&format!(
            "<BasedOn type=\"object\">{kind}/$ID/{}</BasedOn>",
            escape(base)
        ));
    }
    if let Some(value) = leading {
        out.push_str(&format!("<Leading type=\"unit\">{value}</Leading>"));
    }
    if let Some(family) = family {
        out.push_str(&format!(
            "<AppliedFont type=\"string\">{}</AppliedFont>",
            escape(family)
        ));
    }
    if let Some(value) = automatic_direction {
        out.push_str(&format!(
            r#"<Label><KeyValuePair Key="{}" Value="{value}"/></Label>"#,
            crate::auto_direction::STYLE_LABEL
        ));
    }
    out.push_str(&format!("</Properties></{kind}>"));
}

fn font_style(out: &mut String, bold: Option<bool>, italic: Option<bool>) {
    if bold.is_some() || italic.is_some() {
        attr(
            out,
            "FontStyle",
            match (bold.unwrap_or(false), italic.unwrap_or(false)) {
                (true, true) => "Bold Italic",
                (true, false) => "Bold",
                (false, true) => "Italic",
                _ => "Regular",
            },
        );
    }
}

pub fn paragraph(style: &ParagraphStyle) -> String {
    let mut out = String::from("<ParagraphStyle");
    attr(
        &mut out,
        "Self",
        format!("ParagraphStyle/$ID/{}", style.name),
    );
    attr(&mut out, "Name", &style.name);
    font_style(&mut out, style.bold, style.italic);
    optional(&mut out, "Underline", style.underline);
    optional(&mut out, "StrikeThru", style.strikethrough);
    optional(
        &mut out,
        "FillColor",
        style.fill.as_ref().map(crate::color_codec::reference),
    );
    optional(
        &mut out,
        "StrokeColor",
        style.stroke.as_ref().map(crate::color_codec::reference),
    );
    optional(&mut out, "FillTint", style.fill_tint.map(|v| v * 100.0));
    optional(&mut out, "StrokeTint", style.stroke_tint.map(|v| v * 100.0));
    optional(&mut out, "OverprintFill", style.overprint_fill);
    optional(&mut out, "OverprintStroke", style.overprint_stroke);
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
    );
    out
}

pub fn character(style: &CharacterStyle) -> String {
    let mut out = String::from("<CharacterStyle");
    attr(
        &mut out,
        "Self",
        format!("CharacterStyle/$ID/{}", style.name),
    );
    attr(&mut out, "Name", &style.name);
    optional(
        &mut out,
        "FillColor",
        style.fill.as_ref().map(crate::color_codec::reference),
    );
    optional(
        &mut out,
        "StrokeColor",
        style.stroke.as_ref().map(crate::color_codec::reference),
    );
    optional(&mut out, "FillTint", style.fill_tint.map(|v| v * 100.0));
    optional(&mut out, "StrokeTint", style.stroke_tint.map(|v| v * 100.0));
    optional(&mut out, "OverprintFill", style.overprint_fill);
    optional(&mut out, "OverprintStroke", style.overprint_stroke);
    optional(&mut out, "PointSize", style.point_size);
    optional(&mut out, "Tracking", style.tracking);
    font_style(&mut out, style.bold, style.italic);
    optional(&mut out, "Underline", style.underline);
    optional(&mut out, "StrikeThru", style.strikethrough);
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
    );
    out
}
