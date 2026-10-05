//! Design Mode's pointer handling.
//!
//! The pasteboard speaks a different vocabulary from the photo canvas: a
//! click selects, a drag makes a frame or moves one, and there is no
//! painting, no brush pressure and no tool registry involved. So these
//! handlers are short and answer only what a page layout editor can ask.
//!
//! They run before the raster handlers and claim the event, so nothing
//! here can be reached by a stray brush stroke.

use gpui::{Context, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
use schist_ui as ui;

use crate::design::tools;

use super::Workspace;

impl Workspace {
    /// Handle a click on the pasteboard. Returns whether it was claimed.
    pub(super) fn design_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if ev.button != MouseButton::Left && ev.button != MouseButton::Middle {
            return true;
        }
        if ev.button == MouseButton::Middle
            || self.space_held
            || self.design.tool == crate::design::DesignTool::Hand
        {
            let local = self.to_local(ev.position);
            self.design.pan = Some((
                schist_layout::Point::new(f32::from(local.x), f32::from(local.y)),
                self.design.view.origin,
            ));
            self.refit_design = false;
            cx.notify();
            return true;
        }
        if self.design.tool == crate::design::DesignTool::Zoom {
            if ev.click_count > 1 {
                self.fit_to_view();
            } else {
                self.zoom_by(
                    if ev.modifiers.alt { 0.5 } else { 2.0 },
                    Some(self.to_local(ev.position)),
                );
            }
            cx.notify();
            return true;
        }
        self.commit_focused_field();
        crate::design::composition::commit(&mut self.design);
        let local = self.to_local(ev.position);
        let local = schist_layout::Point::new(f32::from(local.x), f32::from(local.y));
        if let Some(page) = self.design.plan().and_then(|plan| {
            plan.pages
                .iter()
                .find(|p| p.page.trim.contains(local))
                .map(|p| p.page.page)
        }) {
            self.design.page = Some(page);
            self.design.view.page = Some(page);
        }
        let at = self.design_page_point(ev.position);
        if self.design.tool == crate::design::DesignTool::Select {
            if crate::design::guides::hit(&mut self.design, at) {
                cx.notify();
                return true;
            }
            if ev.modifiers.shift {
                if let Some(id) = self
                    .design
                    .plan()
                    .and_then(|plan| crate::design::select::hit_test(&plan, local).object())
                {
                    if self.design.selection.contains(&id) {
                        self.design.selection.retain(|selected| *selected != id);
                    } else {
                        self.design.selection.push(id);
                    }
                    cx.notify();
                    return true;
                }
            }
        }
        // The tool is read first: a press sets it, so passing it in and
        // borrowing the state mutably at once is two borrows of one thing.
        let tool = self.design.tool;
        if self.design.thread_source.is_some()
            || matches!(
                tool,
                crate::design::DesignTool::Select | crate::design::DesignTool::TextFrame
            )
        {
            if let Some(changed) = crate::design::text::thread_press(&mut self.design, at) {
                if !changed {
                    self.status = schist_i18n::t("design.choose_empty_frame").into();
                }
                cx.notify();
                return true;
            }
        }
        let previous = self.design.typing;
        tools::press(&mut self.design, tool, at);
        if ev.modifiers.shift && self.design.text_selecting {
            if let (Some(previous), Some(typing)) = (previous, self.design.typing.as_mut()) {
                if previous.story == typing.story {
                    typing.anchor = previous.anchor;
                }
            }
        }
        cx.notify();
        true
    }

    /// Continue a drag on the pasteboard.
    pub(super) fn design_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some((start, origin)) = self.design.pan {
            let local = self.to_local(ev.position);
            self.design.view.origin =
                origin + schist_layout::Point::new(f32::from(local.x), f32::from(local.y)) - start;
            cx.notify();
            return;
        }
        if self.design.guide_drag.is_some() {
            let at = self.design_page_point(ev.position);
            crate::design::guides::drag(&mut self.design, at);
            cx.notify();
            return;
        }
        if self.design.band.is_none()
            && !self.design.text_selecting
            && self.design.drag.is_none()
            && self.design.drawing.is_none()
            && self.design.anchor.is_none()
            && self.design.pen.as_ref().is_none_or(|p| p.draft.is_none())
        {
            return;
        }
        let at = self.design_page_point(ev.position);
        tools::drag(&mut self.design, at);
        cx.notify();
    }

    /// Finish a drag on the pasteboard.
    pub(super) fn design_mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.design.pan.take().is_some() {
            cx.notify();
            return;
        }
        if self.design.guide_drag.is_some() {
            let local = self.to_local(ev.position);
            let remove = f32::from(local.x) < 32.0
                || f32::from(local.y) < 32.0
                || !self.canvas_bounds.contains(&ev.position);
            let at = self.design_page_point(ev.position);
            crate::design::guides::drag(&mut self.design, at);
            crate::design::guides::finish(&mut self.design, remove);
            cx.notify();
            return;
        }
        if self.design.band.is_none()
            && !self.design.text_selecting
            && self.design.drag.is_none()
            && self.design.drawing.is_none()
            && self.design.anchor.is_none()
            && self.design.pen.as_ref().is_none_or(|p| p.draft.is_none())
        {
            return;
        }
        let at = self.design_page_point(ev.position);
        tools::drag(&mut self.design, at);
        if tools::release(&mut self.design) {
            self.after_design_change(cx);
        }
        cx.notify();
    }

    /// Handle a keystroke in Design Mode. Returns whether it was claimed.
    ///
    /// Typed command bindings are registered by app-actions. This handler
    /// owns the remaining canvas/tool keys and text editing, after modal
    /// and inspector field handlers have had the chance to consume them.
    pub(super) fn design_key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.design.composition.is_some() {
            if ev.keystroke.key == "escape" {
                crate::design::composition::cancel(&mut self.design);
                cx.notify();
                return true;
            }
            if ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control {
                crate::design::composition::commit(&mut self.design);
            } else {
                return false;
            }
        }
        // The document-wide commands come first, so ⌘D duplicates however
        // many frames are selected rather than depending on a tool.
        if ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform {
            return self.design_command(ev, cx);
        }
        if self.design.typing.is_none() {
            if ev.keystroke.key == "space" {
                self.space_held = true;
                cx.notify();
                return true;
            }
            if ev.keystroke.key == "escape" {
                self.design.cancel_gesture();
                cx.notify();
                return true;
            }
            if matches!(ev.keystroke.key.as_str(), "backspace" | "delete") {
                self.run_design_command("edit.delete", cx);
                return true;
            }
            if let Some(tool) = tool_shortcut(ev.keystroke.key.as_str()) {
                // Shortcuts are a press and a release, and holding "v" should
                // not flicker the tool; only the press picks it.
                if ev.is_held {
                    return true;
                }
                self.design.cancel_gesture();
                self.design.tool = tool;
                cx.notify();
                return true;
            }
            return false;
        }
        let keystroke = &ev.keystroke;
        let claimed = match keystroke.key.as_str() {
            // Escape is done: the text stays, the caret goes.
            "escape" => {
                crate::design::composition::cancel(&mut self.design);
                tools::stop_typing(&mut self.design);
                true
            }
            // Enter is a paragraph break, which the story already has a
            // point for, so it is typed like any other character rather
            // than being a key the editor swallows.
            "return" | "enter" => tools::type_text(&mut self.design, "\n"),
            "tab" if !keystroke.modifiers.shift => tools::type_text(&mut self.design, "\t"),
            "backspace" => tools::backspace(&mut self.design),
            "delete" => tools::delete_forward(&mut self.design),
            // The arrow keys move the caret. Left and right step by a
            // whole character, so a non-ASCII document does not lose
            // bytes as the user walks through it.
            "left" | "right" | "up" | "down" => {
                move_caret_extended(
                    &mut self.design,
                    keystroke.key.as_str(),
                    keystroke.modifiers.shift,
                );
                true
            }
            // Alt is a modifier a user holds to reach a character, so it
            // is not a reason to refuse the keystroke.
            _ if keystroke.modifiers.control => false,
            _ => match schist_ui::typed_text(keystroke) {
                Some(text) if !text.is_empty() => tools::type_text(&mut self.design, text),
                _ => false,
            },
        };
        if claimed {
            cx.notify();
        }
        claimed
    }

    /// A Design Mode document command, from a ⌘-modified keystroke.
    ///
    /// Alignments come first, so ⌘R is align-right rather than something
    /// else. They need the modifier so they cannot collide with the
    /// single-letter tool shortcuts, and so they read as commands on the
    /// document rather than as a tool.
    fn design_command(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if ev.keystroke.key == "z" {
            self.run_design_command(
                if ev.keystroke.modifiers.shift {
                    "edit.redo"
                } else {
                    "edit.undo"
                },
                cx,
            );
            return true;
        }
        if let Some(typing) = self.design.typing {
            let mut edit = ui::LineEdit {
                text: schist_layout::authoring::text_of(&self.design.document, typing.story),
                cursor: typing.at,
                anchor: typing.anchor,
                active: true,
                multiline: true,
            };
            let result = edit.key(ev, cx);
            if result == ui::LineEditKey::Changed
                && !schist_layout::authoring::set_text(
                    &mut self.design.document,
                    &mut self.design.history,
                    typing.story,
                    &edit.text,
                )
            {
                return true;
            }
            self.design.text_buffer = edit.text;
            self.design.typing = Some(crate::design::Typing {
                at: edit.cursor,
                anchor: edit.anchor,
                ..typing
            });
            cx.notify();
            return result != ui::LineEditKey::Ignored;
        }
        let key = ev.keystroke.key.as_str();
        let claimed = match align_shortcut(key) {
            Some(how) => align_selection(&mut self.design, how),
            None => match key {
                "d" => duplicate_selection(&mut self.design),
                "a" => {
                    crate::design::dragging::select_all(&mut self.design);
                    true
                }
                // Escape with everything selected drops the selection,
                // which is the gesture a user expects from any editor.
                "escape" => {
                    self.design.selection.clear();
                    true
                }
                "delete" | "backspace" => delete_selection(&mut self.design),
                // Distribute across or down, with the shift deciding which.
                "h" => distribute_selection(&mut self.design, !ev.keystroke.modifiers.shift),
                _ => false,
            },
        };
        if claimed {
            cx.notify();
        }
        claimed
    }

    /// Registry commands are raster-bound. This dispatch owns every Design
    /// command so an unhandled menu entry cannot edit a hidden photograph.
    pub(super) fn run_design_command(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(command) = crate::actions::DesignCommand::from_id(id) else {
            self.status = schist_i18n::t("design.command_unavailable").into();
            cx.notify();
            return;
        };
        self.run_layout_command(command, cx);
    }

    pub(super) fn run_layout_command(
        &mut self,
        command: crate::actions::DesignCommand,
        cx: &mut Context<Self>,
    ) {
        if !self.design_mode() {
            return;
        }
        self.commit_focused_field();
        use crate::actions::DesignCommand as Command;
        match command {
            Command::Undo | Command::Redo => {
                self.design.undo_or_redo(command == Command::Redo);
            }
            Command::Delete => {
                delete_selection(&mut self.design);
            }
            Command::Duplicate => {
                duplicate_selection(&mut self.design);
            }
            Command::SelectAll => crate::design::dragging::select_all(&mut self.design),
            Command::Deselect => self.design.selection.clear(),
        }
        self.after_design_change(cx);
    }

    /// A window point in the document's own page space.
    ///
    /// Remove the window's canvas origin, invert the Design viewport,
    /// then subtract the active page's position in the document. Raster
    /// pan and zoom have no part in this transform.
    pub(crate) fn design_page_point(
        &self,
        position: gpui::Point<gpui::Pixels>,
    ) -> schist_layout::Point {
        let local = self.to_local(position);
        let on_pasteboard = schist_layout::Point::new(f32::from(local.x), f32::from(local.y));
        self.design.to_page(on_pasteboard)
    }

    /// A change to the layout document, after the pasteboard has moved on.
    ///
    /// The history is the design state's own, so an undo here undoes a
    /// frame that was created and not a pixel that was painted.
    pub(super) fn after_design_change(&mut self, cx: &mut Context<Self>) {
        // The pasteboard is now stale, so it is repainted; the fit is not
        // redone, because a frame that was drawn on a page does not
        // change the page.
        self.refit_design = self.refit_design || self.design.needs_refit;
        cx.notify();
    }
}

