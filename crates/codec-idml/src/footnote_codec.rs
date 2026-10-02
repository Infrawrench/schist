//! FootnoteOption from public IDML §6.3.19, enum tables and PSU native fixtures.
//! Settings retain absence and unresolved identities; composition remains gated.
use crate::{
    color_codec,
    import::Report,
    style_codec::{self, References},
    xml::Element,
};
use schist_layout::{decorations::DecorationStroke, footnotes::*, Ink, LayoutDocument};

trait NativeValue: Sized {
    fn decode(value: &str) -> Self;
    fn native(&self) -> (&str, bool);
}
macro_rules! values {
    ($kind:ident { $($variant:ident => $native:literal),+ $(,)? }) => {
        impl NativeValue for $kind {
            fn decode(value: &str) -> Self {
                match value { $($native => Self::$variant,)+ other => Self::Other(other.into()) }
            }
            fn native(&self) -> (&str, bool) {
                match self { $(Self::$variant => ($native, true),)+ Self::Other(value) => (value, false) }
            }
        }
    };
}
values!(FootnoteNumbering {
    Arabic => "Arabic", RomanUpper => "UpperRoman", RomanLower => "LowerRoman",
    LettersUpper => "UpperLetters", LettersLower => "LowerLetters", Symbols => "Symbols",
    Kanji => "Kanji", FullWidthArabic => "FullWidthArabic", SingleLeadingZeros => "SingleLeadingZeros",
    DoubleLeadingZeros => "DoubleLeadingZeros", Asterisks => "Asterisks", ArabicAlifBaTah => "ArabicAlifBaTah",
    ArabicAbjad => "ArabicAbjad", HebrewBiblical => "HebrewBiblical", HebrewNonStandard => "HebrewNonStandard"
});
values!(FootnoteRestart { Continuous => "DontRestart", Page => "PageRestart", Spread => "SpreadRestart", Section => "SectionRestart" });
values!(FootnoteAffixes { None => "NoPrefixSuffix", Reference => "PrefixSuffixReference", Note => "PrefixSuffixMarker", Both => "PrefixSuffixBoth" });
values!(FootnoteMarkerPosition { Normal => "NormalMarker", Superscript => "SuperscriptMarker", Subscript => "SubscriptMarker", Ruby => "RubyMarker" });
values!(FootnoteFirstBaseline { Ascent => "AscentOffset", CapHeight => "CapHeight", Leading => "LeadingOffset", EmBox => "EmboxHeight", XHeight => "XHeight", Fixed => "FixedHeight" });

