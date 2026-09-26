//! Modal dialogs, their numeric/text fields, and the colour picker.

use super::*;
use crate::ui;

fn modal_enter_confirms(field: Option<&str>, key: &str, mods: gpui::Modifiers) -> bool {
    key == "enter"
        && !(field.is_some_and(super::gallery_metadata::is_caption)
            && mods.shift
            && !mods.platform
            && !mods.control)
}

impl Workspace {
    // ----- modals and numeric fields -----

    pub fn open_modal(&mut self, modal: Modal, cx: &mut Context<Self>) {
        self.cancel_printing();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.photo_merge_job = None;
        }
        if !crate::feature_enabled("schist-cloud")
            && matches!(
                modal,
                Modal::Cloud { .. } | Modal::CloudGenerate | Modal::BucketName { cloud: true, .. }
            )
        {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(self.modal, Some(Modal::VersionHistory)) {
            self.library.versions = None;
        }
        if !matches!(modal, Modal::MaskRefine { .. }) {
            self.cancel_mask_refine();
        }
        // A dialog opened from the menus replaces whatever was up,
        // suspended parents included; only `open_color_picker_on` stacks.
        self.filter_canvas = Default::default();
        self.modal_stack.clear();
        self.cloud.set_people_modal(Some(&modal));
        self.modal = Some(modal);
        self.context_menu = None;
        self.focused_field = None;
        self.field_buffer.clear();
        self.open_popup = None;
        self.dropdown_search.clear();
        cx.notify();
    }

    /// Open Photoshop's Color Picker on one of the two editor colours.
    pub fn open_color_picker(&mut self, target: ColorTarget, cx: &mut Context<Self>) {
        let original = match target {
            ColorTarget::Foreground => self.editor.foreground,
            ColorTarget::Background => self.editor.background,
            // Not reachable from the editor colour wells: these belong to
            // a dialog, which opens the picker through
            // `open_color_picker_on` and supplies the colour itself.
            ColorTarget::Note => self.editor.note_color,
            ColorTarget::StyleEffect(_) | ColorTarget::ColorRange | ColorTarget::SpotInk(_) => {
                return;
            }
        };
        self.open_color_picker_on(target, original, cx);
    }

    /// Open the picker over the dialog that is already up, which is
    /// suspended rather than closed: `close_modal` brings it back whether
    /// the picker is OK'd or cancelled.
    pub fn open_color_picker_on(
        &mut self,
        target: ColorTarget,
        original: Rgba,
        cx: &mut Context<Self>,
    ) {
        let hsv = crate::color_picker::rgb_to_hsv(original.r, original.g, original.b);
        let parent = self.modal.take();
        self.open_modal(
            Modal::ColorPicker {
                target,
                hsv,
                original,
            },
            cx,
        );
        // After `open_modal`, which clears the stack.
        if let Some(parent) = parent {
            self.modal_stack.push(parent);
        }
    }

