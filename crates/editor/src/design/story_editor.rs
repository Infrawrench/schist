//! A separate text window sharing the layout document and its undo stack.
use crate::workspace::Workspace;
use gpui::{div, prelude::*, px, *};
use schist_layout::{Story, StoryId};
use schist_ui as ui;
use std::{ops::Range, sync::Arc};

pub struct StoryEditor {
    workspace: WeakEntity<Workspace>,
    session: Arc<()>,
    story: StoryId,
    base: Story,
    edit: ui::LineEdit,
    focus: FocusHandle,
    marked: Option<Range<usize>>,
    notice: String,
}

impl StoryEditor {
    fn new(workspace: Entity<Workspace>, story: StoryId, cx: &mut Context<Self>) -> Self {
        let ws = workspace.read(cx);
        let session = ws.design.session.clone();
        let base = ws.design.document.story(story).cloned().unwrap_or_default();
        let mut edit = ui::LineEdit::focused(base.text());
        edit.multiline = true;
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace: workspace.downgrade(),
            session,
            story,
            base,
            edit,
            focus: cx.focus_handle(),
            marked: None,
            notice: String::new(),
        }
    }

    fn sync(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        let ws = workspace.read(cx);
        if !Arc::ptr_eq(&self.session, &ws.design.session) {
            return false;
        }
        let Some(story) = ws.design.document.story(self.story) else {
            return false;
        };
        if *story != self.base {
            self.base = story.clone();
            self.edit.text = story.text();
            self.edit
                .move_caret(self.edit.cursor.min(self.edit.text.len()), false);
            self.marked = None;
        }
        true
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let accepted = workspace.update(cx, |ws, cx| {
            if !Arc::ptr_eq(&self.session, &ws.design.session)
                || ws.design.document.story(self.story) != Some(&self.base)
            {
                return false;
            }
            let changed = schist_layout::authoring::set_text(
                &mut ws.design.document,
                &mut ws.design.history,
                self.story,
                &self.edit.text,
            );
            if changed {
                ws.design.typing = None;
                cx.notify();
            }
            changed || self.edit.text == self.base.text()
        });
        if accepted {
            self.base = workspace
                .read(cx)
                .design
                .document
                .story(self.story)
                .cloned()
                .unwrap_or_default();
            self.notice.clear();
        } else {
            self.edit.text = self.base.text();
            self.edit.move_caret(self.edit.cursor, false);
            self.notice = schist_i18n::t("design.story_edit_refused").to_string();
            self.sync(cx);
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sync(cx) {
            return;
        }
        let modified = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
        if modified && event.keystroke.key == "w" {
            window.remove_window();
            return;
        }
        if modified && event.keystroke.key == "z" {
            self.marked = None;
            if let Some(workspace) = self.workspace.upgrade() {
                workspace.update(cx, |ws, cx| {
                    ws.design.undo_or_redo(event.keystroke.modifiers.shift);
                    cx.notify();
                });
                self.sync(cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key == "escape" && self.marked.take().is_some() {
            self.edit.text = self.base.text();
            self.edit.move_caret(self.edit.cursor, false);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let result =
            if !modified && !event.keystroke.modifiers.shift && event.keystroke.key == "tab" {
                self.edit.insert("\t");
                ui::LineEditKey::Changed
            } else {
                self.edit.key(event, cx)
            };
        match result {
            ui::LineEditKey::Changed => {
                self.marked = None;
                self.commit(cx);
                cx.stop_propagation();
            }
            ui::LineEditKey::Ignored => {}
            _ => {
                cx.notify();
                cx.stop_propagation();
            }
        }
    }
}

impl Render for StoryEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = self.sync(cx);
        let mut stories = Vec::new();
        let mut markers = Vec::new();
        if valid {
            if let Some(workspace) = self.workspace.upgrade() {
                let ws = workspace.read(cx);
                for index in 0..ws.design.document.stories.len() {
                    let story = StoryId(index as u32);
                    stories.push(
                        ui::Button::new(
                            ("story", index),
                            schist_i18n::tf!("design.story_number", number = index + 1),
                        )
                        .active(self.story == story)
                        .on_click(cx.listener(move |editor, _, _, cx| {
                            editor.story = story;
                            editor.base = Story::default();
                            editor.marked = None;
                            editor.edit.clear();
                            editor.edit.active = true;
                            editor.edit.multiline = true;
                            editor.sync(cx);
                            cx.notify();
                        })),
                    );
                }
                let composed =
                    schist_layout::compose::compose_story(&ws.design.document, self.story);
                let mut start = 0;
                for frame in &composed.frames {
                    let Some(object) = ws.design.document.object(frame.object) else {
                        continue;
                    };
                    let from = self.base.text()[..start].chars().count();
                    let to = self.base.text()[..frame.consumed_to].chars().count();
                    let label = schist_i18n::tf!(
                        "design.story_frame_range",
                        page = ws.design.document.page_number(object.page),
                        start = from,
                        end = to
                    );
                    markers.push(div().text_sm().child(label).child(if frame.lost {
                        schist_i18n::t("design.overflowed").to_string()
                    } else {
                        String::new()
                    }));
                    start = frame.consumed_to;
                }
            }
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        let input = gpui::canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)
            },
        )
        .absolute()
        .size_0();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(ui::palette().window_bg))
            .text_color(rgb(ui::palette().text))
            .track_focus(&self.focus)
            .key_context("DesignStoryEditor")
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .p_3()
                    .text_lg()
                    .child(schist_i18n::t("design.story_editor")),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("story-list")
                            .w(px(150.0))
                            .p_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .overflow_y_scroll()
                            .children(stories),
                    )
                    .child(
                        div()
                            .id("story-body")
                            .flex_1()
                            .min_w(px(0.0))
                            .p_3()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(markers)
                            .when(valid && self.base.retained_structures() > 0, |body| {
                                body.child(div().text_sm().child(schist_i18n::tf!(
                                    "design.story_structure_count",
                                    count = self.base.retained_structures()
                                )))
                            })
                            .when(!valid || !self.notice.is_empty(), |body| {
                                body.child(if valid {
                                    self.notice.clone()
                                } else {
                                    schist_i18n::t("design.story_session_closed").to_string()
                                })
                            })
                            .when(valid, |body| {
                                body.child(
                                    ui::TextInput::edit("story-text", &self.edit)
                                        .w_full()
                                        .min_h(px(240.0))
                                        .flex_none()
                                        .text_size(px(14.0))
                                        .on_focus(cx.listener(|editor, press, window, cx| {
                                            editor.edit.press(press);
                                            window.focus(&editor.focus);
                                            cx.notify();
                                        }))
                                        .on_select_to(cx.listener(|editor, at: &usize, _, cx| {
                                            editor.edit.extend_to(*at);
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(input)
    }
}

impl Workspace {
    pub fn open_story_editor(&mut self, story: StoryId, cx: &mut Context<Self>) {
        if self.design.document.story(story).is_none() {
            return;
        }
        self.commit_focused_field();
        self.design.cancel_gesture();
        let workspace = cx.entity().downgrade();
        let session = self.design.session.clone();
        // Both construction and the first synchronous window draw read the
        // workspace. Defer the entire open until this update has released it.
        // Deferring only StoryEditor::new would still panic in render/sync.
        cx.defer(move |cx| {
            let Some(workspace) = workspace.upgrade() else {
                return;
            };
            if !Arc::ptr_eq(&session, &workspace.read(cx).design.session)
                || workspace.read(cx).design.document.story(story).is_none()
            {
                return;
            }
            let editor_workspace = workspace.clone();
            let bounds = Bounds::centered(None, size(px(820.0), px(600.0)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(schist_i18n::t("design.story_editor").into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let editor = cx.new(|cx| StoryEditor::new(editor_workspace, story, cx));
                    window.focus(&editor.read(cx).focus);
                    editor
                },
            ) {
                workspace.update(cx, |workspace, cx| {
                    workspace.status =
                        schist_i18n::tf!("design.story_window_failed", error = error).into();
                    cx.notify();
                });
            }
        });
        cx.notify();
    }
}

fn byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return at;
        }
        units += ch.len_utf16();
    }
    text.len()
}
fn units(text: &str, at: usize) -> usize {
    text[..at].encode_utf16().count()
}

impl EntityInputHandler for StoryEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range =
            byte(&self.edit.text, range.start.min(range.end))..byte(&self.edit.text, range.end);
        *adjusted = Some(units(&self.edit.text, range.start)..units(&self.edit.text, range.end));
        Some(self.edit.text[range].to_string())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let range = self.edit.selection();
        Some(UTF16Selection {
            range: units(&self.edit.text, range.start)..units(&self.edit.text, range.end),
            reversed: self.edit.cursor < self.edit.anchor,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|r| units(&self.edit.text, r.start)..units(&self.edit.text, r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            self.commit(cx);
        }
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sync(cx) {
            return;
        }
        let range = range
            .map(|r| byte(&self.edit.text, r.start)..byte(&self.edit.text, r.end))
            .or_else(|| self.marked.take())
            .unwrap_or_else(|| self.edit.selection());
        self.edit.anchor = range.start;
        self.edit.cursor = range.end;
        self.edit.insert(text);
        self.marked = None;
        self.commit(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sync(cx) {
            return;
        }
        let range = range
            .map(|r| byte(&self.edit.text, r.start)..byte(&self.edit.text, r.end))
            .or_else(|| self.marked.take())
            .unwrap_or_else(|| self.edit.selection());
        let start = range.start;
        self.edit.anchor = start;
        self.edit.cursor = range.end;
        self.edit.insert(text);
        self.marked = Some(start..start + text.len());
        if let Some(selected) = selected {
            self.edit.anchor = start + byte(text, selected.start);
            self.edit.cursor = start + byte(text, selected.end);
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
