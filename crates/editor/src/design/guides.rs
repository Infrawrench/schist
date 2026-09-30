//! Page guide gestures keep a draft; release records one page edit.
use super::DesignState;
use schist_layout::{edit::snapshot_page, geometry::RulerGuide, LayoutEdit, Page, Point, Rect};

pub struct GuideDrag {
    pub page: usize,
    pub index: Option<usize>,
    pub before: Page,
    pub guide: RulerGuide,
}

pub fn begin(state: &mut DesignState, horizontal: bool, at: Point) {
    state.cancel_gesture();
    let page = state.current_page();
    let Some(before) = state.document.pages.get(page).cloned() else {
        return;
    };
    state.guide_drag = Some(GuideDrag {
        page,
        index: None,
        before,
        guide: RulerGuide {
            horizontal,
            position: if horizontal { at.y } else { at.x },
            locked: false,
        },
    });
}
pub fn hit(state: &mut DesignState, at: Point) -> bool {
    if !state.show_guides {
        return false;
    }
    let page = state.current_page();
    let Some(before) = state.document.pages.get(page) else {
        return false;
    };
    let Some((index, guide)) = before
        .guides
        .iter()
        .enumerate()
        .filter(|(_, g)| !g.locked)
        .filter(|(_, g)| {
            ((if g.horizontal { at.y } else { at.x }) - g.position).abs() * state.view.scale <= 5.0
        })
        .min_by(|(_, a), (_, b)| {
            ((if a.horizontal { at.y } else { at.x }) - a.position)
                .abs()
                .total_cmp(&((if b.horizontal { at.y } else { at.x }) - b.position).abs())
        })
    else {
        return false;
    };
    state.guide_drag = Some(GuideDrag {
        page,
        index: Some(index),
        before: before.clone(),
        guide: *guide,
    });
    true
}
pub fn drag(state: &mut DesignState, at: Point) {
    if let Some(drag) = &mut state.guide_drag {
        let position = if drag.guide.horizontal { at.y } else { at.x };
        if position.is_finite() {
            drag.guide.position = position;
        }
    }
}
pub fn finish(state: &mut DesignState, remove: bool) -> bool {
    let Some(drag) = state.guide_drag.take() else {
        return false;
    };
    if state.document.pages.get(drag.page) != Some(&drag.before) {
        return false;
    }
    let mut after = drag.before.clone();
    if let Some(index) = drag.index {
        if remove {
            after.guides.remove(index);
        } else {
            after.guides[index] = drag.guide;
        }
    } else if !remove {
        after.guides.push(drag.guide);
    }
    after != drag.before
        && state.history.apply(
            &mut state.document,
            LayoutEdit::PageChanged {
                index: drag.page,
                before: snapshot_page(&drag.before),
                after: snapshot_page(&after),
            },
        )
}

/// Snap a move without changing its dimensions or relative object positions.
pub fn snap_delta(state: &DesignState, page: usize, bounds: Rect, delta: Point) -> Point {
    if !state.snap_guides {
        return delta;
    }
    let Some(page) = state.document.pages.get(page) else {
        return delta;
    };
    let moved = bounds.translated(delta);
    let mut result = delta;
    let mut distances = [6.0 / state.view.scale; 2];
    for guide in &page.guides {
        let axis = usize::from(guide.horizontal);
        let edges = if guide.horizontal {
            [moved.y, moved.y + moved.height / 2.0, moved.bottom()]
        } else {
            [moved.x, moved.x + moved.width / 2.0, moved.right()]
        };
        for edge in edges {
            let distance = guide.position - edge;
            if distance.abs() < distances[axis] {
                distances[axis] = distance.abs();
                if guide.horizontal {
                    result.y = delta.y + distance;
                } else {
                    result.x = delta.x + distance;
                }
            }
        }
    }
    if state.document.grids.document.snap_objects && result.y == delta.y {
        result.y = state.document.grids.document.snap_y(page, moved.y) - bounds.y;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guide_drag_commit_cancel_and_delete_are_independent_of_update_count() {
        for count in [1, 5, 50] {
            for horizontal in [true, false] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                begin(&mut state, horizontal, Point::ZERO);
                for i in 1..=count {
                    let value = 123.0 * i as f32 / count as f32;
                    drag(&mut state, Point::new(value, value));
                }
                assert_eq!(state.document, before);
                assert!(finish(&mut state, false));
                let after = state.document.clone();
                assert_eq!(state.history.undo_depth(), 1);
                assert!(hit(&mut state, Point::new(123.0, 123.0)));
                assert!(finish(&mut state, true));
                state.history.undo(&mut state.document);
                assert_eq!(state.document, after);
                state.history.undo(&mut state.document);
                assert_eq!(state.document, before);
                begin(&mut state, horizontal, Point::ZERO);
                state.cancel_gesture();
                assert!(!finish(&mut state, false));
                assert_eq!(state.document, before);
            }
        }
    }
}
