//! Keyboard editing for the Type tool's compact numeric fields.

use super::*;
use schist_plugin_api::{OptionKind, OptionValue};

impl Workspace {
    pub(crate) fn type_field_selected(&self, id: &str) -> bool {
        self.focused_field == Some(id) && self.field_fresh
    }

    pub(super) fn type_field_option(&self) -> Option<&'static str> {
        if self.editor.active_tool != "type" {
            return None;
        }
        match self.focused_field? {
            "type-size" | "character-size" => Some("type-size"),
            "character-leading" => Some("type-leading"),
            "character-tracking" => Some("type-tracking"),
            "character-path-offset" => Some("type-path-offset"),
            _ => None,
        }
    }

    pub(super) fn type_field_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(key) = self.type_field_option() else {
            return false;
        };
        let Some(option) = self
            .registry
            .tools()
            .find(|tool| tool.id() == "type")
            .and_then(|tool| tool.options().into_iter().find(|option| option.key == key))
        else {
            return false;
        };
        let OptionKind::Slider { min, max, .. } = option.kind else {
            return false;
        };
        if ev.keystroke.key == "a"
            && (ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform)
        {
            self.field_fresh = true;
            return true;
        }
        let value = match ev.keystroke.key.as_str() {
            "up" | "down" => {
                let step = if key == "type-leading" { 0.1 } else { 1.0 };
                let step = step
                    * if ev.keystroke.modifiers.shift {
                        10.0
                    } else {
                        1.0
                    };
                let direction = if ev.keystroke.key == "up" { 1.0 } else { -1.0 };
                let value = (option.value.num() + direction * step).clamp(min, max);
                self.focus_field(self.focused_field.unwrap(), format!("{value:.1}"));
                Some(value)
            }
            "backspace" | "delete" if self.field_fresh => {
                self.field_buffer.clear();
                self.field_fresh = false;
                None
            }
            _ => {
                if let Some(text) = ev.keystroke.key_char.as_deref() {
                    let base = if self.field_fresh {
                        ""
                    } else {
                        &self.field_buffer
                    };
                    if !text.is_empty() && !numeric_accepts(base, text) {
                        return true;
                    }
                    if self.field_fresh && !text.is_empty() {
                        self.field_buffer.clear();
                    }
                }
                self.field_key(&ev.keystroke.key, ev.keystroke.key_char.as_deref());
                self.field_buffer.parse::<f32>().ok()
            }
        };
        if let Some(value) = value.filter(|value| value.is_finite()) {
            self.set_tool_option(key, OptionValue::Num(value.clamp(min, max)), cx);
        }
        // A numeric field owns all typing, including rejected letters:
        // they must never leak into the text layer behind it.
        true
    }
}
