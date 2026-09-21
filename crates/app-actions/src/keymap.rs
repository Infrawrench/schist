//! Keymap assembly and file dialogs.
//!
//! Defaults come from the plugin registry (each command/tool declares its
//! own binding); a user keymap file overlays them. "cmd-" in
//! plugin bindings means the platform primary modifier and is rewritten to
//! "ctrl-" on Linux/Windows.

use crate::*;
use gpui::{Action, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate};
use schist_plugin_api::PluginRegistry;
use std::path::PathBuf;

/// Commands that act on the document. Suppressed while typing and while a
/// modal is open: GPUI dispatches a matching binding *before* the
/// element's `on_key_down`, and actions stop propagation by default, so a
/// bound keystroke never reaches the text-entry code at all. Excluding the
/// binding is the only way to let the keystroke through.
const CONTEXT: Option<&str> = Some("Workspace && !text_entry && !modal");
/// Context for bindings without modifiers, i.e. the single-letter tool
/// shortcuts. `editable` is present only in the ordinary state.
const TYPING_SAFE: Option<&str> = Some("Workspace && editable");
/// Bindings that must stay live in every state. Escape is how you leave a
/// text session or a dialog, so it cannot be suppressed by either.
const ALWAYS: Option<&str> = Some("Workspace");
const SEARCH: Option<&str> = Some("Workspace && !modal");

fn translate(binding: &str) -> String {
    // Apple keyboards, on the desktop and on an iPad, have Command.
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        binding.to_string()
    } else {
        binding.replace("cmd-", "ctrl-")
    }
}

