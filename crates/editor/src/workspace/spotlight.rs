//! Global search. Owns its focus and query so dismissing it leaves the
//! editor and gallery exactly where they were. Menu actions use the same
//! dispatch as the menu bar; photo ranking shares the gallery's engine.
use super::*;
use crate::panels::MenuEntry;
use crate::ui::{self, LineEdit, LineEditKey};
use gpui::{
    prelude::FluentBuilder as _, ScrollHandle, StatefulInteractiveElement as _, StyledImage as _,
};
use schist_i18n::{t, tn};
use schist_ui::{icon, IconButton, TextInput, TextInputColors};

const MAX_RESULTS: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    All,
    Actions,
    Documents,
    Layers,
    Photos,
}
impl Category {
    fn label(self) -> &'static str {
        t(match self {
            Self::All => "common.all",
            Self::Actions => "common.edit",
            Self::Documents => "common.documents",
            Self::Layers => "common.layers",
            Self::Photos => "common.photos",
        })
    }
}

#[derive(Clone, PartialEq)]
enum Target {
    App(AppItem),
    Command(&'static str),
    Tool(&'static str),
    Filter(&'static str),
    Adjustment(schist_core::AdjustmentKind),
    Tab(usize),
    Layer(schist_core::LayerId),
    #[cfg(not(target_arch = "wasm32"))]
    Photo(PathBuf),
    #[cfg(not(target_arch = "wasm32"))]
    Recent(PathBuf),
    CloudPhoto(Box<schist_cloud::Asset>),
    #[cfg(not(target_arch = "wasm32"))]
    SearchPhotos,
    SearchCloud,
}

impl Target {
    fn needs_document(&self) -> bool {
        match self {
            Self::Command(_) | Self::Filter(_) | Self::Adjustment(_) | Self::Layer(_) => true,
            Self::App(item) => {
                use AppItem::*;
                !matches!(
                    item,
                    Workspaces
                        | WorkspaceSave
                        | WorkspaceUpdate
                        | WorkspaceRename
                        | WorkspaceDelete
                        | WorkspaceReset
                        | WorkspaceStarter(_)
                        | WorkspaceSelect(_)
                        | Search
                        | CloudSignIn
                        | CloudGenerate
                        | CloudBrowse
                        | CloudSignOut
                        | New
                        | Open
                        | Quit
                        | Plugins
                        | Preferences
                        | CheckForUpdates
                        | ManageModels
                        | ManageFonts
                        | ToggleRulers
                        | ToggleGrid
                        | ToggleGuides
                        | ToggleNotes
                        | ToggleExtras
                        | ToggleSnap
                        | ToggleAi
                        | ScreenModeItem
                        | OpenGallery
                        | GalleryAddFolder
                        | GalleryImportCamera
                        | GalleryRefresh
                        | GalleryEditSelected
                        | GalleryMapFilter
                        | OpenRecent(_)
                )
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
struct SearchResult {
    label: String,
    detail: String,
    hint: String,
    icon: &'static str,
    category: Category,
    target: Target,
}
impl SearchResult {
    fn new(
        label: impl Into<String>,
        detail: impl Into<String>,
        icon: &'static str,
        category: Category,
        target: Target,
    ) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            icon,
            category,
            target,
            hint: String::new(),
        }
    }
}

pub(crate) struct Spotlight {
    pub open: bool,
    pub input: LineEdit,
    pub focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    candidates: Vec<SearchResult>,
    results: Vec<SearchResult>,
    category: Category,
    selected: usize,
    navigated: bool,
    scroll: ScrollHandle,
    seq: u64,
    #[cfg(not(target_arch = "wasm32"))]
    photos: Vec<(PathBuf, f32)>,
    searching: bool,
}
impl Spotlight {
    pub fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            input: LineEdit::default(),
            focus,
            previous_focus: None,
            candidates: Vec::new(),
            results: Vec::new(),
            category: Category::All,
            selected: 0,
            navigated: false,
            scroll: ScrollHandle::new(),
            seq: 0,
            #[cfg(not(target_arch = "wasm32"))]
            photos: Vec::new(),
            searching: false,
        }
    }
}

/// A literal match outranks a word prefix, substring, then an abbreviation.
/// Work in Unicode characters: byte offsets cannot measure fuzzy gaps.
fn match_score(query: &str, label: &str, detail: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let label = label.to_lowercase();
    if label.trim_end_matches(['…', '.']) == query {
        return Some(0);
    }
    if label.starts_with(&query) {
        return Some(10);
    }
    if label.contains(&query) {
        return Some(20);
    }
    let detail = detail.to_lowercase();
    let mut score = 30;
    for word in query.split_whitespace() {
        if label
            .split(|c: char| !c.is_alphanumeric())
            .any(|part| part.starts_with(word))
        {
            score += 1;
        } else if label.contains(word) {
            score += 5;
        } else if detail.contains(word) {
            score += 20;
        } else {
            // Short abbreviations are useful for commands, but a lone
            // unrelated letter should not match every description.
            if word.chars().count() < 2 {
                return None;
            }
            let mut chars = label.chars();
            for c in word.chars() {
                let gap = chars.by_ref().position(|next| next == c)?;
                score += 10 + gap;
            }
        }
    }
    Some(score)
}

fn menu_candidates(
    ws: &Workspace,
    entries: Vec<MenuEntry>,
    path: &str,
    out: &mut Vec<SearchResult>,
) {
    for entry in entries {
        let mut result = match entry {
            MenuEntry::Sub(label, children) => {
                menu_candidates(ws, children, &format!("{path} › {label}"), out);
                continue;
            }
            MenuEntry::App(_, AppItem::Search | AppItem::OpenRecent(_), _)
            | MenuEntry::Dynamic(_, AppItem::OpenRecent(_))
            | MenuEntry::Sep => continue,
            MenuEntry::App(label, item, kb) => {
                let mut r = SearchResult::new(
                    label,
                    path,
                    "settings",
                    Category::Actions,
                    Target::App(item),
                );
                r.hint = panels::keybind_hint(kb);
                r
            }
            MenuEntry::Dynamic(label, item) => {
                SearchResult::new(label, path, "folder", Category::Actions, Target::App(item))
            }
            MenuEntry::Cmd(id) => {
                let Some(c) = ws.registry.command(id) else {
                    continue;
                };
                let mut r = SearchResult::new(
                    c.title,
                    path,
                    "adjust",
                    Category::Actions,
                    Target::Command(id),
                );
                r.hint = panels::keybind_hint(c.keybind);
                r
            }
            MenuEntry::Filter(id) => SearchResult::new(
                panels::filter_menu_label(ws, id),
                path,
                "filter",
                Category::Actions,
                Target::Filter(id),
            ),
            MenuEntry::Adjustment(kind) => SearchResult::new(
                ui::adjustment_name(kind),
                path,
                "adjust",
                Category::Actions,
                Target::Adjustment(kind),
            ),
        };
        // An id can appear in several menus; one result is enough.
        if out.iter().any(|r| r.target == result.target) {
            continue;
        }
        result.label = result.label.trim_end_matches('…').to_string();
        out.push(result);
    }
}

impl Workspace {
    pub(crate) fn show_spotlight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            return;
        }
        if self.spotlight.open {
            self.dismiss_spotlight(window, cx);
            return;
        }
        self.close_popup(cx);
        self.close_context_menu(cx);
        self.close_tool_flyout(cx);
        self.cloud.context = None;
        self.gallery_more = None;
        self.ai.model_menu = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.context = None;
        }

        self.spotlight.previous_focus = window.focused(cx);
        self.spotlight.open = true;
        self.spotlight.input = LineEdit::focused("");
        self.spotlight.category = Category::All;
        self.spotlight.scroll = ScrollHandle::new();
        let mut candidates = Vec::new();
        for (index, (title, _)) in self.tab_strip().into_iter().enumerate() {
            candidates.push(SearchResult::new(
                title.to_string(),
                t("common.document"),
                "folder",
                Category::Documents,
                Target::Tab(index),
            ));
        }
        for (path, entries) in panels::search_menus(self) {
            menu_candidates(self, entries, path, &mut candidates);
        }
        for tool in self.registry.tools() {
            let mut result = SearchResult::new(
                tool.name(),
                tool.description(),
                tool.icon(),
                Category::Actions,
                Target::Tool(tool.id()),
            );
            result.hint = panels::keybind_hint(tool.shortcut());
            candidates.push(result);
        }
        // Include commands installed by plugins even when they have no menu placement.
        for command in self.registry.commands() {
            if !candidates
                .iter()
                .any(|r| matches!(r.target, Target::Command(id) if id == command.id))
            {
                let mut result = SearchResult::new(
                    command.title,
                    command.description,
                    "adjust",
                    Category::Actions,
                    Target::Command(command.id),
                );
                result.hint = panels::keybind_hint(command.keybind);
                candidates.push(result);
            }
        }
        for filter in self.registry.filters() {
            if !candidates
                .iter()
                .any(|r| matches!(r.target, Target::Filter(id) if id == filter.id()))
            {
                candidates.push(SearchResult::new(
                    filter.name(),
                    t("menu.filter"),
                    "filter",
                    Category::Actions,
                    Target::Filter(filter.id()),
                ));
            }
        }
        if let Some(doc) = &self.doc {
            for layer in doc.tree.iter() {
                candidates.push(SearchResult::new(
                    &layer.name,
                    &doc.title,
                    "layer-new",
                    Category::Layers,
                    Target::Layer(layer.id),
                ));
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut paths = FxHashSet::default();
            for path in &self.library.recents {
                if self.view.gallery_hide_nsfw && self.library.is_flagged(path) {
                    continue;
                }
                let mut result = photo_result(path);
                result.category = Category::Documents;
                result.icon = "folder";
                result.target = Target::Recent(path.clone());
                candidates.push(result);
            }
            for path in self
                .library
                .sections
                .iter()
                .flat_map(|s| s.entries.iter().map(|e| &e.path))
            {
                if !paths.insert(path.clone())
                    || (self.view.gallery_hide_nsfw && self.library.is_flagged(path))
                {
                    continue;
                }
                candidates.push(photo_result(path));
            }
        }
        for asset in self
            .cloud
            .assets
            .iter()
            .filter(|_| crate::feature_enabled("schist-cloud"))
        {
            let mut detail = vec![t("menu.file.schist_cloud").to_string()];
            detail.extend(asset.place_name.iter().cloned());
            detail.extend(asset.tags.iter().cloned());
            candidates.push(SearchResult::new(
                &asset.name,
                detail.join(" · "),
                "image-size",
                Category::Photos,
                Target::CloudPhoto(Box::new(asset.clone())),
            ));
        }
        self.spotlight.candidates = candidates;
        self.spotlight_changed(cx);
        window.focus(&self.spotlight.focus);
    }

    pub(crate) fn dismiss_spotlight(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.spotlight.open {
            return false;
        }
        self.spotlight.open = false;
        self.spotlight.input.active = false;
        self.spotlight.seq += 1;
        self.spotlight.candidates.clear();
        self.spotlight.results.clear();
        let focus = self
            .spotlight
            .previous_focus
            .take()
            .unwrap_or_else(|| self.focus.clone());
        window.focus(&focus);
        cx.notify();
        true
    }

    fn spotlight_changed(&mut self, cx: &mut Context<Self>) {
        self.spotlight.seq += 1;
        self.spotlight.selected = 0;
        self.spotlight.navigated = false;
        self.spotlight.scroll.set_offset(point(px(0.0), px(0.0)));
        self.reset_caret_phase();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.spotlight.photos.clear();
            let query = self.spotlight.input.text.trim().to_string();
            self.spotlight.searching = !query.is_empty() && !self.library.sections.is_empty();
            if self.spotlight.searching {
                let seq = self.spotlight.seq;
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(180))
                        .await;
                    let _ = this.update(cx, |ws, cx| {
                        if ws.spotlight.open && ws.spotlight.seq == seq {
                            ws.search_photo_query(query, seq, true, cx);
                        }
                    });
                })
                .detach();
            }
        }
        self.refresh_spotlight_results();
        cx.notify();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn spotlight_photos_ready(
        &mut self,
        seq: u64,
        photos: Vec<(PathBuf, f32)>,
        cx: &mut Context<Self>,
    ) {
        if !self.spotlight.open || self.spotlight.seq != seq {
            return;
        }
        self.spotlight.searching = false;
        self.spotlight.photos = photos;
        // Keep an explicit keyboard choice, but otherwise select the
        // best match when photos arrive after the immediate name matches.
        let selected = if self.spotlight.navigated {
            self.spotlight
                .results
                .get(self.spotlight.selected)
                .map(|r| r.target.clone())
        } else {
            None
        };
        self.refresh_spotlight_results();
        if let Some(target) = selected {
            if let Some(index) = self
                .spotlight
                .results
                .iter()
                .position(|r| r.target == target)
            {
                self.spotlight.selected = index;
            }
        }
        if !self.spotlight.navigated {
            self.spotlight.selected = 0;
        }
        self.spotlight
            .scroll
            .scroll_to_item(self.spotlight.selected);
        cx.notify();
    }

    fn refresh_spotlight_results(&mut self) {
        let query = self.spotlight.input.text.trim();
        let category = self.spotlight.category;
        let accepts = |c| category == Category::All || category == c;
        let mut ranked: Vec<_> = self
            .spotlight
            .candidates
            .iter()
            .filter(|r| accepts(r.category))
            .filter_map(|r| match_score(query, &r.label, &r.detail).map(|score| (score, r.clone())))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        if accepts(Category::Photos) {
            for (index, (path, _)) in self.spotlight.photos.iter().enumerate() {
                if self.view.gallery_hide_nsfw && self.library.is_flagged(path) {
                    continue;
                }
                if !ranked
                    .iter()
                    .any(|(_, r)| matches!(&r.target, Target::Photo(p) if p == path))
                {
                    ranked.push((25 + index, photo_result(path)));
                }
            }
        }
        ranked.sort_by_key(|(score, _)| *score);
        ranked.truncate(MAX_RESULTS);
        let mut results: Vec<_> = ranked.into_iter().map(|(_, result)| result).collect();
        if !query.is_empty() && accepts(Category::Photos) {
            #[cfg(not(target_arch = "wasm32"))]
            results.push(SearchResult::new(
                query,
                t("library.search.placeholder"),
                "search",
                Category::Photos,
                Target::SearchPhotos,
            ));
            if crate::feature_enabled("schist-cloud") && self.cloud.account.is_some() {
                results.push(SearchResult::new(
                    query,
                    t("menu.file.schist_cloud"),
                    "search",
                    Category::Photos,
                    Target::SearchCloud,
                ));
            }
        }
        self.spotlight.results = results;
        self.spotlight.selected = self
            .spotlight
            .selected
            .min(self.spotlight.results.len().saturating_sub(1));
    }

    fn choose_spotlight(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(result) = self.spotlight.results.get(index).cloned() else {
            return;
        };
        let query = self.spotlight.input.text.trim().to_string();
        // Document actions keep the palette open with an explanation if
        // there is no document yet, instead of silently doing nothing.
        if self.doc.is_none() && result.target.needs_document() {
            self.status = t("common.no_document_open").into();
            cx.notify();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let photo_matches = if result.target == Target::SearchPhotos {
            self.spotlight
                .candidates
                .iter()
                .filter_map(|candidate| {
                    if let Target::Photo(path) = &candidate.target {
                        match_score(&query, &candidate.label, &candidate.detail)
                            .map(|_| (path.clone(), 1.0))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.dismiss_spotlight(window, cx);
        self.commit_focused_field();
        self.commit_layer_rename(cx);
        self.commit_note_edit(cx);
        match result.target {
            Target::App(item) => {
                if Target::App(item).needs_document() {
                    self.cloud_set_visible(false);
                }
                panels::run_app_item(self, item, window, cx);
            }
            Target::Command(id) => {
                self.cloud_set_visible(false);
                self.run_command(id, cx);
            }
            Target::Tool(id) => {
                self.cloud_set_visible(false);
                self.activate_tool(id, cx);
            }
            Target::Filter(id) => {
                self.cloud_set_visible(false);
                self.open_filter_dialog(id, cx);
            }
            Target::Adjustment(kind) => {
                self.cloud_set_visible(false);
                self.add_adjustment(kind, cx);
            }
            Target::Tab(index) => {
                self.cloud_set_visible(false);
                self.select_tab(index, cx);
            }
            Target::Layer(id) => {
                self.cloud_set_visible(false);
                self.commit_recording_transform(cx);
                if let Some(doc) = &mut self.doc {
                    if doc.tree.find(id).is_some() {
                        doc.active_layer = Some(id);
                        doc.selected = vec![id];
                    }
                }
                self.record_selected_action_layer();
            }
            #[cfg(not(target_arch = "wasm32"))]
            Target::Photo(path) => self.open_from_gallery(path, cx),
            #[cfg(not(target_arch = "wasm32"))]
            Target::Recent(path) => self.load_file(path, cx),
            Target::CloudPhoto(asset) => self.cloud_open(*asset, cx),
            #[cfg(not(target_arch = "wasm32"))]
            Target::SearchPhotos => {
                self.cloud.show = false;
                if !self.library.open {
                    self.toggle_gallery(cx);
                }
                self.library.bucket_filter = None;
                self.library.folder_filter = None;
                self.library.person_filter = None;
                self.library.viewer = None;
                self.library.video = None;
                self.library.map_filter = None;
                self.library.map_filter_name = None;
                self.library.search_results = Some(photo_matches);
                self.library.search = LineEdit::focused(query);
                self.gallery_search_changed(cx);
            }
            Target::SearchCloud => {
                self.cloud.search = LineEdit::focused(query.clone());
                self.cloud.query.text = query;
                self.cloud_browse(schist_cloud::Scope::Library, cx);
            }
        }
        window.focus(&self.focus);
        cx.notify();
    }

    fn spotlight_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.spotlight.results.len();
        match ev.keystroke.key.as_str() {
            "escape" => {
                self.dismiss_spotlight(window, cx);
            }
            "enter" => self.choose_spotlight(self.spotlight.selected, window, cx),
            "up" | "down" if count > 0 => {
                self.spotlight.navigated = true;
                self.spotlight.selected = if ev.keystroke.key == "up" {
                    (self.spotlight.selected + count - 1) % count
                } else {
                    (self.spotlight.selected + 1) % count
                };
                self.spotlight
                    .scroll
                    .scroll_to_item(self.spotlight.selected);
            }
            "tab" => {
                let categories = [
                    Category::All,
                    Category::Actions,
                    Category::Documents,
                    Category::Layers,
                    Category::Photos,
                ];
                let at = categories
                    .iter()
                    .position(|c| *c == self.spotlight.category)
                    .unwrap_or(0);
                self.spotlight.category =
                    categories[(at + if ev.keystroke.modifiers.shift { 4 } else { 1 }) % 5];
                self.spotlight.selected = 0;
                self.spotlight.navigated = false;
                self.spotlight.scroll.set_offset(point(px(0.0), px(0.0)));
                self.refresh_spotlight_results();
            }
            _ => {
                if self.spotlight.input.key(ev, cx) == LineEditKey::Changed {
                    self.spotlight_changed(cx);
                }
                self.reset_caret_phase();
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_spotlight(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.spotlight.open {
            return None;
        }
        let p = ui::palette();
        let insets = window.safe_area_insets();
        let available_h = f32::from(window.viewport_size().height - insets.top - insets.bottom);
        let top = (available_h * 0.14).clamp(12.0, 110.0);
        let list_h = (available_h - top - 170.0).clamp(52.0, 380.0);
        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(68.0))
            .flex_none()
            .child(icon("search", 23.0, p.text_dim))
            .child(
                TextInput::edit("spotlight-input", &self.spotlight.input)
                    .placeholder(t("common.search"))
                    .caret_on(self.caret_on())
                    .colors(TextInputColors {
                        bg: p.popup_bg,
                        focus_border: p.popup_bg,
                        ..Default::default()
                    })
                    .border_0()
                    .h(px(48.0))
                    .text_size(px(23.0))
                    .flex_grow()
                    .min_w_0()
                    .on_focus(cx.listener(|ws, press, _, cx| {
                        ws.spotlight.input.press(press);
                        ws.reset_caret_phase();
                        cx.notify();
                    }))
                    .on_select_to(cx.listener(|ws, offset, _, cx| {
                        ws.spotlight.input.extend_to(*offset);
                        cx.notify();
                    }))
                    .on_clear(cx.listener(|ws, _, _, cx| {
                        ws.spotlight.input.set_text(String::new());
                        ws.spotlight_changed(cx);
                    })),
            )
            .child(
                IconButton::new("spotlight-close", "close")
                    .size(if ui::touch() { 44.0 } else { 28.0 })
                    .tooltip(t("common.close"), Some("Esc".into()))
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.dismiss_spotlight(window, cx);
                    })),
            );
        let mut tabs = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_b_1()
            .border_color(gpui::rgb(p.divider));
        for (index, category) in [
            Category::All,
            Category::Actions,
            Category::Documents,
            Category::Layers,
            Category::Photos,
        ]
        .into_iter()
        .enumerate()
        {
            tabs = tabs.child(
                div()
                    .id(("spotlight-category", index))
                    .flex()
                    .items_center()
                    .min_h(px(if ui::touch() { 44.0 } else { 24.0 }))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(gpui::rgb(p.text_dim))
                    .when(self.spotlight.category == category, |d| {
                        d.bg(gpui::rgb(p.control_bg)).text_color(gpui::rgb(p.text))
                    })
                    .hover(|d| d.bg(gpui::rgb(p.hover)))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.spotlight.category = category;
                        ws.spotlight.selected = 0;
                        ws.spotlight.navigated = false;
                        ws.spotlight.scroll.set_offset(point(px(0.0), px(0.0)));
                        ws.refresh_spotlight_results();
                        cx.notify();
                    }))
                    .child(category.label()),
            );
        }
        let mut list = div()
            .id("spotlight-results")
            .flex()
            .flex_col()
            .min_h(px(0.0))
            .max_h(px(list_h))
            .overflow_y_scroll()
            .track_scroll(&self.spotlight.scroll)
            .p_2();
        for (index, result) in self.spotlight.results.iter().enumerate() {
            let selected = index == self.spotlight.selected;
            let unavailable = self.doc.is_none() && result.target.needs_document();
            let preview = match &result.target {
                #[cfg(not(target_arch = "wasm32"))]
                Target::Photo(path) => self
                    .library
                    .entry_of(path)
                    .and_then(|entry| self.library.thumb(entry)),
                Target::CloudPhoto(asset) => self
                    .cloud
                    .thumbnails
                    .get(&asset.id)
                    .map(|(_, image)| image.clone()),
                _ => None,
            };
            let leading = if let Some(preview) = preview {
                gpui::img(preview)
                    .size(px(32.0))
                    .object_fit(gpui::ObjectFit::Cover)
                    .rounded_md()
                    .into_any_element()
            } else {
                icon(result.icon, 17.0, p.text_dim).into_any_element()
            };
            list =
                list.child(
                    div()
                        .id(("spotlight-result", index))
                        .flex()
                        .items_center()
                        .gap_3()
                        .h(px(54.0))
                        .flex_none()
                        .px_3()
                        .rounded_md()
                        .cursor_pointer()
                        .when(selected, |d| d.bg(gpui::rgb(p.selection_bg)))
                        .when(!selected, |d| d.hover(|d| d.bg(gpui::rgb(p.hover))))
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            ws.choose_spotlight(index, window, cx)
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(32.0))
                                .flex_none()
                                .rounded_md()
                                .bg(gpui::rgb(p.control_bg))
                                .child(leading),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_grow()
                                .min_w_0()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(result.label.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(gpui::rgb(p.text_dim))
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(if unavailable {
                                            t("common.no_document_open").to_string()
                                        } else {
                                            result.detail.clone()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(gpui::rgb(p.text_dim))
                                .child(result.hint.clone()),
                        ),
                );
        }
        if self.spotlight.results.is_empty() {
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .h(px(150.0))
                    .text_color(gpui::rgb(p.text_dim))
                    .child(icon("search", 26.0, p.text_faint))
                    .child(tn("common.n_items", 0)),
            );
        }
        let footer = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap_2()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(gpui::rgb(p.divider))
            .text_size(px(11.0))
            .text_color(gpui::rgb(p.text_dim))
            .child(if self.spotlight.searching {
                t("common.loading").to_string()
            } else {
                tn("common.n_items", self.spotlight.results.len() as u64)
            })
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(format!("↑↓  {}", t("common.select")))
                    .child(format!("↵  {}", t("common.choose")))
                    .child(format!("Esc  {}", t("common.close"))),
            );
        Some(
            div()
                .id("spotlight-overlay")
                .absolute()
                .top(insets.top)
                .bottom(insets.bottom)
                .left(insets.left)
                .right(insets.right)
                .flex()
                .justify_center()
                .items_start()
                .pt(px(top))
                .px_3()
                .bg(gpui::rgba(0x00000066))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| {
                        ws.dismiss_spotlight(window, cx);
                        cx.stop_propagation();
                    }),
                )
                .child(
                    div()
                        .id("spotlight")
                        .track_focus(&self.spotlight.focus)
                        .occlude()
                        .w(px(620.0))
                        .max_h(px((available_h - top - 12.0).max(0.0)))
                        .max_w(gpui::relative(1.0))
                        .flex()
                        .flex_col()
                        .rounded(px(14.0))
                        .overflow_hidden()
                        .bg(gpui::rgb(p.popup_bg))
                        .border_1()
                        .border_color(gpui::rgb(p.edge))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(
                            cx.listener(|ws, ev, window, cx| ws.spotlight_key(ev, window, cx)),
                        )
                        .child(header)
                        .child(tabs)
                        .child(list)
                        .child(footer),
                )
                .into_any_element(),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn photo_result(path: &std::path::Path) -> SearchResult {
    SearchResult::new(
        path.file_name().unwrap_or_default().to_string_lossy(),
        path.parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        "image-size",
        Category::Photos,
        Target::Photo(path.to_path_buf()),
    )
}

