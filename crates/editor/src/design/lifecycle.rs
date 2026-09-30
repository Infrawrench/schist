//! Layout save state is independent of raster tabs. Dirty detection compares
//! content, so undoing back to the saved document becomes clean again.
use schist_layout::LayoutDocument;
use std::{path::PathBuf, sync::Arc};

pub enum Transition {
    New,
    Close,
    Open {
        path: PathBuf,
        document: Box<LayoutDocument>,
        skipped: Vec<String>,
    },
    Quit,
}
pub struct Lifecycle {
    saved: LayoutDocument,
    pub pending: Option<Transition>,
    pub waiting_save: bool,
    pub load: Arc<()>,
}
impl Lifecycle {
    pub fn new(document: &LayoutDocument) -> Self {
        Self {
            saved: document.clone(),
            pending: None,
            waiting_save: false,
            load: Arc::new(()),
        }
    }
    pub fn dirty(&self, document: &LayoutDocument) -> bool {
        self.saved != *document
    }
    pub fn saved(&mut self, document: &LayoutDocument) {
        self.saved = document.clone();
    }
    pub fn cancel(&mut self) {
        self.pending = None;
        self.waiting_save = false;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_content_not_undo_depth_defines_dirty_state_across_branches() {
        let mut doc = schist_layout::blank_a4();
        let mut history = schist_layout::History::default();
        let mut life = Lifecycle::new(&doc);
        for count in 1..6 {
            let saved = doc.clone();
            for n in 0..count {
                schist_layout::authoring::rectangle(
                    &mut doc,
                    &mut history,
                    0,
                    schist_layout::Rect::new(n as f32, 0.0, 20.0, 20.0),
                    schist_layout::authoring::Paint::none(),
                )
                .unwrap();
            }
            assert!(life.dirty(&doc));
            for _ in 0..count {
                history.undo(&mut doc);
            }
            assert_eq!(doc, saved);
            assert!(!life.dirty(&doc));
            schist_layout::authoring::rectangle(
                &mut doc,
                &mut history,
                0,
                schist_layout::Rect::new(40.0, 20.0, 30.0, 20.0),
                schist_layout::authoring::Paint::none(),
            )
            .unwrap();
            life.saved(&doc);
            assert!(!life.dirty(&doc));
        }
    }
}
