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
use schist_layout::{LayoutDocument, StoryId};

/// Page-item kinds a story can anchor that Schist draws itself.
const KINDS: [&str; 6] = [
    "Rectangle",
    "Oval",
    "Polygon",
    "GraphicLine",
    "TextFrame",
    "Group",
];

/// Page items a group can hold that Schist draws.
const MEMBERS: [&str; 5] = ["Rectangle", "Oval", "Polygon", "GraphicLine", "TextFrame"];

/// Type the anchored items of every story. `stories` are the package's
/// stories by native id, in document order. Items restored from Schist's own
/// retention record are already typed.
pub(crate) fn read(
    document: &mut LayoutDocument,
    stories: &[(String, schist_layout::Story)],
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
            structure.anchored = item(&element, stories, colors, &mut assets, report).map(Box::new);
        }
    }
    document.assets = assets;
    untype_cycles(document, report);
}

/// An anchored text frame whose story leads back to the story holding it
/// would compose forever; it is reported and left untyped.
fn untype_cycles(document: &mut LayoutDocument, report: &mut Report) {
    for host in 0..document.stories.len() {
        for index in 0..document.stories[host].structures.len() {
            let structure = &document.stories[host].structures[index];
            let Some(item) = &structure.anchored else {
                continue;
            };
            if !schist_layout::anchored::reaches(document, item, StoryId(host as u32)) {
                continue;
            }
            report.skip(schist_i18n::tf!(
                "design.idml_anchored_invalid",
                name = item.object.name,
                property = "ParentStory",
                value = xml::parse(&structure.payload)
                    .ok()
                    .and_then(|e| e.attr("ParentStory").map(str::to_owned))
                    .unwrap_or_default()
            ));
            document.stories[host].structures[index].anchored = None;
        }
    }
}

fn item(
    element: &xml::Element,
    stories: &[(String, schist_layout::Story)],
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
    let (object, members) = if element.name == "Group" {
        group(element, stories, colors, assets, report)?
    } else {
        let object = crate::import::placed_object(
            element,
            stories,
            crate::import::Transform::default(),
            colors,
            report,
            assets,
        )?;
        (object, Vec::new())
    };
    Some(AnchoredItem {
        position,
        y_offset,
        placement,
        object,
        members,
    })
}

/// An anchored group: its page items flattened through nested groups, as
/// spread groups are, and a container spanning them that carries the
/// group's name and text wrap.
fn group(
    element: &xml::Element,
    stories: &[(String, schist_layout::Story)],
    colors: &crate::color_codec::Colors,
    assets: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    report: &mut Report,
) -> Option<(
    schist_layout::PlacedObject,
    Vec<schist_layout::PlacedObject>,
)> {
    let mut members = Vec::new();
    flatten(
        element,
        crate::import::transform(element.attr("ItemTransform")),
        1.0,
        false,
        &mut Flatten {
            stories,
            colors,
            assets,
            report,
            out: &mut members,
        },
    );
    let bounds = members
        .iter()
        .map(|m| m.visual_bounds())
        .reduce(|a, b| a.union(b))?;
    let mut container = members[0].clone();
    container.id = schist_layout::ObjectId::next();
    container.object = schist_layout::LayoutObject::Group {
        children: Vec::new(),
    };
    container.bounds = bounds;
    container.rotation = 0.0;
    container.transform = schist_layout::affine::Affine::IDENTITY;
    container.name = element.attr("Name").unwrap_or_default().to_owned();
    container.hidden = false;
    container.transparency = 1.0;
    container.appearance = Default::default();
    container.appearance.text_wrap = crate::text_wrap_codec::read(element, &container.name, report);
    Some((container, members))
}

struct Flatten<'a, 'b> {
    stories: &'a [(String, schist_layout::Story)],
    colors: &'a crate::color_codec::Colors,
    assets: &'b mut std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    report: &'b mut Report,
    out: &'b mut Vec<schist_layout::PlacedObject>,
}

fn flatten(
    element: &xml::Element,
    transform: crate::import::Transform,
    opacity: f32,
    hidden: bool,
    to: &mut Flatten<'_, '_>,
) {
    let opacity = opacity * crate::color_codec::opacity(element);
    let hidden = hidden || element.boolean("Visible") == Some(false);
    for child in &element.children {
        if child.name == "Group" {
            let inner = crate::import::transform(child.attr("ItemTransform")).then(transform);
            flatten(child, inner, opacity, hidden, to);
        } else if MEMBERS.contains(&child.name.as_str()) {
            if let Some(mut placed) = crate::import::placed_object(
                child, to.stories, transform, to.colors, to.report, to.assets,
            ) {
                placed.transparency *= opacity;
                placed.hidden |= hidden;
                to.out.push(placed);
            }
        }
    }
}

fn invalid(report: &mut Report, element: &xml::Element, property: &str, value: &str) {
    report.skip(schist_i18n::tf!(
        "design.idml_anchored_invalid",
        name = element.attr("Name").unwrap_or_default(),
        property = property,
        value = value
    ));
}
