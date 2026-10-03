//! Captured initial counts and character rules. Count resets preserve the rule
//! list; explicit rule inheritance restores the complete parent list.
use super::{DesignState, Target};
use schist_layout::{properties, ParagraphStyle};

fn edit(
    state: &mut DesignState,
    target: &Target,
    change: impl FnOnce(&mut ParagraphStyle),
) -> bool {
    let Target::Paragraph(name) = target else {
        return false;
    };
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        if let Some(style) = styles
            .paragraphs
            .iter_mut()
            .find(|style| style.name == *name)
        {
            change(style);
        }
    })
}

pub fn inherit(state: &mut DesignState, target: &Target) -> bool {
    edit(state, target, |style| {
        style.drop_caps_lines = None;
        style.drop_caps_characters = None;
        style.drop_caps_detail = None;
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterChoice {
    Inherit,
    Style(schist_layout::nested_styles::CharacterStyle),
}

pub fn character_choice(style: &ParagraphStyle) -> CharacterChoice {
    use schist_layout::nested_styles::CharacterStyle;
    match &style.nested_styles {
        None => CharacterChoice::Inherit,
        Some(rules) => CharacterChoice::Style(
            rules
                .first()
                .filter(|rule| rule.is_initial())
                .map_or(CharacterStyle::None, |rule| rule.character_style.clone()),
        ),
    }
}

/// Native rules inherit as a whole ordered list. A style choice changes only
/// its canonical leading initial, materializing inherited rules when needed.
/// Inherit intentionally restores the complete parent list, not just one rule.
pub fn set_character_choice(
    state: &mut DesignState,
    target: &Target,
    choice: CharacterChoice,
) -> bool {
    use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};
    let Target::Paragraph(name) = target else {
        return false;
    };
    let Some(own) = state.document.styles.paragraph(name) else {
        return false;
    };
    if character_choice(own) == choice {
        return false;
    }
    let rules = match choice {
        CharacterChoice::Inherit => None,
        CharacterChoice::Style(CharacterStyle::Unresolved(_)) => return false,
        CharacterChoice::Style(character) => {
            if let CharacterStyle::Named(name) = &character {
                if state.document.styles.character(name).is_none() {
                    return false;
                }
            }
            let mut rules = state
                .document
                .styles
                .resolve_paragraph(name)
                .nested_styles
                .unwrap_or_default();
            if let Some(first) = rules.first_mut().filter(|rule| rule.is_initial()) {
                first.character_style = character;
            } else {
                rules.insert(
                    0,
                    NestedStyle {
                        character_style: character,
                        delimiter: Delimiter::Enumeration("Dropcap".into()),
                        repetition: 1,
                        inclusive: true,
                    },
                );
            }
            Some(rules)
        }
    };
    edit(state, target, |style| style.nested_styles = rules)
}

pub(super) fn commit(state: &mut DesignState, target: &Target, id: &str, text: &str) -> bool {
    let max = match id {
        "design-prop-initial-lines" => 25,
        "design-prop-initial-characters" => 150,
        _ => return false,
    };
    let text = text.trim();
    let value = if text.is_empty() {
        None
    } else {
        let Ok(value) = text.parse::<usize>() else {
            return false;
        };
        if value > max {
            return false;
        }
        Some(value)
    };
    edit(state, target, |style| {
        if id == "design-prop-initial-lines" {
            style.drop_caps_lines = value;
        } else {
            style.drop_caps_characters = value;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::nested_styles::{CharacterStyle as NestedCharacter, Delimiter, NestedStyle};

    fn initial(character_style: NestedCharacter) -> NestedStyle {
        NestedStyle {
            character_style,
            delimiter: Delimiter::Enumeration("Dropcap".into()),
            repetition: 1,
            inclusive: true,
        }
    }

    fn with_rules() -> DesignState {
        let mut state = state();
        for name in ["A", "B"] {
            state
                .document
                .styles
                .add_character(schist_layout::CharacterStyle {
                    name: name.into(),
                    ..Default::default()
                });
        }
        let mut other = initial(NestedCharacter::Named("B".into()));
        other.delimiter = Delimiter::Text(" \tש".into());
        other.repetition = i32::MIN;
        other.inclusive = false;
        state
            .document
            .styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Base")
            .unwrap()
            .nested_styles = Some(vec![initial(NestedCharacter::Named("A".into())), other]);
        state
    }

    fn state() -> DesignState {
        let mut state = DesignState::new();
        state.document.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            drop_caps_lines: Some(4),
            drop_caps_characters: Some(2),
            drop_caps_detail: Some(3),
            ..Default::default()
        });
        state.document.styles.add_paragraph(ParagraphStyle {
            name: "Captured".into(),
            based_on: Some("Base".into()),
            drop_caps_lines: Some(0),
            drop_caps_characters: Some(1),
            drop_caps_detail: Some(i32::MIN),
            ..Default::default()
        });
        state.document.add_story(schist_layout::Story::from_text(
            "E\u{301}ach source grapheme stays intact.\nAnother paragraph.",
            "Captured",
        ));
        state.controls.paragraph = Some("Body".into());
        state
    }

    #[test]
    fn initial_count_edits_capture_the_style_preserve_native_flags_and_undo_once() {
        for (id, max) in [("lines", 25), ("characters", 150)] {
            let id = format!("design-prop-initial-{id}");
            for text in (0..=max).map(|n| n.to_string()).chain([String::new()]) {
                let mut state = state();
                let before = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                let changed = super::super::commit(&mut state, &id, &text);
                let after = state.document.clone();
                assert_eq!(changed, before != after);
                assert_eq!(state.history.undo_depth(), usize::from(changed));
                let style = after.styles.paragraph("Captured").unwrap();
                let expected = (!text.is_empty()).then(|| text.parse::<usize>().unwrap());
                if id.ends_with("lines") {
                    assert_eq!(style.drop_caps_lines, expected);
                    assert_eq!(style.drop_caps_characters, Some(1));
                } else {
                    assert_eq!(style.drop_caps_characters, expected);
                    assert_eq!(style.drop_caps_lines, Some(0));
                }
                assert_eq!(style.drop_caps_detail, Some(i32::MIN));
                assert_eq!(after.stories, before.stories);
                assert_eq!(
                    after.styles.paragraph("Body"),
                    before.styles.paragraph("Body")
                );
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                assert!(!super::super::commit(&mut state, &id, &text));
                if changed {
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                    assert!(state.history.redo(&mut state.document));
                    assert_eq!(state.document, after);
                }
            }
            for text in [
                "-1",
                "1.5",
                "NaN",
                "no",
                "999999999999999999999999999999",
                &(max + 1).to_string(),
            ] {
                let mut state = state();
                let before = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                assert!(!super::super::commit(&mut state, &id, text));
                assert_eq!(state.document, before);
                assert_eq!(state.history.undo_depth(), 0);
            }
        }
    }

    #[test]
    fn initial_count_reset_restores_counts_and_placement_atomically_and_stale_targets_are_noops() {
        let target = Target::Paragraph("Captured".into());
        let mut state = state();
        let before = state.document.clone();
        assert!(inherit(&mut state, &target));
        assert_eq!(state.history.undo_depth(), 1);
        let resolved = state.document.styles.resolve_paragraph("Captured");
        assert_eq!(resolved.drop_caps_lines, Some(4));
        assert_eq!(resolved.drop_caps_characters, Some(2));
        assert_eq!(resolved.drop_caps_detail, Some(3));
        assert!(!inherit(&mut state, &target));
        assert!(state.history.undo(&mut state.document));
        assert_eq!(state.document, before);
        for target in [
            Target::Paragraph("missing".into()),
            Target::Character("Default".into()),
        ] {
            assert!(!inherit(&mut state, &target));
            state.controls.field = Some(target);
            assert!(!super::super::commit(
                &mut state,
                "design-prop-initial-lines",
                "3"
            ));
            assert_eq!(state.document, before);
            assert_eq!(state.history.undo_depth(), 0);
        }
    }

    #[test]
    fn initial_character_choices_capture_one_style_preserve_ordered_rules_and_undo_once() {
        let seed = with_rules();
        let base = seed
            .document
            .styles
            .resolve_paragraph("Base")
            .nested_styles
            .unwrap();
        let missing = vec![
            initial(NestedCharacter::Unresolved("native/missing".into())),
            base[1].clone(),
        ];
        let locals = [
            None,
            Some(Vec::new()),
            Some(base.clone()),
            Some(vec![base[1].clone()]),
            Some(missing),
        ];
        for local in locals {
            for choice in [
                CharacterChoice::Inherit,
                CharacterChoice::Style(NestedCharacter::None),
                CharacterChoice::Style(NestedCharacter::Named("A".into())),
                CharacterChoice::Style(NestedCharacter::Named("B".into())),
            ] {
                let mut state = with_rules();
                state
                    .document
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Captured")
                    .unwrap()
                    .nested_styles = local.clone();
                let before = state.document.clone();
                let previous = before
                    .styles
                    .resolve_paragraph("Captured")
                    .nested_styles
                    .unwrap_or_default();
                let target = Target::Paragraph("Captured".into());
                let changed = set_character_choice(&mut state, &target, choice.clone());
                let after = state.document.clone();
                assert_eq!(changed, before != after);
                assert_eq!(state.history.undo_depth(), usize::from(changed));
                let selected = after.styles.paragraph("Captured").unwrap();
                assert_eq!(character_choice(selected), choice);
                match &choice {
                    CharacterChoice::Inherit => assert!(selected.nested_styles.is_none()),
                    CharacterChoice::Style(character) if changed => {
                        let rules = selected.nested_styles.as_ref().unwrap();
                        assert!(rules[0].is_initial());
                        assert_eq!(&rules[0].character_style, character);
                        let skip =
                            usize::from(previous.first().is_some_and(NestedStyle::is_initial));
                        assert_eq!(&rules[1..], &previous[skip..]);
                    }
                    _ => {}
                }
                let mut expected = before.clone();
                expected
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Captured")
                    .unwrap()
                    .nested_styles = selected.nested_styles.clone();
                assert_eq!(
                    after, expected,
                    "only the captured paragraph rule list may change"
                );
                assert!(!set_character_choice(&mut state, &target, choice));
                if changed {
                    assert_eq!(state.history.undo_depth(), 1);
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, before);
                    assert!(state.history.redo(&mut state.document));
                    assert_eq!(state.document, after);
                }
            }
        }
    }

    #[test]
    fn initial_character_choices_retain_missing_and_dormant_settings_and_survive_count_reset() {
        for lines in [0, 1, 3] {
            for character in [
                NestedCharacter::Named("A".into()),
                NestedCharacter::Named("missing".into()),
                NestedCharacter::Unresolved("native/missing".into()),
            ] {
                let mut state = with_rules();
                let style = state
                    .document
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Captured")
                    .unwrap();
                style.drop_caps_lines = Some(lines);
                style.nested_styles = Some(vec![initial(character.clone())]);
                let target = Target::Paragraph("Captured".into());
                let choice = CharacterChoice::Style(character);
                assert_eq!(character_choice(style), choice);
                let before = state.document.clone();
                assert!(!set_character_choice(&mut state, &target, choice));
                assert_eq!(state.document, before);
                assert!(set_character_choice(
                    &mut state,
                    &target,
                    CharacterChoice::Style(NestedCharacter::Named("B".into()))
                ));
                let changed = state.document.clone();
                assert_eq!(
                    changed
                        .styles
                        .paragraph("Captured")
                        .unwrap()
                        .drop_caps_lines,
                    Some(lines)
                );
                assert!(inherit(&mut state, &target));
                assert_eq!(
                    state
                        .document
                        .styles
                        .paragraph("Captured")
                        .unwrap()
                        .nested_styles,
                    changed.styles.paragraph("Captured").unwrap().nested_styles
                );
                assert_eq!(state.history.undo_depth(), 2);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, changed);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
        }
    }

    #[test]
    fn initial_character_choices_reject_stale_targets_and_unavailable_new_references() {
        for target in [
            Target::Paragraph("missing".into()),
            Target::Character("Default".into()),
            Target::Paragraph("Captured".into()),
        ] {
            for choice in [
                CharacterChoice::Style(NestedCharacter::Named("deleted".into())),
                CharacterChoice::Style(NestedCharacter::Unresolved("native/missing".into())),
            ] {
                let mut state = with_rules();
                let before = state.document.clone();
                assert!(!set_character_choice(&mut state, &target, choice));
                assert_eq!(state.document, before);
                assert_eq!(state.history.undo_depth(), 0);
            }
        }
    }
}
