//! Captured paragraph keep edits, including older symmetric keep settings.
use super::{DesignState, Target};
use schist_layout::{
    paragraph_keeps::ParagraphKeeps, properties, styles::ParagraphStart, ParagraphStyle,
};

pub fn local(style: &ParagraphStyle) -> ParagraphKeeps {
    style.keeps.over(&ParagraphKeeps::from_legacy(
        style.keep_with_next,
        style.keep_lines,
    ))
}

fn edit_style(
    state: &mut DesignState,
    target: &Target,
    change: impl FnOnce(&mut ParagraphStyle),
) -> bool {
    let Target::Paragraph(name) = target else {
        return false;
    };
    properties::edit_styles(&mut state.document, &mut state.history, |styles| {
        let Some(style) = styles.paragraphs.iter_mut().find(|s| s.name == *name) else {
            return;
        };
        change(style);
    })
}

fn edit(
    state: &mut DesignState,
    target: &Target,
    change: impl FnOnce(&mut ParagraphKeeps),
) -> bool {
    edit_style(state, target, |style| {
        let before = local(style);
        let mut after = before.clone();
        change(&mut after);
        if after != before {
            // Migrate the local aliases together: otherwise clearing one native
            // count would reveal this style's legacy value instead of its base.
            // A no-op leaves the original representation untouched.
            style.keeps = after;
            style.keep_with_next = None;
            style.keep_lines = None;
        }
    })
}

#[derive(Clone, Copy)]
pub enum Flag {
    Enabled,
    All,
    Previous,
}

pub fn set_flag(state: &mut DesignState, target: &Target, flag: Flag, value: Option<bool>) -> bool {
    edit(state, target, |keeps| match flag {
        Flag::Enabled => keeps.enabled = value,
        Flag::All => keeps.all = value,
        Flag::Previous => keeps.previous = value,
    })
}

pub fn inherit(state: &mut DesignState, target: &Target) -> bool {
    edit_style(state, target, |style| {
        style.keeps = ParagraphKeeps::default();
        style.keep_lines = None;
        style.keep_with_next = None;
        style.start_paragraph = None;
    })
}

pub fn set_start(state: &mut DesignState, target: &Target, value: Option<ParagraphStart>) -> bool {
    edit_style(state, target, |style| style.start_paragraph = value)
}

