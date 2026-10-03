//! Compact shared-variable authoring. Draft fields never mutate the document;
//! Save, Insert and each explicit removal are separate reversible gestures.
use super::{
    story_editor::{byte, units},
    DesignState,
};
use crate::workspace::Workspace;
use gpui::{prelude::*, *};
use schist_i18n::t;
use schist_layout::text_variables::{self as variables, Cursor, TextVariable};
use schist_ui as ui;
use std::{ops::Range, sync::Arc};

struct Draft {
    expected: Option<TextVariable>,
    fields: [ui::LineEdit; 2],
    active: usize,
}

impl Draft {
    fn new(expected: Option<TextVariable>) -> Self {
        let (name, contents) = expected
            .as_ref()
            .map(|v| (v.name.clone(), v.contents.clone()))
            .unwrap_or_default();
        let mut fields = [ui::LineEdit::focused(name), ui::LineEdit::focused(contents)];
        fields[0].select_all();
        fields[1].active = false;
        Self {
            expected,
            fields,
            active: 0,
        }
    }

    fn valid(&self) -> bool {
        !self.fields[0].text.trim().is_empty()
            && !self.fields[0].text.chars().any(char::is_control)
            && variables::valid_contents(&self.fields[1].text)
    }
}

pub struct TextVariables {
    workspace: WeakEntity<Workspace>,
    session: Arc<()>,
    selected: Option<String>,
    cursor: Option<Cursor>,
    draft: Option<Draft>,
    focus: FocusHandle,
    marked: Option<Range<usize>>,
    notice: String,
}

fn cursor(ws: &Workspace) -> Option<Cursor> {
    let typing = ws.design.typing?;
    (typing.at == typing.anchor)
        .then(|| Cursor::capture(&ws.design.document, typing.story, typing.at))
        .flatten()
}

