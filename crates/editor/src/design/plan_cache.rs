//! Repainting a selection, a panel or the caret does not recompose the document.
//! Keep one complete input snapshot instead of relying on every edit site to
//! remember an invalidation counter. Embedded assets remain shared through Arc.
use schist_layout::{LayoutDocument, Pasteboard, PasteboardView};

#[derive(Default)]
pub(super) struct PlanCache {
    entry: Option<Entry>,
    #[cfg(test)]
    builds: usize,
}
struct Entry {
    document: LayoutDocument,
    view: PasteboardView,
    fonts: usize,
    plan: Pasteboard,
}
impl PlanCache {
    pub(super) fn clear(&mut self) {
        self.entry = None;
    }
    pub(super) fn get(
        &mut self,
        document: &LayoutDocument,
        view: &PasteboardView,
        fonts: usize,
    ) -> Option<Pasteboard> {
        if let Some(entry) = &self.entry {
            if entry.fonts == fonts && entry.view == *view && entry.document == *document {
                return Some(entry.plan.clone());
            }
        }
        // Never keep a plan belonging to a document that no longer has pages.
        self.entry = None;
        let plan = schist_layout::pasteboard::pasteboard(document, view)?;
        #[cfg(test)]
        {
            self.builds += 1;
        }
        self.entry = Some(Entry {
            document: document.clone(),
            view: view.clone(),
            fonts,
            plan: plan.clone(),
        });
        Some(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{composition, DesignState, Typing};
    use schist_layout::{authoring, Display, Rect, Story};

    #[test]
    fn unchanged_paints_reuse_layout_and_every_changed_input_matches_a_fresh_plan() {
        let mut state = DesignState::new();
        let frame = authoring::text_frame(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(20.0, 20.0, 160.0, 180.0),
        )
        .unwrap();
        state.document.stories[frame.story.0 as usize] =
            Story::from_text("Aé body text with several words to wrap", "Body");
        let mut cache = PlanCache::default();
        let mut view = state.view_for_mode();
        let mut fonts = 0;
        for change in 0..10 {
            match change {
                0 => {}
                1 => state.document.stories[0] = Story::from_text("Changed 空 body", "Body"),
                2 => state.document.styles.paragraphs[0].point_size = Some(18.0),
                3 => state.document.objects[0].bounds.width = 90.0,
                4 => state.document.pages[0].margins.top += 5.0,
                5 => view.scale = 1.75,
                6 => view.origin.x += 13.0,
                7 => view.show_baseline = !view.show_baseline,
                8 => fonts += 1,
                9 => state.document.pages[0].hidden = true,
                _ => unreachable!(),
            }
            let fresh = schist_layout::pasteboard::pasteboard(&state.document, &view);
            for _ in 0..5 {
                assert_eq!(cache.get(&state.document, &view, fonts), fresh);
            }
            assert_eq!(cache.builds, change + 1);
        }
        state.document.pages.clear();
        assert!(cache.get(&state.document, &view, fonts).is_none());
        assert!(cache.entry.is_none());
    }

    #[test]
    fn selection_guide_visibility_ime_drafts_and_undo_do_not_leave_a_stale_plan() {
        let mut state = DesignState::new();
        let frame = authoring::text_frame(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(20.0, 20.0, 160.0, 180.0),
        )
        .unwrap();
        assert!(authoring::set_text(
            &mut state.document,
            &mut state.history,
            frame.story,
            "Aé"
        ));
        state.document.pages[0]
            .guides
            .push(schist_layout::geometry::RulerGuide {
                horizontal: true,
                position: 45.0,
                locked: false,
            });
        let original = state.plan().unwrap();
        assert!(original.pages.iter().any(|p| p
            .guides
            .iter()
            .any(|g| g.kind() == schist_layout::pasteboard::GuideKind::Ruler)));
        state.selection = vec![frame.object];
        for _ in 0..10 {
            assert_eq!(state.plan().unwrap(), original);
        }
        assert_eq!(state.plan_cache.borrow().builds, 1);
        state.show_guides = false;
        let hidden = state.plan().unwrap();
        assert!(hidden.pages.iter().all(|p| p
            .guides
            .iter()
            .all(|g| g.kind() != schist_layout::pasteboard::GuideKind::Ruler)));
        state.show_guides = true;
        assert_eq!(state.plan().unwrap(), original);
        assert_eq!(state.plan_cache.borrow().builds, 1);
        crate::design::guides::begin(&mut state, true, schist_layout::Point::new(0.0, 60.0));
        let guide_draft = state.plan().unwrap();
        assert_ne!(guide_draft, original);
        assert_eq!(state.document.pages[0].guides.len(), 1);
        crate::design::guides::drag(&mut state, schist_layout::Point::new(0.0, 75.0));
        assert_ne!(state.plan().unwrap(), guide_draft);
        state.guide_drag = None;
        assert_eq!(state.plan().unwrap(), original);
        state.typing = Some(Typing {
            object: frame.object,
            story: frame.story,
            at: 3,
            anchor: 3,
        });
        state.text_buffer = "Aé".into();
        assert!(composition::replace(&mut state, None, "空", None, true));
        let draft = state.plan().unwrap();
        assert_ne!(draft, original);
        assert!(draft
            .pages
            .iter()
            .flat_map(|p| &p.objects)
            .any(|d| matches!(d, Display::Text { text, .. } if text.contains('空'))));
        assert_eq!(state.document.stories[0].text(), "Aé");
        composition::cancel(&mut state);
        assert_eq!(state.plan().unwrap(), original);
        assert!(authoring::set_text(
            &mut state.document,
            &mut state.history,
            frame.story,
            "New committed text"
        ));
        let changed = state.plan().unwrap();
        assert_ne!(changed, original);
        assert!(state.history.undo(&mut state.document));
        assert_eq!(state.plan().unwrap(), original);
        assert!(state.history.redo(&mut state.document));
        assert_eq!(state.plan().unwrap(), changed);
    }
}
