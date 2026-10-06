//! Native Color resources, distinct from Ink (press/trapping settings).
//! Color/Swatch references are opaque IDs; display names can contain slashes.
use crate::{
    designmap::DesignPackage,
    import::Report,
    xml::{self, Element},
};
use schist_layout::gradients::{Gradient, GradientFill, GradientStop};
use schist_layout::Ink;

/// Graphic.xml's swatches by Self: colours and tints, gradients, and the
/// stroke styles items name in StrokeType.
#[derive(Default)]
pub struct Colors(
    Vec<(String, Ink)>,
    Vec<(String, Gradient)>,
    Vec<(String, schist_layout::StrokeType)>,
);
impl Colors {
    pub(crate) fn stroke_type(&self, reference: &str) -> Option<&schist_layout::StrokeType> {
        self.2
            .iter()
            .find(|(id, _)| id == reference)
            .map(|(_, stroke)| stroke)
    }
    pub(crate) fn set_stroke_types(&mut self, types: Vec<(String, schist_layout::StrokeType)>) {
        self.2 = types;
    }
    pub fn values(&self) -> impl Iterator<Item = &Ink> {
        self.0.iter().map(|(_, ink)| ink)
    }
    pub(crate) fn get(&self, reference: &str) -> Option<&Ink> {
        self.0
            .iter()
            .find(|(id, _)| id == reference)
            .map(|(_, ink)| ink)
    }
    pub(crate) fn gradient(&self, reference: &str) -> Option<&Gradient> {
        self.1
            .iter()
            .find(|(id, _)| id == reference)
            .map(|(_, gradient)| gradient)
    }
}

pub fn read(opened: &DesignPackage<'_>, report: &mut Report) -> Colors {
    let mut colors = Colors::default();
    let mut tints = Vec::new();
    for part in opened.listed.iter().filter(|p| p.role == "Graphic") {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            continue;
        };
        tints.extend(root.find_all("Tint").into_iter().cloned());
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
                // The specification's ColorSpace is LAB; Schist's earlier
                // saves wrote Lab.
                (Some("LAB" | "Lab"), [l, a, b], true) => Ink::spot(name, [*l, *a, *b]),
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
    // Resolve after every Graphic part, so XML/resource order is irrelevant.
    // Native BaseColor references Color, never another Tint.
    for el in tints {
        let Some(id) = el.attr("Self") else { continue };
        let name = el.attr("Name").unwrap_or(id);
        let raw = el.attr("TintValue").unwrap_or("100");
        let Some(value) = raw
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
        else {
            report.skip(schist_i18n::tf!(
                "design.idml_tint_invalid",
                property = "TintValue",
                value = raw
            ));
            continue;
        };
        let Some(base) = el
            .attr("BaseColor")
            .and_then(|id| colors.get(id))
            .filter(|ink| ink.tint.is_none())
        else {
            report.skip(schist_i18n::tf!(
                "design.idml_color_unread",
                name = el.attr("BaseColor").unwrap_or(id)
            ));
            continue;
        };
        let ink = base
            .named_tint(name, value / 100.0)
            .expect("validated tint");
        colors.0.push((id.to_owned(), ink));
    }
    // Gradients after every colour and tint their stops name.
    for part in opened.listed.iter().filter(|p| p.role == "Graphic") {
        let Ok(text) = opened.text_of(&part.name) else {
            continue;
        };
        let Ok(root) = xml::parse(text) else {
            continue;
        };
        for el in root.find_all("Gradient") {
            let Some(id) = el.attr("Self") else { continue };
            if let Some(gradient) = gradient(el, &colors, report) {
                colors.1.push((id.to_owned(), gradient));
            }
        }
    }
    colors
}

/// A Gradient swatch: its type and stops, each stop's colour resolved and
/// its location and midpoint as fractions. One that cannot be read is
/// reported and left out, so items filled with it are unfilled.
fn gradient(el: &Element, colors: &Colors, report: &mut Report) -> Option<Gradient> {
    let id = el.attr("Self").unwrap_or_default();
    let name = el
        .attr("Name")
        .filter(|s| !s.is_empty() && *s != "$ID/")
        .unwrap_or(id)
        .trim_start_matches("$ID/")
        .to_owned();
    let mut stops = Vec::new();
    for stop in el.children_named("GradientStop") {
        let Some(ink) = stop.attr("StopColor").and_then(|c| colors.get(c)) else {
            report.skip(schist_i18n::tf!(
                "design.idml_color_unread",
                name = stop.attr("StopColor").unwrap_or(id)
            ));
            return None;
        };
        let percent = |key: &str, default: f32| {
            stop.attr(key)
                .map_or(Some(default), xml::parse_number)
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
                .map(|v| v / 100.0)
        };
        let (Some(location), Some(midpoint)) =
            (percent("Location", 0.0), percent("Midpoint", 50.0))
        else {
            report.skip(schist_i18n::tf!("design.idml_color_unread", name = name));
            return None;
        };
        stops.push(GradientStop {
            ink: ink.clone(),
            location,
            midpoint,
        });
    }
    let gradient = Gradient {
        name,
        radial: match el.attr("Type") {
            None | Some("Linear") => false,
            Some("Radial") => true,
            Some(_) => {
                report.skip(schist_i18n::tf!("design.idml_color_unread", name = id));
                return None;
            }
        },
        stops,
    };
    if gradient.valid() {
        Some(gradient)
    } else {
        report.skip(schist_i18n::tf!("design.idml_color_unread", name = id));
        None
    }
}