pub(crate) fn search_button(cx: &mut Context<Workspace>) -> impl IntoElement {
    IconButton::new("spotlight-search", "search")
        .tooltip(
            t("common.search"),
            Some(panels::keybind_hint(Some("cmd-shift-p")).into()),
        )
        .on_click(cx.listener(|ws, _, window, cx| ws.show_spotlight(window, cx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spotlight_ranks_names_before_descriptions_and_accepts_abbreviations() {
        assert!(
            match_score("brush", "Brush", "Paint").unwrap()
                < match_score("brush", "History Brush", "Paint").unwrap()
        );
        assert!(
            match_score("brush", "History Brush", "Paint").unwrap()
                < match_score("brush", "Pencil", "Brush tools").unwrap()
        );
        assert!(match_score("gblur", "Gaussian Blur", "Filter").is_some());
        assert!(match_score("blur gaussian", "Gaussian Blur", "Filter").is_some());
        assert_eq!(match_score("zzzz", "Gaussian Blur", "Filter"), None);
    }
    #[test]
    fn spotlight_matches_unicode_case_and_whitespace() {
        assert_eq!(match_score("  ÉTÉ  ", "Été", "Photos"), Some(0));
        assert_eq!(match_score("写真", "写真", ""), Some(0));
        assert!(match_score("图层", "新建图层", "").is_some());
        assert_eq!(match_score("   ", "Anything", ""), Some(0));
        assert_eq!(match_score("save", "Save…", "File"), Some(0));
    }
}
