//! Automatic lists from the public IDML paragraph/style and designmap schemas.
use crate::{
    designmap::DesignPackage,
    import::Report,
    style_codec,
    xml::{self, Element},
    Error,
};
use schist_layout::lists::{
    BulletSymbol, ListKind, ListStyle, ListTab, MarkerAlignment, NumberingFormat, NumberingList,
    RestartPolicy,
};

fn warn(report: &mut Report, property: &str) {
    let message = schist_i18n::tf!("design.idml_list_unsupported", value = property);
    if !report.skipped.contains(&message) {
        report.skip(message);
    }
}
fn boolean(element: &Element, key: &str, report: &mut Report) -> Option<bool> {
    match element.attr(key)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => {
            warn(report, key);
            None
        }
    }
}
fn unsigned(element: &Element, key: &str, report: &mut Report) -> Option<u32> {
    let value = element.attr(key)?;
    match value.parse() {
        Ok(v) => Some(v),
        Err(_) => {
            warn(report, key);
            None
        }
    }
}
fn alignment(element: &Element, key: &str, report: &mut Report) -> Option<MarkerAlignment> {
    match element.attr(key)? {
        "LeftAlign" => Some(MarkerAlignment::Left),
        "CenterAlign" => Some(MarkerAlignment::Center),
        "RightAlign" => Some(MarkerAlignment::Right),
        _ => {
            warn(report, key);
            None
        }
    }
}
pub(crate) fn read(
    element: &Element,
    refs: &style_codec::References,
    report: &mut Report,
) -> ListStyle {
    let property = |key| style_codec::property(element, key).map(str::to_owned);
    let character = |key| style_codec::property(element, key).map(|v| refs.character(v));
    let props = element.child("Properties");
    let bullet = props.and_then(|p| p.child("BulletChar")).and_then(|b| {
        Some(BulletSymbol {
            kind: b.attr("BulletCharacterType")?.into(),
            value: unsigned(b, "BulletCharacterValue", report)?,
        })
    });
    if bullet.as_ref().is_some_and(|b| b.character().is_none()) {
        warn(report, "BulletChar");
    }
    let restart_policy = props
        .and_then(|p| p.child("NumberingRestartPolicies"))
        .map(|r| RestartPolicy {
            policy: r.attr("RestartPolicy").unwrap_or_default().into(),
            lower: unsigned(r, "LowerLevel", report).unwrap_or(0),
            upper: unsigned(r, "UpperLevel", report).unwrap_or(0),
        });
    let tabs = props.and_then(|p| p.child("TabList")).map(|tabs| {
        tabs.children
            .iter()
            .filter(|t| t.name == "ListItem")
            .filter_map(|t| {
                let position = t
                    .child("Position")
                    .and_then(|p| p.trimmed().parse::<f32>().ok());
                let Some(position) = position.filter(|p| p.is_finite()) else {
                    warn(report, "TabList.Position");
                    return None;
                };
                let get = |key: &str, default: &str| {
                    t.child(key)
                        // Leader and alignment characters are literal strings;
                        // spaces can be part of the repeating leader pattern.
                        .map(|e| {
                            if key == "Alignment" {
                                e.trimmed()
                            } else {
                                e.text.as_str()
                            }
                        })
                        .unwrap_or(default)
                        .to_owned()
                };
                Some(ListTab {
                    position,
                    alignment: get("Alignment", "LeftAlign"),
                    alignment_character: get("AlignmentCharacter", "."),
                    leader: get("Leader", ""),
                })
            })
            .collect()
    });
    ListStyle {
        kind: element
            .attr("BulletsAndNumberingListType")
            .and_then(|v| match v {
                "NoList" => Some(ListKind::None),
                "BulletList" => Some(ListKind::Bullet),
                "NumberedList" => Some(ListKind::Numbered),
                _ => {
                    warn(report, "BulletsAndNumberingListType");
                    None
                }
            }),
        bullet,
        tabs,
        restart_policy,
        start: unsigned(element, "NumberingStartAt", report),
        level: unsigned(element, "NumberingLevel", report),
        continue_numbering: boolean(element, "NumberingContinue", report),
        apply_restart_policy: boolean(element, "NumberingApplyRestartPolicy", report),
        list: property("AppliedNumberingList"),
        format: props
            .and_then(|p| p.child("NumberingFormat"))
            .map(|format| {
                // String properties are literal data, including surrounding
                // whitespace. The generic property helper trims references.
                if format.attr("type") == Some("enumeration") {
                    NumberingFormat::Enumeration {
                        enumeration: format.text.clone(),
                    }
                } else {
                    NumberingFormat::Named(format.text.clone())
                }
            })
            .or_else(|| element.attr("NumberingFormat").map(NumberingFormat::from)),
        expression: property("NumberingExpression"),
        text_after: property("BulletsTextAfter"),
        bullet_character_style: character("BulletsCharacterStyle"),
        numbering_character_style: character("NumberingCharacterStyle"),
        bullet_font: property("BulletsFont"),
        bullet_font_style: property("BulletsFontStyle"),
        bullet_alignment: alignment(element, "BulletsAlignment", report),
        numbering_alignment: alignment(element, "NumberingAlignment", report),
        ..Default::default()
    }
}
fn attr(out: &mut String, key: &str, value: impl ToString) {
    out.push_str(&format!(
        " {key}=\"{}\"",
        style_codec::escape(&value.to_string())
    ));
}
fn optional(out: &mut String, key: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        attr(out, key, value);
    }
}
pub(crate) fn attributes(out: &mut String, list: &ListStyle) {
    optional(
        out,
        "BulletsAndNumberingListType",
        list.kind.map(|v| match v {
            ListKind::None => "NoList",
            ListKind::Bullet => "BulletList",
            ListKind::Numbered => "NumberedList",
        }),
    );
    optional(out, "NumberingStartAt", list.start);
    optional(out, "NumberingLevel", list.level);
    optional(out, "NumberingContinue", list.continue_numbering);
    optional(
        out,
        "NumberingApplyRestartPolicy",
        list.apply_restart_policy,
    );
    optional(out, "NumberingExpression", list.expression.as_ref());
    optional(out, "BulletsTextAfter", list.text_after.as_ref());
    for (key, alignment) in [
        ("BulletsAlignment", list.bullet_alignment),
        ("NumberingAlignment", list.numbering_alignment),
    ] {
        optional(
            out,
            key,
            alignment.map(|a| match a {
                MarkerAlignment::Left => "LeftAlign",
                MarkerAlignment::Center => "CenterAlign",
                MarkerAlignment::Right => "RightAlign",
            }),
        );
    }
}
pub(crate) fn properties(out: &mut String, list: &ListStyle) {
    let mut body = String::new();
    for (key, kind, value) in [
        ("AppliedNumberingList", "object", list.list.clone()),
        (
            "NumberingFormat",
            if matches!(list.format, Some(NumberingFormat::Enumeration { .. })) {
                "enumeration"
            } else {
                "string"
            },
            list.format.as_ref().map(|format| format.value().to_owned()),
        ),
        ("BulletsFont", "string", list.bullet_font.clone()),
        ("BulletsFontStyle", "string", list.bullet_font_style.clone()),
        (
            "BulletsCharacterStyle",
            "object",
            list.bullet_character_style
                .as_ref()
                .map(|v| format!("CharacterStyle/$ID/{v}")),
        ),
        (
            "NumberingCharacterStyle",
            "object",
            list.numbering_character_style
                .as_ref()
                .map(|v| format!("CharacterStyle/$ID/{v}")),
        ),
    ] {
        if let Some(value) = value {
            let kind = if (key == "BulletsFontStyle"
                && matches!(value.as_str(), "Nothing" | "Auto"))
                || (key == "BulletsFont" && value == "Auto")
            {
                "enumeration"
            } else {
                kind
            };
            body.push_str(&format!(
                "<{key} type=\"{kind}\">{}</{key}>",
                style_codec::escape(&value)
            ));
        }
    }
    if let Some(bullet) = &list.bullet {
        body.push_str("<BulletChar");
        attr(&mut body, "BulletCharacterType", &bullet.kind);
        attr(&mut body, "BulletCharacterValue", bullet.value);
        body.push_str("/>");
    }
    if let Some(policy) = &list.restart_policy {
        body.push_str("<NumberingRestartPolicies");
        attr(&mut body, "RestartPolicy", &policy.policy);
        attr(&mut body, "LowerLevel", policy.lower);
        attr(&mut body, "UpperLevel", policy.upper);
        body.push_str("/>");
    }
    if let Some(tabs) = &list.tabs {
        body.push_str("<TabList type=\"list\">");
        for tab in tabs {
            if !tab.position.is_finite() {
                continue;
            }
            body.push_str("<ListItem type=\"record\">");
            for (key, kind, value) in [
                ("Alignment", "enumeration", tab.alignment.clone()),
                (
                    "AlignmentCharacter",
                    "string",
                    tab.alignment_character.clone(),
                ),
                ("Leader", "string", tab.leader.clone()),
                ("Position", "unit", tab.position.to_string()),
            ] {
                body.push_str(&format!(
                    "<{key} type=\"{kind}\">{}</{key}>",
                    style_codec::escape(&value)
                ));
            }
            body.push_str("</ListItem>");
        }
        body.push_str("</TabList>");
    }
    if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &body);
    }
}
pub(crate) fn read_resources(
    opened: &DesignPackage<'_>,
    report: &mut Report,
) -> Result<Vec<NumberingList>, Error> {
    let root = xml::parse(opened.text_of(&opened.root)?).map_err(|message| Error::Xml {
        part: opened.root.clone(),
        message,
    })?;
    Ok(root
        .find_all("NumberingList")
        .into_iter()
        .filter_map(|r| {
            Some(NumberingList {
                id: r.attr("Self")?.into(),
                name: r.attr("Name").unwrap_or_default().into(),
                across_stories: boolean(r, "ContinueNumbersAcrossStories", report).unwrap_or(false),
                across_documents: boolean(r, "ContinueNumbersAcrossDocuments", report)
                    .unwrap_or(false),
                labels: r
                    .child("Properties")
                    .and_then(|p| p.child("Label"))
                    .map_or_else(Vec::new, |l| {
                        l.children
                            .iter()
                            .filter_map(|e| Some((e.attr("Key")?.into(), e.attr("Value")?.into())))
                            .collect()
                    }),
            })
        })
        .collect())
}
pub(crate) fn resources(lists: &[NumberingList]) -> String {
    let mut out = String::new();
    for list in lists {
        out.push_str("<NumberingList");
        attr(&mut out, "Self", &list.id);
        attr(&mut out, "Name", &list.name);
        attr(
            &mut out,
            "ContinueNumbersAcrossStories",
            list.across_stories,
        );
        attr(
            &mut out,
            "ContinueNumbersAcrossDocuments",
            list.across_documents,
        );
        out.push_str("><Properties><Label>");
        for (key, value) in &list.labels {
            out.push_str("<KeyValuePair");
            attr(&mut out, "Key", key);
            attr(&mut out, "Value", value);
            out.push_str("/>");
        }
        out.push_str("</Label></Properties></NumberingList>");
    }
    out
}

