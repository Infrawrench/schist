//! The Schist Cloud gallery: the same room as the local library — the
//! strip, the sidebar, the grouped grid, the tray, the right-click menu
//! — showing a remote library over the workspace socket. Everything
//! visual comes from `gallery_chrome`; what this file adds is the
//! remote half: cloud folders and buckets in the sidebar, assets
//! grouped by month or folder in the grid, the server-side search box,
//! and the dialogs behind the mutations. The browser build, which has
//! no local gallery, composes its whole gallery from these parts.
use super::cloud::{CloudContext, PAGE_SIZE};
use super::gallery_chrome::{
    self as chrome, cell_frame, empty_note, grid_column, grid_frame, lead_probe, menu_frame,
    menu_row, menu_sep, pal, search_field, section_header, sidebar_link, sidebar_row_frame,
    DragGhost, GroupBy, MenuAction, TrayInfo,
};
#[cfg(target_arch = "wasm32")]
use super::gallery_chrome::{group_chips, sidebar_caption, sidebar_column};
#[cfg(not(target_arch = "wasm32"))]
use super::library::GalleryDrag;
use super::*;
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct GalleryDrag {
    paths: Vec<PathBuf>,
}

use crate::ui;
use gpui::{AppContext as _, StatefulInteractiveElement as _};
use schist_cloud::{
    protocol::{map, value},
    Asset, Bucket, Filters, Folder, Rule, Scope, Value,
};
use std::collections::BTreeMap;

/// A drag of remote items — assets or a folder — headed for a bucket.
#[derive(Clone)]
struct RemoteDrag {
    items: Vec<Value>,
    label: String,
}
pub(crate) struct DragLabel(pub String);
#[derive(Clone)]
pub(crate) struct LocalFolderDrag {
    pub path: PathBuf,
}
impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .bg(gpui::rgb(ui::palette().accent))
            .text_color(gpui::rgb(0xffffff))
            .child(self.0.clone())
    }
}
fn caption(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(11.0))
        .text_color(gpui::rgb(ui::palette().text_dim))
        .child(text.into())
}
fn form(
    ws: &mut Workspace,
    kind: &'static str,
    fields: Vec<(&'static str, String, String)>,
    cx: &mut Context<Workspace>,
) {
    ws.open_modal(Modal::Cloud { kind, fields }, cx);
}
fn field(
    key: &'static str,
    label: &str,
    value: impl Into<String>,
) -> (&'static str, String, String) {
    (key, label.into(), value.into())
}
pub(crate) fn filter_fields(
    query: &schist_cloud::AssetQuery,
) -> Vec<(&'static str, String, String)> {
    let f = &query.filters;
    vec![
        field("cloud-query", "Search", query.text.clone()),
        field(
            "cloud-types",
            "File types (MIME, comma separated)",
            f.mime_types
                .as_ref()
                .map(|v| v.join(", "))
                .unwrap_or_default(),
        ),
        field(
            "cloud-tags",
            "Tags (comma separated)",
            f.tags.as_ref().map(|v| v.join(", ")).unwrap_or_default(),
        ),
        field(
            "cloud-edited",
            "Edited: any / yes / no",
            f.edited
                .map(|v| if v { "yes" } else { "no" })
                .unwrap_or("any"),
        ),
        field(
            "cloud-content",
            "Content: all / safe / flagged",
            f.content.as_deref().unwrap_or("all"),
        ),
        field(
            "cloud-rating",
            "Minimum rating (0–5)",
            f.min_rating.map(|v| v.to_string()).unwrap_or_default(),
        ),
        field(
            "cloud-after",
            "Captured after (YYYY-MM-DD)",
            f.captured_after
                .map(schist_cloud::format_date)
                .unwrap_or_default(),
        ),
        field(
            "cloud-before",
            "Captured before (YYYY-MM-DD)",
            f.captured_before
                .map(schist_cloud::format_date)
                .unwrap_or_default(),
        ),
        field(
            "cloud-bounds",
            "Map boundary: south, west, north, east",
            f.bounds
                .as_ref()
                .map(|b| format!("{}, {}, {}, {}", b.south, b.west, b.north, b.east))
                .unwrap_or_default(),
        ),
    ]
}

/// How many of a rule's filters are set, for the bucket header.
fn filter_count(f: &Filters) -> usize {
    [
        f.mime_types.is_some(),
        f.tags.is_some(),
        f.edited.is_some(),
        f.content.is_some(),
        f.captured_after.is_some(),
        f.captured_before.is_some(),
        f.min_rating.is_some(),
        f.bounds.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count()
}

/// A smart bucket's rule as a header subtitle: the query in quotes,
/// then how many filters it adds.
fn rule_label(rule: &Rule) -> String {
    let mut parts = Vec::new();
    if !rule.text.trim().is_empty() {
        parts.push(format!("\u{201c}{}\u{201d}", rule.text.trim()));
    }
    match filter_count(&rule.filters) {
        0 => {}
        1 => parts.push("1 filter".to_string()),
        n => parts.push(format!("{n} filters")),
    }
    if parts.is_empty() {
        "smart bucket".to_string()
    } else {
        parts.join(" · ")
    }
}

/// A cloud folder's name, and its path from the top for a subtitle.
fn folder_names(folders: &[Folder], id: &str) -> (String, String) {
    let find = |id: &str| folders.iter().find(|f| f.id == id);
    let Some(folder) = find(id) else {
        return ("Folder".to_string(), String::new());
    };
    let mut path = vec![folder.name.clone()];
    let mut parent = folder.parent_id.clone();
    // A bounded walk: a cycle in the catalogue must not hang the
    // render.
    for _ in 0..32 {
        let Some(next) = parent.as_deref().and_then(find) else {
            break;
        };
        path.push(next.name.clone());
        parent = next.parent_id.clone();
    }
    path.reverse();
    (folder.name.clone(), path.join(" / "))
}

/// The folders as a tree: children under their parent, siblings by
/// name, as the file manager would show them, each with its depth. A
/// folder whose parent is off this page of the catalogue would
/// otherwise vanish, so orphans list at the top level.
fn folder_tree(folders: &[Folder]) -> Vec<(usize, Folder)> {
    fn walk(
        parent: Option<&str>,
        depth: usize,
        folders: &[Folder],
        out: &mut Vec<(usize, Folder)>,
    ) {
        if depth > 16 {
            return;
        }
        let mut children: Vec<&Folder> = folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == parent)
            .collect();
        children.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        for child in children {
            out.push((depth, child.clone()));
            walk(Some(&child.id), depth + 1, folders, out);
        }
    }
    let mut ordered = Vec::new();
    walk(None, 0, folders, &mut ordered);
    let listed: std::collections::HashSet<&str> =
        ordered.iter().map(|(_, f)| f.id.as_str()).collect();
    let orphans: Vec<(usize, Folder)> = folders
        .iter()
        .filter(|f| !listed.contains(f.id.as_str()))
        .map(|f| (0, f.clone()))
        .collect();
    ordered.extend(orphans);
    ordered
}

/// One page of assets grouped for the grid: a bucket or a search as
/// one strip, otherwise by month (newest first, a photo without a
/// capture time filing under its upload time) or by folder (the
/// unfiled last).
fn group_assets(
    assets: &[Asset],
    folders: &[Folder],
    buckets: &[Bucket],
    scope: &Scope,
    text: &str,
    group_by: GroupBy,
) -> Vec<(String, String, Vec<Asset>)> {
    if let Scope::Bucket { id } = scope {
        let bucket = buckets.iter().find(|b| &b.id == id);
        let name = bucket
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "Bucket".to_string());
        let rule = bucket
            .and_then(|b| b.rule.as_ref())
            .map(rule_label)
            .unwrap_or_default();
        let title = if text.trim().is_empty() {
            format!("Bucket · {name}")
        } else {
            format!("Bucket · {name} · Search results")
        };
        return vec![(title, rule, assets.to_vec())];
    }
    if !text.trim().is_empty() {
        let title = match scope {
            Scope::Folder { id, .. } => {
                format!("{} · Search results", folder_names(folders, id).0)
            }
            _ => "Search results".to_string(),
        };
        return vec![(title, String::new(), assets.to_vec())];
    }
    match group_by {
        GroupBy::Folder => {
            let mut groups: BTreeMap<(bool, String, String), Vec<Asset>> = BTreeMap::new();
            for asset in assets {
                let key = match &asset.folder_id {
                    Some(id) => {
                        let (name, path) = folder_names(folders, id);
                        (false, name, path)
                    }
                    None => (true, "Unfiled".to_string(), String::new()),
                };
                groups.entry(key).or_default().push(asset.clone());
            }
            groups
                .into_iter()
                .map(|((_, name, path), assets)| (name, path, assets))
                .collect()
        }
        _ => {
            let taken = |a: &Asset| a.captured_at.unwrap_or(a.modified_at);
            let mut months: BTreeMap<String, Vec<Asset>> = BTreeMap::new();
            for asset in assets {
                let key = chrome::month_key(taken(asset) as i64);
                months.entry(key).or_default().push(asset.clone());
            }
            months
                .into_iter()
                .rev()
                .map(|(key, mut assets)| {
                    assets.sort_by_key(|a| std::cmp::Reverse(taken(a)));
                    (chrome::month_title(&key), String::new(), assets)
                })
                .collect()
        }
    }
}

