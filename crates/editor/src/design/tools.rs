//! The Design Mode tools: what a click does under each one.
//!
//! Kept apart from the pointer handling, which only knows about presses
//! and moves. A tool is a *rule* about what a gesture means, and rules are
//! worth testing on their own: "a drag with the text tool makes exactly
//! one frame and one undo step" is a statement about behaviour, not about
//! event plumbing, and it is worth a test that says so.
//!
//! Everything a tool does goes through
//! [`schist_layout::authoring`], so every frame a user creates and every
//! character they type is undoable through the same stack as every move.

use schist_layout::authoring;
use schist_layout::{ObjectId, Rect};

use super::{Anchor, DesignState, DesignTool, Drawing, Typing};

/// Begin a gesture with `tool` at a **page-space** point.
///
/// Every tool works in page space, because that is the space a document
/// is written in. The pasteboard's own scale and origin are the view's
/// business and are undone by the caller, so a zoom change does not move
/// a frame.
pub fn press(state: &mut DesignState, tool: DesignTool, at: schist_layout::Point) {
    if state.tool != tool {
        state.cancel_gesture();
    }
    state.tool = tool;
    if tool == DesignTool::TextFrame && super::text::press(state, at) {
        return;
    }
    match tool {
        DesignTool::Pen => super::pen::press(state, at),
        DesignTool::Hand | DesignTool::Zoom => {}
        DesignTool::Eyedropper => {
            sample_paint(state, at);
        }
        // A drawing tool makes something when the drag ends, so the
        // gesture is remembered rather than acted on now.
        tool if tool.draws() => {
            state.drag = None;
            state.drawing = Some(Drawing { from: at, to: at });
            state.selection.clear();
            // A frame is about to be made, so a caret on the old one is
            // stale the moment this one appears.
            state.typing = None;
        }
        // A delete acts on the click, not on a drag: dragging a delete
        // tool across a page would otherwise remove everything it
        // crossed.
        tool if tool.removes() => {
            state.drawing = None;
            if let Some(object) = hit(state, at).object() {
                delete(state, object);
            }
        }
        // A direct selection grabs the shape's own points if the click
        // lands near one, and the whole shape otherwise. Both are useful
        // and they cannot both be the default, so the point wins when it is
        // close enough to be deliberate.
        DesignTool::DirectSelect => {
            state.drawing = None;
            state.typing = None;
            state.anchor = None;
            if let Some((object, point)) = anchor_at(state, at) {
                begin_anchor(state, object, point, at);
            } else {
                state.selection = hit(state, at).object().into_iter().collect();
            }
        }
        // A selected object moves the whole selection. Empty space starts a band.
        _ => {
            state.drawing = None;
            state.typing = None;
            state.anchor = None;
            if let Some(id) = hit(state, at).object() {
                super::dragging::begin(state, id, at);
            } else {
                state.selection.clear();
                state.band = Some(Drawing { from: at, to: at });
            }
        }
    }
}

/// Continue a gesture.
pub fn drag(state: &mut DesignState, at: schist_layout::Point) {
    if let Some(band) = state.band.as_mut() {
        band.to = at;
        let drawing = *band;
        if let Some(plan) = state.plan() {
            state.selection = super::select::within(
                &plan,
                Rect::from_corners(
                    state.to_pasteboard(drawing.from),
                    state.to_pasteboard(drawing.to),
                ),
            )
            .into_iter()
            .filter(|id| state.document.object(*id).is_some())
            .collect();
        }
        return;
    }
    if state.text_selecting {
        super::text::drag(state, at);
        return;
    }
    if state.pen.as_ref().is_some_and(|p| p.draft.is_some()) {
        super::pen::drag(state, at);
        return;
    }
    if let Some(mut drawing) = state.drawing {
        let at = super::guides::snap_delta(
            state,
            state.current_page(),
            Rect::new(0.0, 0.0, 0.0, 0.0),
            at,
        );
        drawing.to = at;
        state.drawing = Some(drawing);
        return;
    }
    if state.anchor.is_some() {
        drag_anchor(state, at);
        return;
    }
    super::dragging::drag_to(state, at);
}

/// The anchor point near a point, if any.
pub fn anchor_at(
    state: &DesignState,
    at: schist_layout::Point,
) -> Option<(ObjectId, schist_layout::authoring::PointRef)> {
    let on_pasteboard = state.to_pasteboard(at);
    state.plan().and_then(|plan| {
        crate::design::select::hit_anchor(&plan, on_pasteboard, ANCHOR_GRAB, &state.selection)
    })
}