/// Copy everything selected, and select the copies.
fn duplicate_selection(state: &mut crate::design::DesignState) -> bool {
    let selection = state.selection.clone();
    if selection.is_empty() {
        return false;
    }
    let mut made = Vec::new();
    for id in selection {
        if let Some(copy) =
            schist_layout::authoring::duplicate(&mut state.document, &mut state.history, id)
        {
            made.push(copy);
        }
    }
    if made.is_empty() {
        return false;
    }
    // The selection follows the copies, so ⌘D twice does not copy the
    // same frame over and over.
    state.selection = made;
    true
}

/// Delete everything selected.
///
/// One undo step for the lot, which is what `authoring::delete_all` is
/// for: deleting four frames and pressing Z four times is what makes
/// people stop trusting undo.
///
/// A locked frame in the selection refuses the whole delete, so the
/// selection stays put and the user can see which frame is the problem.
fn delete_selection(state: &mut crate::design::DesignState) -> bool {
    let selection = std::mem::take(&mut state.selection);
    if !schist_layout::authoring::delete_all(&mut state.document, &mut state.history, &selection) {
        state.selection = selection;
        return false;
    }
    state.typing = None;
    true
}

/// The alignment a key asks for, if it asks for one.
///
/// The letters are laid out by axis, as they are in every editor that has
/// them: `l`, `c` and `r` line up left edges, middles and right edges,
/// and `t`, `m` and `b` the other axis. `m` is shared with the rectangle
/// tool, which is only a conflict when the modifier is not held, and it is
/// worth the overlap rather than a mnemonic nobody can remember.
fn align_shortcut(key: &str) -> Option<schist_layout::authoring::Align> {
    use schist_layout::authoring::Align;
    match key {
        "l" => Some(Align::Left),
        "c" => Some(Align::CentreX),
        "r" => Some(Align::Right),
        "t" => Some(Align::Top),
        "m" => Some(Align::MiddleY),
        "b" => Some(Align::Bottom),
        _ => None,
    }
}

