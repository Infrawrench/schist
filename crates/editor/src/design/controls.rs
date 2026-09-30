//! Field targets are captured on focus. Typing edits a buffer; committing
//! applies one reversible operation to the original selection or style.
use super::DesignState;
use properties::ObjectProperty;
use schist_layout::{properties, LayoutObject, ObjectId};

#[derive(Default)]
pub struct Controls {
    pub swatch: Option<usize>,
    pub paragraph: Option<String>,
    pub character: Option<String>,
    pub field: Option<Target>,
}
#[derive(Clone)]
pub enum Target {
    TextPreferences,
    Swatch(schist_layout::Ink),
    Pages(Vec<usize>),
    Section(usize),
    Objects(Vec<ObjectId>),
    Paragraph(String),
    Character(String),
}

pub fn page_property(id: &str) -> Option<properties::PageProperty> {
    use properties::PageProperty::*;
    Some(match id {
        "design-prop-page-width" => Width,
        "design-prop-page-height" => Height,
        "design-prop-bleed" => Bleed,
        "design-prop-slug" => Slug,
        "design-prop-bleed-top" => BleedTop,
        "design-prop-bleed-bottom" => BleedBottom,
        "design-prop-bleed-inside" => BleedInside,
        "design-prop-bleed-outside" => BleedOutside,
        "design-prop-slug-top" => SlugTop,
        "design-prop-slug-bottom" => SlugBottom,
        "design-prop-slug-inside" => SlugInside,
        "design-prop-slug-outside" => SlugOutside,
        "design-prop-margin-top" => MarginTop,
        "design-prop-margin-right" => MarginRight,
        "design-prop-margin-bottom" => MarginBottom,
        "design-prop-margin-left" => MarginLeft,
        _ => return None,
    })
}

pub fn object_property(id: &str) -> Option<ObjectProperty> {
    Some(match id {
        "design-prop-x" => ObjectProperty::X,
        "design-prop-y" => ObjectProperty::Y,
        "design-prop-width" => ObjectProperty::Width,
        "design-prop-height" => ObjectProperty::Height,
        "design-prop-columns" => ObjectProperty::Columns,
        "design-prop-gutter" => ObjectProperty::Gutter,
        "design-prop-inset" => ObjectProperty::Inset,
        "design-prop-fill-tint" => ObjectProperty::FillTint,
        "design-prop-stroke-tint" => ObjectProperty::StrokeTint,
        _ => return None,
    })
}

/// Position and explicit baseline offset are independent. Choosing Normal
/// resets inherited positioning, while None restores it. One style edit undoes it.
pub fn set_position(
    state: &mut DesignState,
    target: &Target,
    position: Option<schist_layout::styles::TextPosition>,
) -> bool {
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let fields = match target {
            Target::Paragraph(name) => styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.position, &mut s.baseline_shift)),
            Target::Character(name) => styles
                .characters
                .iter_mut()
                .find(|s| s.name == *name)
                .map(|s| (&mut s.position, &mut s.baseline_shift)),
            _ => None,
        };
        if let Some((field, shift)) = fields {
            *field = position;
            // Remove only legacy conflated script state; keep numeric offsets.
            if matches!(
                shift,
                Some(
                    schist_layout::styles::BaselineShift::Superscript
                        | schist_layout::styles::BaselineShift::Subscript
                )
            ) {
                *shift = None;
            }
        }
    })
}

/// Show explicit legacy face choices too; blank must mean inheritance.
pub fn font_style_value(styles: &schist_layout::StyleSet, target: &Target) -> String {
    let (named, bold, italic, resolved) = match target {
        Target::Paragraph(name) => {
            let Some(s) = styles.paragraph(name) else {
                return String::new();
            };
            let r = styles.resolve_paragraph(name);
            (&s.font_style, s.bold, s.italic, (r.bold, r.italic))
        }
        Target::Character(name) => {
            let Some(s) = styles.character(name) else {
                return String::new();
            };
            let r = styles.resolve_character(name);
            (&s.font_style, s.bold, s.italic, (r.bold, r.italic))
        }
        _ => return String::new(),
    };
    if let Some(name) = named {
        return name.clone();
    }
    if bold.is_none() && italic.is_none() {
        return String::new();
    }
    match (resolved.0.unwrap_or(false), resolved.1.unwrap_or(false)) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        _ => "Regular",
    }
    .into()
}

