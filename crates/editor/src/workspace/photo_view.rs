//! Transient thumbnail state for Photo Development. Layout lives in preferences.
use super::*;
use schist_app_settings::workspaces::PhotoDisplay;
use schist_core::DocumentId;

#[derive(Default)]
pub(crate) struct PhotoView {
    thumbnails: FxHashMap<DocumentId, (u64, Arc<RenderImage>)>,
    pub scroll: gpui::ScrollHandle,
    pub reveal_selection: bool,
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
