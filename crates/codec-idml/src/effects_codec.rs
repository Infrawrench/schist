//! Item effects: a TransparencySetting's blend mode and drop shadow are read
//! and saved with its opacity. The specification's other effects, and blend
//! modes Schist draws Normal, are kept out and reported when applied; their
//! settings at InDesign's defaults say nothing.
use crate::{
    color_codec::{self, Colors},
    import::Report,
    xml::Element,
};
use schist_layout::{
    effects::{BlendMode, DropShadow},
    PlacedObject,
};

const MODES: [(&str, BlendMode); 16] = [
    ("Normal", BlendMode::Normal),
    ("Multiply", BlendMode::Multiply),
    ("Screen", BlendMode::Screen),
    ("Overlay", BlendMode::Overlay),
    ("SoftLight", BlendMode::SoftLight),
    ("HardLight", BlendMode::HardLight),
    ("ColorDodge", BlendMode::ColorDodge),
    ("ColorBurn", BlendMode::ColorBurn),
    ("Darken", BlendMode::Darken),
    ("Lighten", BlendMode::Lighten),
    ("Difference", BlendMode::Difference),
    ("Exclusion", BlendMode::Exclusion),
    ("Hue", BlendMode::Hue),
    ("Saturation", BlendMode::Saturation),
    ("Color", BlendMode::Color),
    ("Luminosity", BlendMode::Luminosity),
];

/// Effects reported when applied, with the attribute that says so.
const OTHER_EFFECTS: [(&str, &str); 8] = [
    ("InnerShadowSetting", "Applied"),
    ("OuterGlowSetting", "Applied"),
    ("InnerGlowSetting", "Applied"),
    ("BevelAndEmbossSetting", "Applied"),
    ("SatinSetting", "Applied"),
    ("DirectionalFeatherSetting", "Applied"),
    ("GradientFeatherSetting", "Applied"),
    ("FeatherSetting", "Mode"),
];

fn mode_name(mode: BlendMode) -> &'static str {
    MODES
        .iter()
        .find(|(_, m)| *m == mode)
        .map_or("Normal", |(name, _)| name)
}

fn blend_mode(element: &Element, report: &mut Report) -> Option<BlendMode> {
    let raw = element.attr("BlendMode")?;
    match MODES.iter().find(|(name, _)| *name == raw) {
        Some((_, mode)) => Some(*mode),
        None => {
            report.skip(schist_i18n::tf!(
                "design.idml_object_paint_invalid",
                property = "BlendMode"
            ));
            None
        }
    }
}

fn unapplied(report: &mut Report, effect: &str, name: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_effect",
        effect = effect,
        name = name
    ));
}

/// An item's blend mode, other than Normal, and its drop shadow, reporting
/// what Schist does not draw.
pub(crate) fn read(
    element: &Element,
    colors: &Colors,
    name: &str,
    report: &mut Report,
) -> (Option<BlendMode>, Option<DropShadow>) {
    for part in [
        "FillTransparencySetting",
        "StrokeTransparencySetting",
        "ContentTransparencySetting",
    ] {
        if element.child(part).is_some_and(applied) {
            unapplied(report, part, name);
        }
    }
    let Some(setting) = element.child("TransparencySetting") else {
        return (None, None);
    };
    let blending = setting.child("BlendingSetting");
    let blend = blending
        .and_then(|b| blend_mode(b, report))
        .filter(|mode| *mode != BlendMode::Normal);
    if let Some(mode) = blend.filter(|mode| !mode.separable()) {
        unapplied(report, mode_name(mode), name);
    }
    for key in ["KnockoutGroup", "IsolateBlending"] {
        if blending.is_some_and(|b| b.boolean(key) == Some(true)) {
            unapplied(report, key, name);
        }
    }
    others(setting, name, report);
    let shadow = setting
        .child("DropShadowSetting")
        .filter(|shadow| shadow.attr("Mode") == Some("Drop"))
        .map(|shadow| drop_shadow(shadow, colors, name, report));
    (blend, shadow)
}

