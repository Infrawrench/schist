//! TextWrapPreference, IgnoreWrap and the TextPreference wrap settings follow
//! the public IDML specification. Invalid values are reported and read as the
//! published defaults.
use crate::{
    import::Report,
    xml::{self, Element},
};
use schist_layout::text_wrap::{ContourType, TextWrap, WrapMode, WrapPreferences, WrapSide};
use schist_layout::{Insets, PlacedObject};

const MODES: [(&str, WrapMode); 5] = [
    ("None", WrapMode::None),
    ("JumpObjectTextWrap", WrapMode::JumpObject),
    ("NextColumnTextWrap", WrapMode::NextColumn),
    ("BoundingBoxTextWrap", WrapMode::BoundingBox),
    ("Contour", WrapMode::Contour),
];

const SIDES: [(&str, WrapSide); 6] = [
    ("BothSides", WrapSide::BothSides),
    ("LeftSide", WrapSide::LeftSide),
    ("RightSide", WrapSide::RightSide),
    ("SideTowardsSpine", WrapSide::SideTowardsSpine),
    ("SideAwayFromSpine", WrapSide::SideAwayFromSpine),
    ("LargestArea", WrapSide::LargestArea),
];

const CONTOURS: [(&str, ContourType); 6] = [
    ("BoundingBox", ContourType::BoundingBox),
    ("PhotoshopPath", ContourType::PhotoshopPath),
    ("DetectEdges", ContourType::DetectEdges),
    ("AlphaChannel", ContourType::AlphaChannel),
    ("GraphicFrame", ContourType::GraphicFrame),
    ("SameAsClipping", ContourType::SameAsClipping),
];

/// The published empty path name.
const NO_PATH: &str = "$ID/";

fn lookup<T: Copy>(table: &[(&str, T)], raw: &str) -> Option<T> {
    table.iter().find(|(name, _)| *name == raw).map(|(_, v)| *v)
}