/// Begin dragging one of a shape's own points.
fn begin_anchor(
    state: &mut DesignState,
    object: ObjectId,
    point: schist_layout::authoring::PointRef,
    at: schist_layout::Point,
) {
    let Some(placed) = state.document.object(object) else {
        return;
    };
    if state.document.object_locked(placed.id) {
        // A locked shape shows its points but will not move them, the same
        // way it will not move.
        return;
    }
    state.anchor = Some(Anchor {
        object,
        at: point,
        from: at,
        to: at,
        before: schist_layout::edit::snapshot_object(placed),
    });
    state.selection = vec![object];
}

/// Move the dragged point, live, with nothing recorded yet.
fn drag_anchor(state: &mut DesignState, at: schist_layout::Point) {
    let Some(anchor) = &mut state.anchor else {
        return;
    };
    anchor.to = at;
    let (object, point) = (anchor.object, anchor.at);
    // Restore the starting geometry before each update. Returning to the
    // starting pointer restores exact bytes, even after bounds rebasing.
    let edit = schist_layout::LayoutEdit::ObjectChanged {
        id: object.0,
        before: anchor.before.clone(),
        after: anchor.before.clone(),
    };
    schist_layout::edit::reverse(&mut state.document, &edit);
    let delta = at - anchor.from;
    if delta == schist_layout::Point::ZERO {
        return;
    }
    let Some(placed) = state.document.object(object) else {
        return;
    };
    let schist_layout::LayoutObject::Shape { path, .. } = &placed.object else {
        return;
    };
    let Some(sub) = path.subpaths.get(point.subpath) else {
        return;
    };
    let Some(anchor_point) = sub.points.get(point.index) else {
        return;
    };
    let handles = sub.handles_at(point.index);
    let start = match point.part {
        authoring::PointPart::Anchor => *anchor_point,
        authoring::PointPart::Incoming => handles.incoming.unwrap_or(*anchor_point),
        authoring::PointPart::Outgoing => handles.outgoing.unwrap_or(*anchor_point),
    } + placed.bounds.origin();
    let start = schist_layout::affine::point(placed.content_transform(), start);
    authoring::set_point(&mut state.document, object, point, start + delta);
}

/// Finish an anchor drag, recording one undo entry.
pub fn end_anchor(state: &mut DesignState) -> bool {
    let Some(anchor) = state.anchor.take() else {
        return false;
    };
    let Some(placed) = state.document.object(anchor.object) else {
        return false;
    };
    let after = schist_layout::edit::snapshot_object(placed);
    if anchor.before == after {
        // A point dragged back where it started is a click, and a click
        // must not leave a step on the stack.
        return false;
    }
    state
        .history
        .record(schist_layout::LayoutEdit::ObjectChanged {
            id: anchor.object.0,
            before: anchor.before,
            after,
        });
    true
}

/// How close a click has to be to an anchor to grab it, in page points.
///
/// Wide enough to be forgiving at the zoom a page is usually worked at,
/// narrow enough that clicking in the middle of a small shape grabs the
/// shape rather than a corner of it.
const ANCHOR_GRAB: f32 = 6.0;

/// Finish a gesture, doing whatever it was for.
pub fn release(state: &mut DesignState) -> bool {
    if state.band.take().is_some() {
        return false;
    }
    if state.text_selecting {
        state.text_selecting = false;
        return false;
    }
    if state.pen.as_ref().is_some_and(|p| p.draft.is_some()) {
        return super::pen::release(state);
    }
    if let Some(drawing) = state.drawing.take() {
        return create(state, drawing);
    }
    if state.anchor.is_some() {
        return end_anchor(state);
    }
    super::dragging::end(state)
}

/// Draw the box a drawing gesture has made, for a caller that shows it
/// while the pointer is still down.
pub fn drawing_bounds(state: &DesignState) -> Option<Rect> {
    state.drawing.map(|drawing| drawing.bounds())
}

