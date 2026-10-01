//! DocumentPreference, TextPreference and Section follow the public IDML specification
//! (sections 6.3.21 and the designmap Section schema). Bleed/slug are
//! document-wide in IDML; layout pages store physical four-sided offsets.
use crate::{
    designmap::{DesignPackage, PartKind},
    error::Error,
    import::Report,
    xml::{self, Element},
};
use schist_layout::{Insets, LayoutDocument, NumberStyle, Section};

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

fn offset(element: &Element, keys: &[&str; 4], uniform: &str, report: &mut Report) -> Insets {
    let values = keys.map(|key| element.number(key).unwrap_or(0.0));
    if values.iter().any(|v| !v.is_finite() || *v < 0.0)
        || keys
            .iter()
            .any(|key| element.attr(key).is_some() && element.number(key).is_none())
    {
        report.skip(schist_i18n::t("design.idml_invalid_page_offsets"));
    }
    let values = values.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    if matches!(element.attr(uniform), Some("true" | "1")) {
        return values[0].into();
    }
    Insets::new(values[0], values[3], values[1], values[2])
}

pub fn read(
    opened: &DesignPackage<'_>,
    document: &mut LayoutDocument,
    report: &mut Report,
) -> Result<(), Error> {
    let mut offsets = None;
    for part in opened.listed.iter().filter(|p| p.role == "Preferences") {
        let root = xml::parse(opened.text_of(&part.name)?).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        if let Some(prefs) = root.find("TextPreference") {
            let text = &mut document.styles.text_preferences;
            for (key, target, range) in [
                ("SmallCap", &mut text.small_cap_size, 1.0..=200.0),
                ("SuperscriptSize", &mut text.superscript_size, 1.0..=200.0),
                (
                    "SuperscriptPosition",
                    &mut text.superscript_position,
                    -500.0..=500.0,
                ),
                ("SubscriptSize", &mut text.subscript_size, 1.0..=200.0),
                (
                    "SubscriptPosition",
                    &mut text.subscript_position,
                    -500.0..=500.0,
                ),
            ] {
                if let Some(raw) = prefs.attr(key) {
                    match raw.parse::<f32>() {
                        Ok(value) if value.is_finite() && range.contains(&value) => *target = value,
                        _ => report.skip(schist_i18n::tf!(
                            "design.idml_text_preference_invalid",
                            property = key,
                            value = raw
                        )),
                    }
                }
            }
        }
        if let Some(prefs) = root.find("DocumentPreference") {
            document.facing_pages = prefs.attr("FacingPages") == Some("true");
            document.page_binding = if prefs.attr("PageBinding") == Some("RightToLeft") {
                schist_layout::PageBinding::RightToLeft
            } else {
                schist_layout::PageBinding::LeftToRight
            };
            let bleed = offset(prefs, &BLEED, "DocumentBleedUniformSize", report);
            let slug = offset(prefs, &SLUG, "DocumentSlugUniformSize", report);
            offsets = Some((bleed, slug));
        }
    }
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    // Match the same valid pages and XML reading order as read_spreads. Spread
    // slots may be sorted physically (including RTL); Section.PageStart is an
    // object reference, never a display label or a position in designmap.xml.
    let mut page_ids = Vec::new();
    for part in opened.listed_of(PartKind::Spread) {
        let root = xml::parse(opened.text_of(&part.name)?).map_err(|message| Error::Xml {
            part: part.name.clone(),
            message,
        })?;
        if let Some(spread) = root.find("Spread") {
            for page in spread.children_named("Page") {
                if crate::import::page_of(page).is_some() {
                    page_ids.push(page.attr("Self").map(str::to_owned));
                }
            }
        }
    }
    let mut sections = std::collections::BTreeMap::new();
    for element in root.children_named("Section") {
        let page = element
            .attr("PageStart")
            .and_then(|id| page_ids.iter().position(|p| p.as_deref() == Some(id)));
        let Some(page) = page.filter(|p| *p < document.pages.len()) else {
            report.skip(schist_i18n::t("design.idml_invalid_section"));
            continue;
        };
        if sections.contains_key(&page) {
            report.skip(schist_i18n::t("design.idml_invalid_section"));
            continue;
        }
        let start = element
            .attr("PageNumberStart")
            .and_then(|v| v.parse::<u32>().ok());
        if element.attr("PageNumberStart").is_some()
            && start.is_none_or(|v| !(1..=999999).contains(&v))
        {
            report.skip(schist_i18n::t("design.idml_number_start"));
        }
        let style = element
            .child("Properties")
            .and_then(|p| p.child("PageNumberStyle"))
            .map(Element::trimmed)
            .or_else(|| element.attr("PageNumberStyle"));
        if element
            .attr("AlternateLayout")
            .is_some_and(|v| !v.is_empty())
            || element
                .attr("PaginationMaster")
                .is_some_and(|v| !v.is_empty() && v != "n")
        {
            report.skip(schist_i18n::t("design.idml_alternate_sections"));
        }
        let section = Section {
            start: start.filter(|v| (1..=999999).contains(v)).unwrap_or(1),
            continue_numbering: !matches!(element.attr("ContinueNumbering"), Some("false" | "0")),
            include_prefix: matches!(element.attr("IncludeSectionPrefix"), Some("true" | "1")),
            prefix: element.attr("SectionPrefix").unwrap_or_default().into(),
            name: element.attr("Name").unwrap_or_default().into(),
            marker: element.attr("Marker").unwrap_or_default().into(),
            style: match style.unwrap_or("Arabic") {
                "Arabic" => NumberStyle::Arabic,
                "LowerRoman" => NumberStyle::RomanLower,
                "UpperRoman" => NumberStyle::RomanUpper,
                "LowerLetters" => NumberStyle::AlphaLower,
                "UpperLetters" => NumberStyle::AlphaUpper,
                _ => {
                    report.skip(schist_i18n::t("design.idml_number_style"));
                    NumberStyle::Arabic
                }
            },
        };
        document.pages[page].section =
            (page != 0 || section != Section::default()).then_some(section);
        sections.insert(page, element);
    }
    // Section lengths are derived from the next boundary, so page edits cannot
    // leave stale ranges. Diagnose inconsistent source lengths before repairing.
    let starts: Vec<_> = sections
        .keys()
        .copied()
        .chain(std::iter::once(document.pages.len()))
        .collect();
    for pair in starts.windows(2) {
        if sections[&pair[0]]
            .attr("Length")
            .is_some_and(|v| v.parse::<usize>().ok() != Some(pair[1] - pair[0]))
        {
            report.skip(schist_i18n::t("design.idml_invalid_section"));
        }
    }
    if let Some((bleed, slug)) = offsets {
        for i in 0..document.pages.len() {
            document.pages[i].bleed = physical_offsets(document, i, bleed);
            document.pages[i].slug = physical_offsets(document, i, slug);
        }
    }
    Ok(())
}

