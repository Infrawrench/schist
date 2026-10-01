//! Pen gestures keep a draft outside the document until pointer release.
//! Each click or handle drag adds one anchor and exactly one undo step.
use super::DesignState;
use schist_layout::{authoring, BezierHandles, ObjectId, Point, ShapePath, SubPath};

#[derive(Default)]
pub struct Pen {
    pub object: Option<ObjectId>,
    pub page: usize,
    pub draft: Option<ShapePath>,
    closing: bool,
}

pub fn press(state: &mut DesignState, at: Point) {
    state.typing = None;
    state.drag = None;
    state.drawing = None;
    let page = state.current_page();
    let mut pen = state.pen.take().filter(|p| p.page == page).unwrap_or(Pen {
        page,
        ..Default::default()
    });
    let mut path = pen
        .object
        .and_then(|id| {
            let object = state.document.object(id)?;
            if state.document.object_locked(id) {
                return None;
            }
            let schist_layout::LayoutObject::Shape { path, .. } = &object.object else {
                return None;
            };
            let mut path = path.clone();
            path.map_points(|p| p + object.bounds.origin());
            Some(path)
        })
        .unwrap_or_default();
    if path.subpaths.is_empty() {
        pen.object = None;
        path.subpaths.push(SubPath::default());
    }
    let sub = &mut path.subpaths[0];
    let tolerance = 6.0 / state.view.scale.max(0.01);
    pen.closing = sub.points.len() >= 2
        && sub
            .points
            .first()
            .is_some_and(|first| (first.x - at.x).hypot(first.y - at.y) <= tolerance);
    if pen.closing {
        sub.closed = true;
    } else {
        sub.points.push(at);
    }
    pen.draft = Some(path);
    state.pen = Some(pen);
}

pub fn drag(state: &mut DesignState, at: Point) {
    let Some(pen) = state.pen.as_mut() else {
        return;
    };
    let Some(path) = pen.draft.as_mut() else {
        return;
    };
    let sub = &mut path.subpaths[0];
    let index = if pen.closing { 0 } else { sub.points.len() - 1 };
    let anchor = sub.points[index];
    sub.set_handles(
        index,
        if at == anchor {
            BezierHandles::default()
        } else {
            BezierHandles {
                incoming: Some(anchor.scale(2.0) - at),
                outgoing: Some(at),
            }
        },
    );
}

pub fn release(state: &mut DesignState) -> bool {
    let Some(mut pen) = state.pen.take() else {
        return false;
    };
    let Some(path) = pen.draft.take() else {
        state.pen = Some(pen);
        return false;
    };
    let changed = if let Some(id) = pen.object {
        authoring::replace_path(&mut state.document, &mut state.history, id, path)
    } else {
        let paint = state
            .document
            .inks
            .first()
            .map(|ink| authoring::Paint::stroked(&ink.name, 1.0))
            .unwrap_or_default();
        pen.object = authoring::path_shape(
            &mut state.document,
            &mut state.history,
            pen.page,
            path,
            paint,
        );
        pen.object.is_some()
    };
    if let Some(id) = pen.object {
        state.selection = vec![id];
    }
    if !pen.closing {
        state.pen = Some(pen);
    }
    changed
}