impl Workspace {
    /// The order the provider sorts a page in: by relevance while
    /// searching, newest first under month headers, by name under
    /// folder headers.
    pub(crate) fn cloud_sort(&self) -> String {
        if !self.cloud.query.text.trim().is_empty() {
            "relevance"
        } else {
            match self.gallery_group_by() {
                GroupBy::Date => "captured_desc",
                _ => "name",
            }
        }
        .into()
    }

    /// The strip's Refresh: the folder and bucket lists and the current
    /// page, again.
    pub(crate) fn cloud_refresh(&mut self, cx: &mut Context<Self>) {
        self.cloud_refresh_catalogue();
        self.cloud_watch_assets(true);
        cx.notify();
    }

    /// The Filters… dialog: everything but the search text, which has
    /// its own box in the strip.
    pub(crate) fn cloud_open_filters(&mut self, cx: &mut Context<Self>) {
        let fields = filter_fields(&self.cloud.query)
            .into_iter()
            .filter(|(key, _, _)| *key != "cloud-query")
            .collect();
        form(self, "filters", fields, cx);
    }

    pub(crate) fn cloud_filters_active(&self) -> bool {
        self.cloud.query.filters != Filters::default()
    }

    pub(crate) fn cloud_clear_filters(&mut self, cx: &mut Context<Self>) {
        self.cloud.query.filters = Filters::default();
        self.cloud.query.offset = 0;
        self.cloud_watch_assets(true);
        cx.notify();
    }

    /// The lead of the selection — what Enter opens and the tray names.
    pub(crate) fn cloud_lead_asset(&self) -> Option<Asset> {
        let id = self.cloud.selected.last()?;
        self.cloud.assets.iter().find(|a| &a.id == id).cloned()
    }

    fn cloud_asset(&self, id: &str) -> Option<Asset> {
        self.cloud.assets.iter().find(|a| a.id == id).cloned()
    }

    fn cloud_is_selected(&self, id: &str) -> bool {
        self.cloud.selected.iter().any(|s| s == id)
    }

    pub(crate) fn cloud_select_single(&mut self, id: String) {
        self.cloud.select_anchor = Some(id.clone());
        self.cloud.selected = vec![id];
    }

    fn cloud_toggle_selected(&mut self, id: String) {
        if let Some(at) = self.cloud.selected.iter().position(|s| s == &id) {
            self.cloud.selected.remove(at);
        } else {
            self.cloud.select_anchor = Some(id.clone());
            self.cloud.selected.push(id);
        }
    }

    /// Shift-click: select the display-order range from the anchor to
    /// this asset, which becomes the lead.
    fn cloud_select_range_to(&mut self, id: String) {
        let flat = self.cloud_flat_order();
        let anchor = self
            .cloud
            .select_anchor
            .clone()
            .unwrap_or_else(|| id.clone());
        let (Some(a), Some(b)) = (
            flat.iter().position(|p| p == &anchor),
            flat.iter().position(|p| p == &id),
        ) else {
            self.cloud_select_single(id);
            return;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        let mut range: Vec<String> = flat[lo..=hi].to_vec();
        if a > b {
            range.reverse();
        }
        self.cloud.select_anchor = Some(anchor);
        self.cloud.selected = range;
    }

    /// Every asset the grid is showing, in display order — what arrows
    /// walk and Shift-clicks span.
    pub(crate) fn cloud_flat_order(&self) -> Vec<String> {
        self.cloud_grouped()
            .into_iter()
            .flat_map(|(_, _, assets)| assets)
            .map(|a| a.id)
            .collect()
    }

    /// The page grouped the way the sidebar's chips ask — the same
    /// readings the local grid has, minus Place, which the cloud does
    /// not send positions for. A bucket or a search shows as one strip,
    /// exactly as locally.
    pub(crate) fn cloud_grouped(&self) -> Vec<(String, String, Vec<Asset>)> {
        group_assets(
            &self.cloud.assets,
            &self.cloud.folders,
            &self.cloud.buckets,
            &self.cloud.query.scope,
            &self.cloud.query.text,
            self.gallery_group_by(),
        )
    }

    /// A keystroke while the cloud search box has the keyboard.
    pub(crate) fn cloud_search_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::ui::LineEditKey;
        match self.cloud.search.key(ev, cx) {
            LineEditKey::Ignored => return false,
            LineEditKey::Changed => self.cloud_search_changed(cx),
            // Enter asks now rather than after the pause.
            LineEditKey::Submitted => self.cloud_search_apply(cx),
            LineEditKey::Moved => cx.notify(),
        }
        self.reset_caret_phase();
        true
    }