pub fn commit(state: &mut DesignState, id: &str, text: &str) -> bool {
    let Some(target) = state.controls.field.take() else {
        return false;
    };
    if matches!(
        id,
        "design-prop-font-style" | "design-prop-paragraph-font-style"
    ) {
        let name = text.trim();
        // Focusing and leaving a legacy face must not turn its independent
        // inheritance into a new atomic named override.
        if name == font_style_value(&state.document.styles, &target) {
            return false;
        }
        if name.chars().any(char::is_control) {
            return false;
        }
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let fields = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| (&mut s.font_style, &mut s.bold, &mut s.italic)),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| (&mut s.font_style, &mut s.bold, &mut s.italic)),
                _ => None,
            };
            if let Some((style, bold, italic)) = fields {
                *style = (!name.is_empty()).then(|| name.to_owned());
                // Blank restores face inheritance, including old imported flags.
                *bold = None;
                *italic = None;
            }
        });
    }
    if matches!(id, "design-prop-leading" | "design-prop-char-leading") {
        use schist_layout::styles::Leading;
        let text = text.trim();
        let leading = if text.is_empty() {
            None
        } else if text.eq_ignore_ascii_case("Auto")
            || text.eq_ignore_ascii_case(schist_i18n::t("design.leading_auto"))
        {
            Some(Leading::Auto)
        } else {
            match text.parse::<f32>() {
                Ok(v) if v.is_finite() && v >= 0.0 => Some(Leading::Points(v)),
                _ => return false,
            }
        };
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.leading),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.leading),
                _ => None,
            };
            if let Some(field) = field {
                *field = leading;
            }
        });
    }
    if matches!(
        id,
        "design-prop-paragraph-features" | "design-prop-char-features"
    ) {
        let Some(features) = parse_features(text) else {
            return false;
        };
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.features),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.features),
                _ => None,
            };
            if let Some(field) = field {
                *field = features;
            }
        });
    }
    if let Target::Section(page) = target {
        if matches!(
            id,
            "design-prop-number-prefix" | "design-prop-section-name" | "design-prop-section-marker"
        ) {
            return schist_layout::numbering::edit_section(
                &mut state.document,
                &mut state.history,
                page,
                |section| match id {
                    "design-prop-number-prefix" => section.prefix = text.into(),
                    "design-prop-section-name" => section.name = text.into(),
                    "design-prop-section-marker" => section.marker = text.into(),
                    _ => unreachable!(),
                },
            );
        }
    }
    if id == "design-prop-family" || id == "design-prop-paragraph-family" {
        let family = text.trim().to_owned();
        return properties::edit_styles(&mut state.document, &mut state.history, |styles| {
            let field = match &target {
                Target::Paragraph(name) => styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.family),
                Target::Character(name) => styles
                    .characters
                    .iter_mut()
                    .find(|s| s.name == *name)
                    .map(|s| &mut s.family),
                _ => None,
            };
            if let Some(field) = field {
                *field = (!family.is_empty()).then_some(family);
            }
        });
    }
    if matches!(id, "design-prop-style-name" | "design-prop-char-name") {
        let (paragraph, name) = match &target {
            Target::Paragraph(name) => (true, name),
            Target::Character(name) => (false, name),
            _ => return false,
        };
        let changed = properties::rename_style(
            &mut state.document,
            &mut state.history,
            paragraph,
            name,
            text,
        );
        if changed {
            if paragraph {
                state.controls.paragraph = Some(text.trim().into());
            } else {
                state.controls.character = Some(text.trim().into());
            }
        }
        return changed;
    }
    let value = if text.trim().is_empty() {
        None
    } else {
        match text.trim().parse::<f32>() {
            Ok(value) if value.is_finite() => Some(value),
            _ => return false,
        }
    };
    if id.ends_with("-tint") && value.is_some_and(|v| !(0.0..=100.0).contains(&v)) {
        return false;
    }
    match target {
        Target::TextPreferences => {
            let Some(value) = value else {
                return false;
            };
            let size = matches!(
                id,
                "design-prop-superscript-size" | "design-prop-subscript-size"
            );
            let range = if size { 1.0..=200.0 } else { -500.0..=500.0 };
            if !range.contains(&value) {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let prefs = &mut styles.text_preferences;
                match id {
                    "design-prop-superscript-size" => prefs.superscript_size = value,
                    "design-prop-superscript-position" => prefs.superscript_position = value,
                    "design-prop-subscript-size" => prefs.subscript_size = value,
                    "design-prop-subscript-position" => prefs.subscript_position = value,
                    _ => {}
                }
            })
        }
        Target::Swatch(ink) => {
            let Some(value) = value else {
                return false;
            };
            let mut after;
            if id == "design-prop-swatch-tint" {
                return schist_layout::swatches::set_tint(
                    &mut state.document,
                    &mut state.history,
                    &ink,
                    value / 100.0,
                );
            } else if id.starts_with("design-prop-swatch-cmyk-") {
                let channel = match id {
                    "design-prop-swatch-cmyk-c" => 0,
                    "design-prop-swatch-cmyk-m" => 1,
                    "design-prop-swatch-cmyk-y" => 2,
                    "design-prop-swatch-cmyk-k" => 3,
                    _ => return false,
                };
                let Some(mut cmyk) = ink.source_cmyk else {
                    return false;
                };
                if ink.tint.is_some() || !(0.0..=100.0).contains(&value) {
                    return false;
                }
                cmyk[channel] = value / 100.0;
                after = schist_layout::Ink::cmyk(ink.name.clone(), cmyk);
                after.spot = ink.spot;
            } else {
                let channel = match id {
                    "design-prop-swatch-red" => 0,
                    "design-prop-swatch-green" => 1,
                    "design-prop-swatch-blue" => 2,
                    _ => return false,
                };
                if ink.tint.is_some() || !(0.0..=255.0).contains(&value) {
                    return false;
                }
                let mut rgb = ink.preview_rgb;
                rgb[channel] = value / 255.0;
                after = schist_layout::Ink::process(ink.name.clone(), rgb);
                after.spot = ink.spot;
            }
            schist_layout::swatches::replace(&mut state.document, &mut state.history, &ink, after)
        }
        Target::Pages(pages) => value.is_some_and(|value| {
            page_property(id).is_some_and(|property| {
                properties::set_page_property(
                    &mut state.document,
                    &mut state.history,
                    &pages,
                    property,
                    value,
                )
            })
        }),
        Target::Section(page) => {
            if id != "design-prop-number-start" {
                return false;
            }
            let Some(value) = value.filter(|v| (1.0..=999999.0).contains(v) && v.fract() == 0.0)
            else {
                return false;
            };
            schist_layout::numbering::edit_section(
                &mut state.document,
                &mut state.history,
                page,
                |section| {
                    section.start = value as u32;
                    section.continue_numbering = false;
                },
            )
        }
        Target::Objects(ids) => value.is_some_and(|value| {
            object_property(id).is_some_and(|property| {
                properties::set_object_property(
                    &mut state.document,
                    &mut state.history,
                    &ids,
                    property,
                    value,
                )
            })
        }),
        Target::Paragraph(name) => {
            let tint_base = state
                .document
                .styles
                .resolve_paragraph(&name)
                .character(
                    state
                        .document
                        .styles
                        .resolve_character(&state.document.default_character_style),
                )
                .fill
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            if id == "design-prop-size" && value.is_some_and(|value| value <= 0.0) {
                return false;
            }
            if id == "design-prop-auto-leading"
                && value.is_some_and(|v| !(0.0..=500.0).contains(&v))
            {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let Some(style) = styles
                    .paragraphs
                    .iter_mut()
                    .find(|style| style.name == name)
                else {
                    return;
                };
                match id {
                    "design-prop-paragraph-fill-tint" => {
                        style.fill_tint = value.map(|v| v / 100.0);
                        if value.is_some() && tint_base.is_some() {
                            style.fill = tint_base;
                        }
                    }
                    "design-prop-size" => style.point_size = value,
                    "design-prop-auto-leading" => style.auto_leading = value,
                    "design-prop-tracking" => style.tracking = value,
                    "design-prop-baseline" => {
                        style.baseline_shift =
                            value.map(schist_layout::styles::BaselineShift::Offset)
                    }
                    "design-prop-before" => style.space_before = value,
                    "design-prop-after" => style.space_after = value,
                    "design-prop-left" => style.left_indent = value,
                    "design-prop-right" => style.right_indent = value,
                    "design-prop-first" => style.first_line_indent = value,
                    _ => {}
                }
            })
        }
        Target::Character(name) => {
            let tint_base = state
                .document
                .styles
                .resolve_character(&name)
                .fill
                .filter(|ink| ink.tint.is_some())
                .map(|ink| ink.base_color().into_owned());
            if matches!(id, "design-prop-char-size" | "design-prop-char-leading")
                && value.is_some_and(|value| value <= 0.0)
            {
                return false;
            }
            properties::edit_styles(&mut state.document, &mut state.history, |styles| {
                let Some(style) = styles
                    .characters
                    .iter_mut()
                    .find(|style| style.name == name)
                else {
                    return;
                };
                match id {
                    "design-prop-char-fill-tint" => {
                        style.fill_tint = value.map(|v| v / 100.0);
                        if value.is_some() && tint_base.is_some() {
                            style.fill = tint_base;
                        }
                    }
                    "design-prop-char-size" => style.point_size = value,
                    "design-prop-char-tracking" => style.tracking = value,
                    "design-prop-char-baseline" => {
                        style.baseline_shift =
                            value.map(schist_layout::styles::BaselineShift::Offset)
                    }
                    _ => {}
                }
            })
        }
    }
}

