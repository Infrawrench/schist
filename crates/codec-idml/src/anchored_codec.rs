//! Page items anchored inside stories. Their AnchoredObjectSetting and drawn
//! geometry are typed for composition; the retained XML stays the saved form.
//! Attribute names, enumerations and defaults follow the public IDML
//! specification (Appendix C defaults). An invalid position or offset leaves
//! the item untyped; an invalid reference, alignment or flag is reported and
//! read as its default, as InDesign's export of a public sample does for
//! unknown vertical references.
use crate::{import::Report, xml};
use schist_layout::anchored::{
    AnchorPoint, AnchoredItem, AnchoredPosition, HorizontalAlignment, HorizontalReference,
    Placement, VerticalAlignment, VerticalReference,
};
use schist_layout::LayoutDocument;

/// Page-item kinds a story can anchor that Schist draws itself. Anchored text
/// frames and groups stay retained but untyped.
const KINDS: [&str; 4] = ["Rectangle", "Oval", "Polygon", "GraphicLine"];

pub(crate) fn read(
    document: &mut LayoutDocument,
    colors: &crate::color_codec::Colors,
    report: &mut Report,
) {
    let mut assets = std::mem::take(&mut document.assets);
    for story in &mut document.stories {
        for structure in &mut story.structures {
            if structure.anchored.is_some() || !KINDS.contains(&structure.kind.as_str()) {
                continue;
            }
            let Ok(element) = xml::parse(&structure.payload) else {
                continue;
            };
            structure.anchored = item(&element, colors, &mut assets, report).map(Box::new);
        }
    }
    document.assets = assets;
}

fn item(
    element: &xml::Element,
    colors: &crate::color_codec::Colors,
    assets: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    report: &mut Report,
) -> Option<AnchoredItem> {
    let setting = element.child("AnchoredObjectSetting");
    let attr = |name| setting.and_then(|s| s.attr(name));
    let position = match attr("AnchoredPosition") {
        None | Some("InlinePosition") => AnchoredPosition::Inline,
        Some("AboveLine") => AnchoredPosition::AboveLine,
        Some("Anchored") => AnchoredPosition::Anchored,
        Some(other) => {
            invalid(report, element, "AnchoredPosition", other);
            return None;
        }
    };
    let mut offset = |name| match attr(name) {
        None => Some(0.0),
        Some(raw) => match xml::parse_number(raw) {
            Some(value) if value.is_finite() && value.abs() <= 10_000.0 => Some(value),
            _ => {
                invalid(report, element, name, raw);
                None
            }
        },
    };
    let y_offset = offset("AnchorYoffset")?;
    let x_offset = offset("AnchorXoffset")?;
    let space_above = offset("AnchorSpaceAbove")?;
    let mut placement = Placement {
        x_offset,
        space_above,
        ..Placement::default()
    };
    macro_rules! choice {
        ($field:ident, $name:literal, { $($raw:literal => $value:expr),+ $(,)? }) => {
            match attr($name) {
                None => {}
                $(Some($raw) => placement.$field = $value,)+
                Some(other) => invalid(report, element, $name, other),
            }
        };
    }
    choice!(anchor_point, "AnchorPoint", {
        "TopLeftAnchor" => AnchorPoint::TopLeft,
        "TopCenterAnchor" => AnchorPoint::TopCenter,
        "TopRightAnchor" => AnchorPoint::TopRight,
        "LeftCenterAnchor" => AnchorPoint::LeftCenter,
        "CenterAnchor" => AnchorPoint::Center,
        "RightCenterAnchor" => AnchorPoint::RightCenter,
        "BottomLeftAnchor" => AnchorPoint::BottomLeft,
        "BottomCenterAnchor" => AnchorPoint::BottomCenter,
        "BottomRightAnchor" => AnchorPoint::BottomRight,
    });
    choice!(horizontal_alignment, "HorizontalAlignment", {
        "LeftAlign" => HorizontalAlignment::Left,
        "CenterAlign" => HorizontalAlignment::Center,
        "RightAlign" => HorizontalAlignment::Right,
        "TextAlign" => HorizontalAlignment::Text,
    });
    choice!(horizontal_reference, "HorizontalReferencePoint", {
        "TextFrame" => HorizontalReference::TextFrame,
        "ColumnEdge" => HorizontalReference::ColumnEdge,
        "PageMargins" => HorizontalReference::PageMargins,
        "PageEdge" => HorizontalReference::PageEdge,
        "AnchorLocation" => HorizontalReference::AnchorLocation,
    });
    choice!(vertical_alignment, "VerticalAlignment", {
        "TopAlign" => VerticalAlignment::Top,
        "CenterAlign" => VerticalAlignment::Center,
        "BottomAlign" => VerticalAlignment::Bottom,
    });
    choice!(vertical_reference, "VerticalReferencePoint", {
        "LineBaseline" => VerticalReference::LineBaseline,
        "LineXheight" => VerticalReference::LineXHeight,
        "LineAscent" => VerticalReference::LineAscent,
        "Capheight" => VerticalReference::CapHeight,
        "TopOfLeading" => VerticalReference::TopOfLeading,
        "ColumnEdge" => VerticalReference::ColumnEdge,
        "TextFrame" => VerticalReference::TextFrame,
        "PageMargins" => VerticalReference::PageMargins,
        "PageEdge" => VerticalReference::PageEdge,
    });
    choice!(spine_relative, "SpineRelative", { "true" => true, "false" => false });
    choice!(pin_position, "PinPosition", { "true" => true, "false" => false });
    choice!(lock_position, "LockPosition", { "true" => true, "false" => false });
    let object = crate::import::placed_object(
        element,
        &[],
        crate::import::Transform::default(),
        colors,
        report,
        assets,
    )?;
    Some(AnchoredItem {
        position,
        y_offset,
        placement,
        object,
    })
}

fn invalid(report: &mut Report, element: &xml::Element, property: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_anchored_invalid",
        name = element.attr("Name").unwrap_or_default(),
        property = property,
        value = value
    ));
}
