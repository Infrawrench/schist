//! Bind gallery controls to local/cloud navigation in the editor.
use super::*;
pub use schist_gallery_ui::*;
use schist_i18n::{t, tn};
use schist_ui::ProgressBar;
#[cfg(not(target_arch = "wasm32"))]
use schist_ui::{Button, ButtonColors, IconButton};
pub type GridAccess = schist_gallery_ui::GridAccess<Workspace>;
pub type MenuAction = schist_gallery_ui::MenuAction<Workspace>;

/// What the strip's buttons do, shared with the touch strip's menu.
mod strip_actions {
    use super::*;

    pub fn import(ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>) {
        #[cfg(not(target_arch = "wasm32"))]
        ws.gallery_import_camera(cx);
        #[cfg(target_arch = "wasm32")]
        ws.cloud_pick_upload(false, cx);
    }

    pub fn add_folder(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        if ws.cloud.show {
            ws.cloud_pick_upload(true, cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !ws.cloud.show {
            ws.gallery_add_folder(window, cx);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = window;
    }

    pub fn refresh(ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>) {
        if ws.cloud.show {
            ws.cloud_refresh(cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !ws.cloud.show {
            ws.library_rescan(cx);
        }
    }

    pub fn settings(ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>) {
        ws.snapshot_preferences();
        ws.open_modal(Modal::Preferences, cx);
    }

    pub fn new_file(ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>) {
        ws.open_new_file_picker(cx);
    }

    pub fn back_to_editor(ws: &mut Workspace, _w: &mut Window, cx: &mut Context<Workspace>) {
        ws.gallery_back_to_editor(cx);
    }
}

/// The same toolbar for local and cloud photos; only the action's destination changes.
pub fn top_strip(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    // A phone folds the strip; an iPad has the width for the whole row.
    #[cfg(not(target_arch = "wasm32"))]
    if crate::ui::touch() && ws.gallery_compact {
        return touch_strip(ws, cx);
    }
    let has_doc = ws.doc.is_some();
    let cloud = ws.cloud.show;
    let strip = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        // Wraps rather than clips: a phone is narrower than the row.
        .flex_wrap()
        .min_h(px(38.0))
        .flex_none()
        .px_2()
        .py_1()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .child(gallery_button(
            t("library.strip.import"),
            true,
            strip_actions::import,
            cx,
        ))
        .child(gallery_button(
            t("library.strip.add_folder"),
            false,
            strip_actions::add_folder,
            cx,
        ))
        .child(gallery_button(
            t("common.refresh"),
            false,
            strip_actions::refresh,
            cx,
        ));
    #[cfg(not(target_arch = "wasm32"))]
    let strip = strip.children(
        (!cloud && ws.library.video.is_none())
            .then(|| super::library_culling::toolbar_button(ws, cx)),
    );
    let strip = strip.child(div().flex_grow());
    let strip = if cloud {
        strip
            .children(super::cloud_view::filter_chip(ws, cx))
            .child(super::cloud_view::search_box(ws, cx))
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::library_view::local_strip_search(strip, ws, cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            strip
        }
    };
    strip
        .child(div().flex_grow())
        .child(gallery_button(
            t("library.strip.settings"),
            false,
            strip_actions::settings,
            cx,
        ))
        .child(gallery_button(
            t("common.open_ellipsis"),
            false,
            crate::keymap::open_file_dialog,
            cx,
        ))
        .child(gallery_button(
            t("library.strip.new_file"),
            false,
            strip_actions::new_file,
            cx,
        ))
        .children((has_doc || cfg!(target_arch = "wasm32")).then(|| {
            gallery_button(
                t("library.strip.back_to_editing"),
                false,
                strip_actions::back_to_editor,
                cx,
            )
        }))
        .into_any_element()
}

/// The strip on a phone-width touch window: one row, so it keeps the
/// drawer button, Import (the thing the gallery is for) and the search
/// box, and folds every other button into a "⋯" menu at the right end.
/// The buttons themselves are the desktop's; only their home changes.
/// A wide touch window (an iPad) shows the desktop's full row.
#[cfg(not(target_arch = "wasm32"))]
fn touch_strip(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let m = crate::ui::metrics();
    let cloud = ws.cloud.show;
    let strip = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .min_h(px(m.options_bar_h))
        .flex_none()
        .px_2()
        .py_1()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .children(ws.gallery_compact.then(|| {
            IconButton::new("gallery-drawer", "folder")
                .size(m.icon_button)
                .icon_size(m.icon_button_icon)
                .active(ws.gallery_drawer_open)
                .on_click(cx.listener(|ws, _e, _w, cx| {
                    ws.gallery_drawer_open = !ws.gallery_drawer_open;
                    cx.notify();
                }))
        }))
        .child(gallery_button(
            t("library.strip.import"),
            true,
            strip_actions::import,
            cx,
        ))
        .children(
            (!cloud && ws.library.video.is_none())
                .then(|| super::library_culling::toolbar_button(ws, cx)),
        );
    let strip = if cloud {
        strip
            .children(super::cloud_view::filter_chip(ws, cx))
            .child(super::cloud_view::search_box(ws, cx))
    } else {
        super::library_view::touch_strip_search(strip, ws, cx)
    };
    strip
        .child(div().flex_grow())
        .child(
            Button::new("gallery-more", "\u{22ef}")
                .colors(ButtonColors {
                    bg: Some(pal().button_bg),
                    hover: pal().button_hover,
                    text: pal().text,
                    border: Some(pal().chrome_edge),
                })
                .rounded_md()
                .on_click(cx.listener(|ws, ev: &gpui::ClickEvent, _w, cx| {
                    ws.gallery_more = if ws.gallery_more.is_some() {
                        None
                    } else {
                        Some(ev.position())
                    };
                    cx.notify();
                })),
        )
        .into_any_element()
}

/// The "⋯" menu of the touch strip, when it is open: the buttons the
/// strip has no room for, as rows. Hangs from the button, kept on
/// screen at the right edge.
#[cfg(not(target_arch = "wasm32"))]
pub fn gallery_more_menu(
    ws: &mut Workspace,
    cx: &mut Context<Workspace>,
) -> Option<gpui::AnyElement> {
    let at = ws.gallery_more?;
    let dismiss: fn(&mut Workspace) = |ws| ws.gallery_more = None;
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let row = |rows: &mut Vec<gpui::AnyElement>,
               label: &str,
               act: fn(&mut Workspace, &mut Window, &mut Context<Workspace>),
               cx: &mut Context<Workspace>| {
        rows.push(menu_row(
            label.to_string(),
            dismiss,
            std::rc::Rc::new(act),
            cx,
        ));
    };
    row(
        &mut rows,
        t("library.strip.add_folder"),
        strip_actions::add_folder,
        cx,
    );
    row(&mut rows, t("common.refresh"), strip_actions::refresh, cx);
    rows.push(menu_sep());
    if !ws.cloud.show && super::library_view::search_offer_needed(ws) {
        row(
            &mut rows,
            t("library.strip.enable_search_menu"),
            |ws, _w, cx| ws.open_modal(Modal::SearchModels, cx),
            cx,
        );
    }
    row(
        &mut rows,
        t("library.strip.settings"),
        strip_actions::settings,
        cx,
    );
    rows.push(menu_sep());
    row(
        &mut rows,
        t("common.open_ellipsis"),
        crate::keymap::open_file_dialog,
        cx,
    );
    row(
        &mut rows,
        t("library.strip.new_file"),
        strip_actions::new_file,
        cx,
    );
    if ws.doc.is_some() {
        rows.push(menu_sep());
        row(
            &mut rows,
            t("library.strip.back_to_editing"),
            strip_actions::back_to_editor,
            cx,
        );
    }
    // Hangs from the tap, which is on the strip's last button, with
    // its right edge a little past it.
    let position = at + Point::new(px(12.0), px(20.0));
    Some(menu_frame_at(
        position,
        gpui::Corner::TopRight,
        rows,
        dismiss,
        cx,
    ))
}

pub fn photo_count(count: usize) -> String {
    tn("common.n_photos", count as u64)
}

/// What a tray button does.
pub type TrayAction = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// What the tray says about the current selection and library, in
/// either room.
pub struct TrayInfo {
    /// The green Edit button's action, when one photo leads.
    pub edit: Option<TrayAction>,
    /// A second button beside Edit — the cloud's Download….
    pub extra: Option<(&'static str, TrayAction)>,
    pub name: Option<String>,
    pub selected: usize,
    /// Dim remarks: "edited — versions kept beside the file", "3 hidden
    /// by the content filter".
    pub notes: Vec<String>,
    /// "128 photos".
    pub count: String,
}

/// The bottom tray: selection details and the green Edit button on the
/// left, the photo count and size slider on the right.
pub fn tray(ws: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let info = if ws.cloud.show {
        super::cloud_view::tray_info(ws)
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::library_view::tray_info(ws)
        }
        #[cfg(target_arch = "wasm32")]
        {
            super::cloud_view::tray_info(ws)
        }
    };
    let thumb_px = ws.gallery_thumb_px();
    let ratio = (thumb_px - 80.0) / 160.0;
    div()
        .id("gallery-tray")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(40.0))
        .flex_none()
        .px_2()
        .bg(gpui::rgb(pal().tray_bg))
        .border_t_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .children(info.edit.map(|edit| {
            gallery_button(t("common.edit"), true, move |ws, w, cx| edit(ws, w, cx), cx)
        }))
        .children(
            info.extra.map(|(label, act)| {
                gallery_button(label, false, move |ws, w, cx| act(ws, w, cx), cx)
            }),
        )
        .children(info.name.map(|name| {
            div()
                .text_size(px(12.0))
                .text_color(gpui::rgb(pal().text))
                .child(name)
        }))
        .children((info.selected > 1).then(|| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(tn("common.n_selected", info.selected as u64))
        }))
        .children(info.notes.into_iter().map(|note| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(note)
        }))
        .child(div().flex_grow())
        // The editor's status bar is hidden here, so the tray carries the
        // status line — otherwise an import's outcome lands nowhere. A
        // cloud transfer under way shows as a bar instead.
        .child(match ws.cloud.progress.clone() {
            Some((done, total, label)) => {
                let ratio = if total == 0 {
                    0.0
                } else {
                    (done as f32 / total as f32).clamp(0.0, 1.0)
                };
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .w(px(300.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(gpui::rgb(pal().text_dim))
                            .child(label),
                    )
                    .child(ProgressBar::new(ratio).colors(track_colors()))
                    .into_any_element()
            }
            None => div()
                .max_w(px(420.0))
                .truncate()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(ws.status.clone())
                .into_any_element(),
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(11.0))
                .text_color(gpui::rgb(pal().text_dim))
                .children(
                    (ws.cloud.show && ws.cloud.is_loading())
                        .then(|| loading_spinner("cloud-count-loading")),
                )
                .child(info.count),
        )
        // Touch pinches the grid instead (see `grid_frame`).
        .children((!crate::ui::touch()).then(|| size_slider(ratio, cx)))
}

impl Workspace {
    /// Thumbnail cell edge in pixels, the tray slider's value. One
    /// setting for both rooms on desktop; the browser has only the
    /// cloud's.
    pub(crate) fn gallery_thumb_px(&self) -> f32 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.thumb_px
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.cloud.thumb_px
        }
    }

    /// One grouping choice for both local and cloud photos.
    pub(crate) fn gallery_group_by(&self) -> GroupBy {
        #[cfg(not(target_arch = "wasm32"))]
        let group = self.library.group_by;
        #[cfg(target_arch = "wasm32")]
        let group = self.cloud.group_by;
        group
    }

    /// Whether a gallery search box is taking typing, for the key
    /// context.
    pub(crate) fn gallery_typing(&self) -> bool {
        if !self.gallery_open() {
            return false;
        }
        if self.cloud.show {
            return self.cloud.search.active;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.search.active || self.focused_field == Some("face-name")
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// A key while the gallery has the keyboard and no dialog is up:
    /// the search box first, then the arrows over the grid.
    pub(crate) fn gallery_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.cloud.show {
            return self.cloud_search_key(ev, cx) || self.cloud_nav_key(ev, cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.video_key(ev, cx) {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.similar_review_key(ev, cx) {
                return true;
            }
            if ev.keystroke.key == "enter"
                && !self.gallery_typing()
                && self.focused_field.is_none()
                && !self.ai.input.active
                && !self.ai.model_menu
                && !self.spotlight.open
            {
                return self.gallery_enter(cx);
            }
            self.gallery_culling_key(ev, cx)
                || self.gallery_viewer_key(ev, cx)
                || self.gallery_search_key(ev, cx)
                || self.gallery_nav_key(ev, cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Escape dismisses a popover before leaving the search or photo viewer.
    pub(crate) fn gallery_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.gallery_open() {
            return false;
        }
        if self.open_popup.is_some() {
            self.close_popup(cx);
            return true;
        }
        if self.cloud.show {
            return self.cloud_search_clear(cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.local_gallery_escape(cx)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Enter in the gallery opens the selected photo, unless the search
    /// box has the keyboard. Returns whether the gallery took it.
    pub(crate) fn gallery_enter(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.gallery_open() {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !self.cloud.show && self.video_enter(cx) {
            return true;
        }
        if self.cloud.show {
            if self.cloud.search.active {
                return false;
            }
            if let Some(asset) = self.cloud_lead_asset() {
                self.cloud_open(asset, cx);
            }
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.library.similar.open && self.library.viewer.is_none() {
                return true;
            }
            if self.library.search.active {
                return false;
            }
            let path = self
                .library
                .comparison
                .as_ref()
                .map(|c| c.paths[c.active].clone())
                .or_else(|| self.library.lead_selected().cloned());
            if let Some(path) = path {
                self.open_from_gallery(path, cx);
            }
            true
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// The strip's "Back to Editing".
    pub(crate) fn gallery_back_to_editor(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.toggle_gallery(cx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.cloud_set_visible(false);
            cx.notify();
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Workspace {
    pub fn set_gallery_thumb_px(&mut self, value: f32) {
        self.cloud.thumb_px = value.clamp(80.0, 240.0);
    }

    pub fn nudge_gallery_thumb_px(&mut self, wheel_dy: f32) {
        let value = self.cloud.thumb_px + wheel_dy * 0.2;
        self.set_gallery_thumb_px(value);
    }

    pub fn set_gallery_group(&mut self, group: GroupBy, cx: &mut Context<Self>) {
        self.cloud.group_by = group;
        self.cloud.query.offset = 0;
        self.cloud.query.sort = self.cloud_sort();
        self.cloud_watch_assets(true);
        cx.notify();
    }
}

impl schist_gallery_ui::GalleryHost for Workspace {
    fn reset_caret_phase(&mut self) {
        Workspace::reset_caret_phase(self);
    }
    fn gallery_thumb_px(&self) -> f32 {
        Workspace::gallery_thumb_px(self)
    }
    fn set_gallery_thumb_px(&mut self, size: f32) {
        Workspace::set_gallery_thumb_px(self, size);
    }
    fn nudge_gallery_thumb_px(&mut self, delta: f32) {
        Workspace::nudge_gallery_thumb_px(self, delta);
    }
    fn set_gallery_group(&mut self, group: GroupBy, cx: &mut Context<Self>) {
        Workspace::set_gallery_group(self, group, cx);
    }
}
