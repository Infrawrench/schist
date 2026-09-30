//! Moving things on the pasteboard, reversibly.
//!
//! Every edit goes through `schist_layout::History` rather than mutating
//! the document directly, so Design Mode's undo works the same way the
//! photo editor's does and a mistake on a page is one keystroke to fix
//! rather than a lost document.
//!
//! The rule that matters here: a drag records **one** edit, at the end,
//! from where the object started to where it ended. Recording per-frame
//! would fill the undo stack with a hundred entries for one gesture, and
//! undo would then walk back through the drag a pixel at a time.

use schist_layout::edit::snapshot_object;
use schist_layout::{LayoutEdit, ObjectId, Point, Rect};

use super::Drag;

/// How far the pointer has moved since a drag began.
fn pointer_delta(drag: &Drag) -> Point {
    Point::new(drag.to.x - drag.from.x, drag.to.y - drag.from.y)
}

/// How far a drag must move before it counts as a move rather than a
/// click, in page points.
///
/// Without this, a click that jitters the pointer by a fraction of a point
/// would nudge the object and record an undo entry, so clicking something
/// would slowly walk it across the page.
const THRESHOLD: schist_layout::Pt = 2.0;

/// A move of one object, from one place to another.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub object: ObjectId,
    /// The object's bounds before the drag.
    pub before: Rect,
    /// The object's bounds after it.
    pub after: Rect,
}

impl Move {
    /// How far the object moved, as a vector.
    pub fn delta(&self) -> Point {
        Point::new(self.after.x - self.before.x, self.after.y - self.before.y)
    }

    /// Whether the move is worth recording at all.
    ///
    /// A drag that ended where it started is a click, and a click that
    /// records an undo entry would make the stack lie about what the user
    /// did.
    pub fn worth_recording(&self) -> bool {
        self.before != self.after
    }

    /// Whether a drag from `from` to `to` has passed the threshold.
    pub fn passed_threshold(from: Point, to: Point) -> bool {
        (to.x - from.x).abs() > THRESHOLD || (to.y - from.y).abs() > THRESHOLD
    }

    /// The edit that performs this move, or `None` if the object has gone
    /// since the drag started.
    pub fn to_edit(&self, state: &super::DesignState) -> Option<LayoutEdit> {
        let placed = state.document.object(self.object)?;
        let after = snapshot_object(placed);
        let mut before = after.clone();
        before.bounds = [
            self.before.x,
            self.before.y,
            self.before.width,
            self.before.height,
        ];
        Some(LayoutEdit::ObjectChanged {
            id: self.object.0,
            before,
            after,
        })
    }

    /// Record the move, applying it in one step.
    pub fn record(&self, state: &mut super::DesignState) -> bool {
        if !self.worth_recording() {
            return false;
        }
        let Some(edit) = self.to_edit(state) else {
            return false;
        };
        // `forward` is the document's own application of an edit, so a
        // move cannot drift from what undo would put back.
        if !schist_layout::edit::forward(&mut state.document, &edit) {
            return false;
        }
        state.history.record(edit);
        true
    }
}

/// Begin a drag on an object.
pub fn begin(state: &mut super::DesignState, object: ObjectId, at: Point) {
    let Some(bounds) = state.document.object(object).map(|p| p.bounds) else {
        return;
    };
    if !state.selection.contains(&object) {
        state.selection = vec![object];
    }
    state.drag_objects = state
        .selection
        .iter()
        .filter_map(|id| state.document.object(*id).map(|o| (*id, o.bounds)))
        .collect();
    state.drag = Some(Drag {
        object,
        from: at,
        to: at,
        bounds,
    });
}

/// Continue a drag, moving the object live.
///
/// The document is changed directly here, with no history entry: the
/// gesture is not finished, so there is nothing yet to undo. A cancel
/// puts the object back.
pub fn drag_to(state: &mut super::DesignState, at: Point) {
    let Some(drag) = state.drag else {
        return;
    };
    if state
        .drag_objects
        .iter()
        .any(|(id, _)| state.document.object(*id).is_none() || state.document.object_locked(*id))
    {
        cancel(state);
        return;
    }
    let page = state
        .document
        .object(drag.object)
        .map_or(state.current_page(), |o| o.page);
    let delta = if Move::passed_threshold(drag.from, at) {
        super::guides::snap_delta(
            state,
            page,
            drag.bounds,
            pointer_delta(&Drag { to: at, ..drag }),
        )
    } else {
        Point::ZERO
    };
    for (id, before) in &state.drag_objects {
        if let Some(object) = state.document.objects.iter_mut().find(|o| o.id == *id) {
            object.bounds = before.translated(delta);
        }
    }
    state.drag = Some(Drag { to: at, ..drag });
}