    /// The text changed: ask the provider after a short pause, so a
    /// word typed at speed is one query rather than six. Each change
    /// supersedes the last.
    fn cloud_search_changed(&mut self, cx: &mut Context<Self>) {
        self.cloud.search_seq += 1;
        let seq = self.cloud.search_seq;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |ws, cx| {
                if ws.cloud.search_seq == seq {
                    ws.cloud_search_apply(cx);
                }
            });
        })
        .detach();
    }

    /// Make the box's text the query.
    fn cloud_search_apply(&mut self, cx: &mut Context<Self>) {
        let text = self.cloud.search.text.trim().to_string();
        if self.cloud.query.text != text {
            self.cloud.query.text = text;
            self.cloud.query.offset = 0;
            self.cloud_watch_assets(true);
        }
        cx.notify();
    }

    /// Leave the search: clear the box and show the folder again.
    /// Wired into the always-on Escape path. Returns whether there was
    /// a search to leave.
    pub(crate) fn cloud_search_clear(&mut self, cx: &mut Context<Self>) -> bool {
        if self.cloud.context.take().is_some() {
            cx.notify();
            return true;
        }
        let searching = self.cloud.search.active
            || !self.cloud.search.text.is_empty()
            || !self.cloud.query.text.is_empty();
        if !searching {
            return false;
        }
        self.cloud.search.clear();
        self.cloud.search_seq += 1;
        if !self.cloud.query.text.is_empty() {
            self.cloud.query.text.clear();
            self.cloud.query.offset = 0;
            self.cloud_watch_assets(true);
        }
        cx.notify();
        true
    }

    /// An arrow key while the cloud gallery has the keyboard: move the
    /// selection through the page in display order — left/right by
    /// one, up/down by a visual row.
    pub(crate) fn cloud_nav_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.cloud.search.active {
            return false;
        }
        let columns = self.cloud.grid.columns(self.gallery_thumb_px()) as isize;
        let step: isize = match ev.keystroke.key.as_str() {
            "left" => -1,
            "right" => 1,
            "up" => -columns,
            "down" => columns,
            _ => return false,
        };
        let flat = self.cloud_flat_order();
        if flat.is_empty() {
            return false;
        }
        let next = match self
            .cloud
            .selected
            .last()
            .and_then(|lead| flat.iter().position(|p| p == lead))
        {
            Some(at) => (at as isize + step).clamp(0, flat.len() as isize - 1) as usize,
            // Nothing selected yet: any arrow lands on the first photo.
            None => 0,
        };
        let lead = flat[next].clone();
        if ev.keystroke.modifiers.shift {
            // Shift+arrow: the range from the anchor to wherever the
            // lead moved, in display order.
            let anchor = self
                .cloud
                .select_anchor
                .clone()
                .unwrap_or_else(|| lead.clone());
            let a = flat.iter().position(|p| p == &anchor).unwrap_or(next);
            let (lo, hi) = (a.min(next), a.max(next));
            let mut range: Vec<String> = flat[lo..=hi].to_vec();
            if a > next {
                // The lead must stay last, so arrows keep moving it.
                range.reverse();
            }
            self.cloud.select_anchor = Some(anchor);
            self.cloud.selected = range;
        } else {
            self.cloud_select_single(lead);
        }
        self.cloud.grid.reveal = true;
        cx.notify();
        true
    }

    /// Nudge the grid until the keyboard-moved selection is on screen.
    pub(crate) fn cloud_reveal_tick(&mut self, cx: &mut Context<Self>) {
        if self.cloud.grid.reveal_tick() {
            cx.notify();
        }
    }

    /// Everything the right-click menu acts on: the selection when the
    /// clicked photo is in it, that photo alone otherwise.
    fn cloud_acting(&self, id: &str) -> Vec<String> {
        if self.cloud_is_selected(id) {
            self.cloud.selected.clone()
        } else {
            vec![id.to_string()]
        }
    }

    fn cloud_add_to_bucket(&mut self, bucket: String, ids: &[String]) {
        let items = ids
            .iter()
            .map(|id| map([("kind", "asset".into()), ("id", id.clone().into())]))
            .collect();
        self.cloud_drop_remote(bucket, items);
    }

    fn cloud_remove_from_bucket(&mut self, bucket: String, ids: &[String]) {
        self.cloud_mutate(
            "bucket.remove",
            vec![("id", bucket.into()), ("asset_ids", value(ids.to_vec()))],
        );
    }
}

/// The chip announcing active filters, in the strip beside the search.
pub(crate) fn filter_chip(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    ws.cloud_filters_active().then(|| {
        chrome::filter_chip(
            format!("Filters on ({})", filter_count(&ws.cloud.query.filters)),
            |ws, cx| ws.cloud_open_filters(cx),
            |ws, cx| ws.cloud_clear_filters(cx),
            cx,
        )
        .into_any_element()
    })
}

/// The search box in the strip: the provider ranks the page by it.
pub(crate) fn search_box(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let placeholder: SharedString = if ws.cloud.connected {
        "Search cloud photos\u{2026}".into()
    } else {
        "Connecting\u{2026}".into()
    };
    let caret_on = ws.caret_on();
    search_field(
        &ws.cloud.search,
        placeholder,
        caret_on,
        |ws, _cx| ws.cloud.search.focus(),
        |ws, cx| {
            ws.cloud_search_clear(cx);
        },
        cx,
    )
}

/// What the tray says about the cloud gallery: the lead photo's name,
/// its Edit and Download buttons, the page's count.
pub(crate) fn tray_info(ws: &Workspace) -> TrayInfo {
    let lead = ws.cloud_lead_asset();
    let one = ws.cloud.selected.len() == 1;
    let mut notes = Vec::new();
    if lead.as_ref().is_some_and(|a| a.edited) {
        notes.push("edited — the edits live in Schist Cloud".to_string());
    }
    type Act = chrome::TrayAction;
    TrayInfo {
        edit: lead.clone().map(|asset| {
            Box::new(
                move |ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>| {
                    ws.cloud_open(asset.clone(), cx)
                },
            ) as Act
        }),
        extra: one.then(|| {
            (
                "Download…",
                Box::new(
                    |ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>| {
                        ws.cloud_download_selected(cx)
                    },
                ) as Act,
            )
        }),
        name: lead.map(|a| a.name),
        selected: ws.cloud.selected.len(),
        notes,
        count: if ws.cloud.loaded {
            format!("{} photos", ws.cloud.total)
        } else if ws.cloud.connected {
            "Loading\u{2026}".to_string()
        } else {
            ws.cloud.message.clone()
        },
    }
}

