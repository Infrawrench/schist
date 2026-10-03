//! Captured initial counts; blank fields restore inheritance without rewriting native flags.
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
    fn initial_reset_restores_all_inherited_settings_atomically_and_stale_targets_are_noops() {
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
}
