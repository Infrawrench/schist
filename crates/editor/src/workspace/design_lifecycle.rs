use super::*;
use crate::design::lifecycle::Transition;

impl Workspace {
    pub fn has_unsaved_changes(&mut self) -> bool {
        self.commit_focused_field();
        self.design.lifecycle.dirty(&self.design.document) || self.first_dirty_tab().is_some()
    }
    pub fn request_design_transition(&mut self, transition: Transition, cx: &mut Context<Self>) {
        if self.design.lifecycle.waiting_save {
            return;
        }
        self.commit_focused_field();
        self.design.cancel_gesture();
        if self.design.lifecycle.dirty(&self.design.document) {
            self.design.lifecycle.pending = Some(transition);
            self.set_mode(crate::design::WorkspaceMode::Design, cx);
            self.open_modal(Modal::ConfirmCloseDesign, cx);
        } else {
            self.finish_design_transition(transition, cx);
        }
    }
    pub(super) fn finish_design_transition(
        &mut self,
        transition: Transition,
        cx: &mut Context<Self>,
    ) {
        match transition {
            Transition::Open {
                path,
                document,
                skipped,
            } => self.finish_layout_load(path, *document, skipped, cx),
            transition => {
                self.design = crate::design::DesignState::new();
                self.design_path = None;
                self.refit_design = true;
                match transition {
                    Transition::New => {
                        self.set_mode(crate::design::WorkspaceMode::Design, cx);
                    }
                    Transition::Close => {
                        self.set_mode(crate::design::WorkspaceMode::Photo, cx);
                    }
                    Transition::Quit => self.request_quit(cx),
                    Transition::Open { .. } => unreachable!(),
                }
            }
        }
        cx.notify();
    }
    pub fn discard_design_transition(&mut self, cx: &mut Context<Self>) {
        let transition = self.design.lifecycle.pending.take();
        self.close_modal(cx);
        if let Some(transition) = transition {
            self.finish_design_transition(transition, cx);
        }
    }
    pub fn save_design_transition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let transition = self.design.lifecycle.pending.take();
        self.close_modal(cx);
        self.design.lifecycle.pending = transition;
        self.design.lifecycle.waiting_save = true;
        self.save_design(window, cx);
    }
}