/// A row for a remote folder or bucket that takes drops: local gallery
/// photos, a watched local folder and files from the file manager
/// upload; remote items add by reference to a bucket.
fn droppable(
    row: gpui::Stateful<gpui::Div>,
    bucket: Option<String>,
    folder: Option<String>,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<gpui::Div> {
    let (b1, f1) = (bucket.clone(), folder.clone());
    let (b2, f2) = (bucket.clone(), folder.clone());
    let (b3, f3) = (bucket.clone(), folder.clone());
    let mut row = row
        .drag_over::<GalleryDrag>(|s, _, _, _| s.bg(gpui::rgb(pal().select_border)))
        .drag_over::<LocalFolderDrag>(|s, _, _, _| s.bg(gpui::rgb(pal().select_border)))
        .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(gpui::rgb(pal().select_border)))
        .on_drop(cx.listener(move |ws, drag: &GalleryDrag, _, cx| {
            cx.stop_propagation();
            ws.cloud_drop_local(b1.clone(), f1.clone(), drag.paths.clone(), cx)
        }))
        .on_drop(cx.listener(move |ws, drag: &LocalFolderDrag, _, cx| {
            cx.stop_propagation();
            ws.cloud_drop_local(b2.clone(), f2.clone(), vec![drag.path.clone()], cx)
        }))
        .on_drop(cx.listener(move |ws, drag: &ExternalPaths, _, cx| {
            cx.stop_propagation();
            ws.cloud_drop_local(b3.clone(), f3.clone(), drag.paths().to_vec(), cx)
        }));
    if let Some(bucket) = bucket {
        row = row
            .drag_over::<RemoteDrag>(|s, _, _, _| s.bg(gpui::rgb(pal().select_border)))
            .on_drop(cx.listener(move |ws, drag: &RemoteDrag, _, cx| {
                cx.stop_propagation();
                ws.cloud_drop_remote(bucket.clone(), drag.items.clone());
                cx.notify();
            }));
    }
    row
}

/// The cloud rows' badge: the one thing that tells a cloud folder or
/// bucket from a local one in the same list.
pub(crate) const CLOUD_GLYPH: &str = "\u{2601}";

/// The library root still gets its count from the current asset page.
/// Individual folders and buckets get theirs from the catalogue.
fn scope_count(ws: &Workspace, this: &Scope) -> Option<usize> {
    (ws.cloud.show && ws.cloud.loaded && &ws.cloud.query.scope == this)
        .then_some(ws.cloud.total as usize)
}

/// The "+ New folder" answer for the cloud.
pub(crate) fn new_cloud_folder(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    form(
        ws,
        "new-folder",
        vec![field("cloud-name", "Folder name", "")],
        cx,
    )
}

/// The "+ New bucket" answer for the cloud.
pub(crate) fn new_cloud_bucket(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    let mut fields = vec![field("cloud-name", "Bucket name", "")];
    let mut q = ws.cloud.query.clone();
    q.text.clear();
    q.filters = Default::default();
    fields.extend(filter_fields(&q));
    form(ws, "new-bucket", fields, cx)
}

/// The Schist Cloud rows of the FOLDERS list: the library itself as a
/// root, its folders as a tree beneath it, the page links — or, signed
/// out, the way in.
pub(crate) fn folder_rows(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if ws.cloud.account.is_none() {
        rows.push(
            sidebar_link(
                format!("{CLOUD_GLYPH} Sign into Schist Cloud…"),
                |ws, _w, cx| ws.cloud_sign_in(cx),
                cx,
            )
            .into_any_element(),
        );
        return rows;
    }
    let showing = ws.cloud.show;
    let scope = ws.cloud.query.scope.clone();
    // The whole library, the root the folders hang from.
    {
        let selected = showing && scope == Scope::Library;
        let row = sidebar_row_frame(
            "cloud-library",
            format!("{CLOUD_GLYPH} Schist Cloud"),
            scope_count(ws, &Scope::Library),
            selected,
            0,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _e: &MouseDownEvent, _w, cx| ws.cloud_browse(Scope::Library, cx)),
        );
        rows.push(droppable(row, None, None, cx).into_any_element());
    }
    let ordered = folder_tree(&ws.cloud.folders);
    for (i, (depth, folder)) in ordered.into_iter().enumerate() {
        let selected = showing && matches!(&scope, Scope::Folder { id, .. } if id == &folder.id);
        let browse = folder.id.clone();
        let context = folder.id.clone();
        let drag = RemoteDrag {
            items: vec![map([
                ("kind", "folder".into()),
                ("id", folder.id.clone().into()),
                ("recursive", true.into()),
            ])],
            label: folder.name.clone(),
        };
        let row = sidebar_row_frame(
            ("cloud-folder", i),
            format!("\u{25b8} {}", folder.name),
            folder
                .asset_count
                .and_then(|count| usize::try_from(count).ok()),
            selected,
            depth + 1,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                ws.cloud_browse(
                    Scope::Folder {
                        id: browse.clone(),
                        recursive: true,
                    },
                    cx,
                )
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                ws.cloud.context = Some((ev.position, CloudContext::Folder(context.clone())));
                cx.notify();
            }),
        )
        .on_drag(drag, |drag, _, _, cx| {
            cx.new(|_| DragLabel(drag.label.clone()))
        });
        rows.push(droppable(row, None, Some(folder.id.clone()), cx).into_any_element());
    }
    rows.extend(catalogue_pages(true, ws, cx));
    rows
}

/// The Schist Cloud rows of the BUCKETS list, after the local ones.
pub(crate) fn bucket_rows(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if ws.cloud.account.is_none() {
        return rows;
    }
    let showing = ws.cloud.show;
    let scope = ws.cloud.query.scope.clone();
    for (i, bucket) in ws.cloud.buckets.clone().into_iter().enumerate() {
        let this = Scope::Bucket {
            id: bucket.id.clone(),
        };
        let selected = showing && scope == this;
        let browse = bucket.id.clone();
        let context = bucket.id.clone();
        let label = if bucket.rule.is_some() {
            format!("{CLOUD_GLYPH} \u{2726} {}", bucket.name)
        } else {
            format!("{CLOUD_GLYPH} {}", bucket.name)
        };
        let row = sidebar_row_frame(
            ("cloud-bucket", i),
            label,
            bucket
                .asset_count
                .and_then(|count| usize::try_from(count).ok()),
            selected,
            0,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                ws.cloud_browse(Scope::Bucket { id: browse.clone() }, cx)
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
                ws.cloud.context = Some((ev.position, CloudContext::Bucket(context.clone())));
                cx.notify();
            }),
        );
        rows.push(droppable(row, Some(bucket.id.clone()), None, cx).into_any_element());
    }
    rows.extend(catalogue_pages(false, ws, cx));
    // A library with more folders or buckets than one page lists gets
    // the finder; a smaller one has them all on screen already.
    if ws.cloud.folders_total > 500
        || ws.cloud.buckets_total > 500
        || !ws.cloud.catalogue.is_empty()
    {
        rows.push(
            sidebar_link(
                if ws.cloud.catalogue.is_empty() {
                    "Find folders / buckets…".to_string()
                } else {
                    format!("Showing \u{201c}{}\u{201d}…", ws.cloud.catalogue)
                },
                |ws, _w, cx| {
                    form(
                        ws,
                        "catalogue",
                        vec![field(
                            "cloud-query",
                            "Name contains",
                            ws.cloud.catalogue.clone(),
                        )],
                        cx,
                    )
                },
                cx,
            )
            .into_any_element(),
        );
    }
    rows
}