/// Create whatever the tool was set to make.
fn create(state: &mut DesignState, drawing: Drawing) -> bool {
    let bounds = drawing.bounds();
    let page = state.current_page();
    let (document, history) = (&mut state.document, &mut state.history);
    let made = match state.tool {
        DesignTool::TextFrame => {
            // A click rather than a drag still makes a frame, sized so it
            // can be selected: a tool that does nothing on a click looks
            // broken.
            authoring::text_frame(document, history, page, bounds).map(|frame| {
                // Typing starts immediately, because the user drew a box
                // to put words in.
                state.typing = Some(Typing {
                    object: frame.object,
                    story: frame.story,
                    at: 0,
                    anchor: 0,
                });
                state.text_buffer.clear();
                state.selection = vec![frame.object];
                frame.object
            })
        }
        DesignTool::Line => {
            if drawing.from == drawing.to {
                return false;
            }
            let path = schist_layout::ShapePath {
                subpaths: vec![schist_layout::SubPath {
                    points: vec![drawing.from, drawing.to],
                    handles: Vec::new(),
                    closed: false,
                }],
                even_odd: false,
            };
            authoring::path_shape(
                document,
                history,
                page,
                path,
                authoring::Paint::stroked(schist_layout::Ink::black().name, 1.0),
            )
        }
        // Every shape tool is the same gesture and one call to the same
        // function, differing only in the kind. A second implementation
        // per shape would be a second place the "one edit" rule could go
        // wrong.
        tool => match tool.shape_kind() {
            Some(kind) => authoring::shape(
                document,
                history,
                page,
                bounds,
                kind,
                authoring::Paint::none(),
            ),
            None => None,
        },
    };
    match made {
        Some(object) => {
            state.selection = vec![object];
            true
        }
        None => false,
    }
}

/// Delete an object, if it can be.
pub fn delete(state: &mut DesignState, object: ObjectId) -> bool {
    state.anchor = None;
    if !authoring::delete(&mut state.document, &mut state.history, object) {
        return false;
    }
    // A selection that includes what went has to go with it, or the
    // pasteboard would outline something that is not there.
    state.selection.retain(|selected| *selected != object);
    if state.typing.map(|typing| typing.object) == Some(object) {
        state.typing = None;
    }
    true
}

/// Type a character into the frame being edited, or start editing a frame.
///
/// Each call records one edit through [`authoring::set_text`]. Keystrokes
/// are not coalesced here; undo follows the individual calls.
pub fn type_text(state: &mut DesignState, text: &str) -> bool {
    let Some(typing) = state.typing else {
        return false;
    };
    replace_selection(
        state,
        typing.at.min(typing.anchor)..typing.at.max(typing.anchor),
        text,
    )
}

pub fn replace_selection(
    state: &mut DesignState,
    range: std::ops::Range<usize>,
    text: &str,
) -> bool {
    let Some(typing) = state.typing else {
        return false;
    };
    if state.document.object_locked(typing.object) {
        return false;
    }
    let current = authoring::text_of(&state.document, typing.story);
    let start = floor_boundary(&current, range.start.min(current.len()));
    if !authoring::replace_text(
        &mut state.document,
        &mut state.history,
        typing.story,
        range,
        text,
    ) {
        return false;
    }
    state.text_buffer = authoring::text_of(&state.document, typing.story);
    state.typing = Some(Typing {
        at: start + text.len(),
        anchor: start + text.len(),
        ..typing
    });
    true
}

pub fn backspace(state: &mut DesignState) -> bool {
    let Some(typing) = state.typing else {
        return false;
    };
    if typing.at != typing.anchor {
        return type_text(state, "");
    }
    let current = authoring::text_of(&state.document, typing.story);
    let at = floor_boundary(&current, typing.at.min(current.len()));
    if at == 0 {
        return false;
    }
    replace_selection(state, floor_boundary(&current, at - 1)..at, "")
}

pub fn delete_forward(state: &mut DesignState) -> bool {
    let Some(typing) = state.typing else {
        return false;
    };
    if typing.at != typing.anchor {
        return type_text(state, "");
    }
    let current = authoring::text_of(&state.document, typing.story);
    let at = floor_boundary(&current, typing.at.min(current.len()));
    let next = current[at..]
        .chars()
        .next()
        .map_or(at, |c| at + c.len_utf8());
    replace_selection(state, at..next, "")
}

pub fn move_caret(state: &mut DesignState, at: usize) {
    select_to(state, at, false);
}

pub fn select_to(state: &mut DesignState, at: usize, extend: bool) {
    let Some(typing) = state.typing else {
        return;
    };
    state.text_buffer = authoring::text_of(&state.document, typing.story);
    let at = floor_boundary(&state.text_buffer, at.min(state.text_buffer.len()));
    state.typing = Some(Typing {
        at,
        anchor: if extend { typing.anchor } else { at },
        ..typing
    });
}

