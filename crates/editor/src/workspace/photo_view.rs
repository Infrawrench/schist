//! Transient thumbnail state for Photo Development. Layout lives in preferences.
use super::*;
use schist_app_settings::workspaces::PhotoDisplay;
use schist_core::DocumentId;

#[derive(Default)]
pub(crate) struct PhotoView {
    thumbnails: FxHashMap<DocumentId, (u64, Arc<RenderImage>)>,
    pub scroll: gpui::ScrollHandle,
    pub reveal_selection: bool,
    /// Tool controls stay out of the initial photo layout until a tool is chosen.
    pub tool_options: bool,
}

impl Workspace {
    pub(super) fn canvas_surround(&self) -> u32 {
        if self.photo_workspace() {
            crate::panels::photo::VIEWER_BG
        } else {
            crate::ui::palette().canvas_bg
        }
    }

    pub(crate) fn photo_workspace(&self) -> bool {
        self.view.photo_layout.enabled && !self.design_mode()
    }

    pub(crate) fn set_photo_display(&mut self, display: PhotoDisplay, cx: &mut Context<Self>) {
        let mut next = self.view.clone();
        next.photo_layout.display = display;
        if self.commit_workspace_view(next, cx) {
            self.photo_view.reveal_selection = true;
        }
    }

    /// Open the gallery's persisted bucket, including smart membership.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_photo_bucket(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.library.buckets.len() {
            return;
        }
        self.cloud.show = false;
        self.library.bucket_filter = Some(index);
        self.library.folder_filter = None;
        self.library.person_filter = None;
        self.library.map_view = false;
        self.library.viewer = None;
        self.library.video = None;
        self.library.comparison = None;
        self.close_similar_review();
        if self.library.tethered.open {
            self.close_tethered(cx);
        }
        self.library.selected.clear();
        if !self.library.open {
            self.toggle_gallery(cx);
        }
        self.gallery_search_clear(cx);
        cx.notify();
    }

    /// Same ordering as tab_strip, including the active document's gap.
    /// Parked documents only composite once, rather than on every UI frame.
    pub(crate) fn photo_thumbnails(&mut self) -> Vec<Option<Arc<RenderImage>>> {
        let open: FxHashSet<_> = self
            .doc
            .iter()
            .map(|d| d.id)
            .chain(self.background_tabs.iter().map(|tab| tab.doc.id))
            .collect();
        self.photo_view.thumbnails.retain(|id, (_, image)| {
            if open.contains(id) {
                true
            } else {
                self.retired_images.push(image.clone());
                false
            }
        });
        let active = self.document_thumbnail();
        let mut parked = self.background_tabs.iter();
        (0..self.tab_count())
            .map(|index| {
                if index == self.active_tab {
                    return active.clone();
                }
                let doc = &parked.next()?.doc;
                if let Some((revision, image)) = self.photo_view.thumbnails.get(&doc.id) {
                    if *revision == doc.revision {
                        return Some(image.clone());
                    }
                }
                let image = super::chrome::make_thumbnail(doc, &mut TileCache::new())?;
                if let Some((_, old)) = self
                    .photo_view
                    .thumbnails
                    .insert(doc.id, (doc.revision, image.clone()))
                {
                    self.retired_images.push(old);
                }
                Some(image)
            })
            .collect()
    }
}
