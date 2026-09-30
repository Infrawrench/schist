//! Field targets are captured on focus. Typing edits a buffer; committing
//! applies one reversible operation to the original selection or style.
use super::DesignState;
use properties::ObjectProperty;
use schist_layout::{properties, LayoutObject, ObjectId};

#[derive(Default)]
pub struct Controls {
    pub paragraph: Option<String>,
    pub character: Option<String>,
    pub field: Option<Target>,
}
#[derive(Clone)]
pub enum Target {
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

pub fn commit(state: &mut DesignState, id: &str, text: &str) -> bool {
    let Some(target) = state.controls.field.take() else {
        return false;
    };
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
            if matches!(id, "design-prop-size" | "design-prop-leading")
                && value.is_some_and(|value| value <= 0.0)
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
                    "design-prop-paragraph-fill-tint" => style.fill_tint = value.map(|v| v / 100.0),
                    "design-prop-size" => style.point_size = value,
                    "design-prop-leading" => style.leading = value,
                    "design-prop-tracking" => style.tracking = value,
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
                    "design-prop-char-fill-tint" => style.fill_tint = value.map(|v| v / 100.0),
                    "design-prop-char-size" => style.point_size = value,
                    "design-prop-char-leading" => style.leading = value,
                    "design-prop-char-tracking" => style.tracking = value,
                    _ => {}
                }
            })
        }
    }
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
}