/// Diagnostics follow resolved active paragraphs, so inherited defaults that
/// are unused do not produce warnings about a list which is not on the page.
pub(crate) fn diagnostics(doc: &schist_layout::LayoutDocument) -> Vec<String> {
    let mut report = Report::default();
    for (index, story) in doc.stories.iter().enumerate() {
        let counters = schist_layout::list_counters::StoryCounters::new(doc, story);
        let has_text_path = doc.objects.iter().chain(doc.parents.iter().flat_map(|p| p.objects.iter().map(|o| &o.object))).any(|o| matches!(o.object, schist_layout::LayoutObject::TextFrame { story, text_path: Some(_), .. } if story.0 as usize == index));
        for (point, at) in story.points.iter().zip(story.point_offsets()) {
            let schist_layout::StoryPoint::Paragraph { style, text } = point else {
                continue;
            };
            let paragraph = doc.styles.resolve_paragraph(style);
            let list = &paragraph.list;

            for property in schist_layout::tabs::unsupported_in_mode(
                &paragraph,
                text,
                has_text_path,
                schist_layout::compose::writing_mode_at(story, at, doc),
            ) {
                let message = schist_i18n::tf!("design.idml_tabs_unsupported", value = property);
                if !report.skipped.contains(&message) {
                    report.skip(message);
                }
            }
            if text.contains('\t') && list.tabs.as_ref().is_none_or(|tabs| tabs.is_empty()) {
                let message = schist_i18n::t("design.idml_tabs_implicit").to_string();
                if !report.skipped.contains(&message) {
                    report.skip(message);
                }
            }

            if !matches!(list.kind, Some(ListKind::Bullet | ListKind::Numbered)) {
                continue;
            }
            if list.legacy_gap.is_some() {
                warn(&mut report, "Schist.List.legacy_gap");
            }
            for property in schist_layout::list_composition::unsupported(list) {
                warn(&mut report, property);
            }
            for property in schist_layout::list_composition::unsupported_paragraph(&paragraph, text)
            {
                warn(&mut report, property);
            }
            if let Some(property) = counters.issue(at) {
                warn(&mut report, property);
            }
            if paragraph.drop_caps_lines.unwrap_or(0) > 1 {
                warn(&mut report, "DropCapLines + BulletsAndNumberingListType");
            }
            if story.prefs.orientation == schist_layout::StoryOrientation::Vertical
                || matches!(
                    paragraph.writing_mode,
                    Some(
                        schist_layout::WritingMode::VerticalRightToLeft
                            | schist_layout::WritingMode::VerticalLeftToRight
                    )
                )
            {
                warn(
                    &mut report,
                    "StoryOrientation + BulletsAndNumberingListType",
                );
            }
            if has_text_path {
                warn(&mut report, "TextPath + BulletsAndNumberingListType");
            }
            if let Some(resource) = doc.styles.numbering_lists.iter().find(|r| {
                list.kind == Some(ListKind::Numbered)
                    && r.id == schist_layout::list_counters::sequence_id(list)
            }) {
                if resource.across_stories && counters.issue(at).is_none() {
                    let message = schist_i18n::t("design.idml_cross_story_order").to_string();
                    if !report.skipped.contains(&message) {
                        report.skip(message);
                    }
                }
                if resource.across_documents {
                    warn(&mut report, "ContinueNumbersAcrossDocuments");
                }
            }
            if list.tabs.as_ref().is_none_or(|tabs| tabs.is_empty()) && list.legacy_gap.is_none() {
                let value = if list.kind == Some(ListKind::Bullet) {
                    list.text_after.as_deref().unwrap_or("^t")
                } else {
                    list.expression.as_deref().unwrap_or("^#.^t")
                };
                if value.ends_with("^t") {
                    let message = schist_i18n::t("design.idml_list_implicit_tabs").to_string();
                    if !report.skipped.contains(&message) {
                        report.skip(message);
                    }
                }
            }
        }
    }
    report.skipped
}