/// Report a group's effects, which Schist does not apply to its items.
pub(crate) fn report_group(element: &Element, name: &str, report: &mut Report) {
    let Some(setting) = element.child("TransparencySetting") else {
        return;
    };
    if setting
        .child("BlendingSetting")
        .and_then(|b| b.attr("BlendMode"))
        .is_some_and(|mode| mode != "Normal")
    {
        unapplied(report, "BlendMode", name);
    }
    if setting
        .child("DropShadowSetting")
        .is_some_and(|shadow| shadow.attr("Mode") == Some("Drop"))
    {
        unapplied(report, "DropShadowSetting", name);
    }
    others(setting, name, report);
}

fn others(setting: &Element, name: &str, report: &mut Report) {
    for (effect, key) in OTHER_EFFECTS {
        let on = setting.child(effect).is_some_and(|e| match key {
            "Mode" => e.attr(key).is_some_and(|mode| mode != "None"),
            _ => e.boolean(key) == Some(true),
        });
        if on {
            unapplied(report, effect, name);
        }
    }
}

/// Whether a fill, stroke or content setting changes anything.
fn applied(setting: &Element) -> bool {
    let blending = setting.child("BlendingSetting");
    blending
        .and_then(|b| b.number("Opacity"))
        .is_some_and(|opacity| opacity < 100.0)
        || blending
            .and_then(|b| b.attr("BlendMode"))
            .is_some_and(|mode| mode != "Normal")
        || setting
            .child("DropShadowSetting")
            .is_some_and(|shadow| shadow.attr("Mode") == Some("Drop"))
        || OTHER_EFFECTS.iter().any(|(effect, key)| {
            setting.child(effect).is_some_and(|e| match *key {
                "Mode" => e.attr(key).is_some_and(|mode| mode != "None"),
                _ => e.boolean(key) == Some(true),
            })
        })
}

fn drop_shadow(element: &Element, colors: &Colors, name: &str, report: &mut Report) -> DropShadow {
    let defaults = DropShadow::default();
    let number = |key: &str, default: f32| {
        element
            .number(key)
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    };
    let color = match element.attr("EffectColor") {
        None | Some("n" | "Swatch/None") => None,
        Some(_) => color_codec::resolve(element, "EffectColor", colors, report),
    };
    let shadow = DropShadow {
        color,
        opacity: (number("Opacity", 75.0) / 100.0).clamp(0.0, 1.0),
        blend: blend_mode(element, report).unwrap_or(defaults.blend),
        x_offset: number("XOffset", defaults.x_offset),
        y_offset: number("YOffset", defaults.y_offset),
        size: number("Size", defaults.size).max(0.0),
        spread: (number("Spread", 0.0) / 100.0).clamp(0.0, 1.0),
        noise: (number("Noise", 0.0) / 100.0).clamp(0.0, 1.0),
        knocked_out: element.boolean("KnockedOut").unwrap_or(true),
    };
    if !shadow.blend.separable() {
        unapplied(report, mode_name(shadow.blend), name);
    }
    for (value, effect) in [(shadow.spread, "Spread"), (shadow.noise, "Noise")] {
        if value > 0.0 {
            unapplied(report, effect, name);
        }
    }
    shadow
}

/// The TransparencySetting that saves an item's opacity, blend mode and
/// drop shadow.
pub(crate) fn write(object: &PlacedObject) -> String {
    let opacity = object.transparency.clamp(0.0, 1.0) * 100.0;
    let mut out = format!("<TransparencySetting><BlendingSetting Opacity=\"{opacity}\"");
    if let Some(mode) = object.appearance.blend_mode {
        out.push_str(&format!(" BlendMode=\"{}\"", mode_name(mode)));
    }
    out.push_str("/>");
    if let Some(shadow) = &object.appearance.drop_shadow {
        let color = shadow
            .color
            .as_ref()
            .map_or_else(|| "n".to_string(), color_codec::reference);
        out.push_str(&format!(
            "<DropShadowSetting Mode=\"Drop\" BlendMode=\"{}\" Opacity=\"{}\" XOffset=\"{}\" YOffset=\"{}\" Size=\"{}\" EffectColor=\"{}\" Noise=\"{}\" Spread=\"{}\" KnockedOut=\"{}\"/>",
            mode_name(shadow.blend),
            shadow.opacity * 100.0,
            shadow.x_offset,
            shadow.y_offset,
            shadow.size,
            crate::export::escape(&color),
            shadow.noise * 100.0,
            shadow.spread * 100.0,
            shadow.knocked_out,
        ));
    }
    out.push_str("</TransparencySetting>");
    out
}