/// Previous/More links under a folder or bucket list that overflows
/// one page of the catalogue.
fn catalogue_pages(
    folders: bool,
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> Vec<gpui::AnyElement> {
    let (offset, total) = if folders {
        (ws.cloud.folders_offset, ws.cloud.folders_total)
    } else {
        (ws.cloud.buckets_offset, ws.cloud.buckets_total)
    };
    let mut links = Vec::new();
    if offset > 0 {
        links.push(
            sidebar_link(
                "\u{2191} Previous",
                move |ws, _w, cx| {
                    if folders {
                        ws.cloud.folders_offset = offset.saturating_sub(500);
                    } else {
                        ws.cloud.buckets_offset = offset.saturating_sub(500);
                    }
                    ws.cloud_refresh_catalogue();
                    cx.notify();
                },
                cx,
            )
            .into_any_element(),
        );
    }
    if offset + 500 < total {
        links.push(
            sidebar_link(
                "\u{2193} More",
                move |ws, _w, cx| {
                    if folders {
                        ws.cloud.folders_offset = offset + 500;
                    } else {
                        ws.cloud.buckets_offset = offset + 500;
                    }
                    ws.cloud_refresh_catalogue();
                    cx.notify();
                },
                cx,
            )
            .into_any_element(),
        );
    }
    links
}

/// Why the cloud grid is bare.
fn empty_reason(ws: &Workspace) -> String {
    if !ws.cloud.connected {
        return ws.cloud.message.clone();
    }
    if !ws.cloud.loaded {
        return "Loading\u{2026}".into();
    }
    if !ws.cloud.query.text.trim().is_empty() {
        return "Nothing matches the search. Escape clears it.".into();
    }
    if ws.cloud_filters_active() {
        return "Nothing matches the filters. The chip in the strip clears them.".into();
    }
    match &ws.cloud.query.scope {
        Scope::Bucket { id } => {
            let smart = ws
                .cloud
                .buckets
                .iter()
                .any(|b| &b.id == id && b.rule.is_some());
            if smart {
                "Nothing matches this bucket's rule yet. Dragging photos in works too."
            } else {
                "This bucket is empty. Drag photos onto its row in the sidebar to add them."
            }
        }
        Scope::Folder { .. } => {
            "This cloud folder is empty. Drop photos on its row in the sidebar, or use \
             Upload Files…"
        }
        Scope::Library => {
            "No photos in your cloud library yet. Use Upload Files…, or drag photos from \
             the local gallery onto a cloud folder or bucket."
        }
    }
    .into()
}

/// The grid: month or folder headers with a rule, then wrapped
/// thumbnails — one page of the query, with the page links under it.
pub(crate) fn grid(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let cell = ws.gallery_thumb_px();
    let selected = ws.cloud.selected.clone();
    let sections = ws.cloud_grouped();
    let access: chrome::GridAccess = |ws| &mut ws.cloud.grid;
    let mut column = grid_column("cloud-grid", &ws.cloud.grid, access, cx);
    if sections.is_empty() {
        column = column.child(empty_note(empty_reason(ws)));
    }
    let columns = ws.cloud.grid.columns(cell);
    for (title, subtitle, assets) in sections {
        let detail = if subtitle.is_empty() {
            format!("{} photos", assets.len())
        } else {
            format!("{subtitle} — {}", assets.len())
        };
        column = column.child(section_header(title, detail));
        let mut body = div().flex().flex_col();
        for row_assets in assets.chunks(columns) {
            let mut row = div().flex().flex_row().gap_2().mb_2();
            for asset in row_assets {
                row = row.child(cloud_cell(ws, asset.clone(), cell, &selected, cx));
            }
            body = body.child(row);
        }
        column = column.child(body);
    }
    // The page links, in the grid's own voice, only when there is
    // more than one page.
    let offset = ws.cloud.query.offset;
    let total = ws.cloud.total;
    if ws.cloud.loaded && (offset > 0 || offset + PAGE_SIZE < total) {
        let first = offset + 1;
        let last = (offset + PAGE_SIZE).min(total);
        let link =
            |label: &'static str, to: u64, cx: &mut Context<Workspace>| -> gpui::AnyElement {
                div()
                    .px_2()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_size(px(12.0))
                    .text_color(gpui::rgb(pal().header))
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui::rgb(pal().sidebar_selected)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _e: &MouseDownEvent, _w, cx| {
                            ws.cloud.query.offset = to;
                            ws.cloud.selected.clear();
                            ws.cloud.select_anchor = None;
                            ws.cloud_watch_assets(false);
                            cx.notify();
                        }),
                    )
                    .child(label)
                    .into_any_element()
            };
        let mut pager = div().flex().flex_row().items_center().gap_2().pt_2().pb_4();
        if offset > 0 {
            pager = pager.child(link(
                "\u{2190} Previous page",
                offset.saturating_sub(PAGE_SIZE),
                cx,
            ));
        }
        pager = pager.child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(format!("{first}–{last} of {total}")),
        );
        if offset + PAGE_SIZE < total {
            pager = pager.child(link("Next page \u{2192}", offset + PAGE_SIZE, cx));
        }
        column = column.child(pager);
    }
    grid_frame(column, &ws.cloud.grid, access, cx).into_any_element()
}