    /// Take the picker's colour and close it. Cancel does nothing, because
    /// the picker never wrote to the editor while it was open.
    pub fn commit_color_picker(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::ColorPicker { target, hsv, .. }) = self.modal.as_ref() else {
            return;
        };
        let (target, (h, s, v)) = (*target, *hsv);
        let (r, g, b) = crate::color_picker::hsv_to_rgb(h, s, v);
        let colour = Rgba::new(r, g, b, 1.0);
        match target {
            ColorTarget::Foreground => self.editor.foreground = colour,
            ColorTarget::Background => self.editor.background = colour,
            ColorTarget::Note => {
                self.editor.note_color = colour;
                let [r, g, b, _] = colour.to_u8();
                self.view.note_color = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
                self.save_view_options();
            }
            // Written below, once `close_modal` has put the dialog the
            // picker was opened from back in `self.modal`.
            ColorTarget::StyleEffect(_) | ColorTarget::ColorRange | ColorTarget::SpotInk(_) => {}
        }
        self.close_modal(cx);
        match target {
            ColorTarget::SpotInk(id) => {
                if let Some(doc) = self.doc.as_mut() {
                    let mut edit = doc.begin_edit(schist_i18n::t("common.color"));
                    edit.change_ink_channels(|channels| {
                        if let Some(channel) = channels.iter_mut().find(|c| c.info.id == id) {
                            channel.info.color = [colour.r, colour.g, colour.b];
                            channel.info.original_display = None;
                        }
                    });
                    edit.commit();
                }
                self.after_change(cx);
            }
            ColorTarget::StyleEffect(effect) => {
                let mut next = None;
                self.update_modal(|m| {
                    if let Modal::LayerStyle { style, layer, .. } = m {
                        crate::style_dialog::set_color(style, effect, colour);
                        next = Some((*layer, **style));
                    }
                });
                if let Some((layer, style)) = next {
                    self.preview_layer_style(layer, style, cx);
                }
            }
            ColorTarget::ColorRange => {
                self.update_modal(|m| {
                    if let Modal::ColorRange { target, .. } = m {
                        *target = colour;
                    }
                });
                cx.notify();
            }
            ColorTarget::Foreground | ColorTarget::Background | ColorTarget::Note => {}
        }
    }

    /// The ± buttons beside a picker component.
    pub fn nudge_color_component(&mut self, id: &'static str, delta: f32) {
        self.update_modal(|m| {
            if let Modal::ColorPicker { hsv, .. } = m {
                crate::color_picker::nudge(hsv, id, delta);
            }
        });
    }

    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.cancel_printing();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.photo_merge_job = None;
        }
        if matches!(
            self.modal,
            Some(Modal::RecordedActionBatch {
                finished: false,
                ..
            })
        ) {
            if let Some(cancel) = &self.action_recorder.batch_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(self.modal, Some(Modal::VersionHistory)) {
            self.library.versions = None;
        }
        self.cancel_mask_refine();
        // A file picker going away unanswered is a cancel: dropping its
        // sender is what tells the prompt's caller.
        self.file_picker = None;
        // Any filter preview still on the canvas belongs to the dialog that
        // is going away, so put the original pixels back. Committing a
        // filter clears the preview first, so this only fires on cancel.
        self.filter_canvas = Default::default();
        self.cancel_filter_preview(cx);
        // Same for a cancelled Layer Style session: OK clears the modal
        // itself before it gets here, so reaching this means Cancel.
        self.revert_layer_style();
        // Escape out of Preferences is a cancel, like the button.
        if matches!(self.modal, Some(Modal::Preferences)) {
            self.revert_preferences(cx);
        }
        // Same for escaping the update dialog while its download is
        // running: dismissing the thing that asked must not leave an
        // update to land on its own.
        if matches!(self.modal, Some(Modal::UpdateAvailable { .. })) {
            self.update_progress = None;
        }
        // Closing the picker uncovers the dialog it was opened from.
        self.modal = self.modal_stack.pop();
        self.cloud.set_people_modal(self.modal.as_ref());
        self.default_action = None;
        self.focused_field = None;
        self.field_buffer.clear();
        self.open_popup = None;
        cx.notify();
    }

    /// Mutate the open modal's state in place.
    pub fn update_modal(&mut self, f: impl FnOnce(&mut Modal)) {
        if let Some(modal) = &mut self.modal {
            f(modal);
        }
    }

    /// Focus a field, seeded with the text it is currently showing.
    ///
    /// The buffer used to be cleared, and the field falls back to
    /// rendering its committed value while the buffer is empty, so a
    /// freshly clicked field looked full but behaved empty: backspace
    /// popped nothing, and changing 1920 to 1820 meant retyping all four
    /// digits.
    pub fn focus_field(&mut self, id: &'static str, current: impl Into<String>) {
        self.focused_field = Some(id);
        self.field_buffer = current.into();
        self.field_cursor = self.field_buffer.len();
        self.field_anchor = self.field_cursor;
        self.field_fresh = true;
        self.reset_caret_phase();
    }

    /// The focused field's text, caret and selection as the
    /// [`crate::ui::LineEdit`] every other box in the application is
    /// built on, so that a press, a drag, a shifted arrow and typing
    /// over a selection all behave here exactly as they do in the
    /// gallery's search box. The dialogs keep the buffer in three plain
    /// fields of their own -- what a field means on commit depends on
    /// which one it is -- so this borrows them into the model and puts
    /// them back.
    fn with_field_edit<R>(&mut self, f: impl FnOnce(&mut ui::LineEdit) -> R) -> R {
        let mut edit = ui::LineEdit {
            text: std::mem::take(&mut self.field_buffer),
            cursor: self.field_cursor,
            anchor: self.field_anchor,
            active: true,
            multiline: false,
        };
        let out = f(&mut edit);
        self.field_buffer = edit.text;
        self.field_cursor = edit.cursor;
        self.field_anchor = edit.anchor;
        out
    }

    /// What is selected in the focused field, low end first, for the
    /// renderer to draw on the selection fill. Empty when nothing is.
    pub fn field_selection(&self) -> std::ops::Range<usize> {
        let cursor = self.field_cursor.min(self.field_buffer.len());
        let anchor = self.field_anchor.min(self.field_buffer.len());
        cursor.min(anchor)..cursor.max(anchor)
    }

    /// A press in a field: it takes the keyboard if it did not have it,
    /// and the caret lands where the press did -- a double click taking
    /// the word under it, a triple the lot.
    ///
    /// `current` seeds the buffer the first time, the way
    /// [`Workspace::focus_field`] does.
    pub fn press_field(
        &mut self,
        id: &'static str,
        current: impl Into<String>,
        press: &ui::TextPress,
    ) {
        if self.focused_field != Some(id) {
            self.focus_field(id, current);
        }
        self.with_field_edit(|edit| edit.press(press));
        // A press is a caret move, so the caret shows solid from here.
        self.reset_caret_phase();
    }

    /// The pointer dragging across the focused field: the selection
    /// follows it.
    pub fn drag_field(&mut self, id: &'static str, offset: usize) {
        if self.focused_field != Some(id) {
            return;
        }
        self.with_field_edit(|edit| edit.extend_to(offset));
        self.reset_caret_phase();
    }

    /// Whether anything is showing a caret: a dialog field, a gallery
    /// search box, an inline rename, a note, the AI prompt. What the
    /// blink timer runs for, and so only where there is one -- the web
    /// build's carets stay solid and it has no gallery to ask about.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn caret_somewhere(&self) -> bool {
        self.spotlight.open
            || self.dropdown_search.active
            || self.focused_field.is_some()
            || self.gallery_search_active()
            || self.layer_rename.is_some()
            || self.note_edit.is_some()
            || self.ai.input.active
    }

    /// Whether carets are on this instant of the blink. Solid right
    /// after every keystroke, then 530 ms beats.
    pub fn caret_on(&self) -> bool {
        self.caret_phase
            .is_none_or(|phase| (phase.elapsed().as_millis() / 530) % 2 == 0)
    }

    /// A keystroke or a focus: the caret shows solid from here.
    pub(crate) fn reset_caret_phase(&mut self) {
        // `Instant::now` panics on the web target; its carets simply
        // stay solid.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.caret_phase = Some(std::time::Instant::now());
        }
    }

    /// Keep a repaint arriving at each caret blink beat while any text
    /// field has the keyboard. One task at a time; it retires itself
    /// when the last field lets go.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn ensure_caret_blinker(&mut self, cx: &mut Context<Self>) {
        if self.caret_blinker {
            return;
        }
        self.caret_blinker = true;
        cx.spawn(async move |this, cx| loop {
            let wait = match this.update(cx, |ws, _| {
                let active = ws.caret_somewhere();
                if !active {
                    ws.caret_blinker = false;
                }
                active.then(|| {
                    let into = ws
                        .caret_phase
                        .map_or(0, |phase| phase.elapsed().as_millis() as u64 % 530);
                    // A hair past the beat, so the repaint lands on the
                    // caret's other state rather than a boundary tie.
                    530 - into + 5
                })
            }) {
                Ok(Some(ms)) => ms,
                _ => return,
            };
            cx.background_executor()
                .timer(std::time::Duration::from_millis(wait))
                .await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        })
        .detach();
    }

    /// Fire the open dialog's primary button, as Enter should.
    pub fn confirm_modal(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> bool {
        let Some(action) = self.default_action.clone() else {
            return false;
        };
        action(self, window, cx);
        true
    }

    /// Route dialog keys consistently in the editor, gallery and cloud views.
    /// The multiline caption owns Shift+Enter; ordinary Enter saves the dialog.
    pub(super) fn modal_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.modal.is_none() {
            return false;
        }
        if self.workspace_key(ev, cx) {
            cx.stop_propagation();
            return true;
        }
        if modal_enter_confirms(
            self.focused_field,
            &ev.keystroke.key,
            ev.keystroke.modifiers,
        ) {
            self.commit_focused_field();
            self.confirm_modal(window, cx);
        } else {
            self.field_key(
                &ev.keystroke.key,
                ev.keystroke.key_char.as_deref(),
                ev.keystroke.modifiers,
                cx,
            );
        }
        cx.notify();
        cx.stop_propagation();
        true
    }

    /// Push whatever is in the focused field into the modal and unfocus.
    pub fn commit_focused_field(&mut self) {
        if let Some(id) = self.focused_field {
            self.commit_field(id);
        }
    }

    /// Feed a keystroke to the focused numeric field. Returns true when the
    /// field consumed it.
    ///
    /// `mods` is what decides whether an arrow moves the caret or drags
    /// a selection along behind it.
    pub(super) fn field_key(
        &mut self,
        key: &str,
        text: Option<&str>,
        mods: gpui::Modifiers,
        cx: &mut gpui::App,
    ) -> bool {
        let Some(id) = self.focused_field else {
            return false;
        };
        let fresh = std::mem::take(&mut self.field_fresh);
        let shift = mods.shift;
        let primary = mods.platform || mods.control;
        // Text fields (layer and document names) take any printable
        // character; the picker's hex field takes hex digits up to a full
        // triplet; numeric fields only digits.
        let textual = id == "spot-name"
            || id == "layer-name"
            || id == "brush-preset-name"
            || id == "recorded-action-name"
            || id == "workspace-name"
            || id == "new-doc-name"
            || id == "bucket-name"
            || id == "bucket-query"
            || id == "variant-name"
            || id == "face-name"
            || id == "person-name"
            || id == file_picker::NAME_FIELD
            || id == palettes::SEARCH_FIELD
            || id.starts_with("metadata-")
            || id.starts_with("recipe-")
            || id.starts_with("cloud-");
        let hex = id == "cp-hex";
        // The caret belongs to the textual fields; keep it on the rails
        // in case the buffer changed underneath it.
        self.field_cursor = self.field_cursor.min(self.field_buffer.len());
        self.field_anchor = self.field_anchor.min(self.field_buffer.len());
        match key {
            // A ⌘ (or Ctrl) chord the field has no use for is a command,
            // not text: without this ⌘Z in a dialog typed a "z", since
            // the key arrives carrying one.
            _ if primary && !matches!(key, "a" | "c" | "x" | "v" | "left" | "right") => {
                self.field_fresh = fresh;
                return false;
            }
            // The clipboard. A bound ⌘C/⌘X/⌘V is excluded from the
            // typing and modal contexts (see `keymap`), so these
            // keystrokes reach the field rather than the document.
            "c" | "x" if textual && primary && !self.field_selection().is_empty() => {
                let selected = self.field_buffer[self.field_selection()].to_string();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
                if key == "c" {
                    return true;
                }
                self.with_field_edit(|edit| {
                    edit.delete_selection();
                });
            }
            "v" if textual && primary => {
                let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return true;
                };
                // Plain fields stay on one line. Photo captions retain
                // pasted paragraph breaks in their multiline input.
                let pasted: String = pasted
                    .chars()
                    .map(|c| {
                        if c.is_control() && !(super::gallery_metadata::is_caption(id) && c == '\n')
                        {
                            ' '
                        } else {
                            c
                        }
                    })
                    .collect();
                self.with_field_edit(|edit| edit.insert(&pasted));
            }
            "a" if textual && primary => {
                self.with_field_edit(|edit| edit.select_all());
                self.reset_caret_phase();
                return true;
            }
            // ⌘←/⌘→ go to the ends of the line, as they do everywhere
            // else; Shift takes the selection with them.
            "left" | "right" if textual && primary => {
                let to = if key == "left" {
                    0
                } else {
                    self.field_buffer.len()
                };
                self.with_field_edit(|edit| edit.move_caret(to, shift));
                self.reset_caret_phase();
                return true;
            }
            "left" | "right" if textual => {
                self.with_field_edit(|edit| edit.arrow(key == "right", shift));
                self.reset_caret_phase();
                return true;
            }
            "home" | "up" if textual => {
                self.with_field_edit(|edit| edit.move_caret(0, shift));
                self.reset_caret_phase();
                return true;
            }
            "end" | "down" if textual => {
                let end = self.field_buffer.len();
                self.with_field_edit(|edit| edit.move_caret(end, shift));
                self.reset_caret_phase();
                return true;
            }
            "space" if textual => self.with_field_edit(|edit| edit.insert(" ")),
            "backspace" if textual => self.with_field_edit(|edit| {
                if !edit.delete_selection() && edit.cursor > 0 {
                    let from = crate::ui::caret_left(&edit.text, edit.cursor);
                    edit.text.replace_range(from..edit.cursor, "");
                    edit.place_caret(from);
                }
            }),
            "delete" if textual => self.with_field_edit(|edit| {
                if !edit.delete_selection() && edit.cursor < edit.text.len() {
                    let to = crate::ui::caret_right(&edit.text, edit.cursor);
                    edit.text.replace_range(edit.cursor..to, "");
                }
            }),
            "backspace" => {
                self.field_buffer.pop();
            }
            // Escape is handled in `cancel_gesture`, which runs first:
            // it is bound to `CancelGesture` in the always-matching
            // "Workspace" context, so nothing escape-shaped ever reaches
            // here. Kept as a fallback for a build with that binding
            // removed rather than left as a dead arm that looks live.
            "escape" => {
                self.focused_field = None;
                self.field_buffer.clear();
                self.field_cursor = 0;
                self.field_anchor = 0;
                return true;
            }
            "enter" if super::gallery_metadata::is_caption(id) && shift => {
                self.with_field_edit(|edit| edit.insert("\n"));
            }
            "enter" | "tab" => {
                self.commit_field(id);
                return true;
            }
            _ if hex => match hex_field_after(&self.field_buffer, fresh, text.unwrap_or("")) {
                Some(next) => self.field_buffer = next,
                None => return false,
            },
            _ => match text {
                Some(t) if !t.is_empty() && !t.chars().any(char::is_control) && textual => {
                    // Typing over a selection replaces it, as anywhere.
                    self.with_field_edit(|edit| edit.insert(t));
                }
                Some(t)
                    if !t.is_empty()
                        && !t.chars().any(char::is_control)
                        && numeric_accepts(&self.field_buffer, t) =>
                {
                    self.field_buffer.push_str(t)
                }
                _ => return false,
            },
        }
        self.reset_caret_phase();
        // Apply as you type so the dialog stays live.
        self.commit_field_value(id);
        true
    }

    pub(super) fn commit_field(&mut self, id: &'static str) {
        if id == "spot-name" && !self.field_buffer.trim().is_empty() {
            if let Some(doc) = self.doc.as_mut() {
                if let Some(channel) = doc.active_ink {
                    let name = self.field_buffer.trim().to_owned();
                    if doc
                        .ink_channels
                        .iter()
                        .any(|c| c.info.id == channel && c.info.name != name)
                    {
                        let mut edit = doc.begin_edit(schist_i18n::t("common.rename"));
                        edit.change_ink_channels(|channels| {
                            if let Some(c) = channels.iter_mut().find(|c| c.info.id == channel) {
                                c.info.name = name;
                            }
                        });
                        edit.commit();
                    }
                }
            }
        }
        self.commit_field_value(id);
        self.focused_field = None;
        self.field_buffer.clear();
        self.field_cursor = 0;
        self.field_anchor = 0;
    }

    pub(super) fn commit_field_value(&mut self, id: &'static str) {
        if id == "spot-name" {
            return;
        }
        if id == "brush-preset-name" {
            self.brush_preset_name = self
                .field_buffer
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            return;
        }
        let buffer = self.field_buffer.clone();
        if id == "recorded-action-layer" {
            if let Some(Modal::RecordedActions {
                step: Some(index), ..
            }) = self.modal.as_ref()
            {
                if let Some(recorded_actions::Step::SelectLayer { name }) = self
                    .action_recorder
                    .draft
                    .as_mut()
                    .and_then(|a| a.steps.get_mut(*index))
                {
                    *name = buffer;
                }
            }
            return;
        }
        if id == "workspace-name" {
            self.update_modal(|modal| {
                if let Modal::Workspaces { name, .. } = modal {
                    *name = buffer;
                }
            });
            return;
        }
        if id == "recorded-action-name" {
            if let Some(draft) = self.action_recorder.draft.as_mut() {
                draft.name = buffer.clone();
            }
            self.update_modal(|modal| {
                if let Modal::RecordedActions { name, .. } = modal {
                    *name = buffer;
                }
            });
            return;
        }
        if id.starts_with("recipe-") {
            self.update_modal(|modal| {
                if let Modal::ExportRecipes { editor } = modal {
                    match id {
                        "recipe-name" => editor.draft.name = buffer,
                        "recipe-destination" => editor.draft.destination = buffer.into(),
                        "recipe-watermark" => {
                            editor.draft.outputs[editor.output].finishing.text = buffer
                        }
                        "recipe-copyright" => {
                            editor.draft.outputs[editor.output].finishing.copyright = buffer
                        }
                        "recipe-template" => editor.draft.outputs[editor.output].template = buffer,
                        "recipe-max-edge" => match buffer.parse::<u32>() {
                            Ok(value) if value <= 32768 => {
                                editor.draft.outputs[editor.output].max_edge = value
                            }
                            _ => {
                                editor.draft.outputs[editor.output].max_edge = u32::MAX;
                                editor.error =
                                    Some(schist_i18n::t("export_recipes.invalid_size").into());
                            }
                        },
                        _ => {}
                    }
                }
            });
            return;
        }
        if id == palettes::SEARCH_FIELD {
            self.palette_search = buffer;
            return;
        }
        // The file picker's name: committed on Enter before the dialog
        // confirms, so the confirm reads it from the picker.
        if id == file_picker::NAME_FIELD {
            if let Some(picker) = self.file_picker.as_mut() {
                picker.name = buffer;
            }
            return;
        }
        if id == "cloud-generation-input" {
            if let Some(id) = self.cloud.generation.editing.clone() {
                self.cloud
                    .generation
                    .values
                    .insert(id, schist_cloud::generation::Input::Text(buffer));
            }
            return;
        }
        if id.starts_with("cloud-") {
            self.update_modal(|modal| {
                if let Modal::Cloud { kind, fields } = modal {
                    if *kind == "metadata"
                        && super::cloud_gallery::commit_metadata_field(fields, id, &buffer)
                    {
                        return;
                    }
                    if let Some((_, _, value)) = fields.iter_mut().find(|(key, _, _)| *key == id) {
                        *value = buffer;
                    }
                }
            });
            return;
        }
        if id == "layer-name" {
            self.update_modal(|m| {
                if let Modal::LayerProperties { name, .. } = m {
                    *name = buffer;
                }
            });
            return;
        }
        if id == "new-doc-name" {
            self.update_modal(|m| {
                if let Modal::NewDocument { name, .. } = m {
                    *name = buffer;
                }
            });
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if id == "face-name" {
            // The viewer's name field: the live text rides on the
            // viewer, so the panel can offer completions as it grows.
            if let Some(viewer) = &mut self.library.viewer {
                viewer.name = buffer;
            }
            return;
        }
        if id == "variant-name" {
            self.update_modal(|m| {
                if let Modal::VariantName { name, .. } = m {
                    *name = buffer;
                }
            });
            return;
        }
        if id == "person-name" {
            self.update_modal(|m| {
                if let Modal::PersonName { name, .. } = m {
                    *name = buffer;
                }
            });
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if id.starts_with("metadata-") {
            self.update_modal(|m| {
                super::library_metadata::commit_field(m, id, buffer);
            });
            return;
        }
        if id == "bucket-name" || id == "bucket-query" {
            self.update_modal(|m| {
                if let Modal::BucketName { name, query, .. } = m {
                    if id == "bucket-name" {
                        *name = buffer;
                    } else {
                        *query = buffer;
                    }
                }
            });
            return;
        }
        if id == "cp-hex" {
            if let Some(c) = crate::color_picker::parse_hex(&buffer) {
                let typed = crate::color_picker::rgb_to_hsv(c.r, c.g, c.b);
                self.update_modal(|m| {
                    if let Modal::ColorPicker { hsv, .. } = m {
                        // A grey has no hue to report, so keep the one the
                        // dialog already had rather than snapping to red.
                        if typed.1 > 0.0 {
                            hsv.0 = typed.0;
                        }
                        hsv.1 = typed.1;
                        hsv.2 = typed.2;
                    }
                });
            }
            return;
        }
        if id.starts_with("cp-") {
            if let Ok(value) = buffer.parse::<f32>() {
                self.update_modal(|m| {
                    if let Modal::ColorPicker { hsv, .. } = m {
                        crate::color_picker::set_component(hsv, id, value);
                    }
                });
            }
            return;
        }
        let Ok(value) = self.field_buffer.parse::<f32>() else {
            return;
        };
        // Every remaining field is a dimension, and none of them accept
        // zero.
        let value = value.max(1.0);
        let aspect = self
            .doc
            .as_ref()
            .map(|d| d.width as f32 / d.height.max(1) as f32)
            .unwrap_or(1.0);
        self.update_modal(|m| match m {
            #[cfg(not(target_arch = "wasm32"))]
            Modal::VersionHistory => {},
            #[cfg(not(target_arch = "wasm32"))]
            Modal::PhotoMerge { .. } => {},
            Modal::Cloud {..} | Modal::CloudGenerate => {},
            Modal::ImageSize {
                width,
                height,
                link,
                ..
            } => {
                if id == "image-size-w" {
                    *width = value as u32;
                    if *link {
                        *height = (value / aspect).round().max(1.0) as u32;
                    }
                } else if id == "image-size-h" {
                    *height = value as u32;
                    if *link {
                        *width = (value * aspect).round().max(1.0) as u32;
                    }
                }
            }
            Modal::CanvasSize { width, height, .. } => {
                if id == "canvas-size-w" {
                    *width = value as u32;
                } else if id == "canvas-size-h" {
                    *height = value as u32;
                }
            }
            Modal::LayerProperties { name, .. } => {
                if id == "layer-name" {
                    *name = buffer;
                }
            }
            Modal::ContentAwareScale { width, height } => {
                if id == "cas-width" {
                    *width = value as u32;
                } else if id == "cas-height" {
                    *height = value as u32;
                }
            }
            Modal::NewDocument {
                width,
                height,
                resolution,
                ..
            } => {
                if id == "new-doc-w" {
                    *width = (value as u32).min(30000);
                } else if id == "new-doc-h" {
                    *height = (value as u32).min(30000);
                } else if id == "new-doc-dpi" {
                    *resolution = value;
                }
            }
            // These dialogs have no typed fields.
            Modal::DestructiveAdjustment { .. }
            | Modal::Workspaces { .. }
            | Modal::RecordedActions { .. }
            | Modal::RecordedActionBatch { .. }
            | Modal::Busy { .. }
            | Modal::ConfirmCloseTab
            | Modal::SharedImage { .. }
            | Modal::DropImage { .. }
            | Modal::DropFolders { .. }
            | Modal::HeifSupport { .. }
            | Modal::CameraImport { .. }
            | Modal::CameraImportOptions { .. }
            | Modal::CameraImportFailed { .. }
            | Modal::NewFilePicker { .. }
            | Modal::FilePicker
            | Modal::MapFilter
            | Modal::SearchModels
            | Modal::VariantName { .. }
            | Modal::PersonName { .. }
            | Modal::SaveImageAs { .. }
            | Modal::BatchProcess { .. }
            // Handled above, before the numeric parse, like the other
            // text fields.
            | Modal::BucketName { .. }
            | Modal::MetadataEdit { .. }
            | Modal::SpotInk
            | Modal::ModelManager
            | Modal::FilterGallery { .. }
            | Modal::Stroke { .. }
            | Modal::Fill { .. }
            | Modal::SelectModify { .. }
            | Modal::MaskRefine { .. }
            | Modal::ColorRange { .. }
            | Modal::LayerStyle { .. }
            | Modal::Filter { .. }
            | Modal::Adjustment { .. }
            // Handled above, before the numeric parse: a colour component
            // may legitimately be zero.
            | Modal::ColorPicker { .. }
            | Modal::PluginManager
            | Modal::Support
            | Modal::Preferences
            | Modal::Export { .. }
            | Modal::Printing { .. }
            | Modal::ExportRecipes { .. }
            | Modal::MissingFonts { .. }
            | Modal::UpdateAvailable { .. }
            | Modal::Profile { .. } => {}
        });
    }

    /// True when the active tool is capturing raw typing.
    pub fn tool_captures_keys(&mut self) -> bool {
        let id = self.editor.active_tool;
        self.registry
            .tool_mut(id)
            .map(|t| t.captures_keys())
            .unwrap_or(false)
    }

    /// Feed a keystroke to the active tool. Returns true if it consumed it.
    pub(super) fn tool_key(&mut self, ev: &gpui::KeyDownEvent) -> bool {
        let tool_id = self.editor.active_tool;
        let key = ev.keystroke.key.clone();
        let text = ev.keystroke.key_char.clone();
        let modifiers = Modifiers {
            shift: ev.keystroke.modifiers.shift,
            alt: ev.keystroke.modifiers.alt,
            ctrl_or_cmd: ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform,
        };
        let (Some(doc), Some(tool)) = (self.doc.as_mut(), self.registry.tool_mut(tool_id)) else {
            return false;
        };
        let mut ctx = ToolCtx {
            doc,
            state: &mut self.editor,
        };
        tool.on_key(&mut ctx, &key, text.as_deref(), modifiers)
    }

    /// Enter: let the active tool commit its pending gesture.
    pub fn commit_gesture(&mut self, cx: &mut Context<Self>) {
        self.commit_gesture_with_async(true, cx);
    }

    /// Finish a transform before another operation changes its source or recipe.
    /// Even a clean activation holds a snapshot which must not survive the edit.
    pub(super) fn commit_pending_transform(&mut self, cx: &mut Context<Self>) {
        if matches!(self.editor.active_tool, "transform" | "transform.selection") {
            // Also invalidates an already running result whose tool session
            // was consumed by an earlier asynchronous commit. Keep this at
            // the transform boundary so other tools' jobs remain unaffected.
            #[cfg(target_arch = "wasm32")]
            self.cancel_browser_edits();
            if let Some(tool) = self.registry.tool_mut(self.editor.active_tool) {
                let _ = tool.take_gpu_edit();
            }
            self.commit_gesture_with_async(false, cx);
        }
    }

    pub(super) fn commit_gesture_with_async(&mut self, _allow_async: bool, cx: &mut Context<Self>) {
        let tool_id = self.editor.active_tool;
        if let (Some(doc), Some(tool)) = (self.doc.as_mut(), self.registry.tool_mut(tool_id)) {
            if !_allow_async {
                tool.set_async_compute(false);
            }
            let transform = tool.action_transform(doc, &self.editor);
            let revision = doc.revision;
            let mut ctx = ToolCtx {
                doc,
                state: &mut self.editor,
            };
            #[cfg(target_arch = "wasm32")]
            tool.set_async_compute(_allow_async && !self.action_recorder.recording);
            tool.on_commit(&mut ctx);
            let recorded_transform = transform.filter(|_| ctx.doc.revision != revision);

            #[cfg(target_arch = "wasm32")]
            if let Some(request) = tool.take_gpu_edit() {
                self.queue_browser_edit(request, cx);
            }
            if let Some(params) = recorded_transform {
                self.record_action_step(recorded_actions::Step::Transform { params });
            }
        }
        self.after_change(cx);
    }

    pub fn cancel_gesture(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.cancel_background_removal() {
            cx.notify();
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.cancel_browser_edits();
        // Escape reaches here as the CancelGesture action, ahead of the
        // canvas key listener the rename normally types through.
        if self.layer_rename.is_some() {
            self.cancel_layer_rename(cx);
            return;
        }
        // Escape ends an open note but keeps what was typed. Unlike a
        // rename there is no draft to throw away: Photoshop's notes save
        // as you write them, so the only question escape answers is
        // whether the keyboard still belongs to the note.
        if self.note_edit.is_some() {
            self.commit_note_edit(cx);
            return;
        }
        // The model picker closes like any popup.
        if self.ai.model_menu {
            self.close_ai_model_menu(cx);
            return;
        }
        // Same shape for the AI prompt box: escape hands the keyboard
        // back and the draft stays put.
        if self.ai.input.active {
            self.ai.input.active = false;
            cx.notify();
            return;
        }
        if self.tool_flyout.is_some() {
            self.close_tool_flyout(cx);
            return;
        }
        if self.context_menu.is_some() {
            self.close_context_menu(cx);
            return;
        }
        // A dropdown is the innermost thing open, inside a dialog or
        // not: escape folds it up and leaves whatever it sits in alone.
        // It used to be checked after the modal, so escape on an open
        // dropdown in a dialog closed the whole dialog.
        if self.open_popup.is_some() {
            self.close_popup(cx);
            return;
        }
        // A focused field takes the escape first: it drops focus and
        // leaves the dialog up, which is what `field_key`'s "escape" arm
        // meant to do before `CancelGesture` -- bound in the
        // always-matching "Workspace" context -- got there ahead of it
        // and closed the whole dialog on the first press.
        if self.focused_field.is_some()
            && (self.modal.is_some()
                || self.type_field_option().is_some()
                || self.focused_field == Some(palettes::SEARCH_FIELD))
        {
            self.focused_field = None;
            self.field_buffer.clear();
            cx.notify();
            return;
        }
        // Not this one: the run is not ours to cancel, and dropping the
        // overlay would let the document be edited underneath something
        // that is about to write to it.
        if matches!(self.modal, Some(Modal::Busy { .. })) {
            return;
        }
        if self.modal.is_some() {
            self.close_modal(cx);
            return;
        }
        self.pointer_down = false;
        self.pan_last = None;
        let tool_id = self.editor.active_tool;
        if let (Some(doc), Some(tool)) = (self.doc.as_mut(), self.registry.tool_mut(tool_id)) {
            let mut ctx = ToolCtx {
                doc,
                state: &mut self.editor,
            };
            tool.on_cancel(&mut ctx);
        }
        self.after_change(cx);
    }
}

#[cfg(test)]
mod metadata_keyboard_tests {
    use super::modal_enter_confirms;

    #[test]
    fn metadata_caption_shift_enter_does_not_save_the_dialog() {
        let shift = gpui::Modifiers {
            shift: true,
            ..Default::default()
        };
        for field in ["metadata-caption", "cloud-meta-caption"] {
            assert!(!modal_enter_confirms(Some(field), "enter", shift));
            // The ordinary Save shortcut must still work, including after the
            // caption has inserted a newline and retained keyboard focus.
            assert!(modal_enter_confirms(
                Some(field),
                "enter",
                Default::default()
            ));
            assert!(!modal_enter_confirms(Some(field), "a", Default::default()));
            assert!(!modal_enter_confirms(Some(field), "tab", shift));
        }
    }

    #[test]
    fn metadata_caption_exception_does_not_change_other_save_shortcuts() {
        let shift = gpui::Modifiers {
            shift: true,
            ..Default::default()
        };
        for field in [None, Some("metadata-keywords"), Some("new-doc-name")] {
            assert!(modal_enter_confirms(field, "enter", shift));
            assert!(modal_enter_confirms(field, "enter", Default::default()));
        }
        for mods in [
            gpui::Modifiers {
                control: true,
                ..shift
            },
            gpui::Modifiers {
                platform: true,
                ..shift
            },
        ] {
            assert!(modal_enter_confirms(
                Some("metadata-caption"),
                "enter",
                mods
            ));
        }
    }
}