fn spelling<T: PartialEq>(table: &[(&'static str, T)], value: &T) -> &'static str {
    table
        .iter()
        .find(|(_, v)| v == value)
        .map(|(name, _)| *name)
        .expect("every value has a published spelling")
}

fn invalid(report: &mut Report, name: &str, property: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_text_wrap_invalid",
        name = name,
        property = property,
        value = value
    ));
}

/// A page item's own TextWrapPreference, if it has one.
pub(crate) fn read(element: &Element, name: &str, report: &mut Report) -> Option<TextWrap> {
    let preference = element.child("TextWrapPreference")?;
    let mut wrap = TextWrap::default();
    if let Some(raw) = preference.attr("TextWrapMode") {
        match lookup(&MODES, raw) {
            Some(mode) => wrap.mode = mode,
            None => invalid(report, name, "TextWrapMode", raw),
        }
    }
    if let Some(raw) = preference.attr("TextWrapSide") {
        match lookup(&SIDES, raw) {
            Some(side) => wrap.side = side,
            None => invalid(report, name, "TextWrapSide", raw),
        }
    }
    for (key, target) in [
        ("Inverse", &mut wrap.inverse),
        ("ApplyToMasterPageOnly", &mut wrap.master_only),
    ] {
        if let Some(raw) = preference.attr(key) {
            match xml::parse_boolean(raw) {
                Some(value) => *target = value,
                None => invalid(report, name, key, raw),
            }
        }
    }
    if let Some(offset) = preference
        .child("Properties")
        .and_then(|p| p.child("TextWrapOffset"))
    {
        let mut values = [0.0; 4];
        for (value, key) in values.iter_mut().zip(["Top", "Left", "Bottom", "Right"]) {
            if let Some(raw) = offset.attr(key) {
                match xml::parse_number(raw) {
                    Some(v) if v.is_finite() && v.abs() <= TextWrap::MAX_OFFSET => *value = v,
                    _ => invalid(report, name, key, raw),
                }
            }
        }
        let [top, left, bottom, right] = values;
        wrap.offsets = Insets::new(top, right, bottom, left);
    }
    if let Some(contour) = preference.child("ContourOption") {
        if let Some(raw) = contour.attr("ContourType") {
            match lookup(&CONTOURS, raw) {
                Some(kind) => wrap.contour = Some(kind),
                None => invalid(report, name, "ContourType", raw),
            }
        }
        if let Some(raw) = contour.attr("IncludeInsideEdges") {
            match xml::parse_boolean(raw) {
                Some(value) => wrap.inside_edges = value,
                None => invalid(report, name, "IncludeInsideEdges", raw),
            }
        }
        wrap.contour_path = contour
            .attr("ContourPathName")
            .filter(|path| *path != NO_PATH)
            .unwrap_or_default()
            .to_owned();
    }
    Some(wrap)
}

/// A text frame's TextFramePreference IgnoreWrap.
pub(crate) fn ignores(element: &Element, name: &str, report: &mut Report) -> bool {
    let Some(raw) = element
        .child("TextFramePreference")
        .and_then(|p| p.attr("IgnoreWrap"))
    else {
        return false;
    };
    xml::parse_boolean(raw).unwrap_or_else(|| {
        invalid(report, name, "IgnoreWrap", raw);
        false
    })
}

/// A layer's IgnoreWrap.
pub(crate) fn layer_ignores(element: &Element, name: &str, report: &mut Report) -> bool {
    let Some(raw) = element.attr("IgnoreWrap") else {
        return false;
    };
    xml::parse_boolean(raw).unwrap_or_else(|| {
        invalid(report, name, "IgnoreWrap", raw);
        false
    })
}

/// TextPreference AbutTextToTextWrap, ZOrderTextWrap and JustifyTextWraps.
pub(crate) fn read_preferences(element: &Element, report: &mut Report) -> WrapPreferences {
    let mut preferences = WrapPreferences::default();
    for (key, target) in [
        ("AbutTextToTextWrap", &mut preferences.abut),
        ("ZOrderTextWrap", &mut preferences.only_beneath),
        ("JustifyTextWraps", &mut preferences.justify),
    ] {
        if let Some(raw) = element.attr(key) {
            match xml::parse_boolean(raw) {
                Some(value) => *target = value,
                None => report.skip(schist_i18n::tf!(
                    "design.idml_text_preference_invalid",
                    property = key,
                    value = raw
                )),
            }
        }
    }
    preferences
}

pub(crate) fn preference_attributes(preferences: &WrapPreferences) -> String {
    format!(
        r#" AbutTextToTextWrap="{}" ZOrderTextWrap="{}" JustifyTextWraps="{}""#,
        preferences.abut, preferences.only_beneath, preferences.justify
    )
}

/// A TextWrapPreference element.
pub(crate) fn write(wrap: &TextWrap) -> String {
    use crate::export::number;
    let o = wrap.offsets;
    let contour = wrap
        .contour
        .map(|kind| {
            let path = if wrap.contour_path.is_empty() {
                NO_PATH.to_owned()
            } else {
                crate::style_codec::escape(&wrap.contour_path)
            };
            format!(
                r#"<ContourOption ContourType="{}" IncludeInsideEdges="{}" ContourPathName="{path}" />"#,
                spelling(&CONTOURS, &kind),
                wrap.inside_edges
            )
        })
        .unwrap_or_default();
    format!(
        r#"<TextWrapPreference Inverse="{}" ApplyToMasterPageOnly="{}" TextWrapSide="{}" TextWrapMode="{}"><Properties><TextWrapOffset Top="{}" Left="{}" Bottom="{}" Right="{}" /></Properties>{contour}</TextWrapPreference>"#,
        wrap.inverse,
        wrap.master_only,
        spelling(&SIDES, &wrap.side),
        spelling(&MODES, &wrap.mode),
        number(o.top),
        number(o.left),
        number(o.bottom),
        number(o.right),
    )
}

/// Add an item's TextWrapPreference after its own Properties, where page-item
/// settings precede nested content.
pub(crate) fn attach(xml: &mut String, object: &PlacedObject) {
    let Some(wrap) = object.appearance.text_wrap.as_ref() else {
        return;
    };
    let child = write(wrap);
    const CLOSE: &str = "</Properties>";
    if let Some(at) = xml.find(CLOSE) {
        xml.insert_str(at + CLOSE.len(), &child);
    } else if let Some(at) = xml.rfind("</") {
        xml.insert_str(at, &child);
    }
}