/// Finish the entire selection move in one history entry.
pub fn end(state: &mut super::DesignState) -> bool {
    if state.drag.take().is_none() {
        return false;
    }
    let originals = std::mem::take(&mut state.drag_objects);
    let edits: Vec<_> = originals
        .into_iter()
        .filter_map(|(object, before)| {
            let after = state.document.object(object)?.bounds;
            let movement = Move {
                object,
                before,
                after,
            };
            movement
                .worth_recording()
                .then(|| movement.to_edit(state))
                .flatten()
        })
        .collect();
    if edits.is_empty() {
        return false;
    }
    state.history.record(LayoutEdit::Batch { edits });
    true
}

/// Abandon a drag, restoring every participating object exactly.
pub fn cancel(state: &mut super::DesignState) {
    state.drag = None;
    for (id, bounds) in std::mem::take(&mut state.drag_objects) {
        if let Some(object) = state.document.objects.iter_mut().find(|o| o.id == id) {
            object.bounds = bounds;
        }
    }
}

/// Select an object, or the page when there is nothing under the pointer.
pub fn click(state: &mut super::DesignState, hit: super::select::Hit) {
    match hit {
        // A locked object is still selected, so its properties can be
        // read and it can be unlocked. What it will not do is move, which
        // `drag_to` enforces.
        super::select::Hit::Object { object, .. } => {
            state.selection = vec![object];
        }
        super::select::Hit::Page { .. } | super::select::Hit::Nothing => {
            state.selection.clear();
        }
    }
}