/// One remote photo's square: its thumbnail once fetched, the same
/// selection, drag and menu behaviour as a local cell.
fn cloud_cell(
    ws: &mut Workspace,
    asset: Asset,
    cell: f32,
    selected: &[String],
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let thumb = ws
        .cloud
        .thumbnails
        .get(&asset.id)
        .map(|(_, image)| image.clone());
    let failed = asset.thumbnail_url.is_none() || ws.cloud.thumbnail_failed.contains(&asset.id);
    let ghost_thumb = thumb.clone();
    let is_selected = selected.iter().any(|id| id == &asset.id);
    let is_lead = selected.last() == Some(&asset.id);
    let click = asset.clone();
    let context = asset.id.clone();
    // Dragging carries the whole selection when the pressed cell is in
    // it, and just the pressed cell otherwise.
    let carried: Vec<String> = if is_selected {
        selected.to_vec()
    } else {
        vec![asset.id.clone()]
    };
    let drag = RemoteDrag {
        items: carried
            .iter()
            .map(|id| map([("kind", "asset".into()), ("id", id.clone().into())]))
            .collect(),
        label: if carried.len() == 1 {
            asset.name.clone()
        } else {
            format!("{} photos", carried.len())
        },
    };
    cell_frame(
        SharedString::from(format!("cloud-cell-{}", asset.id)),
        cell,
        is_selected,
        thumb,
        failed,
        asset.edited,
    )
    .on_drag(drag, move |drag, _offset, _window, cx| {
        let label = drag.label.clone();
        let count = drag.items.len();
        let thumb = ghost_thumb.clone();
        cx.new(|_| DragGhost {
            label,
            thumb,
            count,
            size: cell,
        })
    })
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
            let id = click.id.clone();
            if ev.modifiers.platform || ev.modifiers.control {
                // ⌘-click: in or out, keeping the rest.
                ws.cloud_toggle_selected(id);
            } else if ev.modifiers.shift {
                ws.cloud_select_range_to(id);
            } else if ev.click_count >= 2 {
                ws.cloud_select_single(id);
                ws.cloud_open(click.clone(), cx);
            } else if !ws.cloud_is_selected(&id) {
                // A plain press on an unselected photo selects it —
                // and on a selected one keeps the selection, so a
                // drag can carry the lot.
                ws.cloud_select_single(id);
            }
            ws.cloud.context = None;
            cx.notify();
        }),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |ws, ev: &MouseDownEvent, _w, cx| {
            // Right-click acts on the selection when it lands in
            // it, on this photo alone otherwise.
            if !ws.cloud_is_selected(&context) {
                ws.cloud_select_single(context.clone());
            }
            ws.cloud.context = Some((ev.position, CloudContext::Photo(context.clone())));
            cx.notify();
        }),
    )
    .children(is_lead.then(|| lead_probe(|ws| &mut ws.cloud.grid, cx)))
}

/// The cloud gallery's right-click menu: on a photo, a folder row or a
/// bucket row.
pub(crate) fn context_menu(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    let (position, target) = ws.cloud.context.clone()?;
    let dismiss: fn(&mut Workspace) = |ws| ws.cloud.context = None;
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let row = |rows: &mut Vec<gpui::AnyElement>,
               label: String,
               act: MenuAction,
               cx: &mut Context<Workspace>| {
        rows.push(menu_row(label, dismiss, act, cx));
    };
    match target {
        CloudContext::Photo(id) => {
            let acting = ws.cloud_acting(&id);
            let n = acting.len();
            if let Some(asset) = ws.cloud_asset(&id) {
                row(
                    &mut rows,
                    "Edit".into(),
                    std::rc::Rc::new(move |ws, _w, cx| ws.cloud_open(asset.clone(), cx)),
                    cx,
                );
            }
            if n == 1 {
                row(
                    &mut rows,
                    "Download\u{2026}".into(),
                    std::rc::Rc::new(|ws, _w, cx| ws.cloud_download_selected(cx)),
                    cx,
                );
            }
            rows.push(menu_sep());
            for bucket in ws.cloud.buckets.clone() {
                let add = acting.clone();
                row(
                    &mut rows,
                    format!("Add to {}", bucket.name),
                    std::rc::Rc::new(move |ws, _w, _cx| {
                        ws.cloud_add_to_bucket(bucket.id.clone(), &add)
                    }),
                    cx,
                );
            }
            row(
                &mut rows,
                "Add to new bucket\u{2026}".into(),
                std::rc::Rc::new(|ws, _w, cx| {
                    let mut fields = vec![field("cloud-name", "Bucket name", "")];
                    let mut q = ws.cloud.query.clone();
                    q.text.clear();
                    q.filters = Default::default();
                    fields.extend(filter_fields(&q));
                    form(ws, "new-bucket", fields, cx)
                }),
                cx,
            );
            if let Scope::Bucket { id: bucket } = ws.cloud.query.scope.clone() {
                let remove = acting.clone();
                row(
                    &mut rows,
                    if n > 1 {
                        format!("Remove {n} from this bucket")
                    } else {
                        "Remove from this bucket".into()
                    },
                    std::rc::Rc::new(move |ws, _w, _cx| {
                        ws.cloud_remove_from_bucket(bucket.clone(), &remove)
                    }),
                    cx,
                );
            }
            rows.push(menu_sep());
            if let Some(asset) = ws.cloud_asset(&id) {
                row(
                    &mut rows,
                    "Delete from Schist Cloud\u{2026}".into(),
                    std::rc::Rc::new(move |ws, _w, cx| {
                        ws.cloud.form_target = Some((asset.id.clone(), asset.revision));
                        form(ws, "delete-asset", vec![], cx)
                    }),
                    cx,
                );
            }
        }
        CloudContext::Folder(id) => {
            let folder = ws.cloud.folders.iter().find(|f| f.id == id).cloned()?;
            let rename = folder.clone();
            row(
                &mut rows,
                "Rename\u{2026}".into(),
                std::rc::Rc::new(move |ws, _w, cx| {
                    ws.cloud.form_target = Some((rename.id.clone(), rename.revision));
                    form(
                        ws,
                        "rename-folder",
                        vec![field("cloud-name", "Name", rename.name.clone())],
                        cx,
                    )
                }),
                cx,
            );
            let parent = folder.id.clone();
            row(
                &mut rows,
                "New folder inside\u{2026}".into(),
                std::rc::Rc::new(move |ws, _w, cx| {
                    ws.cloud.form_target = Some((parent.clone(), 0));
                    form(
                        ws,
                        "new-subfolder",
                        vec![field("cloud-name", "Folder name", "")],
                        cx,
                    )
                }),
                cx,
            );
            rows.push(menu_sep());
            let delete = folder;
            row(
                &mut rows,
                "Delete\u{2026}".into(),
                std::rc::Rc::new(move |ws, _w, cx| {
                    ws.cloud.form_target = Some((delete.id.clone(), delete.revision));
                    form(ws, "delete-folder", vec![], cx)
                }),
                cx,
            );
        }
        CloudContext::Bucket(id) => {
            let bucket = ws.cloud.buckets.iter().find(|b| b.id == id).cloned()?;
            let edit = bucket.clone();
            row(
                &mut rows,
                "Edit bucket\u{2026}".into(),
                std::rc::Rc::new(move |ws, _w, cx| {
                    ws.cloud.form_target = Some((edit.id.clone(), edit.revision));
                    let mut fields = vec![field("cloud-name", "Bucket name", edit.name.clone())];
                    let mut q = schist_cloud::AssetQuery::default();
                    if let Some(rule) = &edit.rule {
                        q.scope = rule.scope.clone();
                        q.text = rule.text.clone();
                        q.filters = rule.filters.clone();
                    }
                    ws.cloud.form_scope = q.scope.clone();
                    fields.extend(filter_fields(&q));
                    form(ws, "edit-bucket", fields, cx)
                }),
                cx,
            );
            if !ws.cloud.selected.is_empty() {
                let add = ws.cloud.selected.clone();
                let into = bucket.id.clone();
                row(
                    &mut rows,
                    format!("Add selected ({})", add.len()),
                    std::rc::Rc::new(move |ws, _w, _cx| ws.cloud_add_to_bucket(into.clone(), &add)),
                    cx,
                );
            }
            rows.push(menu_sep());
            let delete = bucket;
            row(
                &mut rows,
                "Delete bucket\u{2026}".into(),
                std::rc::Rc::new(move |ws, _w, cx| {
                    ws.cloud.form_target = Some((delete.id.clone(), delete.revision));
                    form(ws, "delete-bucket", vec![], cx)
                }),
                cx,
            );
        }
    }
    Some(menu_frame(position, rows, dismiss, cx))
}