pub fn preview(state: &DesignState) -> Option<ShapePath> {
    let pen = state.pen.as_ref()?;
    let mut path = pen.draft.clone().or_else(|| {
        let placed = state.document.object(pen.object?)?;
        let schist_layout::LayoutObject::Shape { path, .. } = &placed.object else {
            return None;
        };
        let mut path = path.clone();
        path.map_points(|p| p + placed.bounds.origin());
        Some(path)
    })?;
    let origin = state.document.page_origin(pen.page).unwrap_or_default();
    let view = state.view_for_mode();
    path.map_points(|p| view.to_pasteboard(p + origin));
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{tools, DesignTool};

    #[test]
    fn every_anchor_gesture_is_one_exact_undo_regardless_of_pointer_events() {
        for count in 1..12 {
            let mut state = DesignState::new();
            for index in 0..5 {
                let before = state.document.clone();
                let depth = state.history.undo_depth();
                let at = Point::new(20.0 + index as f32 * 40.0, 100.0);
                tools::press(&mut state, DesignTool::Pen, at);
                for step in 0..count {
                    tools::drag(&mut state, at + Point::new(0.0, step as f32));
                }
                assert_eq!(state.document, before, "draft is not an edit");
                assert!(tools::release(&mut state));
                assert_eq!(state.history.undo_depth(), depth + 1);
                let after = state.document.clone();
                assert!(state.history.undo(&mut state.document));
                assert_eq!(state.document, before);
                assert!(state.history.redo(&mut state.document));
                assert_eq!(state.document, after);
            }
        }
    }

    #[test]
    fn closing_is_an_edit_and_cancelling_discards_only_the_live_draft() {
        let mut state = DesignState::new();
        for point in [
            Point::new(50.0, 50.0),
            Point::new(100.0, 50.0),
            Point::new(100.0, 100.0),
        ] {
            press(&mut state, point);
            release(&mut state);
        }
        let open = state.document.clone();
        press(&mut state, Point::new(50.0, 50.0));
        assert!(release(&mut state));
        assert!(state.pen.is_none());
        state.history.undo(&mut state.document);
        assert_eq!(state.document, open);
        press(&mut state, Point::new(200.0, 100.0));
        drag(&mut state, Point::new(200.0, 150.0));
        state.cancel_gesture();
        assert_eq!(state.document, open, "Escape discards uncommitted drag");
        assert!(state.pen.is_none());
    }

    #[test]
    fn direct_handles_move_one_step_and_keep_other_page_space_geometry() {
        use schist_layout::authoring::{Paint, PointPart, PointRef, ShapeKind};
        for part in [PointPart::Anchor, PointPart::Incoming, PointPart::Outgoing] {
            let mut state = DesignState::new();
            let id = authoring::shape(
                &mut state.document,
                &mut state.history,
                0,
                schist_layout::Rect::new(100.0, 100.0, 200.0, 100.0),
                ShapeKind::Ellipse,
                Paint::none(),
            )
            .unwrap();
            state.selection = vec![id];
            let before = state.document.clone();
            let placed = state.document.object(id).unwrap();
            let schist_layout::LayoutObject::Shape { path, .. } = &placed.object else {
                unreachable!()
            };
            let h = path.subpaths[0].handles_at(0);
            let local = match part {
                PointPart::Anchor => path.subpaths[0].points[0],
                PointPart::Incoming => h.incoming.unwrap(),
                PointPart::Outgoing => h.outgoing.unwrap(),
            };
            let start = local + placed.bounds.origin();
            assert_eq!(
                tools::anchor_at(&state, start),
                Some((
                    id,
                    PointRef {
                        subpath: 0,
                        index: 0,
                        part
                    }
                ))
            );
            let depth = state.history.undo_depth();
            tools::press(&mut state, DesignTool::DirectSelect, start);
            for delta in [
                Point::new(100.0, 80.0),
                Point::new(200.0, -90.0),
                Point::ZERO,
            ] {
                tools::drag(&mut state, start + delta);
            }
            assert!(
                !tools::release(&mut state),
                "returning to start records nothing"
            );
            assert_eq!(state.document, before);
            tools::press(&mut state, DesignTool::DirectSelect, start);
            tools::drag(&mut state, start + Point::new(120.0, -150.0));
            assert!(tools::release(&mut state));
            assert_eq!(state.history.undo_depth(), depth + 1);
            let after = state.document.clone();
            state.history.undo(&mut state.document);
            assert_eq!(state.document, before);
            state.history.redo(&mut state.document);
            assert_eq!(state.document, after);
        }
    }
}