/// Select everything on the current page, front to back.
pub fn select_all(state: &mut super::DesignState) {
    let page = state.current_page();
    // Going through the plan rather than the document, so "everything on
    // this page" means everything the reader can see, including a parent
    // page's items and excluding the other page of a spread.
    let plan = match schist_layout::pasteboard::pasteboard(&state.document, &state.view) {
        Some(plan) => plan,
        None => {
            state.selection.clear();
            return;
        }
    };
    state.selection = plan
        .pages
        .iter()
        .find(|p| p.page.page == page)
        .map(super::select::ids)
        .unwrap_or_default();
    state
        .selection
        .retain(|id| state.document.object(*id).is_some());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::DesignState;
    use schist_layout::pasteboard::pasteboard;
    use schist_layout::{
        blank_a4, LayoutDocument, LayoutObject, ObjectId, PlacedObject, Point, Rect, Story,
    };

    /// A document with one text frame, and the frame's id.
    fn doc_with_frame() -> (LayoutDocument, ObjectId) {
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text("hello", "Body"));
        let id = ObjectId::next();
        doc.add_object(PlacedObject {
            id,
            page: 0,
            bounds: Rect::new(100.0, 100.0, 200.0, 40.0),
            object: LayoutObject::TextFrame {
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Body".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        (doc, id)
    }

    fn state_with_frame() -> (DesignState, ObjectId) {
        let (document, id) = doc_with_frame();
        let mut state = DesignState::new();
        state.document = document;
        (state, id)
    }

    #[test]
    fn a_drag_moves_the_object_and_records_one_edit() {
        let (mut state, id) = state_with_frame();
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(190.0, 160.0));
        assert_eq!(
            state.document.object(id).unwrap().bounds,
            Rect::new(140.0, 140.0, 200.0, 40.0)
        );
        assert!(
            !state.history.can_undo(),
            "a drag in flight is not yet undoable"
        );
        assert!(end(&mut state));
        assert_eq!(state.history.undo_depth(), 1, "one drag is one entry");
    }

    #[test]
    fn a_drag_depends_only_on_its_endpoints_and_can_return_to_its_start() {
        for steps in [1, 2, 10, 100] {
            let (mut state, id) = state_with_frame();
            let before = state.document.object(id).unwrap().bounds;
            begin(&mut state, id, Point::new(150.0, 120.0));
            for step in 1..=steps {
                let distance = 40.0 * step as f32 / steps as f32;
                drag_to(&mut state, Point::new(150.0 + distance, 120.0 + distance));
            }
            assert_eq!(
                state.document.object(id).unwrap().bounds,
                before.translated(Point::new(40.0, 40.0))
            );
            drag_to(&mut state, Point::new(150.0, 120.0));
            assert!(!end(&mut state));
            assert_eq!(state.document.object(id).unwrap().bounds, before);
            assert_eq!(state.history.undo_depth(), 0);
        }
    }

    #[test]
    fn undo_puts_a_moved_object_back() {
        let (mut state, id) = state_with_frame();
        let start = state.document.object(id).unwrap().bounds;
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(190.0, 160.0));
        end(&mut state);
        assert_ne!(state.document.object(id).unwrap().bounds, start);
        state.history.undo(&mut state.document);
        assert_eq!(state.document.object(id).unwrap().bounds, start);
    }

    #[test]
    fn a_drag_of_one_point_is_a_click_and_records_nothing() {
        // Otherwise every click would walk the object a little and fill
        // the undo stack.
        let (mut state, id) = state_with_frame();
        let start = state.document.object(id).unwrap().bounds;
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(150.5, 120.5));
        assert!(!end(&mut state));
        assert_eq!(state.history.undo_depth(), 0);
        assert_eq!(state.document.object(id).unwrap().bounds, start);
    }

    #[test]
    fn a_threshold_separates_a_click_from_a_drag() {
        assert!(!Move::passed_threshold(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0)
        ));
        assert!(Move::passed_threshold(
            Point::new(0.0, 0.0),
            Point::new(0.0, 3.0)
        ));
    }

    #[test]
    fn a_cancelled_drag_puts_the_object_back() {
        let (mut state, id) = state_with_frame();
        let start = state.document.object(id).unwrap().bounds;
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(190.0, 160.0));
        assert_ne!(state.document.object(id).unwrap().bounds, start);
        cancel(&mut state);
        assert_eq!(state.document.object(id).unwrap().bounds, start);
        assert_eq!(state.history.undo_depth(), 0, "a cancel records nothing");
    }

    #[test]
    fn a_locked_object_does_not_move() {
        let (mut state, id) = state_with_frame();
        if let Some(placed) = state.document.objects.iter_mut().find(|o| o.id == id) {
            placed.locked = true;
        }
        let start = state.document.object(id).unwrap().bounds;
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(190.0, 160.0));
        assert_eq!(state.document.object(id).unwrap().bounds, start);
        assert!(state.drag.is_none(), "the drag ends rather than hanging");
        assert!(!end(&mut state));
    }

    #[test]
    fn a_drag_of_a_vanished_object_is_harmless() {
        // The object can go away mid-gesture if an undo lands, and a
        // pointer handler must not panic on it.
        let (mut state, id) = state_with_frame();
        begin(&mut state, id, Point::new(150.0, 120.0));
        state.document.objects.retain(|o| o.id != id);
        drag_to(&mut state, Point::new(190.0, 160.0));
        assert!(!end(&mut state));
    }

    #[test]
    fn clicking_an_object_selects_it_and_clicking_paper_clears() {
        let (mut state, id) = state_with_frame();
        click(
            &mut state,
            crate::design::select::Hit::Object {
                object: id,
                bounds: Rect::new(100.0, 100.0, 200.0, 40.0),
                at: Point::new(150.0, 120.0),
                inherited: false,
                locked: false,
            },
        );
        assert_eq!(state.selection, vec![id]);
        click(&mut state, crate::design::select::Hit::Page { page: 0 });
        assert!(state.selection.is_empty());
    }

    #[test]
    fn a_locked_object_is_selected_so_it_can_be_unlocked() {
        // Selecting it is harmless; what it will not do is move.
        let (mut state, id) = state_with_frame();
        click(
            &mut state,
            crate::design::select::Hit::Object {
                object: id,
                bounds: Rect::new(100.0, 100.0, 200.0, 40.0),
                at: Point::new(150.0, 120.0),
                inherited: false,
                locked: true,
            },
        );
        assert_eq!(state.selection, vec![id]);
    }

    #[test]
    fn select_all_takes_the_current_page_only() {
        let (mut state, id) = state_with_frame();
        state.page = Some(0);
        select_all(&mut state);
        assert_eq!(state.selection, vec![id]);
        // Another page has none of them.
        state.page = Some(1);
        select_all(&mut state);
        assert!(state.selection.is_empty());
    }

    #[test]
    fn a_move_onto_another_page_is_a_move_not_a_copy() {
        // Bounds are page-relative, so a page change is the caller's job;
        // a drag must not silently invent one.
        let (mut state, id) = state_with_frame();
        state.page = Some(0);
        state.view.page = Some(0);
        let plan = pasteboard(&state.document, &state.view).unwrap();
        assert_eq!(plan.pages.len(), 1, "one page, so nothing to spread");
        begin(&mut state, id, Point::new(150.0, 120.0));
        drag_to(&mut state, Point::new(150.0, 400.0));
        end(&mut state);
        assert_eq!(
            state.document.object(id).unwrap().page,
            0,
            "a drag does not change which page an object is on"
        );
    }

    #[test]
    fn a_move_reports_its_delta() {
        let move_edit = Move {
            object: ObjectId::next(),
            before: Rect::new(10.0, 10.0, 20.0, 20.0),
            after: Rect::new(15.0, 4.0, 20.0, 20.0),
        };
        assert_eq!(move_edit.delta(), Point::new(5.0, -6.0));
        assert!(move_edit.worth_recording());
    }
}