pub(crate) fn dialog(
    ws: &mut Workspace,
    kind: &'static str,
    fields: Vec<(&'static str, String, String)>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let title = match kind {
        "sign-in" => "Sign into Schist Cloud",
        "search" => "Search cloud photos",
        "filters" => "Filter cloud photos",
        "catalogue" => "Find cloud folders and buckets",
        "new-folder" | "new-subfolder" => "New cloud folder",
        "new-bucket" => "New cloud bucket",
        "edit-bucket" => "Edit cloud bucket",
        "rename-folder" => "Rename cloud folder",
        "delete-folder" => "Delete cloud folder?",
        "delete-bucket" => "Delete cloud bucket?",
        "delete-asset" => "Delete cloud photo?",
        "upload-document" => "Upload document to Schist Cloud",
        "download" => "Download cloud photo",
        _ => "Schist Cloud",
    };
    let mut body = div().flex().flex_col().gap_2();
    if kind.starts_with("delete-") {
        body = body.child(caption(match kind {
            "delete-folder" => "Only an empty folder can be deleted.",
            "delete-asset" => {
                "The photo and its cloud edits are removed for good; buckets holding it \
                 let it go."
            }
            _ => "Photos remain in your cloud library.",
        }));
    }
    if kind == "filters" {
        body = body.child(caption(
            "Leave a field empty to not filter by it. Dates are YYYY-MM-DD.",
        ));
    }
    if kind.ends_with("bucket") && !kind.starts_with("delete") {
        body = body.child(caption(
            "Leave search and filters empty for a bucket filled by dragging photos in.",
        ));
    }
    for (key, label, committed) in fields {
        if key == "cloud-download-format" {
            let mut options = vec![(String::new(), "Current editable document".to_string())];
            if let Some(capabilities) = &ws.cloud.capabilities {
                if capabilities.original_download {
                    options.push(("original".into(), "Original file".into()));
                }
                let mut seen = std::collections::HashSet::new();
                for format in &capabilities.formats {
                    if !format.can_export {
                        continue;
                    }
                    for extension in &format.extensions {
                        if extension != "original"
                            && schist_cloud::transfer::valid_format(extension)
                            && seen.insert(extension.clone())
                        {
                            options.push((
                                extension.clone(),
                                format!("{} (.{})", format.name, extension),
                            ));
                        }
                    }
                }
            }
            let mut choices = div()
                .id("cloud-download-formats")
                .flex()
                .flex_col()
                .gap_1()
                .max_h(px(320.0))
                .overflow_y_scroll();
            for (id, name) in options {
                let display = format!("{} {}", if id == committed { "●" } else { "○" }, name);
                choices = choices.child(ui::button(
                    display,
                    false,
                    move |ws, _, cx| {
                        ws.update_modal(|modal| {
                            if let Modal::Cloud { fields, .. } = modal {
                                if let Some((_, _, selected)) = fields
                                    .iter_mut()
                                    .find(|(key, _, _)| *key == "cloud-download-format")
                                {
                                    *selected = id.clone();
                                }
                            }
                        });
                        cx.notify();
                    },
                    cx,
                ));
            }
            body = body.child(ui::field_row("Format", choices));
            continue;
        }
        if key == "cloud-folder" {
            let mut choices = div().flex().flex_col().gap_1();
            for (id, name) in std::iter::once((String::new(), "Unfiled".to_string())).chain(
                ws.cloud
                    .folders
                    .iter()
                    .map(|f| (f.id.clone(), f.name.clone())),
            ) {
                let display = format!("{} {}", if id == committed { "●" } else { "○" }, name);
                choices = choices.child(ui::button(
                    display,
                    false,
                    move |ws, _, cx| {
                        ws.update_modal(|modal| {
                            if let Modal::Cloud { fields, .. } = modal {
                                if let Some((_, _, v)) =
                                    fields.iter_mut().find(|(k, _, _)| *k == "cloud-folder")
                                {
                                    *v = id.clone();
                                }
                            }
                        });
                        cx.notify();
                    },
                    cx,
                ));
            }
            body = body.child(ui::field_row("Folder", choices));
            continue;
        }
        let active = ws.focused_field == Some(key);
        let shown = if active {
            ws.field_buffer.clone()
        } else {
            committed.clone()
        };
        let value = if active {
            let at = ws.field_cursor.min(shown.len());
            ui::caret_run(
                shown[..at].to_string(),
                shown[at..].to_string(),
                ws.caret_on(),
                ui::palette().text,
            )
            .into_any_element()
        } else {
            div().child(shown).into_any_element()
        };
        body = body.child(ui::field_row(
            label,
            div()
                .w(px(270.0))
                .min_h(px(24.0))
                .px_1()
                .bg(gpui::rgb(ui::palette().field_bg))
                .border_1()
                .border_color(gpui::rgb(if active {
                    ui::palette().accent
                } else {
                    ui::palette().field_bg
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _, _, cx| {
                        ws.focus_field(key, committed.clone());
                        cx.notify();
                    }),
                )
                .child(value),
        ));
    }
    let actions = div()
        .flex()
        .gap_2()
        .child(ui::button(
            "Cancel",
            false,
            |ws, _, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            if kind == "sign-in" {
                "Continue in browser"
            } else if kind == "download" {
                "Download…"
            } else if kind.starts_with("delete-") {
                "Delete"
            } else {
                "Apply"
            },
            true,
            move |ws, _, cx| {
                ws.commit_focused_field();
                let Some(Modal::Cloud { fields, .. }) = ws.modal.clone() else {
                    return;
                };
                match ws.cloud_submit(kind, fields, cx) {
                    Ok(()) => ws.close_modal(cx),
                    Err(e) => {
                        ws.status = e.to_string().into();
                        ws.cloud.message = e.to_string();
                        cx.notify();
                    }
                }
            },
            cx,
        ));
    ui::modal_frame(title, 620.0, body, actions).into_any_element()
}

/// The browser's gallery: the cloud room on its own, since the web has
/// no watched folders. The same strip, sidebar, grid and tray.
#[cfg(target_arch = "wasm32")]
pub(super) fn browser_gallery(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let context_menu = context_menu(ws, cx);
    let sidebar = sidebar_column("cloud-sidebar")
        .child(group_chips(ws.gallery_group_by(), &GroupBy::CLOUD, cx))
        .child(sidebar_caption("FOLDERS"))
        .children(folder_rows(ws, cx))
        .child(sidebar_link(
            "+ New folder…",
            |ws, _w, cx| new_cloud_folder(ws, cx),
            cx,
        ))
        .child(sidebar_caption("BUCKETS"))
        .children(bucket_rows(ws, cx))
        .child(sidebar_link(
            "+ New bucket…",
            |ws, _w, cx| new_cloud_bucket(ws, cx),
            cx,
        ));
    let root = div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        .bg(gpui::rgb(pal().grid_bg))
        .text_color(gpui::rgb(pal().text))
        .track_focus(&ws.focus)
        .on_key_down(cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
            if ws.modal.is_some() {
                if ev.keystroke.key == "enter" {
                    ws.commit_focused_field();
                    ws.confirm_modal(window, cx);
                } else {
                    ws.field_key(&ev.keystroke.key, ev.keystroke.key_char.as_deref());
                }
                cx.notify();
                cx.stop_propagation();
                return;
            }
            if ws.gallery_key(ev, cx) {
                cx.stop_propagation();
            }
        }))
        .child(chrome::top_strip(ws, cx))
        .children(
            (ws.cloud.account.is_none() && ws.cloud.message != "Not signed in").then(|| {
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.0))
                    .child(ws.cloud.message.clone())
            }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_grow()
                .min_h(px(0.0))
                .child(sidebar)
                .child(grid(ws, cx)),
        )
        .child(chrome::tray(ws, cx))
        .children(context_menu)
        .into_any_element();
    ws.cloud_reveal_tick(cx);
    root
}