/// Empty restores inheritance. Four-byte tags and explicit 0/1 values prevent
/// a malformed field from silently clearing other feature overrides.
fn parse_features(text: &str) -> Option<Vec<(String, bool)>> {
    let mut result = Vec::new();
    if text.trim().is_empty() {
        return Some(result);
    }
    for item in text.split(',') {
        let (tag, value) = item.split_once('=')?;
        let tag = tag.trim();
        if tag.len() != 4
            || !tag.bytes().all(|b| b.is_ascii_alphanumeric())
            || result.iter().any(|(previous, _)| previous == tag)
        {
            return None;
        }
        let enabled = match value.trim() {
            "1" => true,
            "0" => false,
            _ => return None,
        };
        result.push((tag.to_owned(), enabled));
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Some(result)
}

pub fn feature_text(features: &[(String, bool)]) -> String {
    features
        .iter()
        .map(|(tag, enabled)| format!("{tag}={}", u8::from(*enabled)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn all_text_frames(state: &DesignState) -> bool {
    !state.selection.is_empty()
        && state.selection.iter().all(|id| {
            state
                .document
                .object(*id)
                .is_some_and(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_and_document_preferences_undo_once_without_changing_explicit_offsets() {
        use schist_layout::styles::{BaselineShift, TextPosition};
        let mut state = DesignState::new();
        state.document.styles.characters[0].baseline_shift = Some(BaselineShift::Offset(8.0));
        let name = state.document.styles.characters[0].name.clone();
        let original = state.document.clone();
        for position in [
            TextPosition::Normal,
            TextPosition::Superscript,
            TextPosition::Subscript,
        ] {
            assert!(set_position(
                &mut state,
                &Target::Character(name.clone()),
                Some(position)
            ));
            assert_eq!(state.history.undo_depth(), 1);
            assert_eq!(
                state
                    .document
                    .styles
                    .character(&name)
                    .unwrap()
                    .baseline_shift,
                Some(BaselineShift::Offset(8.0))
            );
            assert_eq!(
                state.document.styles.character(&name).unwrap().position,
                Some(position)
            );
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
        for (id, value) in [
            ("design-prop-superscript-size", "65"),
            ("design-prop-subscript-size", "75"),
            ("design-prop-superscript-position", "-45"),
            ("design-prop-subscript-position", "25"),
        ] {
            for invalid in ["NaN", "inf", "501", "-501", ""] {
                state.controls.field = Some(Target::TextPreferences);
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
            }
            state.controls.field = Some(Target::TextPreferences);
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
    }

    #[test]
    fn swatch_fields_edit_captured_definitions_keep_cmyk_and_reject_invalid_values() {
        let mut state = DesignState::new();
        let base = schist_layout::Ink::cmyk("Native", [0.6, 0.4, 0.2, 0.1]);
        let tint = base.named_tint("Quarter", 0.25).unwrap();
        state.document.inks.extend([base.clone(), tint.clone()]);
        let original = state.document.clone();
        for (id, ink, value) in [
            ("design-prop-swatch-tint", &tint, "50"),
            ("design-prop-swatch-cmyk-c", &base, "80"),
        ] {
            for invalid in ["NaN", "inf", "-1", "101", ""] {
                state.controls.field = Some(Target::Swatch(ink.clone()));
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
            }
            state.controls.field = Some(Target::Swatch(ink.clone()));
            state.controls.swatch = Some(0);
            assert!(commit(&mut state, id, value));
            assert_eq!(state.history.undo_depth(), 1);
            let changed = state.document.ink(&ink.name).unwrap();
            if ink.tint.is_some() {
                assert_eq!(changed.tint_amount(), 0.5);
            } else {
                assert_eq!(changed.source_cmyk, Some([0.8, 0.4, 0.2, 0.1]));
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
        }
    }
    #[test]
    fn baseline_fields_commit_once_restore_inheritance_and_reject_nonfinite_values() {
        for (id, target) in [
            ("design-prop-baseline", Target::Paragraph("Body".into())),
            (
                "design-prop-char-baseline",
                Target::Character("Default".into()),
            ),
        ] {
            let mut state = DesignState::new();
            let original = state.document.clone();
            for invalid in ["NaN", "inf", "-inf", "garbage"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, original);
                assert_eq!(state.history.undo_depth(), 0);
            }
            for value in ["-12.5", "0", "8.25"] {
                state.controls.field = Some(target.clone());
                state.controls.paragraph = Some("Other".into());
                state.controls.character = Some("Other".into());
                assert!(commit(&mut state, id, value));
                assert_eq!(state.history.undo_depth(), 1);
                let actual = match &target {
                    Target::Paragraph(name) => {
                        state
                            .document
                            .styles
                            .paragraph(name)
                            .unwrap()
                            .baseline_shift
                    }
                    Target::Character(name) => {
                        state
                            .document
                            .styles
                            .character(name)
                            .unwrap()
                            .baseline_shift
                    }
                    _ => unreachable!(),
                };
                assert_eq!(
                    actual,
                    Some(schist_layout::styles::BaselineShift::Offset(
                        value.parse().unwrap()
                    ))
                );
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, original);
                assert!(state.history.redo(&mut state.document));
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                assert_eq!(state.document, original);
                state.history.clear();
            }
        }
    }

    #[test]
    fn tint_fields_commit_once_to_the_captured_targets_and_blank_styles_inherit() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let (id, name, target) = if paragraph {
                (
                    "design-prop-paragraph-fill-tint",
                    "Body",
                    Target::Paragraph("Body".into()),
                )
            } else {
                (
                    "design-prop-char-fill-tint",
                    "Default",
                    Target::Character("Default".into()),
                )
            };
            let before = state.document.clone();
            for invalid in ["-1", "100.01", "NaN", "inf"] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, before);
            }
            state.controls.field = Some(target.clone());
            assert!(commit(&mut state, id, "12.5"));
            assert_eq!(state.history.undo_depth(), 1);
            let value = if paragraph {
                state.document.styles.paragraph(name).unwrap().fill_tint
            } else {
                state.document.styles.character(name).unwrap().fill_tint
            };
            assert_eq!(value, Some(0.125));
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
            assert!(state.history.redo(&mut state.document));
            state.controls.field = Some(target);
            assert!(commit(&mut state, id, ""));
            assert_eq!(state.document, before);
        }
        for id in ["design-prop-fill-tint", "design-prop-stroke-tint"] {
            let mut state = DesignState::new();
            let objects: Vec<_> = (0..3)
                .map(|_| {
                    schist_layout::authoring::rectangle(
                        &mut state.document,
                        &mut schist_layout::History::default(),
                        0,
                        schist_layout::Rect::new(5.0, 5.0, 10.0, 10.0),
                        schist_layout::authoring::Paint::filled("Black"),
                    )
                    .unwrap()
                })
                .collect();
            let before = state.document.clone();
            state.controls.field = Some(Target::Objects(objects));
            state.selection.clear();
            assert!(commit(&mut state, id, "25"));
            assert!(state
                .document
                .objects
                .iter()
                .all(|o| object_property(id).unwrap().value(o) == Some(25.0)));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn section_fields_edit_the_captured_boundary_once_and_reject_invalid_starts() {
        for field in [
            "design-prop-number-start",
            "design-prop-number-prefix",
            "design-prop-section-name",
            "design-prop-section-marker",
        ] {
            let mut state = DesignState::new();
            for _ in 0..4 {
                state.document.add_page(schist_layout::Page::a4());
            }
            state.document.pages[2].section = Some(schist_layout::Section::default());
            let before = state.document.clone();
            state.controls.field = Some(Target::Section(2));
            state.page = Some(4);
            assert!(commit(&mut state, field, "37"));
            assert_eq!(state.history.undo_depth(), 1);
            assert_eq!(state.document.pages[0], before.pages[0]);
            assert!(state.document.pages[4].section.is_none());
            if field == "design-prop-number-start" {
                assert_eq!(state.document.page_number(4), "39");
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
        let mut state = DesignState::new();
        let before = state.document.clone();
        for text in ["", "NaN", "inf", "-1", "0", "1.5", "1000000", "garbage"] {
            state.controls.field = Some(Target::Section(0));
            assert!(!commit(&mut state, "design-prop-number-start", text));
        }
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), 0);
    }

    #[test]
    fn committing_targets_the_original_selection_once() {
        for count in 1..12 {
            let mut state = DesignState::new();
            let ids: Vec<_> = (0..count)
                .map(|_| {
                    schist_layout::authoring::rectangle(
                        &mut state.document,
                        &mut state.history,
                        0,
                        schist_layout::Rect::new(0.0, 0.0, 20.0, 10.0),
                        schist_layout::authoring::Paint::none(),
                    )
                    .unwrap()
                })
                .collect();
            state.history.clear();
            let before = state.document.clone();
            state.controls.field = Some(Target::Objects(ids.clone()));
            state.selection.clear();
            assert!(commit(&mut state, "design-prop-width", "45"));
            assert!(ids
                .iter()
                .all(|id| state.document.object(*id).unwrap().bounds.width == 45.0));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(!commit(&mut state, "design-prop-width", "46"));
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }
    #[test]
    fn empty_style_numbers_restore_inheritance_and_invalid_numbers_do_nothing() {
        let mut state = DesignState::new();
        let name = state.document.styles.paragraphs[0].name.clone();
        for text in ["NaN", "inf", "-1", "0", "garbage"] {
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph(name.clone()));
            assert!(!commit(&mut state, "design-prop-size", text));
            assert_eq!(state.document, before);
        }
        state.controls.field = Some(Target::Paragraph(name.clone()));
        commit(&mut state, "design-prop-size", "20");
        state.controls.field = Some(Target::Paragraph(name.clone()));
        assert!(commit(&mut state, "design-prop-size", ""));
        assert_eq!(
            state.document.styles.paragraph(&name).unwrap().point_size,
            None
        );
    }

    #[test]
    fn named_font_fields_capture_the_target_and_restore_inheritance_in_one_edit() {
        for paragraph in [false, true] {
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-paragraph-font-style"
            } else {
                "design-prop-font-style"
            };
            for face in ["Light", "Regular", "Bold Condensed", "書体 W3 & Narrow"] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                state.controls.field = Some(target.clone());
                state.controls.character = Some("Bold".into());
                state.controls.paragraph = Some("Default".into());
                assert!(commit(&mut state, id, face));
                assert_eq!(state.history.undo_depth(), 1);
                let selected = if paragraph {
                    &state.document.styles.paragraph(name).unwrap().font_style
                } else {
                    &state.document.styles.character(name).unwrap().font_style
                };
                assert_eq!(selected.as_deref(), Some(face));
                let after = state.document.clone();
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, "Bad\nFace"));
                assert_eq!(state.document, after);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, after);
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn an_unchanged_font_field_does_not_flatten_legacy_face_inheritance() {
        for paragraph in [false, true] {
            for bold in [None, Some(false), Some(true)] {
                for italic in [None, Some(false), Some(true)] {
                    if bold.is_none() && italic.is_none() {
                        continue;
                    }
                    let mut state = DesignState::new();
                    let target = if paragraph {
                        let style = state
                            .document
                            .styles
                            .paragraphs
                            .iter_mut()
                            .find(|s| s.name == "Body")
                            .unwrap();
                        style.bold = bold;
                        style.italic = italic;
                        Target::Paragraph("Body".into())
                    } else {
                        let style = state
                            .document
                            .styles
                            .characters
                            .iter_mut()
                            .find(|s| s.name == "Default")
                            .unwrap();
                        style.bold = bold;
                        style.italic = italic;
                        Target::Character("Default".into())
                    };
                    let shown = font_style_value(&state.document.styles, &target);
                    assert!(!shown.is_empty());
                    let before = state.document.clone();
                    state.controls.field = Some(target.clone());
                    assert!(!commit(&mut state, "design-prop-font-style", &shown));
                    assert_eq!(state.document, before);
                    assert_eq!(state.history.undo_depth(), 0);
                    state.controls.field = Some(target.clone());
                    assert!(commit(&mut state, "design-prop-font-style", ""));
                    assert!(font_style_value(&state.document.styles, &target).is_empty());
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                }
            }
        }
    }

    #[test]
    fn leading_fields_distinguish_auto_zero_and_inheritance_with_one_step_undo() {
        use schist_layout::styles::Leading;
        for paragraph in [false, true] {
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-leading"
            } else {
                "design-prop-char-leading"
            };
            for (text, expected) in [
                ("Auto", Some(Leading::Auto)),
                ("0", Some(Leading::Points(0.0))),
                ("19.5", Some(Leading::Points(19.5))),
            ] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, text));
                assert_eq!(state.history.undo_depth(), 1);
                let actual = if paragraph {
                    state.document.styles.paragraph(name).unwrap().leading
                } else {
                    state.document.styles.character(name).unwrap().leading
                };
                assert_eq!(actual, expected);
                let after = state.document.clone();
                for invalid in ["-1", "NaN", "inf", "19 pt", "automatic"] {
                    state.controls.field = Some(target.clone());
                    assert!(!commit(&mut state, id, invalid));
                    assert_eq!(state.document, after);
                    assert_eq!(state.history.undo_depth(), 1);
                }
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, after);
                state.controls.field = Some(target.clone());
                assert!(commit(&mut state, id, ""));
                let cleared = if paragraph {
                    state.document.styles.paragraph(name).unwrap().leading
                } else {
                    state.document.styles.character(name).unwrap().leading
                };
                assert_eq!(cleared, None);
            }
        }
        for percent in ["0", "150", "500"] {
            let mut state = DesignState::new();
            let before = state.document.clone();
            state.controls.field = Some(Target::Paragraph("Body".into()));
            assert!(commit(&mut state, "design-prop-auto-leading", percent));
            assert_eq!(state.history.undo_depth(), 1);
            for invalid in ["-1", "501", "NaN"] {
                state.controls.field = Some(Target::Paragraph("Body".into()));
                assert!(!commit(&mut state, "design-prop-auto-leading", invalid));
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
        }
    }

    #[test]
    fn feature_fields_commit_once_to_the_captured_style_and_reject_partial_invalid_input() {
        for paragraph in [false, true] {
            let mut state = DesignState::new();
            let name = if paragraph { "Body" } else { "Default" };
            let target = if paragraph {
                Target::Paragraph(name.into())
            } else {
                Target::Character(name.into())
            };
            let id = if paragraph {
                "design-prop-paragraph-features"
            } else {
                "design-prop-char-features"
            };
            let before = state.document.clone();
            state.controls.field = Some(target.clone());
            assert!(commit(&mut state, id, "liga=1, kern=0, ss03=1"));
            assert_eq!(state.history.undo_depth(), 1);
            let features = if paragraph {
                &state.document.styles.paragraph(name).unwrap().features
            } else {
                &state.document.styles.character(name).unwrap().features
            };
            assert_eq!(feature_text(features), "kern=0, liga=1, ss03=1");
            let after = state.document.clone();
            for invalid in [
                "liga",
                "liga=2",
                "liga=1,",
                "liga=0, liga=1",
                "abc=1",
                "éabc=1",
                "liga=1, kern=no",
            ] {
                state.controls.field = Some(target.clone());
                assert!(!commit(&mut state, id, invalid));
                assert_eq!(state.document, after);
                assert_eq!(state.history.undo_depth(), 1);
            }
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, before);
            assert!(state.history.redo(&mut state.document));
            assert_eq!(state.document, after);
            state.controls.field = Some(target);
            assert!(commit(&mut state, id, ""));
            assert_eq!(state.document, before);
        }
    }
}
