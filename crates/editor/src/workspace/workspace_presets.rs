//! Apply layout transactions only after persistence succeeds.
use super::*;
use schist_app_settings::workspaces::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEdit {
    Save,
    Update,
    Rename,
    Delete,
    Reset,
}

impl Workspace {
    pub fn open_workspace_command(&mut self, action: WorkspaceEdit, cx: &mut Context<Self>) {
        let selected = if matches!(action, WorkspaceEdit::Save | WorkspaceEdit::Reset)
            || self.view.workspaces.saved.is_empty()
        {
            None
        } else {
            self.view
                .workspaces
                .saved
                .iter()
                .position(|p| p.layout == Layout::capture(&self.view))
        };
        let name = selected
            .map(|i| self.view.workspaces.saved[i].name.clone())
            .unwrap_or_default();
        self.open_modal(
            Modal::Workspaces {
                primary: Some(action),
                selected,
                name: name.clone(),
                error: None,
            },
            cx,
        );
        if matches!(action, WorkspaceEdit::Save | WorkspaceEdit::Rename) {
            self.focus_field("workspace-name", name);
        }
    }

    /// Keyboard route independent of pointer-only text-field activation.
    pub(super) fn workspace_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(Modal::Workspaces { selected, name, .. }) = self.modal.clone() else {
            return false;
        };
        let mods = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        if mods.alt && matches!(key, "up" | "down") {
            self.commit_focused_field();
            let len = self.view.workspaces.saved.len();
            let next = match (selected, key) {
                (None, "down") if len > 0 => Some(0),
                (None, "up") if len > 0 => Some(len - 1),
                (Some(i), "down") if i + 1 < len => Some(i + 1),
                (Some(i), "up") if i > 0 => Some(i - 1),
                _ => None,
            };
            let name = next
                .map(|i| self.view.workspaces.saved[i].name.clone())
                .unwrap_or_default();
            self.update_modal(|m| {
                if let Modal::Workspaces {
                    selected,
                    name: n,
                    error,
                    ..
                } = m
                {
                    *selected = next;
                    *n = name;
                    *error = None;
                }
            });
            cx.notify();
            return true;
        }
        if mods.control || mods.platform {
            match key {
                "n" => {
                    self.focus_field("workspace-name", name);
                    cx.notify();
                }
                "s" => self.workspace_edit(WorkspaceEdit::Save, cx),
                "u" => self.workspace_edit(WorkspaceEdit::Update, cx),
                "r" => self.workspace_edit(WorkspaceEdit::Rename, cx),
                "d" => self.workspace_edit(WorkspaceEdit::Delete, cx),
                "0" => self.workspace_edit(WorkspaceEdit::Reset, cx),
                "enter" => {
                    if let Some(i) = selected {
                        self.apply_workspace(self.view.workspaces.saved[i].layout.clone(), cx);
                    }
                }
                _ => return false,
            }
            return true;
        }
        false
    }

    pub fn commit_workspace_view(
        &mut self,
        next: schist_app_settings::ViewOptions,
        cx: &mut Context<Self>,
    ) -> bool {
        match schist_app_settings::try_save_view_options(&next) {
            Ok(()) => {
                self.view = next;
                self.side_panel_resize = None;
                self.update_modal(|modal| {
                    if let Modal::Workspaces { error, .. } = modal {
                        *error = None;
                    }
                });
                cx.notify();
                true
            }
            Err(message) => {
                self.status = message.clone().into();
                self.update_modal(|modal| {
                    if let Modal::Workspaces { error, .. } = modal {
                        *error = Some(message);
                    }
                });
                cx.notify();
                false
            }
        }
    }
    pub fn apply_workspace(&mut self, layout: Layout, cx: &mut Context<Self>) {
        let mut next = self.view.clone();
        layout.apply(&mut next);
        self.commit_workspace_view(next, cx);
    }
    pub fn workspace_edit(&mut self, action: WorkspaceEdit, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::Workspaces { selected, name, .. }) = self.modal.clone() else {
            return;
        };
        let mut next = self.view.clone();
        let result = match action {
            WorkspaceEdit::Save => next.workspaces.save(&name, Layout::capture(&self.view)),
            WorkspaceEdit::Update => selected
                .filter(|i| *i < next.workspaces.saved.len())
                .map(|i| {
                    next.workspaces.saved[i].layout = Layout::capture(&self.view);
                    Ok(())
                })
                .unwrap_or(Ok(())),
            WorkspaceEdit::Rename => selected
                .filter(|i| *i < next.workspaces.saved.len())
                .map(|i| {
                    next.workspaces
                        .name(&name, Some(i))
                        .map(|name| next.workspaces.saved[i].name = name)
                })
                .unwrap_or(Ok(())),
            WorkspaceEdit::Delete => {
                if let Some(i) = selected.filter(|i| *i < next.workspaces.saved.len()) {
                    next.workspaces.saved.remove(i);
                }
                Ok(())
            }
            // Reset restores only the current live dock, never saved presets.
            WorkspaceEdit::Reset => {
                Layout::default().apply(&mut next);
                Ok(())
            }
        };
        if let Err(message) = result {
            self.update_modal(|modal| {
                if let Modal::Workspaces { error, .. } = modal {
                    *error = Some(message);
                }
            });
            cx.notify();
            return;
        }
        if self.commit_workspace_view(next, cx) {
            let selected = if action == WorkspaceEdit::Save {
                Some(self.view.workspaces.saved.len() - 1)
            } else if action == WorkspaceEdit::Delete {
                None
            } else {
                selected
            };
            self.update_modal(|modal| {
                if let Modal::Workspaces {
                    selected: current, ..
                } = modal
                {
                    *current = selected;
                }
            });
        }
    }
}