fn invalid(key: &str, value: impl std::fmt::Display) -> String {
    schist_i18n::tf!(
        "design.idml_text_preference_invalid",
        property = key,
        value = value
    )
}
fn unsupported(key: &str, value: &str) -> String {
    schist_i18n::tf!(
        "design.idml_footnote_unsupported",
        property = key,
        value = value
    )
}
fn enum_value<T: NativeValue>(element: &Element, key: &str, report: &mut Report) -> Option<T> {
    let raw = style_codec::property(element, key)?;
    let value = T::decode(raw);
    if !value.native().1 {
        report.skip(unsupported(key, raw));
    }
    Some(value)
}
fn measure(element: &Element, key: &str, min: f32, max: f32, report: &mut Report) -> Option<f32> {
    let raw = element.attr(key)?;
    match raw.parse::<f32>() {
        Ok(v) if v.is_finite() && (min..=max).contains(&v) => Some(v),
        _ => {
            report.skip(invalid(key, raw));
            None
        }
    }
}
fn boolean(element: &Element, key: &str, report: &mut Report) -> Option<bool> {
    let raw = element.attr(key)?;
    match raw {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => {
            report.skip(invalid(key, raw));
            None
        }
    }
}
fn text(element: &Element, key: &str, report: &mut Report) -> Option<String> {
    let raw = element.attr(key)?;
    if valid_text(raw) {
        Some(raw.into())
    } else {
        report.skip(invalid(key, raw));
        None
    }
}
fn valid_text(value: &str) -> bool {
    value.chars().count() <= 100
        && value.chars().all(|c| {
            matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
        })
}
fn style(
    element: &Element,
    key: &str,
    refs: &References,
    paragraph: bool,
    report: &mut Report,
) -> Option<FootnoteReference<String>> {
    let raw = element.attr(key)?;
    let none = if paragraph {
        "ParagraphStyle/$ID/[No paragraph style]"
    } else {
        "CharacterStyle/$ID/[No character style]"
    };
    if raw == "n" || raw == none {
        return Some(FootnoteReference::None);
    }
    let name = if paragraph {
        refs.known_paragraph(raw)
    } else {
        refs.known_character(raw)
    };
    Some(match name {
        Some(name) => FootnoteReference::Resolved(name.into()),
        None => {
            report.skip(unsupported(key, raw));
            FootnoteReference::Unresolved(raw.into())
        }
    })
}
fn paint(
    element: &Element,
    key: &str,
    colors: &color_codec::Colors,
    report: &mut Report,
) -> Option<FootnoteReference<Ink>> {
    let raw = style_codec::property(element, key)?;
    Some(if matches!(raw, "Swatch/None" | "n") {
        FootnoteReference::None
    } else if let Some(ink) = colors.get(raw) {
        FootnoteReference::Resolved(ink.clone())
    } else {
        report.skip(unsupported(key, raw));
        FootnoteReference::Unresolved(raw.into())
    })
}
fn rule(
    element: &Element,
    prefix: &str,
    colors: &color_codec::Colors,
    refs: &References,
    report: &mut Report,
) -> FootnoteRule {
    let key = format!("{prefix}Type");
    let stroke = style_codec::property(element, &key).map(|raw| {
        if raw == "n" {
            FootnoteReference::None
        } else if raw == "StrokeStyle/$ID/Solid" {
            FootnoteReference::Resolved(DecorationStroke::solid())
        } else if let Some(stroke) = refs.strokes.get(raw) {
            FootnoteReference::Resolved(stroke.clone())
        } else {
            report.skip(unsupported(&key, raw));
            FootnoteReference::Unresolved(raw.into())
        }
    });
    FootnoteRule {
        stroke,
        on: boolean(element, &format!("{prefix}On"), report),
        paint: paint(element, &format!("{prefix}Color"), colors, report),
        gap_paint: paint(element, &format!("{prefix}GapColor"), colors, report),
        weight: measure(element, &format!("{prefix}LineWeight"), 0.0, 1000.0, report),
        tint: measure(element, &format!("{prefix}Tint"), 0.0, 100.0, report).map(|v| v / 100.0),
        gap_tint: measure(element, &format!("{prefix}GapTint"), 0.0, 100.0, report)
            .map(|v| v / 100.0),
        overprint: boolean(element, &format!("{prefix}Overprint"), report),
        gap_overprint: boolean(element, &format!("{prefix}GapOverprint"), report),
        left_indent: measure(
            element,
            &format!("{prefix}LeftIndent"),
            -103680.0,
            103680.0,
            report,
        ),
        width: measure(element, &format!("{prefix}Width"), 0.0, 103680.0, report),
        offset: measure(
            element,
            &format!("{prefix}Offset"),
            -15552.0,
            15552.0,
            report,
        ),
    }
}
pub(crate) fn read(
    element: &Element,
    colors: &color_codec::Colors,
    refs: &References,
    report: &mut Report,
) -> FootnoteOptions {
    let start_at = element
        .attr("StartAt")
        .and_then(|raw| match raw.parse::<u32>() {
            Ok(v) if (1..=100000).contains(&v) => Some(v),
            _ => {
                report.skip(invalid("StartAt", raw));
                None
            }
        });
    FootnoteOptions {
        start_at,
        numbering: enum_value(element, "FootnoteNumberingStyle", report),
        restart: enum_value(element, "RestartNumbering", report),
        affixes: enum_value(element, "ShowPrefixSuffix", report),
        marker_position: enum_value(element, "MarkerPositioning", report),
        first_baseline: enum_value(element, "FootnoteFirstBaselineOffset", report),
        prefix: text(element, "Prefix", report),
        suffix: text(element, "Suffix", report),
        separator: text(element, "SeparatorText", report),
        text_style: style(element, "FootnoteTextStyle", refs, true, report),
        marker_style: style(element, "FootnoteMarkerStyle", refs, false, report),
        space_between: measure(element, "SpaceBetween", 0.0, 864.0, report),
        spacer: measure(element, "Spacer", 0.0, 864.0, report),
        minimum_first_baseline: measure(
            element,
            "FootnoteMinimumFirstBaselineOffset",
            0.0,
            103680.0,
            report,
        ),
        end_of_story: boolean(element, "EosPlacement", report),
        no_splitting: boolean(element, "NoSplitting", report),
        straddle: boolean(element, "EnableStraddling", report),
        rule: rule(element, "Rule", colors, refs, report),
        continuing_rule: rule(element, "ContinuingRule", colors, refs, report),
    }
}

