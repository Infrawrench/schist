//! Native TextPath children use their own IDs in mixed path/box threads.
use crate::{import::Report, xml::Element};
use schist_layout::{text_path::PathText, LayoutObject, PlacedObject};

const FOLLOW_END: &str = "Schist.TextPath.FollowEnd.v1";
const ORIGIN: &str = "Schist.TextPath.LocalBounds.v1";

#[derive(serde::Serialize, serde::Deserialize)]
struct LocalBounds {
    geometry: String,
    width: f32,
    height: f32,
}

/// Recomputing tight cubic bounds after an f32 translation is not idempotent.
/// Retain the author's local origin only while the native curve still agrees;
/// native geometry edits must supersede this authoring precision metadata.
pub(crate) fn retained_bounds(element: &Element) -> Option<schist_layout::Rect> {
    let saved: LocalBounds =
        serde_json::from_str(crate::auto_direction::label(element, ORIGIN)?).ok()?;
    let path = crate::import::path_of(element)?;
    (saved.geometry == crate::export::path_geometry(&path)
        && [saved.width, saved.height]
            .into_iter()
            .all(|v| v.is_finite() && v >= 0.0))
    .then(|| schist_layout::Rect::new(0.0, 0.0, saved.width, saved.height))
}

pub(crate) fn bounds_label(path: &PathText, bounds: schist_layout::Rect) -> String {
    let saved = LocalBounds {
        geometry: crate::export::path_geometry(&path.path),
        width: bounds.width,
        height: bounds.height,
    };
    let value = crate::export::escape(&serde_json::to_string(&saved).unwrap());
    format!(r#"<Label><KeyValuePair Key="{ORIGIN}" Value="{value}"/></Label>"#)
}

pub(crate) fn reference(element: &Element) -> Option<&Element> {
    if element.name == "TextFrame" {
        Some(element)
    } else {
        element.child("TextPath")
    }
}

pub(crate) fn read(element: &Element, report: &mut Report) -> Option<PathText> {
    let native = element.child("TextPath")?;
    // A native item may have a box/image body and an additional path story.
    // The current model has one content container. Keep its primary content
    // and diagnose the extra path instead of attaching the wrong story.
    if element.name == "TextFrame" || element.child("Image").is_some() {
        report.skip(schist_i18n::tf!(
            "design.idml_text_path_unsupported",
            value = if element.name == "TextFrame" {
                "TextFrame/TextPath"
            } else {
                "Image/TextPath"
            }
        ));
        return None;
    }
    for (key, default) in [
        ("PathAlignment", "CenterPathAlignment"),
        ("TextAlignment", "BaselineTextAlignment"),
        ("PathEffect", "RainbowPathEffect"),
        ("FlipPathEffect", "NotFlipped"),
    ] {
        if native.attr(key).is_some_and(|v| v != default) {
            report.skip(schist_i18n::tf!(
                "design.idml_text_path_unsupported",
                value = key
            ));
        }
    }
    if native.attr("PathSpacing").is_some() && native.number("PathSpacing") != Some(0.0) {
        report.skip(schist_i18n::tf!(
            "design.idml_text_path_unsupported",
            value = "PathSpacing"
        ));
    }
    if element.children_named("TextPath").count() != 1 {
        report.skip(schist_i18n::tf!(
            "design.idml_text_path_unsupported",
            value = "TextPath"
        ));
    }
    let Some(geometry) = crate::import::path_of(element) else {
        report.skip(schist_i18n::t("design.idml_text_path_invalid"));
        return None;
    };
    let mut path = PathText {
        path: geometry,
        start: native
            .number("StartBracket")
            .filter(|v| v.is_finite())
            .unwrap_or(0.0),
        end: native.number("EndBracket").filter(|v| v.is_finite()),
    };
    // Follow-end intent survives only while the serialized native bracket is
    // unchanged. External bracket edits supersede the saved authoring choice.
    if native.attr("EndBracket").is_some()
        && crate::auto_direction::label(native, FOLLOW_END) == native.attr("EndBracket")
    {
        path.end = None;
    }
    if path.engine_path().is_none()
        || ["StartBracket", "EndBracket"].iter().any(|key| {
            native.attr(key).is_some() && native.number(key).is_none_or(|v| !v.is_finite())
        })
    {
        report.skip(schist_i18n::t("design.idml_text_path_invalid"));
    }
    Some(path)
}

pub(crate) fn frame_id(frame: &PlacedObject) -> String {
    let id = format!("u{:x}", 0x8000 + frame.id.0);
    if matches!(
        frame.object,
        LayoutObject::TextFrame {
            text_path: Some(_),
            ..
        }
    ) {
        format!("{id}_textpath")
    } else {
        id
    }
}

pub(crate) fn write(
    path: &PathText,
    id: &str,
    story: &str,
    previous: &str,
    next: &str,
    warnings: &mut Vec<String>,
) -> String {
    let end = path
        .end
        .or_else(|| path.engine_path().and_then(|p| p.length()))
        .unwrap_or(0.0);
    if path.engine_path().is_none() {
        warnings.push(schist_i18n::t("design.idml_text_path_invalid").into());
    }
    let end = crate::export::number(end);
    let label = if path.end.is_none() {
        format!(
            r#"<Properties><Label><KeyValuePair Key="{FOLLOW_END}" Value="{end}"/></Label></Properties>"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<TextPath Self="{id}_textpath" ParentStory="{story}" PreviousTextFrame="{previous}" NextTextFrame="{next}" PathAlignment="CenterPathAlignment" TextAlignment="BaselineTextAlignment" PathEffect="RainbowPathEffect" FlipPathEffect="NotFlipped" PathSpacing="0" StartBracket="{}" EndBracket="{end}">{label}</TextPath>"#,
        crate::export::number(path.start)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::{Point, Rect, ShapePath, SubPath};

    #[test]
    fn local_bounds_metadata_yields_to_every_native_geometry_edit() {
        let path = PathText {
            path: ShapePath {
                subpaths: vec![SubPath {
                    points: vec![Point::new(0.0, 0.0), Point::new(200.0, 30.0)],
                    handles: Vec::new(),
                    closed: false,
                }],
                even_odd: false,
            },
            start: 0.0,
            end: None,
        };
        let bounds = Rect::new(0.0, 0.0, 200.0, 30.0);
        let label = bounds_label(&path, bounds);
        for shift in [0.0, -0.01, 0.01, 7.0, 100.0] {
            for closed in [false, true] {
                let mut changed = path.path.clone();
                changed.subpaths[0].points[0].x += shift;
                changed.subpaths[0].closed = closed;
                let element = crate::xml::parse(&format!(
                    "<Polygon><Properties>{}{label}</Properties></Polygon>",
                    crate::export::path_geometry(&changed)
                ))
                .unwrap();
                assert_eq!(
                    retained_bounds(&element),
                    (shift == 0.0 && !closed).then_some(bounds)
                );
            }
        }
        for value in [-1.0, f32::NAN, f32::INFINITY] {
            let invalid = bounds_label(&path, Rect::new(0.0, 0.0, value, 30.0));
            let element = crate::xml::parse(&format!(
                "<Polygon><Properties>{}{invalid}</Properties></Polygon>",
                crate::export::path_geometry(&path.path)
            ))
            .unwrap();
            assert!(retained_bounds(&element).is_none());
        }
    }
}