pub fn build_bindings(registry: &PluginRegistry) -> Vec<KeyBinding> {
    let mut bindings = Vec::new();

    // Plugin-declared command keybinds.
    for command in registry.commands() {
        if let Some(kb) = command.keybind {
            bindings.push(KeyBinding::new(
                &translate(kb),
                RunCommand {
                    id: command.id.to_string(),
                },
                override_context(kb),
            ));
        }
    }
    // Apple keyboards label Backspace as Delete; both clear canvas pixels.
    if registry.command("edit.clear").is_some() {
        bindings.push(KeyBinding::new(
            "backspace",
            RunCommand {
                id: "edit.clear".into(),
            },
            TYPING_SAFE,
        ));
    }
    // Tool activation keys, plus Shift+key to cycle a group's tools.
    for tool in registry.tools() {
        if let Some(key) = tool.shortcut() {
            bindings.push(KeyBinding::new(
                key,
                ActivateTool {
                    id: tool.id().to_string(),
                },
                TYPING_SAFE,
            ));
            if registry
                .tools()
                .filter(|t| t.group() == tool.group())
                .count()
                > 1
            {
                bindings.push(KeyBinding::new(
                    &format!("shift-{key}"),
                    CycleToolGroup {
                        group: tool.group().to_string(),
                    },
                    TYPING_SAFE,
                ));
            }
        }
    }
    bindings.push(KeyBinding::new(
        &translate("cmd-shift-p"),
        ShowSearch,
        SEARCH,
    ));
    // App-level bindings.
    bindings.extend([
        KeyBinding::new(&translate("cmd-n"), NewFile, CONTEXT),
        KeyBinding::new(&translate("cmd-o"), OpenFile, CONTEXT),
        KeyBinding::new(&translate("cmd-shift-s"), SaveFileAs, CONTEXT),
        KeyBinding::new(&translate("cmd-s"), SaveFile, CONTEXT),
        KeyBinding::new(&translate("cmd-w"), CloseTab, CONTEXT),
        KeyBinding::new("ctrl-tab", NextTab, CONTEXT),
        KeyBinding::new("ctrl-shift-tab", PrevTab, CONTEXT),
        KeyBinding::new(&translate("cmd-="), ZoomIn, CONTEXT),
        KeyBinding::new(&translate("cmd--"), ZoomOut, CONTEXT),
        KeyBinding::new(&translate("cmd-0"), ZoomFit, CONTEXT),
        KeyBinding::new(&translate("cmd-1"), ZoomActual, CONTEXT),
        KeyBinding::new("[", BrushSmaller, TYPING_SAFE),
        KeyBinding::new("]", BrushLarger, TYPING_SAFE),
        KeyBinding::new("x", SwapColors, TYPING_SAFE),
        KeyBinding::new("d", DefaultColors, TYPING_SAFE),
        KeyBinding::new("escape", CancelGesture, ALWAYS),
        KeyBinding::new("enter", CommitGesture, TYPING_SAFE),
        KeyBinding::new(
            &translate("cmd-t"),
            ActivateTool {
                id: "transform".into(),
            },
            CONTEXT,
        ),
        KeyBinding::new(&translate("cmd-q"), Quit, CONTEXT),
        KeyBinding::new(&translate("cmd-alt-i"), ShowImageSize, CONTEXT),
        KeyBinding::new(&translate("cmd-alt-c"), ShowCanvasSize, CONTEXT),
        KeyBinding::new(&translate("cmd-k"), ShowPreferences, CONTEXT),
        KeyBinding::new(&translate("cmd-r"), ToggleRulers, CONTEXT),
        KeyBinding::new(&translate("cmd-'"), ToggleGrid, CONTEXT),
        KeyBinding::new(&translate("cmd-;"), ToggleGuides, CONTEXT),
        KeyBinding::new(&translate("cmd-h"), ToggleExtras, CONTEXT),
        KeyBinding::new(&translate("cmd-shift-;"), ToggleSnap, CONTEXT),
        KeyBinding::new(&translate("cmd-alt-;"), ClearGuides, CONTEXT),
        KeyBinding::new("tab", TogglePanels, TYPING_SAFE),
        KeyBinding::new("f", CycleScreenMode, TYPING_SAFE),
        KeyBinding::new(&translate("cmd-shift-a"), ToggleAiPanel, CONTEXT),
        KeyBinding::new(&translate("cmd-shift-g"), ToggleGallery, CONTEXT),
        // Adjustment layers, matching Photoshop's Image ▸ Adjustments keys.
        KeyBinding::new(
            &translate("cmd-l"),
            AddAdjustment {
                kind: "levels".into(),
            },
            CONTEXT,
        ),
        KeyBinding::new(
            &translate("cmd-m"),
            AddAdjustment {
                kind: "curves".into(),
            },
            CONTEXT,
        ),
        KeyBinding::new(
            &translate("cmd-u"),
            AddAdjustment {
                kind: "hue_saturation".into(),
            },
            CONTEXT,
        ),
        KeyBinding::new(
            &translate("cmd-i"),
            AddAdjustment {
                kind: "invert".into(),
            },
            CONTEXT,
        ),
    ]);
    // Digit keys -> tool opacity (1 = 10% … 0 = 100%).
    for digit in 0..=9u32 {
        let percent = if digit == 0 { 100 } else { digit * 10 };
        bindings.push(KeyBinding::new(
            &digit.to_string(),
            SetToolOpacity { percent },
            TYPING_SAFE,
        ));
    }

    // User overrides: ~/.config/schist/keymap.json
    // Format: { "<keystroke>": "command:<id>" | "tool:<id>" }
    if let Some(user) = load_user_keymap() {
        for (keystroke, target) in user {
            let action: Box<dyn Action> = if let Some(id) = target.strip_prefix("command:") {
                Box::new(RunCommand { id: id.to_string() })
            } else if let Some(id) = target.strip_prefix("tool:") {
                Box::new(ActivateTool { id: id.to_string() })
            } else {
                log::warn!("keymap: unknown target {target:?} for {keystroke:?}");
                continue;
            };
            // An unmodified key has to yield to whatever is capturing
            // typing, exactly as the built-in tool shortcuts do. Binding
            // an override in `CONTEXT` meant rebinding `e` to the eraser
            // made the letter "e" unreachable inside a text layer, and
            // since user bindings are appended last they win the tie-break
            // against the built-in binding they were meant to replace.
            let context = override_context(&keystroke);
            match try_binding(&keystroke, action, context) {
                Some(kb) => bindings.push(kb),
                // `KeyBinding::new` panics on a keystroke gpui cannot
                // parse, and "ctrl-page-up" or "cmd-arrow-left" are
                // plausible things to write. One typo used to take the
                // app down at launch, before any window existed to
                // report it, leaving the user to find the file by hand.
                None => log::error!(
                    "keymap: cannot parse keystroke {keystroke:?} (bound to {target:?}); ignoring it"
                ),
            }
        }
    }
    bindings
}