// Native attribute whitespace is normalized by XML readers unless it is escaped
// numerically. Footnote separators commonly contain a tab; keep CR/LF too.
fn escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\t' => "&#9;".into(),
            '\n' => "&#10;".into(),
            '\r' => "&#13;".into(),
            c => crate::export::escape(&c.to_string()),
        })
        .collect()
}
fn attr(out: &mut String, key: &str, value: impl std::fmt::Display) {
    out.push_str(&format!(" {key}=\"{}\"", escape(&value.to_string())));
}
fn write_measure(
    out: &mut String,
    key: &str,
    value: Option<f32>,
    range: std::ops::RangeInclusive<f32>,
    warnings: &mut Vec<String>,
) {
    if let Some(value) = value {
        if value.is_finite() && range.contains(&value) {
            attr(out, key, value);
        } else {
            warnings.push(invalid(key, value));
        }
    }
}
fn write_enum<T: NativeValue>(
    out: &mut String,
    key: &str,
    value: &Option<T>,
    warnings: &mut Vec<String>,
) {
    if let Some(value) = value {
        let (raw, known) = value.native();
        if !known {
            warnings.push(unsupported(key, raw));
        }
        property(out, key, if known { "enumeration" } else { "string" }, raw);
    }
}
fn property(out: &mut String, key: &str, kind: &str, value: &str) {
    out.push_str(&format!("<{key} type=\"{kind}\">{}</{key}>", escape(value)));
}
fn write_style(
    out: &mut String,
    key: &str,
    value: &Option<FootnoteReference<String>>,
    paragraph: bool,
    doc: &LayoutDocument,
    warnings: &mut Vec<String>,
) {
    let Some(value) = value else { return };
    let kind = if paragraph {
        "ParagraphStyle"
    } else {
        "CharacterStyle"
    };
    let reference = match value {
        FootnoteReference::None => format!(
            "{kind}/$ID/[No {} style]",
            if paragraph { "paragraph" } else { "character" }
        ),
        FootnoteReference::Resolved(name) => {
            let exists = if paragraph {
                doc.styles.paragraphs.iter().any(|s| s.name == *name)
            } else {
                doc.styles.characters.iter().any(|s| s.name == *name)
            };
            if !exists {
                warnings.push(unsupported(key, name));
            }
            format!("{kind}/$ID/{name}")
        }
        FootnoteReference::Unresolved(raw) => {
            warnings.push(unsupported(key, raw));
            raw.clone()
        }
    };
    attr(out, key, reference);
}
fn rule_attributes(
    out: &mut String,
    prefix: &str,
    rule: &FootnoteRule,
    warnings: &mut Vec<String>,
) {
    for (suffix, value) in [
        ("On", rule.on),
        ("Overprint", rule.overprint),
        ("GapOverprint", rule.gap_overprint),
    ] {
        if let Some(value) = value {
            attr(out, &format!("{prefix}{suffix}"), value);
        }
    }
    for (suffix, value, range) in [
        ("LineWeight", rule.weight, 0.0..=1000.0),
        ("Tint", rule.tint.map(|v| v * 100.0), 0.0..=100.0),
        ("GapTint", rule.gap_tint.map(|v| v * 100.0), 0.0..=100.0),
        ("LeftIndent", rule.left_indent, -103680.0..=103680.0),
        ("Width", rule.width, 0.0..=103680.0),
        ("Offset", rule.offset, -15552.0..=15552.0),
    ] {
        write_measure(out, &format!("{prefix}{suffix}"), value, range, warnings);
    }
}
fn rule_properties(
    out: &mut String,
    prefix: &str,
    rule: &FootnoteRule,
    warnings: &mut Vec<String>,
) {
    for (suffix, value) in [("Color", &rule.paint), ("GapColor", &rule.gap_paint)] {
        if let Some(value) = value {
            let key = format!("{prefix}{suffix}");
            let reference = match value {
                FootnoteReference::None => "Swatch/None".into(),
                FootnoteReference::Resolved(ink) => color_codec::reference(ink),
                FootnoteReference::Unresolved(raw) => {
                    warnings.push(unsupported(&key, raw));
                    raw.clone()
                }
            };
            property(out, &key, "object", &reference);
        }
    }
    if let Some(stroke) = &rule.stroke {
        let key = format!("{prefix}Type");
        let reference = match stroke {
            FootnoteReference::None => "n".into(),
            FootnoteReference::Resolved(stroke) if stroke.valid() => {
                crate::stroke_style_codec::reference(stroke)
            }
            FootnoteReference::Resolved(stroke) => {
                warnings.push(invalid(&key, &stroke.name));
                return;
            }
            FootnoteReference::Unresolved(raw) => {
                warnings.push(unsupported(&key, raw));
                raw.clone()
            }
        };
        property(out, &key, "object", &reference);
    }
}
pub(crate) fn write(doc: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let options = &doc.footnotes;
    if options.is_empty() {
        return String::new();
    }
    let mut out = "<FootnoteOption".to_string();
    if let Some(value) = options.start_at {
        if (1..=100000).contains(&value) {
            attr(&mut out, "StartAt", value);
        } else {
            warnings.push(invalid("StartAt", value));
        }
    }
    for (key, value) in [
        ("Prefix", &options.prefix),
        ("Suffix", &options.suffix),
        ("SeparatorText", &options.separator),
    ] {
        if let Some(value) = value {
            if valid_text(value) {
                attr(&mut out, key, value);
            } else {
                warnings.push(invalid(key, value));
            }
        }
    }
    write_style(
        &mut out,
        "FootnoteTextStyle",
        &options.text_style,
        true,
        doc,
        warnings,
    );
    write_style(
        &mut out,
        "FootnoteMarkerStyle",
        &options.marker_style,
        false,
        doc,
        warnings,
    );
    for (key, value, max) in [
        ("SpaceBetween", options.space_between, 864.0),
        ("Spacer", options.spacer, 864.0),
        (
            "FootnoteMinimumFirstBaselineOffset",
            options.minimum_first_baseline,
            103680.0,
        ),
    ] {
        write_measure(&mut out, key, value, 0.0..=max, warnings);
    }
    for (key, value) in [
        ("EosPlacement", options.end_of_story),
        ("NoSplitting", options.no_splitting),
        ("EnableStraddling", options.straddle),
    ] {
        if let Some(value) = value {
            attr(&mut out, key, value);
        }
    }
    if let Some(value) = &options.first_baseline {
        let (raw, known) = value.native();
        if !known {
            warnings.push(unsupported("FootnoteFirstBaselineOffset", raw));
        }
        attr(&mut out, "FootnoteFirstBaselineOffset", raw);
    }
    rule_attributes(&mut out, "Rule", &options.rule, warnings);
    rule_attributes(
        &mut out,
        "ContinuingRule",
        &options.continuing_rule,
        warnings,
    );
    out.push_str("><Properties>");
    write_enum(
        &mut out,
        "FootnoteNumberingStyle",
        &options.numbering,
        warnings,
    );
    write_enum(&mut out, "RestartNumbering", &options.restart, warnings);
    write_enum(&mut out, "ShowPrefixSuffix", &options.affixes, warnings);
    write_enum(
        &mut out,
        "MarkerPositioning",
        &options.marker_position,
        warnings,
    );
    rule_properties(&mut out, "Rule", &options.rule, warnings);
    rule_properties(
        &mut out,
        "ContinuingRule",
        &options.continuing_rule,
        warnings,
    );
    out.push_str("</Properties></FootnoteOption>");
    out
}