pub(super) fn commit(state: &mut DesignState, target: &Target, id: &str, text: &str) -> bool {
    let (min, max) = match id {
        "design-prop-keep-first" | "design-prop-keep-last" => (1, 50),
        "design-prop-keep-next" => (0, 5),
        _ => return false,
    };
    let text = text.trim();
    let value = if text.is_empty() {
        None
    } else {
        let Ok(value) = text.parse::<usize>() else {
            return false;
        };
        if !(min..=max).contains(&value) {
            return false;
        }
        Some(value)
    };
    edit(state, target, |keeps| match id {
        "design-prop-keep-first" => keeps.first = value,
        "design-prop-keep-last" => keeps.last = value,
        _ => keeps.next = value,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{commit, DesignState, Target};
    use schist_layout::{paragraph_keeps::ParagraphKeeps, styles::ParagraphStart, ParagraphStyle};

    fn state(legacy: bool) -> DesignState {
        let mut state = DesignState::new();
        state.document.styles.add_paragraph(ParagraphStyle {
            name: "Keep base".into(),
            start_paragraph: Some(ParagraphStart::NextPage),
            keeps: ParagraphKeeps {
                enabled: Some(true),
                all: Some(false),
                first: Some(7),
                last: Some(8),
                next: Some(3),
                previous: Some(true),
            },
            ..Default::default()
        });
        state.document.styles.add_paragraph(ParagraphStyle {
            name: "Keep target".into(),
            based_on: Some("Keep base".into()),
            start_paragraph: Some(ParagraphStart::NextColumn),
            keep_lines: legacy.then_some(2),
            keep_with_next: legacy.then_some(true),
            keeps: if legacy {
                ParagraphKeeps::default()
            } else {
                ParagraphKeeps {
                    enabled: Some(false),
                    all: Some(true),
                    first: Some(2),
                    last: Some(4),
                    next: Some(1),
                    previous: Some(false),
                }
            },
            ..Default::default()
        });
        state.controls.paragraph = Some("Body".into());
        state
    }

    #[test]
    fn keep_count_edits_capture_the_style_preserve_other_policies_and_undo_once() {
        for legacy in [false, true] {
            for (id, text, field) in [
                ("design-prop-keep-first", "12", 0),
                ("design-prop-keep-last", "13", 1),
                ("design-prop-keep-next", "5", 2),
                ("design-prop-keep-first", "", 0),
                ("design-prop-keep-last", "", 1),
                ("design-prop-keep-next", "", 2),
            ] {
                let mut state = state(legacy);
                let original = state.document.clone();
                let before = state.document.styles.resolve_paragraph("Keep target").keeps;
                let mut expected = before.clone();
                let inherited = state.document.styles.resolve_paragraph("Keep base").keeps;
                match field {
                    0 => expected.first = text.parse().ok().or(inherited.first),
                    1 => expected.last = text.parse().ok().or(inherited.last),
                    _ => expected.next = text.parse().ok().or(inherited.next),
                }
                state.controls.field = Some(Target::Paragraph("Keep target".into()));
                assert!(
                    commit(&mut state, id, text),
                    "{id}, {text:?}, legacy={legacy}"
                );
                assert_eq!(state.history.undo_depth(), 1);
                assert_eq!(
                    state.document.styles.resolve_paragraph("Keep target").keeps,
                    expected
                );
                assert_eq!(
                    state.document.styles.paragraph("Body"),
                    original.styles.paragraph("Body")
                );
                let edited = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Keep target".into()));
                assert!(!commit(&mut state, id, text));
                assert_eq!(state.history.undo_depth(), 1);
                assert_eq!(state.document, edited);
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, original);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, edited);
            }
        }
    }

    #[test]
    fn keep_counts_reject_invalid_values_stale_targets_and_unchanged_legacy_values() {
        for (id, invalid) in [
            (
                "design-prop-keep-first",
                vec!["0", "51", "-1", "2.1", "NaN"],
            ),
            ("design-prop-keep-last", vec!["0", "51", "-1", "2.1", "NaN"]),
            ("design-prop-keep-next", vec!["6", "-1", "2.1", "NaN"]),
        ] {
            for text in invalid {
                let mut state = state(true);
                let before = state.document.clone();
                state.controls.field = Some(Target::Paragraph("Keep target".into()));
                assert!(!commit(&mut state, id, text));
                assert_eq!(state.document, before);
                assert_eq!(state.history.undo_depth(), 0);
            }
        }
        for target in [
            Target::Paragraph("missing".into()),
            Target::Character("Default".into()),
        ] {
            let mut state = state(true);
            let before = state.document.clone();
            state.controls.field = Some(target);
            assert!(!commit(&mut state, "design-prop-keep-first", "3"));
            assert_eq!(state.document, before);
            assert_eq!(state.history.undo_depth(), 0);
        }
        let mut state = state(true);
        let before = state.document.clone();
        state.controls.field = Some(Target::Paragraph("Keep target".into()));
        assert!(!commit(&mut state, "design-prop-keep-first", "2"));
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), 0);
    }

    #[test]
    fn toggles_preserve_dormant_counts_and_inherit_independently_with_exact_undo() {
        use super::{set_flag, Flag};
        for legacy in [false, true] {
            for flag in [Flag::Enabled, Flag::All, Flag::Previous] {
                for value in [None, Some(false), Some(true)] {
                    let mut state = state(legacy);
                    let original = state.document.clone();
                    let mut expected =
                        super::local(original.styles.paragraph("Keep target").unwrap());
                    match flag {
                        Flag::Enabled => expected.enabled = value,
                        Flag::All => expected.all = value,
                        Flag::Previous => expected.previous = value,
                    }
                    let changed =
                        expected != super::local(original.styles.paragraph("Keep target").unwrap());
                    let target = Target::Paragraph("Keep target".into());
                    assert_eq!(set_flag(&mut state, &target, flag, value), changed);
                    let inherited = state.document.styles.resolve_paragraph("Keep base").keeps;
                    assert_eq!(
                        state.document.styles.resolve_paragraph("Keep target").keeps,
                        expected.over(&inherited)
                    );
                    assert_eq!(
                        state.document.styles.paragraph("Body"),
                        original.styles.paragraph("Body")
                    );
                    let edited = state.document.clone();
                    assert!(!set_flag(&mut state, &target, flag, value));
                    assert_eq!(state.history.undo_depth(), usize::from(changed));
                    if changed {
                        assert!(state.history.undo(&mut state.document));
                        assert_eq!(state.document, original);
                        assert!(state.history.redo(&mut state.document));
                        assert_eq!(state.document, edited);
                    } else {
                        assert_eq!(state.document, original);
                    }
                }
            }
        }
    }

    #[test]
    fn inherit_clears_native_and_legacy_overrides_together_without_changing_base() {
        for legacy in [false, true] {
            let mut state = state(legacy);
            let original = state.document.clone();
            let target = Target::Paragraph("Keep target".into());
            assert!(super::inherit(&mut state, &target));
            let edited = state.document.clone();
            let style = edited.styles.paragraph("Keep target").unwrap();
            assert_eq!(style.keeps, ParagraphKeeps::default());
            assert_eq!(style.keep_lines, None);
            assert_eq!(style.keep_with_next, None);
            assert_eq!(style.start_paragraph, None);
            assert_eq!(
                edited
                    .styles
                    .resolve_paragraph("Keep target")
                    .start_paragraph,
                Some(ParagraphStart::NextPage)
            );
            assert_eq!(
                edited.styles.paragraph("Keep base"),
                original.styles.paragraph("Keep base")
            );
            assert_eq!(
                edited.styles.resolve_paragraph("Keep target").keeps,
                edited.styles.resolve_paragraph("Keep base").keeps
            );
            assert!(!super::inherit(&mut state, &target));
            assert_eq!(state.history.undo_depth(), 1);
            assert!(state.history.undo(&mut state.document));
            assert_eq!(state.document, original);
            assert!(state.history.redo(&mut state.document));
            assert_eq!(state.document, edited);
        }
    }

    #[test]
    fn start_edits_capture_the_style_without_rewriting_legacy_keeps() {
        for legacy in [false, true] {
            for value in [
                None,
                Some(ParagraphStart::Anywhere),
                Some(ParagraphStart::NextColumn),
                Some(ParagraphStart::NextFrame),
                Some(ParagraphStart::NextPage),
                Some(ParagraphStart::NextOddPage),
                Some(ParagraphStart::NextEvenPage),
            ] {
                let mut state = state(legacy);
                let original = state.document.clone();
                let mut expected = original.clone();
                expected
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Keep target")
                    .unwrap()
                    .start_paragraph = value;
                let changed = expected != original;
                let target = Target::Paragraph("Keep target".into());
                assert_eq!(super::set_start(&mut state, &target, value), changed);
                assert_eq!(state.document, expected);
                assert_eq!(state.history.undo_depth(), usize::from(changed));
                assert!(!super::set_start(&mut state, &target, value));
                if changed {
                    assert!(state.history.undo(&mut state.document));
                    assert_eq!(state.document, original);
                    assert!(state.history.redo(&mut state.document));
                    assert_eq!(state.document, expected);
                }
            }
        }
    }
}