/// Which context a command binding or user override belongs in.
///
/// An unmodified key has to yield to whatever is capturing typing, as the
/// built-in tool shortcuts do. Overrides were bound in `CONTEXT`
/// unconditionally, so rebinding `e` to the eraser made the letter "e"
/// unreachable inside a text layer -- and since user bindings are
/// appended last they also win the tie-break against the built-in
/// binding they were meant to replace, so the behaviour could not be
/// restored without deleting the entry.
fn override_context(keystroke: &str) -> Option<&'static str> {
    if keystroke.contains('-') {
        CONTEXT
    } else {
        TYPING_SAFE
    }
}

/// `KeyBinding::new` without the panic on an unparseable keystroke.
fn try_binding(
    keystroke: &str,
    action: Box<dyn Action>,
    context: Option<&str>,
) -> Option<KeyBinding> {
    let predicate = match context {
        Some(c) => Some(std::rc::Rc::new(KeyBindingContextPredicate::parse(c).ok()?)),
        None => None,
    };
    KeyBinding::load(
        keystroke,
        action,
        predicate,
        false,
        None,
        &DummyKeyboardMapper,
    )
    .ok()
}

/// Where user keybinding overrides live.
pub fn user_keymap_path() -> Option<PathBuf> {
    Some(dirs_config()?.join("schist/keymap.json"))
}

fn load_user_keymap() -> Option<Vec<(String, String)>> {
    let path = user_keymap_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text) {
        Ok(map) => Some(map.into_iter().collect()),
        Err(err) => {
            log::error!("invalid user keymap: {err}");
            None
        }
    }
}