/// The gradient swatch `{part}Color` names on `element`, `part` being Fill
/// or Stroke, run where its Gradient{part}Start (two finite numbers),
/// Gradient{part}Length (positive) and Gradient{part}Angle (degrees, zero
/// by default) say; the specification gives the stroke's the same meanings
/// as the fill's. None when the colour is not a gradient.
pub(crate) fn applied_gradient(
    element: &Element,
    part: &str,
    colors: &Colors,
) -> Option<GradientFill> {
    let gradient = colors
        .gradient(element.attr(&format!("{part}Color"))?)?
        .clone();
    let start = element
        .attr(&format!("Gradient{part}Start"))
        .map(xml::numbers)
        .and_then(|v| match v.as_slice() {
            [x, y] if x.is_finite() && y.is_finite() => Some(schist_layout::Point::new(*x, *y)),
            _ => None,
        });
    Some(GradientFill {
        gradient,
        start,
        length: element
            .number(&format!("Gradient{part}Length"))
            .filter(|v| v.is_finite() && *v > 0.0),
        angle: element
            .number(&format!("Gradient{part}Angle"))
            .filter(|v| v.is_finite())
            .unwrap_or(0.0),
    })
}

/// The Gradient{part}Start, Length and Angle attributes that read back as
/// `applied`.
pub(crate) fn gradient_attributes(part: &str, applied: &GradientFill) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(start) = applied.start {
        out.push((
            format!("Gradient{part}Start"),
            format!("{} {}", start.x, start.y),
        ));
    }
    if let Some(length) = applied.length {
        out.push((format!("Gradient{part}Length"), length.to_string()));
    }
    out.push((format!("Gradient{part}Angle"), applied.angle.to_string()));
    out
}

/// A gradient swatch's reference, unique to its definition.
pub fn gradient_reference(gradient: &Gradient) -> String {
    let bits: String = gradient
        .stops
        .iter()
        .flat_map(|s| [s.location, s.midpoint])
        .map(|v| format!("{:08x}", v.to_bits()))
        .chain(gradient.stops.iter().map(|s| reference(&s.ink)))
        .collect();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bits.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    format!(
        "Gradient/Schist-{}-{}-{hash:016x}",
        gradient.name,
        u8::from(gradient.radial)
    )
}

/// The Gradient swatch resource; its stops name the colours written
/// alongside it.
pub fn gradient_resource(gradient: &Gradient) -> String {
    let id = gradient_reference(gradient);
    let mut out = format!(
        r#"<Gradient Self="{}" Type="{}" Name="{}">"#,
        crate::export::escape(&id),
        if gradient.radial { "Radial" } else { "Linear" },
        crate::export::escape(&gradient.name)
    );
    for (index, stop) in gradient.stops.iter().enumerate() {
        out.push_str(&format!(
            r#"<GradientStop Self="{}Stop{index}" StopColor="{}" Location="{}" Midpoint="{}"/>"#,
            crate::export::escape(&id),
            crate::export::escape(&reference(&stop.ink)),
            stop.location * 100.0,
            stop.midpoint * 100.0
        ));
    }
    out.push_str("</Gradient>");
    out
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
/// an omitted value. Named Tint resources own their fraction and use -1 here.
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
    if let Some(tint) = &ink.tint {
        return format!(
            "Tint/Schist-{}-{:08x}-{}",
            ink.name,
            tint.value.to_bits(),
            reference(&ink.base_color())
        );
    }
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

/// A named Tint is applied with -1, never another percentage (which would
/// detach it to its base Color in the native application).
pub fn paint_tint(ink: Option<&Ink>, value: Option<f32>) -> Option<f32> {
    if ink.is_some_and(|ink| ink.tint.is_some()) {
        Some(-1.0)
    } else {
        value.map(|v| v * 100.0)
    }
}

pub fn resource(ink: &Ink) -> String {
    if let Some(tint) = &ink.tint {
        return format!(
            r#"<Tint Self="{}" Name="{}" BaseColor="{}" TintValue="{}"/>"#,
            crate::export::escape(&reference(ink)),
            crate::export::escape(&ink.name),
            crate::export::escape(&reference(&ink.base_color())),
            tint.value * 100.0
        );
    }
    let (space, values) = if let Some(cmyk) = ink.source_cmyk {
        (
            "CMYK",
            cmyk.iter()
                .map(|v| (v * 100.0).to_string())
                .collect::<Vec<_>>(),
        )
    } else if ink.spot {
        ("LAB", ink.lab.iter().map(ToString::to_string).collect())
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
