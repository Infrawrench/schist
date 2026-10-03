//! Captured paragraph edits; each gesture preserves dormant settings and undoes once.
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

#[derive(Clone, Copy)]
pub enum Flag {
    Enabled,
    Capitals,
    LastWord,
    AcrossColumns,
}

pub fn set_flag(state: &mut DesignState, target: &Target, flag: Flag, value: Option<bool>) -> bool {
    edit(state, target, |style| match flag {
        Flag::Enabled => style.hyphenate = value,
        Flag::Capitals => style.hyphenation.capitalized_words = value,
        Flag::LastWord => style.hyphenation.last_word = value,
        Flag::AcrossColumns => style.hyphenation.across_columns = value,
    })
}

pub fn inherit(state: &mut DesignState, target: &Target) -> bool {
    edit(state, target, |style| {
        style.hyphenate = None;
        style.hyphenation = Default::default();
    })
}

pub(super) fn commit(state: &mut DesignState, target: &Target, id: &str, text: &str) -> bool {
    let text = text.trim();
    if id == "design-prop-hyphen-zone" {
        let value = if text.is_empty() {
            None
        } else {
            let Ok(value) = text.parse::<f32>() else {
                return false;
            };
            if !value.is_finite() || value < 0.0 {
                return false;
            }
            Some(value)
        };
        return edit(state, target, |style| {
            // A formatted field round-trip must retain imported precision.
            if style.hyphenation.zone.map(|v| format!("{v:.3}")).as_deref() != Some(text) {
                style.hyphenation.zone = value;
            }
        });
    }
    let (min, max) = match id {
        "design-prop-hyphen-word" => (3, 25),
        "design-prop-hyphen-before" | "design-prop-hyphen-after" => (1, 15),
        "design-prop-hyphen-limit" => (0, 25),
        "design-prop-hyphen-weight" => (0, 100),
        _ => return false,
    };
    let value = if text.is_empty() {
        None
    } else {
        let Ok(value) = text.parse::<u8>() else {
            return false;
        };
        if !(min..=max).contains(&value) {
            return false;
        }
        Some(value)
    };
    edit(state, target, |style| match id {
        "design-prop-hyphen-word" => style.hyphenation.words_longer_than = value,
        "design-prop-hyphen-before" => style.hyphenation.before_last = value,
        "design-prop-hyphen-after" => style.hyphenation.after_first = value,
        "design-prop-hyphen-limit" => style.hyphenation.ladder_limit = value,
        _ => style.hyphenation.weight = value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> DesignState {
        let mut state = DesignState::new();
        let mut style = ParagraphStyle {
            name: "Captured".into(),
            hyphenate: Some(false),
            ..Default::default()
        };
        style.hyphenation.zone = Some(12.345678);
        style.hyphenation.ladder_limit = Some(2);
        state.document.styles.add_paragraph(style);
        state.controls.paragraph = Some("Body".into());
        state
    }
    #[test]
    fn hyphenation_fields_edit_the_captured_style_once_and_reject_invalid_values() {
        for (id, values, invalid) in [
            ("word", vec!["3", "25"], vec!["2", "26"]),
            ("before", vec!["1", "15"], vec!["0", "16"]),
            ("after", vec!["1", "15"], vec!["0", "16"]),
            ("limit", vec!["0", "25", ""], vec!["26", "256"]),
            ("weight", vec!["0", "100"], vec!["101", "256"]),
            ("zone", vec!["0", "9.25", ""], vec!["NaN", "inf"]),
        ] {
            let id = format!("design-prop-hyphen-{id}");
            for value in values {
                let mut state = state();
                let before = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                assert!(super::super::commit(&mut state, &id, value));
                assert_eq!(state.history.undo_depth(), 1);
                assert_eq!(
                    state.document.styles.paragraph("Body"),
                    before.styles.paragraph("Body")
                );
                assert_eq!(
                    state
                        .document
                        .styles
                        .paragraph("Captured")
                        .unwrap()
                        .hyphenate,
                    Some(false)
                );
                let edited = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                assert!(!super::super::commit(&mut state, &id, value));
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, edited);
            }
            for value in invalid.into_iter().chain(["-1", "bad"]) {
                let mut state = state();
                let before = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Captured".into()));
                assert!(!super::super::commit(&mut state, &id, value));
                assert_eq!(state.document, before);
                assert_eq!(state.history.undo_depth(), 0);
            }
        }
    }
    #[test]
    fn hyphenation_flags_and_reset_preserve_dormant_settings_and_source_with_exact_undo() {
        for flag in [
            Flag::Enabled,
            Flag::Capitals,
            Flag::LastWord,
            Flag::AcrossColumns,
        ] {
            for value in [None, Some(false), Some(true)] {
                let mut state = state();
                let before = state.document.clone();
                let target = Target::Paragraph("Captured".into());
                let changed = set_flag(&mut state, &target, flag, value);
                assert_eq!(
                    state
                        .document
                        .styles
                        .paragraph("Captured")
                        .unwrap()
                        .hyphenation
                        .zone,
                    Some(12.345678)
                );
                assert!(!set_flag(&mut state, &target, flag, value));
                assert_eq!(state.history.undo_depth(), usize::from(changed));
                if changed {
                    assert!(state.history.undo(&mut state.document));
                }
                assert_eq!(state.document, before);
                assert!(inherit(&mut state, &target));
                assert!(!inherit(&mut state, &target));
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
            }
        }
    }
    #[test]
    fn stale_targets_and_unchanged_displayed_zone_preserve_exact_values_and_history() {
        for target in [
            Target::Paragraph("missing".into()),
            Target::Character("Default".into()),
            Target::Paragraph("Captured".into()),
        ] {
            let mut state = state();
            let before = state.document.clone();
            state.controls.field = Some(target);
            assert!(!super::super::commit(
                &mut state,
                "design-prop-hyphen-zone",
                "12.346"
            ));
            assert_eq!(state.document, before);
            assert_eq!(state.history.undo_depth(), 0);
        }
    }
}