/// Line up everything selected.
fn align_selection(
    state: &mut crate::design::DesignState,
    how: schist_layout::authoring::Align,
) -> bool {
    let selection = state.selection.clone();
    schist_layout::authoring::align(&mut state.document, &mut state.history, &selection, how)
}

/// Spread everything selected out evenly.
fn distribute_selection(state: &mut crate::design::DesignState, horizontally: bool) -> bool {
    let selection = state.selection.clone();
    schist_layout::authoring::distribute(
        &mut state.document,
        &mut state.history,
        &selection,
        horizontally,
    )
}

/// The tool a key picks, if it picks one.
///
/// The letters are the ones a page layout editor already uses, so muscle
/// memory from InDesign lands correctly: `v` selects, `a` selects points,
/// `t` types, `m` draws
/// a frame, `e` an ellipse, `l` a line, `p` the Pen, `g` a polygon and `d` deletes. A
/// shortcut that does nothing would be worse than no shortcut.
fn tool_shortcut(key: &str) -> Option<crate::design::DesignTool> {
    use crate::design::DesignTool;
    match key {
        "v" => Some(DesignTool::Select),
        "a" => Some(DesignTool::DirectSelect),
        "t" => Some(DesignTool::TextFrame),
        "m" => Some(DesignTool::Rectangle),
        "e" => Some(DesignTool::Ellipse),
        "l" => Some(DesignTool::Line),
        "p" => Some(DesignTool::Pen),
        "g" => Some(DesignTool::Polygon),
        "d" => Some(DesignTool::Delete),
        "h" => Some(DesignTool::Hand),
        "z" => Some(DesignTool::Zoom),
        "i" => Some(DesignTool::Eyedropper),
        _ => None,
    }
}