impl TextVariables {
    fn new(workspace: Entity<Workspace>, cursor: Option<Cursor>, cx: &mut Context<Self>) -> Self {
        let ws = workspace.read(cx);
        let session = ws.design.session.clone();
        let selected = ws
            .design
            .document
            .text_variables
            .first()
            .map(|v| v.id.clone());
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace: workspace.downgrade(),
            session,
            cursor,
            selected,
            draft: None,
            focus: cx.focus_handle(),
            marked: None,
            notice: String::new(),
        }
    }

    fn valid(&self, cx: &App) -> bool {
        self.workspace.upgrade().is_some_and(|workspace| {
            let ws = workspace.read(cx);
            ws.design_mode() && Arc::ptr_eq(&self.session, &ws.design.session)
        })
    }

    fn selected(&self, cx: &App) -> Option<TextVariable> {
        if !self.valid(cx) {
            return None;
        }
        let workspace = self.workspace.upgrade()?;
        let definitions = &workspace.read(cx).design.document.text_variables;
        let mut matches = definitions
            .iter()
            .filter(|d| Some(&d.id) == self.selected.as_ref());
        let selected = matches.next()?.clone();
        matches.next().is_none().then_some(selected)
    }

    fn apply(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut DesignState) -> bool,
    ) -> bool {
        if !self.valid(cx) {
            return false;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        // Refresh only a cursor that was valid before our own operation. A stale
        // captured story must never become writable merely by editing a resource.
        let refresh = self
            .cursor
            .as_ref()
            .is_some_and(|c| c.valid(&workspace.read(cx).design.document));
        let changed = workspace.update(cx, |ws, cx| {
            let changed = action(&mut ws.design);
            if changed {
                cx.notify();
            }
            changed
        });
        if changed {
            if refresh {
                self.cursor = self.cursor.as_ref().and_then(|c| {
                    Cursor::capture(&workspace.read(cx).design.document, c.story, c.at)
                });
            }
            self.notice.clear();
        } else {
            self.notice = t("design.variable_edit_refused").into();
        }
        cx.notify();
        changed
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = &self.draft else {
            return;
        };
        if !draft.valid() || self.marked.is_some() {
            return;
        }
        let expected = draft.expected.clone();
        let name = draft.fields[0].text.clone();
        let contents = draft.fields[1].text.clone();
        if self.valid(cx)
            && expected.as_ref().is_some_and(|e| {
                self.selected(cx).as_ref() == Some(e) && e.name == name && e.contents == contents
            })
        {
            self.draft = None;
            self.notice.clear();
            cx.notify();
            return;
        }
        let mut selected = None;
        if self.apply(cx, |state| {
            if let Some(expected) = &expected {
                selected = Some(expected.id.clone());
                variables::update(
                    &mut state.document,
                    &mut state.history,
                    expected,
                    &name,
                    &contents,
                )
            } else {
                selected =
                    variables::create(&mut state.document, &mut state.history, &name, &contents);
                selected.is_some()
            }
        }) {
            self.selected = selected;
            self.draft = None;
            self.marked = None;
        }
    }

    fn start(
        &mut self,
        expected: Option<TextVariable>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(cx) {
            return;
        }
        self.draft = Some(Draft::new(expected));
        self.marked = None;
        self.notice.clear();
        window.focus(&self.focus);
        cx.notify();
    }

    fn edit(&mut self) -> Option<&mut ui::LineEdit> {
        let draft = self.draft.as_mut()?;
        Some(&mut draft.fields[draft.active])
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modified = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
        let key = event.keystroke.key.as_str();
        if modified && key == "w" {
            window.remove_window();
            cx.stop_propagation();
            return;
        }
        if key == "escape" {
            if self.draft.take().is_none() {
                window.remove_window();
            }
            self.marked = None;
            self.notice.clear();
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if !self.valid(cx) {
            return;
        }
        if self.draft.is_none() && modified && key == "z" {
            self.apply(cx, |state| {
                state.undo_or_redo(event.keystroke.modifiers.shift)
            });
            self.cursor = None;
            cx.stop_propagation();
            return;
        }
        if self.marked.is_some() {
            return;
        }
        if key == "tab" && !modified && self.marked.is_none() {
            if let Some(draft) = &mut self.draft {
                draft.fields[draft.active].active = false;
                draft.active = 1 - draft.active;
                draft.fields[draft.active].focus();
                draft.fields[draft.active].select_all();
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        let Some(edit) = self.edit() else {
            return;
        };
        match edit.key(event, cx) {
            ui::LineEditKey::Submitted => {
                self.save(cx);
                if let Some(edit) = self.edit() {
                    edit.active = true;
                }
            }
            ui::LineEditKey::Ignored => return,
            _ => {
                self.marked = None;
                cx.notify();
            }
        }
        cx.stop_propagation();
    }
}

impl Render for TextVariables {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let valid = self.valid(cx);
        let editing = self.draft.is_some();
        let selected = self.selected(cx);
        let mut rows = Vec::new();
        let mut occurrences = Vec::new();
        let mut used = false;
        let mut cursor_valid = false;
        if valid {
            let workspace = self.workspace.upgrade().expect("live workspace");
            let doc = &workspace.read(cx).design.document;
            used = selected
                .as_ref()
                .is_some_and(|v| variables::usage_count(doc, &v.id) > 0);
            cursor_valid = self.cursor.as_ref().is_some_and(|c| c.valid(doc));
            for (index, definition) in doc.text_variables.iter().enumerate() {
                let id = definition.id.clone();
                rows.push(
                    div()
                        .id(("variable", index))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .h(px(28.0))
                        .min_w_0()
                        .when(self.selected.as_ref() == Some(&id), |row| {
                            row.bg(rgb(ui::palette().selection_bg))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(definition.name.clone()),
                        )
                        .child(
                            div()
                                .w(px(220.0))
                                .min_w_0()
                                .truncate()
                                .text_color(rgb(ui::palette().text_dim))
                                .child(definition.contents.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.draft.is_none() {
                                this.selected = Some(id.clone());
                                this.notice.clear();
                                cx.notify();
                            }
                        }))
                        .into_any_element(),
                );
            }
            if let Some(cursor) = self.cursor.as_ref().filter(|_| cursor_valid) {
                let target = Arc::new(cursor.clone());
                for (index, structure) in
                    cursor
                        .expected
                        .structures
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            s.at == Some(cursor.at)
                                && s.kind == "TextVariableInstance"
                                && s.footnote.is_none()
                        })
                {
                    let Some(schist_layout::story::InlineControl::TextVariable {
                        variable,
                        name,
                        ..
                    }) = &structure.control
                    else {
                        continue;
                    };
                    let label = doc
                        .text_variables
                        .iter()
                        .find(|d| d.id == *variable)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| {
                            if name.is_empty() {
                                t("common.unknown").into()
                            } else {
                                name.clone()
                            }
                        });
                    let target = target.clone();
                    occurrences.push(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .child(
                                ui::IconButton::new(("remove-variable-instance", index), "trash")
                                    .tooltip(t("design.variable_remove_instance"), None)
                                    .disabled(editing)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.apply(cx, |state| {
                                            target.remove_instance(
                                                &mut state.document,
                                                &mut state.history,
                                                index,
                                            )
                                        });
                                    })),
                            )
                            .into_any_element(),
                    );
                }
            }
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        let input = canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx)
            },
        )
        .absolute()
        .size_0();
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(ui::palette().window_bg))
            .text_color(rgb(ui::palette().text))
            .text_xs()
            .track_focus(&self.focus)
            .key_context("DesignTextVariables")
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .border_b_1()
                    .border_color(rgb(ui::palette().panel_edge))
                    .child(div().flex_1().child(t("design.text_variables")))
                    .child(
                        ui::IconButton::new("new-variable", "plus")
                            .tooltip(t("design.variable_new"), None)
                            .disabled(!valid || editing)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.start(None, window, cx)),
                            ),
                    )
                    .child(
                        ui::IconButton::new("edit-variable", "pencil")
                            .tooltip(t("common.edit"), None)
                            .disabled(selected.is_none() || editing)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(value) = this.selected(cx) {
                                    this.start(Some(value), window, cx);
                                }
                            })),
                    )
                    .child(
                        ui::IconButton::new("delete-variable", "trash")
                            .tooltip(
                                t(if used {
                                    "design.variable_in_use"
                                } else {
                                    "common.delete"
                                }),
                                None,
                            )
                            .disabled(selected.is_none() || used || editing)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(value) = this.selected(cx) {
                                    this.apply(cx, |state| {
                                        variables::remove(
                                            &mut state.document,
                                            &mut state.history,
                                            &value,
                                        )
                                    });
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .id("variable-list")
                    .flex_1()
                    .min_h(px(60.0))
                    .overflow_y_scroll()
                    .p_1()
                    .children(rows),
            );
        if let Some(draft) = &self.draft {
            let mut fields = div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .border_t_1()
                .border_color(rgb(ui::palette().panel_edge));
            for index in 0..2 {
                fields = fields.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(60.0)).child(t(if index == 0 {
                            "common.name"
                        } else {
                            "common.text"
                        })))
                        .child(
                            ui::TextInput::edit(("variable-field", index), &draft.fields[index])
                                .flex_1()
                                .min_w_0()
                                .on_focus(cx.listener(move |this, press, window, cx| {
                                    if let Some(draft) = &mut this.draft {
                                        draft.fields[draft.active].active = false;
                                        draft.active = index;
                                        draft.fields[index].active = true;
                                        draft.fields[index].press(press);
                                    }
                                    this.marked = None;
                                    window.focus(&this.focus);
                                    cx.notify();
                                }))
                                .on_select_to(cx.listener(move |this, at: &usize, _, cx| {
                                    if let Some(draft) = &mut this.draft {
                                        draft.fields[index].extend_to(*at);
                                        cx.notify();
                                    }
                                })),
                        ),
                );
            }
            if !draft.valid() {
                fields = fields.child(
                    div()
                        .text_color(rgb(ui::palette().text_dim))
                        .child(t("design.variable_invalid")),
                );
            }
            body = body.child(
                fields.child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            ui::Button::new("cancel-variable", t("common.cancel")).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.draft = None;
                                    this.marked = None;
                                    this.notice.clear();
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            ui::Button::new("save-variable", t("common.save"))
                                .variant(ui::ButtonVariant::Primary)
                                .disabled(!valid || !draft.valid() || self.marked.is_some())
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                ),
            );
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .border_t_1()
                    .border_color(rgb(ui::palette().panel_edge))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(rgb(ui::palette().text_dim))
                                    .child(t(if cursor_valid {
                                        "design.variable_at_cursor"
                                    } else {
                                        "design.variable_cursor_required"
                                    })),
                            )
                            .child(
                                ui::IconButton::new("capture-variable-cursor", "type")
                                    .tooltip(t("design.variable_capture_cursor"), None)
                                    .disabled(!valid)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.valid(cx) {
                                            if let Some(ws) = this.workspace.upgrade() {
                                                this.cursor = cursor(ws.read(cx));
                                                this.notice.clear();
                                                cx.notify();
                                            }
                                        }
                                    })),
                            )
                            .child(
                                ui::Button::new("insert-variable", t("design.variable_insert"))
                                    .disabled(
                                        !cursor_valid
                                            || selected.as_ref().is_none_or(|v| {
                                                !variables::valid_contents(&v.contents)
                                            }),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let (Some(cursor), Some(value)) =
                                            (this.cursor.clone(), this.selected(cx))
                                        {
                                            this.apply(cx, |state| {
                                                cursor.insert(
                                                    &mut state.document,
                                                    &mut state.history,
                                                    &value,
                                                )
                                            });
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("variable-instances")
                            .max_h(px(100.0))
                            .overflow_y_scroll()
                            .children(occurrences),
                    ),
            );
        }
        body.when(!valid || !self.notice.is_empty(), |body| {
            body.child(div().p_2().child(if valid {
                self.notice.clone()
            } else {
                t("design.variable_edit_refused").into()
            }))
        })
        .when(editing && valid, |body| body.child(input))
    }
}