pub fn begin_typing(state: &mut DesignState, object: ObjectId, at: usize) -> bool {
    let Some(story) = schist_layout::threading::story_of(&state.document, object) else {
        return false;
    };
    if state.document.object_locked(object) {
        return false;
    }
    state.typing = Some(Typing {
        object,
        story,
        at: 0,
        anchor: 0,
    });
    state.selection = vec![object];
    move_caret(state, at);
    true
}

/// The text of the frame being typed into.
pub fn typing_text(state: &DesignState) -> Option<String> {
    state
        .typing
        .map(|typing| authoring::text_of(&state.document, typing.story))
}

/// Stop editing, leaving the frame as it is.
pub fn stop_typing(state: &mut DesignState) {
    state.typing = None;
}

/// The nearest character boundary at or before `at`.
fn floor_boundary(text: &str, at: usize) -> usize {
    if at >= text.len() {
        return text.len();
    }
    let mut at = at;
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// What is under a point, for the tools that act on a click.
///
/// The point arrives in page space, because that is the space a tool
/// works in, and goes out in pasteboard space, because that is the space
/// the plan is drawn in: the plan was scaled and given a margin when it
/// was built, so hit testing has to go the other way. Skipping the
/// conversion looks fine at 100% zoom on a document with no margin and
/// silently selects the wrong frame at any other zoom.
///
/// `plan()` is `Option` because a document can have no pasteboard yet --
/// a layout with no pages cannot be drawn on -- and a click on that is a
/// click on nothing, which is not an error.
fn hit(state: &DesignState, at: schist_layout::Point) -> super::select::Hit {
    let on_pasteboard = state.to_pasteboard(at);
    state
        .plan()
        .map(|plan| super::select::hit_test(&plan, on_pasteboard))
        .filter(|hit| {
            !matches!(
                hit,
                super::select::Hit::Object {
                    inherited: true,
                    ..
                }
            )
        })
        .unwrap_or(super::select::Hit::Nothing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> DesignState {
        let mut state = DesignState::new();
        // A page big enough to draw on.
        state.document.pages[0].width = 600.0;
        state.document.pages[0].height = 800.0;
        state
    }

    fn at(x: f32, y: f32) -> schist_layout::Point {
        schist_layout::Point { x, y }
    }

    #[test]
    fn a_click_with_the_text_tool_makes_a_frame_you_can_type_into() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(50.0, 60.0));
        release(&mut state);
        assert_eq!(
            state.document.objects.len(),
            1,
            "a click still makes a frame"
        );
        let object = state.document.objects[0].id;
        assert!(matches!(
            state.document.object(object).map(|o| &o.object),
            Some(schist_layout::LayoutObject::TextFrame { .. })
        ));
        // And it is already being edited, because the user drew a box to
        // put words in.
        assert!(state.typing.is_some());
        assert!(type_text(&mut state, "Hi"), "typing goes somewhere");
        assert_eq!(typing_text(&state).as_deref(), Some("Hi"));
    }

    #[test]
    fn a_drag_with_the_text_tool_makes_a_frame_the_size_drawn() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(10.0, 10.0));
        drag(&mut state, at(210.0, 60.0));
        assert_eq!(drawing_bounds(&state).map(|b| b.width), Some(200.0));
        release(&mut state);
        assert_eq!(state.document.objects[0].bounds.width, 200.0);
        assert_eq!(state.document.objects[0].bounds.height, 50.0);
    }

    #[test]
    fn a_drag_up_and_to_the_left_makes_a_frame_the_right_way_round() {
        // A box with a negative width cannot be drawn, so the extent is
        // taken absolute.
        let mut state = state();
        press(&mut state, DesignTool::Rectangle, at(200.0, 200.0));
        drag(&mut state, at(100.0, 150.0));
        release(&mut state);
        let bounds = state.document.objects[0].bounds;
        assert_eq!(bounds.x, 100.0);
        assert_eq!(bounds.y, 150.0);
        assert!(bounds.width > 0.0 && bounds.height > 0.0);
    }

    #[test]
    fn the_rectangle_tool_makes_a_shape() {
        let mut state = state();
        press(&mut state, DesignTool::Rectangle, at(0.0, 0.0));
        drag(&mut state, at(100.0, 50.0));
        release(&mut state);
        assert!(matches!(
            state.document.objects[0].object,
            schist_layout::LayoutObject::Shape { .. }
        ));
    }

    #[test]
    fn making_a_frame_is_one_undo_step() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        drag(&mut state, at(100.0, 20.0));
        release(&mut state);
        let depth = state.history.undo_depth();
        assert_eq!(depth, 1);
        state.history.undo(&mut state.document);
        assert!(
            state.document.objects.is_empty(),
            "one undo undid the frame"
        );
    }

    #[test]
    fn an_abandoned_drawing_leaves_nothing_behind() {
        // A drawing drag is not yet an object, so letting go of the
        // pointer outside the page must not leave a zero-size frame.
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(10.0, 10.0));
        drag(&mut state, at(400.0, 400.0));
        state.drawing = None;
        assert!(state.document.objects.is_empty());
    }

    #[test]
    fn typing_is_undoable_as_a_run() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        release(&mut state);
        let before = state.history.undo_depth();
        for character in "Hello".chars() {
            type_text(&mut state, &character.to_string());
        }
        assert_eq!(typing_text(&state).as_deref(), Some("Hello"));
        // Five keystrokes, five steps. Each call is one edit; the run is
        // coalesced by a caller that types a word at a time.
        assert_eq!(state.history.undo_depth(), before + 5);
        state.history.undo(&mut state.document);
        assert_eq!(typing_text(&state).as_deref(), Some("Hell"));
    }

    #[test]
    fn backspace_removes_a_whole_character() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        release(&mut state);
        type_text(&mut state, "héllo");
        assert_eq!(typing_text(&state).as_deref(), Some("héllo"));
        assert!(backspace(&mut state));
        assert_eq!(
            typing_text(&state).as_deref(),
            Some("héll"),
            "not a byte, or the é is mangled"
        );
    }

    #[test]
    fn a_caret_inside_a_character_is_moved_to_its_start() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        release(&mut state);
        type_text(&mut state, "é");
        // The é is two bytes, so offset 1 is inside it.
        move_caret(&mut state, 1);
        type_text(&mut state, "x");
        assert_eq!(
            typing_text(&state).as_deref(),
            Some("xé"),
            "the text stayed encodable"
        );
    }

    #[test]
    fn the_delete_tool_removes_what_was_clicked() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(10.0, 10.0));
        drag(&mut state, at(110.0, 30.0));
        release(&mut state);
        let object = state.document.objects[0].id;
        assert_eq!(state.selection, vec![object]);

        press(&mut state, DesignTool::Delete, at(50.0, 20.0));
        assert!(state.document.objects.is_empty(), "the frame is gone");
        // And the selection does not keep pointing at it.
        assert!(state.selection.is_empty());
    }

    #[test]
    fn a_delete_ignores_a_locked_frame() {
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(10.0, 10.0));
        drag(&mut state, at(110.0, 30.0));
        release(&mut state);
        if let Some(placed) = state.document.objects.first_mut() {
            placed.locked = true;
        }
        press(&mut state, DesignTool::Delete, at(50.0, 20.0));
        assert_eq!(state.document.objects.len(), 1, "a locked frame stays");
    }

    #[test]
    fn a_tool_acts_on_what_is_drawn_where_it_looks() {
        // The plan is scaled and margined; a tool works in page space. If
        // the two do not line up, every tool is subtly wrong and nothing
        // fails loudly, so pin the round trip here.
        let mut state = state();
        press(&mut state, DesignTool::Rectangle, at(0.0, 0.0));
        drag(&mut state, at(100.0, 50.0));
        release(&mut state);
        let view = state.view_for_mode();
        for at in [at(0.0, 0.0), at(50.0, 25.0), at(100.0, 50.0)] {
            assert_eq!(view.to_page(view.to_pasteboard(at)), at, "round trips");
        }
    }

    #[test]
    fn the_select_tool_picks_the_frame_under_the_pointer() {
        let mut state = state();
        press(&mut state, DesignTool::Rectangle, at(0.0, 0.0));
        drag(&mut state, at(100.0, 50.0));
        release(&mut state);
        let object = state.document.objects[0].id;

        state.selection.clear();
        press(&mut state, DesignTool::Select, at(50.0, 25.0));
        assert_eq!(
            state.selection,
            vec![object],
            "a click selects what is there"
        );
        press(&mut state, DesignTool::Select, at(500.0, 700.0));
        assert!(
            state.selection.is_empty(),
            "a click on bare page selects nothing"
        );
    }

    #[test]
    fn the_select_tool_clears_a_frame_being_typed_into() {
        // Clicking elsewhere means the user is done with this frame, and a
        // caret left behind on a frame nobody is editing is a lie.
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        drag(&mut state, at(100.0, 40.0));
        release(&mut state);
        assert!(state.typing.is_some());
        press(&mut state, DesignTool::Rectangle, at(0.0, 0.0));
        assert!(state.typing.is_none());
    }

    #[test]
    fn every_shape_tool_makes_the_kind_it_is_named_for() {
        // The tool and the shape it draws are one decision, not two that
        // can drift apart.
        for (tool, name) in [
            (DesignTool::Rectangle, "Rectangle"),
            (DesignTool::Ellipse, "Ellipse"),
            (DesignTool::Line, "Line"),
            (DesignTool::Polygon, "Polygon"),
        ] {
            let mut state = state();
            press(&mut state, tool, at(0.0, 0.0));
            drag(&mut state, at(100.0, 60.0));
            assert!(release(&mut state), "{name} is made");
            let placed = &state.document.objects[0];
            assert_eq!(placed.name, name);
            assert!(
                matches!(placed.object, schist_layout::LayoutObject::Shape { .. }),
                "{name} is a shape"
            );
        }
    }

    #[test]
    fn a_line_is_clicked_where_its_ink_is() {
        // The frame is the line's bounding box and the path is the
        // diagonal, so the whole box is clickable. A hit test that used
        // the path alone would make a thin diagonal very hard to catch.
        let mut state = state();
        press(&mut state, DesignTool::Line, at(0.0, 0.0));
        drag(&mut state, at(200.0, 100.0));
        release(&mut state);
        let object = state.document.objects[0].id;
        state.selection.clear();
        press(&mut state, DesignTool::Select, at(150.0, 10.0));
        assert_eq!(state.selection, vec![object], "inside the line's box");
    }

    #[test]
    fn a_vertical_drag_makes_a_vertical_line() {
        // The degenerate case: width zero. A line here has to be a
        // vertical one, not a line of no length.
        let mut state = state();
        press(&mut state, DesignTool::Line, at(100.0, 0.0));
        drag(&mut state, at(100.0, 200.0));
        release(&mut state);
        let placed = &state.document.objects[0];
        assert!(placed.bounds.height > 0.0, "the line has length");
        assert_eq!(
            placed.bounds.width, 0.0,
            "hit tolerance must not change geometry"
        );
        let id = placed.id;
        press(&mut state, DesignTool::Select, at(102.0, 100.0));
        assert_eq!(state.selection, vec![id], "a thin line is still clickable");
    }

    /// A rectangle on a page big enough to work on.
    fn rectangle_state() -> DesignState {
        let mut state = state();
        press(&mut state, DesignTool::Rectangle, at(0.0, 0.0));
        drag(&mut state, at(200.0, 100.0));
        release(&mut state);
        state
    }

    #[test]
    fn the_direct_selection_tool_grabs_a_shape_own_corner() {
        let mut state = rectangle_state();
        let object = state.document.objects[0].id;
        state.selection.clear();
        // Just inside the top-left corner, within grabbing distance.
        press(&mut state, DesignTool::DirectSelect, at(1.0, 1.0));
        assert!(state.anchor.is_some(), "an anchor is being dragged");
        assert_eq!(state.selection, vec![object], "and its shape is selected");
    }

    #[test]
    fn the_direct_selection_tool_grabs_the_whole_shape_when_the_click_is_not_on_a_point() {
        // Both are useful and only one can be the default. A click in the
        // middle of a shape is about the shape.
        let mut state = rectangle_state();
        let object = state.document.objects[0].id;
        state.selection.clear();
        press(&mut state, DesignTool::DirectSelect, at(100.0, 50.0));
        assert!(state.anchor.is_none());
        assert_eq!(state.selection, vec![object]);
    }

    #[test]
    fn dragging_a_corner_changes_the_shape_and_is_one_undo_step() {
        let mut state = rectangle_state();
        let before = state.document.clone();
        let depth = state.history.undo_depth();
        press(&mut state, DesignTool::DirectSelect, at(200.0, 100.0));
        drag(&mut state, at(260.0, 40.0));
        assert!(release(&mut state));
        assert_eq!(state.history.undo_depth(), depth + 1);
        assert_ne!(state.document, before, "the shape changed");
        state.history.undo(&mut state.document);
        assert_eq!(state.document, before, "one press put it back");
    }

    #[test]
    fn a_corner_dragged_back_where_it_was_records_nothing() {
        let mut state = rectangle_state();
        let before = state.document.clone();
        let depth = state.history.undo_depth();
        press(&mut state, DesignTool::DirectSelect, at(0.0, 0.0));
        drag(&mut state, at(60.0, 60.0));
        drag(&mut state, at(0.0, 0.0));
        assert!(!release(&mut state), "nothing moved");
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), depth);
    }

    #[test]
    fn a_locked_shape_shows_its_points_but_will_not_move_them() {
        let mut state = rectangle_state();
        // The shape is selected but locked, so its points are still
        // offered and still refuse to move.
        state.document.objects[0].locked = true;
        let before = state.document.clone();
        press(&mut state, DesignTool::DirectSelect, at(0.0, 0.0));
        assert!(
            state.anchor.is_none(),
            "the drag never started, so nothing snaps back on release"
        );
        drag(&mut state, at(80.0, 80.0));
        assert!(!release(&mut state));
        assert_eq!(state.document, before);
    }

    #[test]
    fn the_anchors_a_shape_offers_can_be_listed() {
        // What a user sees and drags, they can also count on.
        let state = rectangle_state();
        let found = anchor_at(&state, at(0.0, 0.0)).expect("a corner is there");
        assert_eq!(found.0, state.document.objects[0].id);
        assert_eq!(
            found.1,
            schist_layout::authoring::PointRef {
                subpath: 0,
                index: 0,
                part: authoring::PointPart::Anchor
            }
        );
        assert!(
            anchor_at(&state, at(300.0, 300.0)).is_none(),
            "far from any corner there is nothing to grab"
        );
    }

    #[test]
    fn a_text_frame_offers_no_anchors() {
        // A frame is a box, not an outline, so there is nothing to drag.
        let mut state = state();
        press(&mut state, DesignTool::TextFrame, at(0.0, 0.0));
        drag(&mut state, at(200.0, 100.0));
        release(&mut state);
        assert!(anchor_at(&state, at(0.0, 0.0)).is_none());
    }

    #[test]
    fn typing_with_no_frame_being_edited_goes_nowhere() {
        let mut state = state();
        assert!(!type_text(&mut state, "x"));
        assert!(!backspace(&mut state));
        assert!(state.history.undo_depth() == 0);
    }
}