const LEGACY_LABEL: &str = "Schist.List.v1";
#[derive(serde::Serialize, serde::Deserialize)]
struct Authored {
    list: ListStyle,
    bullet: Option<schist_layout::styles::Bullet>,
    native: ListStyle,
}
pub(crate) fn native(style: &schist_layout::ParagraphStyle) -> ListStyle {
    let mut list = style.list.over(&ListStyle::from_legacy(style.bullet));
    list.legacy_gap = None;
    list
}
pub(crate) fn restore(
    element: &Element,
    list: ListStyle,
) -> (ListStyle, Option<schist_layout::styles::Bullet>) {
    if let Some(saved) = crate::auto_direction::label(element, LEGACY_LABEL)
        .and_then(|v| serde_json::from_str::<Authored>(v).ok())
        .filter(|v| v.native == list)
    {
        return (saved.list, saved.bullet);
    }
    (list, None)
}
pub(crate) fn label(out: &mut String, style: &schist_layout::ParagraphStyle) {
    if style.bullet.is_none() && style.list.legacy_gap.is_none() {
        return;
    }
    let value = serde_json::to_string(&Authored {
        list: style.list.clone(),
        bullet: style.bullet,
        native: native(style),
    });
    let Ok(value) = value else {
        return;
    };
    let pair = format!(
        "<KeyValuePair Key=\"{LEGACY_LABEL}\" Value=\"{}\"/>",
        style_codec::escape(&value)
    );
    if let Some(at) = out.find("</Label>") {
        out.insert_str(at, &pair);
    } else if let Some(at) = out.find("</Properties>") {
        out.insert_str(at, &format!("<Label>{pair}</Label>"));
    }
}
