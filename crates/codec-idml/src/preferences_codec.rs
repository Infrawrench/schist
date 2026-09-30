//! DocumentPreference and Section follow the public IDML specification
//! (sections 6.3.21 and the designmap Section schema). Bleed/slug are
//! document-wide in IDML; the layout model currently uses a scalar per page.
use crate::{
    designmap::{DesignPackage, PartKind},
    error::Error,
    import::Report,
    xml::{self, Element},
};
use schist_layout::{LayoutDocument, NumberStyle};

const BLEED: [&str; 4] = [
    "DocumentBleedTopOffset",
    "DocumentBleedBottomOffset",
    "DocumentBleedInsideOrLeftOffset",
    "DocumentBleedOutsideOrRightOffset",
];
const SLUG: [&str; 4] = [
    "SlugTopOffset",
    "SlugBottomOffset",
    "SlugInsideOrLeftOffset",
    "SlugRightOrOutsideOffset",
];

fn offset(element: &Element, keys: &[&str; 4], uniform: &str, report: &mut Report) -> f32 {
    let values = keys.map(|key| element.number(key).unwrap_or(0.0));
    if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        report.skip(schist_i18n::t("design.idml_invalid_page_offsets"));
    }
    let values = values.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    if element.attr(uniform) == Some("true") {
        return values[0];
    }
    if values.iter().any(|v| *v != values[0]) {
        report.skip(schist_i18n::t("design.idml_asymmetric_offsets"));
    }
    // Retain the largest requested extent, and disclose the approximation.
    values.into_iter().fold(0.0, f32::max)
}

pub fn read(
    opened: &DesignPackage<'_>,
    document: &mut LayoutDocument,
    report: &mut Report,
) -> Result<(), Error> {
    for part in opened.listed.iter().filter(|p| p.role == "Preferences") {
        let root = xml::parse(opened.text_of(&part.name)?).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        if let Some(prefs) = root.find("DocumentPreference") {
            document.facing_pages = prefs.attr("FacingPages") == Some("true");
            document.page_binding = if prefs.attr("PageBinding") == Some("RightToLeft") {
                schist_layout::PageBinding::RightToLeft
            } else {
                schist_layout::PageBinding::LeftToRight
            };
            let bleed = offset(prefs, &BLEED, "DocumentBleedUniformSize", report);
            // IDML offsets measure from trim; Page::slug is extra paper
            // beyond bleed. A slug entirely inside bleed adds no extent.
            let slug = (offset(prefs, &SLUG, "DocumentSlugUniformSize", report) - bleed).max(0.0);
            for page in &mut document.pages {
                page.bleed = bleed;
                page.slug = slug;
            }
        }
    }
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    let sections: Vec<_> = root.children_named("Section").collect();
    if sections.len() > 1 {
        report.skip(schist_i18n::t("design.idml_multiple_sections"));
    }
    let Some(section) = sections.first() else {
        return Ok(());
    };
    let first_page = opened
        .listed_of(PartKind::Spread)
        .into_iter()
        .find_map(|part| {
            xml::parse(opened.text_of(&part.name).ok()?)
                .ok()?
                .find("Page")?
                .attr("Self")
                .map(str::to_string)
        });
    if section
        .attr("PageStart")
        .is_some_and(|id| Some(id) != first_page.as_deref())
    {
        report.skip(schist_i18n::t("design.idml_section_start"));
        return Ok(());
    }
    document.page_number_start = section
        .attr("PageNumberStart")
        .and_then(|v| v.parse().ok())
        .filter(|v| (1..=999999).contains(v))
        .unwrap_or(1);
    if section.attr("IncludeSectionPrefix") == Some("true") {
        document.page_number_prefix = section.attr("SectionPrefix").unwrap_or_default().into();
    }
    let style = section
        .child("Properties")
        .and_then(|p| p.child("PageNumberStyle"))
        .map(Element::trimmed)
        .or_else(|| section.attr("PageNumberStyle"));
    document.page_number_style = match style.unwrap_or("Arabic") {
        "Arabic" => NumberStyle::Arabic,
        "LowerRoman" => NumberStyle::RomanLower,
        "UpperRoman" => NumberStyle::RomanUpper,
        "LowerLetters" => NumberStyle::AlphaLower,
        "UpperLetters" => NumberStyle::AlphaUpper,
        _ => {
            report.skip(schist_i18n::t("design.idml_number_style"));
            NumberStyle::Arabic
        }
    };
    Ok(())
}

pub fn preferences(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    use crate::export::number;
    let (width, height) = document
        .pages
        .first()
        .map(|p| (p.width, p.height))
        .unwrap_or((595.2756, 841.8898));
    let bleed = document.pages.iter().map(|p| p.bleed).fold(0.0, f32::max);
    let slug = document.pages.iter().map(|p| p.slug).fold(0.0, f32::max);
    if document
        .pages
        .iter()
        .any(|p| p.bleed != bleed || p.slug != slug)
    {
        warnings.push(schist_i18n::t("design.idml_per_page_offsets").into());
    }
    let mut out = format!(
        r#"<DocumentPreference PageWidth="{}" PageHeight="{}" PagesPerDocument="{}" FacingPages="{}" DocumentBleedUniformSize="true" DocumentSlugUniformSize="true" PageBinding="{}""#,
        number(width),
        number(height),
        document.pages.len(),
        document.facing_pages,
        match document.page_binding {
            schist_layout::PageBinding::LeftToRight => "LeftToRight",
            schist_layout::PageBinding::RightToLeft => "RightToLeft",
        }
    );
    for key in BLEED {
        out.push_str(&format!(r#" {key}="{}""#, number(bleed)));
    }
    for key in SLUG {
        out.push_str(&format!(
            r#" {key}="{}""#,
            number(if slug > 0.0 { bleed + slug } else { 0.0 })
        ));
    }
    out.push_str(" />");
    out
}

pub fn section(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    if document.pages.is_empty() {
        return String::new();
    }
    let start = document.page_number_start.clamp(1, 999999);
    if start != document.page_number_start {
        warnings.push(schist_i18n::t("design.idml_number_start").into());
    }
    let style = match document.page_number_style {
        NumberStyle::Arabic => "Arabic",
        NumberStyle::RomanLower => "LowerRoman",
        NumberStyle::RomanUpper => "UpperRoman",
        NumberStyle::AlphaLower => "LowerLetters",
        NumberStyle::AlphaUpper => "UpperLetters",
    };
    format!(
        r#"<Section Self="SchistSection" Length="{}" ContinueNumbering="false" IncludeSectionPrefix="{}" PageNumberStart="{start}" PageStart="SchistPage0" SectionPrefix="{}"><Properties><PageNumberStyle type="enumeration">{style}</PageNumberStyle></Properties></Section>"#,
        document.pages.len(),
        !document.page_number_prefix.is_empty(),
        crate::export::escape(&document.page_number_prefix)
    )
}
