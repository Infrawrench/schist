//! Page items anchored inside stories. Their AnchoredObjectSetting and drawn
//! geometry are typed for composition; the retained XML stays the saved form.
//! AnchoredPosition and AnchorYoffset follow the public IDML specification;
//! an absent position is inline.
use crate::{import::Report, xml};
use schist_layout::anchored::{AnchoredItem, AnchoredPosition};
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
    let position = match setting.and_then(|s| s.attr("AnchoredPosition")) {
        None | Some("InlinePosition") => AnchoredPosition::Inline,
        Some("AboveLine") => AnchoredPosition::AboveLine,
        Some("Anchored") => AnchoredPosition::Anchored,
        Some(other) => {
            invalid(report, element, "AnchoredPosition", other);
            return None;
        }
    };
    let y_offset = match setting.and_then(|s| s.attr("AnchorYoffset")) {
        None => 0.0,
        Some(raw) => match xml::parse_number(raw) {
            Some(value) if value.is_finite() && value.abs() <= 10_000.0 => value,
            _ => {
                invalid(report, element, "AnchorYoffset", raw);
                return None;
            }
        },
    };
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