impl Workspace {
    pub fn open_text_variables(&mut self, cx: &mut Context<Self>) {
        if !self.design_mode() {
            return;
        }
        self.commit_focused_field();
        let captured = cursor(self);
        self.design.cancel_gesture();
        let workspace = cx.entity().downgrade();
        let session = self.design.session.clone();
        cx.defer(move |cx| {
            let Some(workspace) = workspace.upgrade() else {
                return;
            };
            if !workspace.read(cx).design_mode()
                || !Arc::ptr_eq(&session, &workspace.read(cx).design.session)
            {
                return;
            }
            let owner = workspace.clone();
            let bounds = Bounds::centered(None, size(px(540.0), px(390.0)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(t("design.text_variables").into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| TextVariables::new(owner, captured, cx));
                    window.focus(&view.read(cx).focus);
                    view
                },
            ) {
                workspace.update(cx, |ws, cx| {
                    ws.status =
                        schist_i18n::tf!("design.variable_window_failed", error = error).into();
                    cx.notify();
                });
            }
        });
        cx.notify();
    }
}

impl EntityInputHandler for TextVariables {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let edit = self.edit()?;
        let range = byte(&edit.text, range.start.min(range.end))..byte(&edit.text, range.end);
        *adjusted = Some(units(&edit.text, range.start)..units(&edit.text, range.end));
        Some(edit.text[range].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let edit = self.edit()?;
        let range = edit.selection();
        Some(UTF16Selection {
            range: units(&edit.text, range.start)..units(&edit.text, range.end),
            reversed: edit.cursor < edit.anchor,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let draft = self.draft.as_ref()?;
        let text = &draft.fields[draft.active].text;
        self.marked
            .as_ref()
            .map(|r| units(text, r.start)..units(text, r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(cx) {
            return;
        }
        let marked = self.marked.take();
        let Some(edit) = self.edit() else {
            return;
        };
        let range = range
            .map(|r| byte(&edit.text, r.start.min(r.end))..byte(&edit.text, r.end))
            .or(marked)
            .unwrap_or_else(|| edit.selection());
        edit.anchor = range.start;
        edit.cursor = range.end;
        edit.insert(text);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(cx) {
            return;
        }
        let marked = self.marked.take();
        let Some(edit) = self.edit() else {
            return;
        };
        let range = range
            .map(|r| byte(&edit.text, r.start.min(r.end))..byte(&edit.text, r.end))
            .or(marked)
            .unwrap_or_else(|| edit.selection());
        let start = range.start;
        edit.anchor = start;
        edit.cursor = range.end;
        edit.insert(text);
        if let Some(selected) = selected {
            edit.anchor = start + byte(text, selected.start.min(selected.end));
            edit.cursor = start + byte(text, selected.end);
        }
        self.marked = Some(start..start + text.len());
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