/// Transfer the source shape's actual paint, including opacity and overprint.
/// Sampling itself does not select or edit the source. All targets form one edit.
pub fn sample_paint(state: &mut DesignState, at: schist_layout::Point) -> bool {
    let Some(source) = hit(state, at)
        .object()
        .and_then(|id| state.document.object(id))
    else {
        return false;
    };
    let schist_layout::LayoutObject::Shape {
        fill,
        stroke,
        stroke_width,
        fill_overprint,
        stroke_overprint,
        tints,
        ..
    } = &source.object
    else {
        return false;
    };
    let mut edits = Vec::new();
    for id in &state.selection {
        if *id == source.id || state.document.object_locked(*id) {
            continue;
        }
        let Some(target) = state.document.object(*id) else {
            continue;
        };
        let mut after = target.clone();
        let schist_layout::LayoutObject::Shape {
            fill: f,
            stroke: s,
            stroke_width: w,
            fill_overprint: fo,
            stroke_overprint: so,
            tints: target_tints,
            ..
        } = &mut after.object
        else {
            continue;
        };
        f.clone_from(fill);
        s.clone_from(stroke);
        *w = *stroke_width;
        *fo = *fill_overprint;
        *so = *stroke_overprint;
        *target_tints = *tints;
        after.transparency = source.transparency;
        after.overprint = source.overprint;
        if after != *target {
            edits.push(schist_layout::LayoutEdit::ObjectChanged {
                id: id.0,
                before: schist_layout::edit::snapshot_object(target),
                after: schist_layout::edit::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty()
        && state.history.apply(
            &mut state.document,
            schist_layout::LayoutEdit::Batch { edits },
        )
}

#[cfg(test)]
mod selection_gesture_tests {
    use super::*;
    use schist_layout::{LayoutObject, Point};
    #[test]
    fn moving_any_selection_size_is_one_reversible_gesture() {
        for count in 1..9 {
            for updates in [1, 17] {
                let mut state = DesignState::new();
                for index in 0..count {
                    authoring::rectangle(
                        &mut state.document,
                        &mut state.history,
                        0,
                        Rect::new(index as f32 * 30.0, 20.0, 20.0, 20.0),
                        authoring::Paint::none(),
                    )
                    .unwrap();
                }
                state.selection = state.document.objects.iter().map(|o| o.id).collect();
                let before = state.document.clone();
                let depth = state.history.undo_depth();
                press(&mut state, DesignTool::Select, Point::new(10.0, 30.0));
                for index in 1..=updates {
                    drag(
                        &mut state,
                        Point::new(10.0 + 50.0 * index as f32 / updates as f32, 45.0),
                    );
                }
                assert!(release(&mut state));
                assert_eq!(state.history.undo_depth(), depth + 1);
                for (a, b) in before.objects.iter().zip(&state.document.objects) {
                    assert_eq!(b.bounds, a.bounds.translated(Point::new(50.0, 15.0)));
                }
                state.history.undo(&mut state.document);
                assert_eq!(state.document, before);
            }
        }
    }
    #[test]
    fn a_band_selects_all_visible_frames_and_records_no_edit() {
        let mut state = DesignState::new();
        authoring::text_frame(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(50.0, 50.0, 50.0, 50.0),
        )
        .unwrap();
        authoring::rectangle(
            &mut state.document,
            &mut state.history,
            0,
            Rect::new(150.0, 50.0, 50.0, 50.0),
            authoring::Paint::none(),
        )
        .unwrap();
        let before = state.document.clone();
        let depth = state.history.undo_depth();
        press(&mut state, DesignTool::Select, Point::new(10.0, 10.0));
        drag(&mut state, Point::new(220.0, 110.0));
        release(&mut state);
        assert_eq!(state.selection.len(), 2);
        assert_eq!(state.document, before);
        assert_eq!(state.history.undo_depth(), depth);
    }
    #[test]
    fn every_line_direction_keeps_the_exact_gesture_endpoints() {
        for from in [Point::new(10.0, 20.0), Point::new(100.0, 200.0)] {
            for delta in [
                Point::new(40.0, 0.0),
                Point::new(0.0, -30.0),
                Point::new(-20.0, 50.0),
                Point::new(10.0, -40.0),
            ] {
                let mut state = DesignState::new();
                let before = state.document.clone();
                press(&mut state, DesignTool::Line, from);
                drag(&mut state, from + delta);
                assert!(release(&mut state));
                let object = &state.document.objects[0];
                let LayoutObject::Shape { path, .. } = &object.object else {
                    panic!()
                };
                assert_eq!(
                    path.subpaths[0]
                        .points
                        .iter()
                        .map(|p| *p + object.bounds.origin())
                        .collect::<Vec<_>>(),
                    vec![from, from + delta]
                );
                state.history.undo(&mut state.document);
                assert_eq!(state.document, before);
            }
        }
    }
    #[test]
    fn eyedropper_transfers_paint_in_one_edit_without_changing_geometry() {
        for count in 1..8 {
            let mut state = DesignState::new();
            let ink = schist_layout::Ink::black().name;
            authoring::rectangle(
                &mut state.document,
                &mut state.history,
                0,
                Rect::new(10.0, 10.0, 20.0, 20.0),
                authoring::Paint::filled(&ink),
            )
            .unwrap();
            for i in 0..count {
                let id = authoring::rectangle(
                    &mut state.document,
                    &mut state.history,
                    0,
                    Rect::new(50.0 + i as f32 * 30.0, 10.0, 20.0, 20.0),
                    authoring::Paint::none(),
                )
                .unwrap();
                state.selection.push(id);
            }
            let expected_tints = schist_layout::PaintTints {
                fill: count as f32 / 8.0,
                stroke: 0.25,
            };
            let LayoutObject::Shape { tints, .. } = &mut state.document.objects[0].object else {
                panic!()
            };
            *tints = expected_tints;
            let before = state.document.clone();
            let depth = state.history.undo_depth();
            assert!(sample_paint(&mut state, Point::new(20.0, 20.0)));
            assert_eq!(state.history.undo_depth(), depth + 1);
            for (a, b) in before.objects.iter().zip(&state.document.objects) {
                assert_eq!(a.bounds, b.bounds);
                let LayoutObject::Shape { tints, .. } = &b.object else {
                    panic!()
                };
                assert_eq!(*tints, expected_tints);
            }
            state.history.undo(&mut state.document);
            assert_eq!(state.document, before);
        }
    }
}