fn dirs_config() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::{
        build_bindings, override_context, try_binding, ALWAYS, CONTEXT, SEARCH, TYPING_SAFE,
    };
    use crate::{ActivateTool, RunCommand};
    use gpui::{KeyBindingContextPredicate, KeyContext, Keystroke};
    use schist_plugin_api::{Command, CommandPlugin, PluginRegistry};

    /// Does a binding registered with `predicate` fire in `state`?
    fn fires(predicate: Option<&str>, state: &str) -> bool {
        let context = [KeyContext::parse(state).expect("context parses")];
        KeyBindingContextPredicate::parse(predicate.unwrap())
            .expect("predicate parses")
            .eval_inner(&context, &context)
    }

    const ORDINARY: &str = "Workspace editable";
    const TYPING: &str = "Workspace text_entry";
    const MODAL: &str = "Workspace modal";

    #[test]
    fn clear_bindings_work_on_canvas_and_yield_to_typing_and_gallery() {
        struct ClearCommand;
        impl CommandPlugin for ClearCommand {
            fn commands(&self) -> Vec<Command> {
                vec![Command {
                    id: "edit.clear",
                    title: "Clear",
                    description: "Clear pixels",
                    keybind: Some("delete"),
                    run: Box::new(|_| {}),
                }]
            }
        }
        let mut registry = PluginRegistry::new();
        registry.register_commands(&ClearCommand);
        let bindings = build_bindings(&registry);
        for key in ["delete", "backspace"] {
            let keystroke = Keystroke::parse(key).unwrap();
            let binding = bindings
                .iter()
                .find(|binding| {
                    binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                        && binding
                            .action()
                            .as_any()
                            .downcast_ref::<RunCommand>()
                            .is_some_and(|action| action.id == "edit.clear")
                })
                .expect("clear shortcut registered");
            let predicate = binding.predicate().unwrap();
            for (state, expected) in [
                (ORDINARY, true),
                (TYPING, false),
                (MODAL, false),
                ("Workspace gallery", false),
                ("Workspace spotlight text_entry", false),
            ] {
                let context = [KeyContext::parse(state).unwrap()];
                assert_eq!(
                    predicate.eval_inner(&context, &context),
                    expected,
                    "{key} in {state}"
                );
            }
            for modified in [
                format!("alt-{key}"),
                format!("ctrl-{key}"),
                format!("shift-{key}"),
            ] {
                assert_eq!(
                    binding.match_keystrokes(&[Keystroke::parse(&modified).unwrap()]),
                    None
                );
            }
        }
    }

    #[test]
    fn document_commands_do_not_fire_while_typing_or_in_a_modal() {
        // The reported bug: ctrl+a ran the canvas Select All while the
        // caret was in a text layer, because every command was bound
        // against plain "Workspace", which matched in all three states.
        assert!(fires(CONTEXT, ORDINARY), "must work normally");
        assert!(!fires(CONTEXT, TYPING), "ctrl+a must reach the text");
        assert!(
            !fires(CONTEXT, MODAL),
            "ctrl+z must not undo under a dialog"
        );
    }

    #[test]
    fn single_letter_shortcuts_stay_suppressed_while_typing() {
        // These were already correct; the fix must not regress them.
        assert!(fires(TYPING_SAFE, ORDINARY));
        assert!(!fires(TYPING_SAFE, TYPING));
        assert!(!fires(TYPING_SAFE, MODAL));
    }

    #[test]
    fn culling_gallery_keys_reach_the_gallery_without_editor_actions() {
        let gallery = "Workspace gallery";
        assert!(
            !fires(TYPING_SAFE, gallery),
            "rating, pick and reject keys belong to the gallery"
        );
        assert!(
            fires(CONTEXT, gallery),
            "global open/new/preferences bindings stay live"
        );
        assert!(fires(ALWAYS, gallery), "Escape can leave comparison");
        assert!(
            !fires(override_context("x"), gallery),
            "an editor override cannot consume reject"
        );
    }

    #[test]
    fn escape_survives_every_state() {
        // Escape is the way out of a text session and out of a dialog, so
        // suppressing it would trap the user in both.
        assert!(fires(ALWAYS, ORDINARY));
        assert!(fires(ALWAYS, TYPING));
        assert!(fires(ALWAYS, MODAL));
    }

    #[test]
    fn the_three_states_are_mutually_exclusive() {
        // Exactly one of the three tokens is present at a time, which is
        // what lets a predicate name a state by excluding the others.
        for (state, expected) in [
            (ORDINARY, ["editable"].as_slice()),
            (TYPING, ["text_entry"].as_slice()),
            (MODAL, ["modal"].as_slice()),
        ] {
            for token in ["editable", "text_entry", "modal"] {
                let present = fires(Some(token), state);
                assert_eq!(
                    present,
                    expected.contains(&token),
                    "{state:?} should{} carry {token:?}",
                    if expected.contains(&token) {
                        ""
                    } else {
                        " not"
                    }
                );
            }
        }
    }

    #[test]
    fn spotlight_owns_typing_but_keeps_its_toggle_and_escape() {
        let spotlight = "Workspace spotlight text_entry";
        assert!(!fires(CONTEXT, spotlight));
        assert!(!fires(TYPING_SAFE, spotlight));
        assert!(fires(ALWAYS, spotlight));
        assert!(fires(SEARCH, spotlight));
        assert!(fires(SEARCH, TYPING));
        assert!(fires(SEARCH, ORDINARY));
        assert!(!fires(SEARCH, MODAL));
    }

    #[test]
    fn a_bad_user_keystroke_is_skipped_not_fatal() {
        // `KeyBinding::new` panics on anything gpui cannot parse, and it
        // runs before the window exists, so one typo in keymap.json took
        // the app down at launch with a bare unwrap backtrace.
        let tool = || {
            Box::new(ActivateTool {
                id: "eraser".into(),
            })
        };
        assert!(try_binding("ctrl-s", tool(), CONTEXT).is_some());
        // Two non-modifier components: gpui rejects these.
        for bad in ["ctrl-s-a", "ctrl-page-up", "cmd-arrow-left", "alt-num-1"] {
            assert!(
                try_binding(bad, tool(), CONTEXT).is_none(),
                "{bad} should be declined, not panic"
            );
        }
    }

    #[test]
    fn unmodified_overrides_yield_to_typing() {
        assert_eq!(override_context("e"), TYPING_SAFE);
        assert_eq!(override_context("5"), TYPING_SAFE);
        assert_eq!(override_context("ctrl-e"), CONTEXT);
        assert_eq!(override_context("cmd-shift-s"), CONTEXT);
    }
}
