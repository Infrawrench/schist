//! Native TextPath children use their own IDs in mixed path/box threads.
use crate::{import::Report, xml::Element};
use schist_layout::{text_path::PathText, LayoutObject, PlacedObject};

const FOLLOW_END: &str = "Schist.TextPath.FollowEnd.v1";

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
