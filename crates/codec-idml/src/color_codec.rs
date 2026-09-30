//! Native Color resources, distinct from Ink (press/trapping settings).
//! Color/Swatch references are opaque IDs; display names can contain slashes.
use crate::{
    designmap::DesignPackage,
    import::Report,
    xml::{self, Element},
};
use schist_layout::Ink;

#[derive(Default)]
pub struct Colors(Vec<(String, Ink)>);
impl Colors {
    pub fn values(&self) -> impl Iterator<Item = &Ink> {
        self.0.iter().map(|(_, ink)| ink)
    }
    fn get(&self, reference: &str) -> Option<&Ink> {
        self.0
            .iter()
            .find(|(id, _)| id == reference)
            .map(|(_, ink)| ink)
    }
}

pub fn read(opened: &DesignPackage<'_>, report: &mut Report) -> Colors {
    let mut colors = Colors::default();
    for part in opened.listed.iter().filter(|p| p.role == "Graphic") {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            continue;
        };
        for el in root.find_all("Color") {
            let Some(id) = el.attr("Self") else {
                continue;
            };
            let name = el
                .attr("Name")
                .filter(|s| !s.is_empty() && *s != "$ID/")
                .unwrap_or(id)
                .trim_start_matches("$ID/");
            let values = xml::numbers(el.attr("ColorValue").unwrap_or_default());
            let valid = values.iter().all(|v| v.is_finite());
            let mut ink = match (el.attr("Space"), values.as_slice(), valid) {
                (Some("CMYK"), [c, m, y, k], true) => {
                    Ink::cmyk(name, [c / 100.0, m / 100.0, y / 100.0, k / 100.0])
                }
                (Some("RGB"), [r, g, b], true) => {
                    Ink::process(name, [r / 255.0, g / 255.0, b / 255.0])
                }
                (Some("Lab"), [l, a, b], true) => Ink::spot(name, [*l, *a, *b]),
                _ => {
                    report.skip(schist_i18n::tf!("design.idml_color_unread", name = name));
                    continue;
                }
            };
            ink.spot = el.attr("Model") == Some("Spot");
            if el.attr("Model") == Some("Registration") {
                report.skip(schist_i18n::tf!("design.idml_registration", name = name));
            }
            colors.0.push((id.to_owned(), ink));
        }
    }
    colors
}

pub fn resolve(el: &Element, key: &str, colors: &Colors, report: &mut Report) -> Option<Ink> {
    let reference = el.attr(key)?;
    if reference == "Swatch/None" || reference == "n" {
        return None;
    }
    let ink = colors.get(reference).cloned();
    if ink.is_none() {
        report.skip(schist_i18n::tf!(
            "design.idml_color_unread",
            name = reference
        ));
    }
    ink
}

/// Native direct tint percentages; -1 has the same inheritance meaning as
/// an omitted value. Named Tint swatches remain unsupported and are diagnosed
/// by colour-reference resolution instead of being mistaken for full ink.
pub fn tint(el: &Element, key: &str, report: &mut Report) -> Option<f32> {
    let raw = el.attr(key)?;
    match raw.parse::<f32>() {
        Ok(-1.0) => None,
        Ok(value) if value.is_finite() && (0.0..=100.0).contains(&value) => Some(value / 100.0),
        _ => {
            report.skip(schist_i18n::tf!(
                "design.idml_tint_invalid",
                property = key,
                value = raw
            ));
            None
        }
    }
}

pub fn reference(ink: &Ink) -> String {
    // Include the definition: two inline inks may share a display name.
    let bits: String = ink
        .lab
        .iter()
        .chain(&ink.preview_rgb)
        .chain(ink.source_cmyk.iter().flatten())
        .map(|v| format!("{:08x}", v.to_bits()))
        .collect();
    format!("Color/Schist-{}-{}-{bits}", ink.name, u8::from(ink.spot))
}

pub fn resource(ink: &Ink) -> String {
    let (space, values) = if let Some(cmyk) = ink.source_cmyk {
        (
            "CMYK",
            cmyk.iter()
                .map(|v| (v * 100.0).to_string())
                .collect::<Vec<_>>(),
        )
    } else if ink.spot {
        ("Lab", ink.lab.iter().map(ToString::to_string).collect())
    } else {
        (
            "RGB",
            ink.preview_rgb
                .iter()
                .map(|v| (v * 255.0).to_string())
                .collect(),
        )
    };
    format!(
        r#"<Color Self="{}" Name="{}" Model="{}" Space="{space}" ColorValue="{}"/>"#,
        crate::export::escape(&reference(ink)),
        crate::export::escape(&ink.name),
        if ink.spot { "Spot" } else { "Process" },
        values.join(" ")
    )
}

pub fn opacity(el: &Element) -> f32 {
    el.child("TransparencySetting")
        .and_then(|s| s.child("BlendingSetting"))
        .and_then(|b| b.number("Opacity"))
        .filter(|v| v.is_finite())
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
        .unwrap_or(1.0)
}
pub fn transparency(opacity: f32) -> String {
    format!(
        r#"<TransparencySetting><BlendingSetting Opacity="{}"/></TransparencySetting>"#,
        opacity.clamp(0.0, 1.0) * 100.0
    )
}