#[cfg(test)]
mod grouping_tests {
    use super::*;

    fn asset(id: &str, folder: Option<&str>, captured: Option<u64>, modified: u64) -> Asset {
        Asset {
            id: id.into(),
            folder_id: folder.map(str::to_string),
            name: format!("{id}.jpg"),
            mime_type: "image/jpeg".into(),
            revision: 1,
            size: 1,
            edited: false,
            tags: vec![],
            rating: 0,
            captured_at: captured,
            modified_at: modified,
            thumbnail_url: None,
        }
    }
    fn folder(id: &str, parent: Option<&str>, name: &str) -> Folder {
        Folder {
            id: id.into(),
            parent_id: parent.map(str::to_string),
            name: name.into(),
            revision: 1,
            asset_count: Some(0),
        }
    }
    const MARCH_2024: u64 = 1_710_504_000;
    const APRIL_2024: u64 = 1_713_096_000;
    const JAN_2023: u64 = 1_673_000_000;

    #[test]
    fn months_come_newest_first_with_upload_time_standing_in_for_capture() {
        let assets = vec![
            asset("old", None, Some(JAN_2023), APRIL_2024),
            asset("march-a", None, Some(MARCH_2024), 0),
            asset("undated-april", None, None, APRIL_2024),
            asset("march-b", None, Some(MARCH_2024 + 60), 0),
        ];
        let groups = group_assets(&assets, &[], &[], &Scope::Library, "", GroupBy::Date);
        let titles: Vec<&str> = groups.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(titles, ["April 2024", "March 2024", "January 2023"]);
        let march: Vec<&str> = groups[1].2.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(march, ["march-b", "march-a"], "newest first inside a month");
        assert_eq!(groups[0].2[0].id, "undated-april");
    }

    #[test]
    fn folders_group_by_name_with_their_path_and_the_unfiled_last() {
        let folders = vec![
            folder("root", None, "Trips"),
            folder("child", Some("root"), "Alps"),
            folder("zoo", None, "Zoo"),
        ];
        let assets = vec![
            asset("z", Some("zoo"), None, 1),
            asset("loose", None, None, 1),
            asset("a", Some("child"), None, 1),
            asset("gone", Some("missing"), None, 1),
        ];
        let groups = group_assets(&assets, &folders, &[], &Scope::Library, "", GroupBy::Folder);
        let heads: Vec<(&str, &str)> = groups
            .iter()
            .map(|(t, s, _)| (t.as_str(), s.as_str()))
            .collect();
        assert_eq!(
            heads,
            [
                ("Alps", "Trips / Alps"),
                ("Folder", ""),
                ("Zoo", "Zoo"),
                ("Unfiled", ""),
            ]
        );
    }

    #[test]
    fn a_bucket_or_a_search_is_one_strip() {
        let buckets = vec![Bucket {
            id: "b".into(),
            name: "Summer".into(),
            revision: 1,
            asset_count: Some(2),
            rule: Some(Rule {
                scope: Scope::Library,
                text: "beach".into(),
                filters: Filters {
                    min_rating: Some(3),
                    ..Default::default()
                },
            }),
        }];
        let assets = vec![
            asset("x", None, Some(JAN_2023), 0),
            asset("y", None, Some(APRIL_2024), 0),
        ];
        let scope = Scope::Bucket { id: "b".into() };
        let groups = group_assets(&assets, &[], &buckets, &scope, "", GroupBy::Date);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Bucket · Summer");
        assert_eq!(groups[0].1, "\u{201c}beach\u{201d} · 1 filter");
        assert_eq!(groups[0].2.len(), 2, "the provider's order is kept");
        let folders = vec![folder("f", None, "Trips")];
        let scope = Scope::Folder {
            id: "f".into(),
            recursive: true,
        };
        let groups = group_assets(&assets, &folders, &[], &scope, "sun", GroupBy::Folder);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Trips · Search results");
    }

    #[test]
    fn the_sidebar_tree_nests_children_sorts_siblings_and_keeps_orphans() {
        let folders = vec![
            folder("b", None, "Beta"),
            folder("a", None, "Alpha"),
            folder("a2", Some("a"), "Zed"),
            folder("a1", Some("a"), "Apple"),
            folder("lost", Some("off-page"), "Lost"),
        ];
        let tree = folder_tree(&folders);
        let tree: Vec<(usize, &str)> = tree.iter().map(|(d, f)| (*d, f.name.as_str())).collect();
        assert_eq!(
            tree,
            [
                (0, "Alpha"),
                (1, "Apple"),
                (1, "Zed"),
                (0, "Beta"),
                (0, "Lost")
            ]
        );
    }
}