// Mirroring is its own inverse: native inside/outside to physical left/right,
// or physical page offsets back to native document preferences.
fn physical_offsets(document: &LayoutDocument, page: usize, offsets: Insets) -> Insets {
    if document.facing_pages && document.page_is_left(page) {
        offsets.mirrored()
    } else {
        offsets
    }
}

pub fn preferences(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    use crate::export::number;
    let (width, height) = document
        .pages
        .first()
        .map(|p| (p.width, p.height))
        .unwrap_or((595.2756, 841.8898));
    let offsets: Vec<_> = document
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                physical_offsets(document, i, p.bleed),
                physical_offsets(document, i, p.slug),
            )
        })
        .collect();
    let bleed = offsets.iter().map(|p| p.0).fold(Insets::ZERO, Insets::max);
    let slug = offsets.iter().map(|p| p.1).fold(Insets::ZERO, Insets::max);
    if offsets.iter().any(|p| p.0 != bleed || p.1 != slug) {
        warnings.push(schist_i18n::t("design.idml_per_page_offsets").into());
    }
    let mut out = format!(
        r#"<DocumentPreference PageWidth="{}" PageHeight="{}" PagesPerDocument="{}" FacingPages="{}" DocumentBleedUniformSize="{}" DocumentSlugUniformSize="{}" PageBinding="{}""#,
        number(width),
        number(height),
        document.pages.len(),
        document.facing_pages,
        bleed.is_uniform(),
        slug.is_uniform(),
        match document.page_binding {
            schist_layout::PageBinding::LeftToRight => "LeftToRight",
            schist_layout::PageBinding::RightToLeft => "RightToLeft",
        }
    );
    for (keys, offsets) in [(BLEED, bleed), (SLUG, slug)] {
        for (key, value) in
            keys.into_iter()
                .zip([offsets.top, offsets.bottom, offsets.left, offsets.right])
        {
            out.push_str(&format!(r#" {key}="{}""#, number(value)));
        }
    }
    out.push_str(" /><TextPreference");
    let text = document.styles.text_preferences;
    let default = schist_layout::styles::TextPreferences::default();
    for (key, value, fallback, range) in [
        (
            "SmallCap",
            text.small_cap_size,
            default.small_cap_size,
            1.0..=200.0,
        ),
        (
            "SuperscriptSize",
            text.superscript_size,
            default.superscript_size,
            1.0..=200.0,
        ),
        (
            "SuperscriptPosition",
            text.superscript_position,
            default.superscript_position,
            -500.0..=500.0,
        ),
        (
            "SubscriptSize",
            text.subscript_size,
            default.subscript_size,
            1.0..=200.0,
        ),
        (
            "SubscriptPosition",
            text.subscript_position,
            default.subscript_position,
            -500.0..=500.0,
        ),
    ] {
        let value = if value.is_finite() && range.contains(&value) {
            value
        } else {
            warnings.push(schist_i18n::tf!(
                "design.idml_text_preference_invalid",
                property = key,
                value = value
            ));
            fallback
        };
        out.push_str(&format!(r#" {key}="{}""#, number(value)));
    }
    out.push_str(" />");
    out
}

pub fn section(document: &LayoutDocument, warnings: &mut Vec<String>) -> String {
    let starts: Vec<_> = document
        .pages
        .iter()
        .enumerate()
        .filter_map(|(i, page)| (i == 0 || page.section.is_some()).then_some(i))
        .chain(std::iter::once(document.pages.len()))
        .collect();
    let mut out = String::new();
    for pair in starts.windows(2) {
        let page = pair[0];
        let section = document.pages[page].section.clone().unwrap_or_default();
        let start = section.start.clamp(1, 999999);
        if start != section.start {
            warnings.push(schist_i18n::t("design.idml_number_start").into());
        }
        let style = match section.style {
            NumberStyle::Arabic => "Arabic",
            NumberStyle::RomanLower => "LowerRoman",
            NumberStyle::RomanUpper => "UpperRoman",
            NumberStyle::AlphaLower => "LowerLetters",
            NumberStyle::AlphaUpper => "UpperLetters",
        };
        use crate::export::escape;
        out.push_str(&format!(
            r#"<Section Self="SchistSection{page}" Length="{}" ContinueNumbering="{}" IncludeSectionPrefix="{}" PageNumberStart="{start}" PageStart="SchistPage{page}" SectionPrefix="{}" Name="{}" Marker="{}"><Properties><PageNumberStyle type="enumeration">{style}</PageNumberStyle></Properties></Section>"#,
            pair[1] - page, section.continue_numbering, section.include_prefix,
            escape(&section.prefix), escape(&section.name), escape(&section.marker),
        ));
    }
    out
}