/// Move the caret by one character, or a line.
#[cfg(test)]
fn move_caret(state: &mut crate::design::DesignState, key: &str) {
    move_caret_extended(state, key, false);
}

fn move_caret_extended(state: &mut crate::design::DesignState, key: &str, extend: bool) {
    let Some(typing) = state.typing else {
        return;
    };
    let text = crate::design::authoring_text(state, typing.story);
    let mut edit = ui::LineEdit {
        text,
        cursor: typing.at,
        anchor: typing.anchor,
        ..Default::default()
    };
    use schist_text_engine::WritingMode;
    let mode = state
        .document
        .story(typing.story)
        .map_or(WritingMode::Horizontal, |story| {
            schist_layout::compose::writing_mode_at(story, typing.at, &state.document)
        });
    let movement = match (mode, key) {
        (WritingMode::Horizontal, "left") => Some((true, false)),
        (WritingMode::Horizontal, "right") => Some((true, true)),
        (WritingMode::Horizontal, "up") => Some((false, false)),
        (WritingMode::Horizontal, "down") => Some((false, true)),
        (_, "up") => Some((true, false)),
        (_, "down") => Some((true, true)),
        (WritingMode::VerticalRl, "left") | (WritingMode::VerticalLr, "right") => {
            Some((false, true))
        }
        (WritingMode::VerticalRl, "right") | (WritingMode::VerticalLr, "left") => {
            Some((false, false))
        }
        _ => None,
    };
    if let Some((inline, forward)) = movement {
        if inline {
            edit.arrow(forward, extend);
        } else {
            edit.move_caret(
                crate::design::text::adjacent_line_caret(state, forward).unwrap_or(if forward {
                    edit.text.len()
                } else {
                    0
                }),
                extend,
            );
        }
    }
    state.typing = Some(crate::design::Typing {
        at: edit.cursor,
        anchor: edit.anchor,
        ..typing
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_shortcuts_are_the_letters_a_layout_editor_uses() {
        use crate::design::DesignTool;
        assert_eq!(tool_shortcut("v"), Some(DesignTool::Select));
        assert_eq!(tool_shortcut("a"), Some(DesignTool::DirectSelect));
        assert_eq!(tool_shortcut("t"), Some(DesignTool::TextFrame));
        assert_eq!(tool_shortcut("m"), Some(DesignTool::Rectangle));
        assert_eq!(tool_shortcut("e"), Some(DesignTool::Ellipse));
        assert_eq!(tool_shortcut("l"), Some(DesignTool::Line));
        assert_eq!(tool_shortcut("p"), Some(DesignTool::Pen));
        assert_eq!(tool_shortcut("d"), Some(DesignTool::Delete));
        assert_eq!(tool_shortcut("z"), Some(DesignTool::Zoom));
        assert_eq!(tool_shortcut("q"), None, "an unbound key is not a tool");
    }

    fn typing_state() -> (crate::design::DesignState, schist_layout::ObjectId) {
        use crate::design::{tools, DesignTool};
        let mut state = crate::design::DesignState::new();
        state.document.pages[0].width = 600.0;
        state.document.pages[0].height = 800.0;
        tools::press(
            &mut state,
            DesignTool::TextFrame,
            schist_layout::Point::new(0.0, 0.0),
        );
        tools::release(&mut state);
        let object = state.typing.expect("a frame is being edited").object;
        (state, object)
    }

    #[test]
    fn the_caret_walks_the_text_a_character_at_a_time() {
        let (mut state, _) = typing_state();
        tools::type_text(&mut state, "héllo");
        // One step left is before the last character, whatever its width
        // in bytes: "o" starts at byte 5, and the é before it is two.
        move_caret(&mut state, "left");
        tools::type_text(&mut state, "x");
        assert_eq!(tools::typing_text(&state).as_deref(), Some("héllxo"));
        move_caret(&mut state, "down");
        tools::type_text(&mut state, "!");
        assert_eq!(
            tools::typing_text(&state).as_deref(),
            Some("héllxo!"),
            "down goes to the end"
        );
    }

    #[test]
    fn vertical_arrow_keys_use_inline_and_column_axes_with_shift_selection() {
        for (mode, next, previous) in [
            (
                schist_layout::WritingMode::VerticalRightToLeft,
                "left",
                "right",
            ),
            (
                schist_layout::WritingMode::VerticalLeftToRight,
                "right",
                "left",
            ),
        ] {
            let (mut state, object) = typing_state();
            state
                .document
                .styles
                .add_paragraph(schist_layout::ParagraphStyle {
                    name: "Vertical".into(),
                    writing_mode: Some(mode),
                    point_size: Some(12.0),
                    leading: Some(schist_layout::styles::Leading::Points(16.0)),
                    keep_lines: Some(1),
                    ..Default::default()
                });
            let story = state.typing.unwrap().story;
            state.document.objects[0].bounds = schist_layout::Rect::new(0.0, 0.0, 200.0, 80.0);
            *state.document.story_mut(story) =
                schist_layout::Story::from_text("é abc def ghi jkl mno pqr stu vwx yz", "Vertical");
            assert!(tools::begin_typing(&mut state, object, 0));
            move_caret_extended(&mut state, "down", true);
            assert_eq!(
                (state.typing.unwrap().anchor, state.typing.unwrap().at),
                (0, 2)
            );
            move_caret(&mut state, "up");
            assert_eq!(state.typing.unwrap().at, 0);
            move_caret(&mut state, "down");
            let offset = state.typing.unwrap().at;
            let expected = crate::design::text::adjacent_line_caret(&state, true).unwrap();
            move_caret_extended(&mut state, next, true);
            assert_eq!(
                (state.typing.unwrap().anchor, state.typing.unwrap().at),
                (offset, expected)
            );
            let expected_back = crate::design::text::adjacent_line_caret(&state, false).unwrap();
            move_caret(&mut state, previous);
            assert_eq!(state.typing.unwrap().at, expected_back);
        }
    }

    #[test]
    fn a_caret_asked_for_inside_a_character_lands_on_its_boundary() {
        // The important case: the é is bytes 1..3, so an offset of 2 is
        // inside it, and splitting a character would produce text that
        // cannot be encoded.
        let (mut state, _) = typing_state();
        tools::type_text(&mut state, "héllo");
        tools::move_caret(&mut state, 2);
        tools::type_text(&mut state, "x");
        assert_eq!(
            tools::typing_text(&state).as_deref(),
            Some("hxéllo"),
            "the x went at the start of the é, not inside it"
        );
    }

    #[test]
    fn the_caret_clamps_at_both_ends() {
        let (mut state, _) = typing_state();
        tools::type_text(&mut state, "ab");
        move_caret(&mut state, "up");
        move_caret(&mut state, "left");
        tools::type_text(&mut state, "z");
        assert_eq!(
            tools::typing_text(&state).as_deref(),
            Some("zab"),
            "there is nowhere before the start to go"
        );
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use crate::design::{tools, DesignTool};

    fn at(x: f32, y: f32) -> schist_layout::Point {
        schist_layout::Point::new(x, y)
    }

    fn state_with(tool: DesignTool) -> crate::design::DesignState {
        let mut state = crate::design::DesignState::new();
        state.document.pages[0].width = 600.0;
        state.document.pages[0].height = 800.0;
        tools::press(&mut state, tool, at(0.0, 0.0));
        tools::drag(&mut state, at(100.0, 40.0));
        tools::release(&mut state);
        state
    }

    #[test]
    fn duplicating_copies_what_is_selected_and_selects_the_copies() {
        let mut state = state_with(DesignTool::Rectangle);
        let original = state.document.objects[0].id;
        assert!(duplicate_selection(&mut state));
        assert_eq!(state.document.objects.len(), 2);
        assert_eq!(state.selection.len(), 1);
        let copy = state.selection[0];
        assert_ne!(copy, original);
        // And again, so ⌘D ⌘D makes two copies rather than one copy of
        // the copy over and original.
        assert!(duplicate_selection(&mut state));
        assert_eq!(state.document.objects.len(), 3);
    }

    #[test]
    fn duplicating_nothing_does_nothing() {
        let mut state = state_with(DesignTool::Rectangle);
        state.selection.clear();
        let before = state.document.clone();
        assert!(!duplicate_selection(&mut state));
        assert_eq!(state.document, before);
    }

    #[test]
    fn deleting_a_selection_removes_all_of_it() {
        let mut state = state_with(DesignTool::Rectangle);
        let first = state.document.objects[0].id;
        assert!(duplicate_selection(&mut state));
        // Original and copy, both selected together.
        state.selection = vec![first, state.document.objects[1].id];
        assert!(delete_selection(&mut state));
        assert!(state.document.objects.is_empty());
        assert!(state.selection.is_empty());
    }

    #[test]
    fn deleting_a_multi_frame_selection_is_one_press() {
        let mut state = state_with(DesignTool::Rectangle);
        assert!(duplicate_selection(&mut state));
        state.selection = state.document.objects.iter().map(|o| o.id).collect();
        assert_eq!(state.selection.len(), 2);
        let depth = state.history.undo_depth();
        assert!(delete_selection(&mut state));
        assert!(state.document.objects.is_empty());
        assert_eq!(
            state.history.undo_depth(),
            depth + 1,
            "one step for two frames"
        );
        state.history.undo(&mut state.document);
        assert_eq!(
            state.document.objects.len(),
            2,
            "one press brings both back"
        );
    }

    #[test]
    fn a_locked_frame_stays_selected_after_a_delete() {
        // So the user can see it survived rather than wonder where it went.
        let mut state = state_with(DesignTool::Rectangle);
        let locked = state.document.objects[0].id;
        state.document.objects[0].locked = true;
        state.selection = vec![locked];
        assert!(!delete_selection(&mut state), "nothing was deleted");
        assert_eq!(
            state.document.objects.len(),
            1,
            "a locked frame survives a delete"
        );
        assert_eq!(state.selection, vec![locked], "and stays selected");
    }

    #[test]
    fn select_all_covers_what_is_on_the_page() {
        let mut state = state_with(DesignTool::Rectangle);
        let first = state.document.objects[0].id;
        assert!(duplicate_selection(&mut state));
        state.selection.clear();
        crate::design::dragging::select_all(&mut state);
        assert_eq!(state.selection.len(), 2, "both frames are selected");
        assert!(state.selection.contains(&first));
    }
}

#[cfg(test)]
mod align_tests {
    use super::*;
    use schist_layout::authoring::Align;

    fn three_frames() -> crate::design::DesignState {
        let mut state = crate::design::DesignState::new();
        state.document.pages[0].width = 600.0;
        state.document.pages[0].height = 800.0;
        for (x, width) in [(10.0, 40.0), (100.0, 20.0), (130.0, 60.0)] {
            schist_layout::authoring::rectangle(
                &mut state.document,
                &mut state.history,
                0,
                schist_layout::Rect::new(x, 50.0, width, 20.0),
                schist_layout::authoring::Paint::none(),
            )
            .expect("a shape");
        }
        state.selection = state.document.objects.iter().map(|o| o.id).collect();
        state
    }

    #[test]
    fn the_alignment_keys_are_the_six_directions() {
        assert_eq!(align_shortcut("l"), Some(Align::Left));
        assert_eq!(align_shortcut("c"), Some(Align::CentreX));
        assert_eq!(align_shortcut("r"), Some(Align::Right));
        assert_eq!(align_shortcut("t"), Some(Align::Top));
        assert_eq!(align_shortcut("m"), Some(Align::MiddleY));
        assert_eq!(align_shortcut("b"), Some(Align::Bottom));
        assert_eq!(align_shortcut("z"), None);
    }

    #[test]
    fn the_tool_shortcut_and_the_align_key_agree_about_m() {
        // `m` is the rectangle tool bare and align-middle with a
        // modifier. They are distinguished by the modifier, which is the
        // whole point, so the overlap is deliberate and worth a test.
        assert_eq!(
            tool_shortcut("m"),
            Some(crate::design::DesignTool::Rectangle)
        );
        assert_eq!(align_shortcut("m"), Some(Align::MiddleY));
    }

    #[test]
    fn aligning_the_selection_lines_it_up() {
        let mut state = three_frames();
        assert!(align_selection(&mut state, Align::Right));
        for object in &state.document.objects {
            assert!((object.bounds.right() - 190.0).abs() < 0.01);
        }
    }

    #[test]
    fn aligning_with_nothing_selected_does_nothing() {
        let mut state = three_frames();
        state.selection.clear();
        let before = state.document.clone();
        assert!(!align_selection(&mut state, Align::Left));
        assert_eq!(state.document, before);
    }

    #[test]
    fn distributing_needs_three_and_the_editor_says_so_by_doing_nothing() {
        let mut state = three_frames();
        state.selection.truncate(2);
        let before = state.document.clone();
        assert!(!distribute_selection(&mut state, true));
        assert_eq!(state.document, before);
    }

    #[test]
    fn distributing_the_selection_closes_the_gaps() {
        let mut state = three_frames();
        assert!(distribute_selection(&mut state, true));
        let xs: Vec<f32> = state.document.objects.iter().map(|o| o.bounds.x).collect();
        assert_eq!(xs, vec![10.0, 80.0, 130.0]);
    }
}
