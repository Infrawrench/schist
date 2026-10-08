//! Text inside closed shapes. A shaped text frame keeps its outline normalized
//! to its own box in `ObjectAppearance::outline`; composition keeps its lines
//! inside that outline, inset by the frame's top inset (see `text_wrap`).
use crate::{
    authoring::TextFrame, FrameOverflow, History, Insets, LayoutDocument, LayoutEdit, LayoutObject,
    ObjectId, Point, ShapePath, StoryId,
};

/// Whether a frame-relative path is exactly its `width` × `height` box.
pub fn is_box(path: &ShapePath, width: f32, height: f32) -> bool {
    let [sub] = path.subpaths.as_slice() else {
        return false;
    };
    if !sub.closed || sub.points.len() != 4 {
        return false;
    }
    let near = |a: f32, b: f32| (a - b).abs() < 0.001;
    let straight = (0..4).all(|i| {
        let point = sub.points[i];
        let handles = sub.handles_at(i);
        handles.incoming.is_none_or(|h| h == point) && handles.outgoing.is_none_or(|h| h == point)
    });
    // Every edge is axis-aligned and every corner of the box is visited.
    let edges = (0..4).all(|i| {
        let (a, b) = (sub.points[i], sub.points[(i + 1) % 4]);
        near(a.x, b.x) != near(a.y, b.y)
    });
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, height),
        Point::new(0.0, height),
    ]
    .iter()
    .all(|c| sub.points.iter().any(|p| near(p.x, c.x) && near(p.y, c.y)));
    straight && edges && corners
}

/// Turn a closed shape into a text frame that composes inside its outline, as
/// one undo step. The shape's paint stays on the frame; open subpaths are not
/// part of the text area. Open, empty, degenerate or locked shapes are refused.
pub fn attach(doc: &mut LayoutDocument, history: &mut History, id: ObjectId) -> Option<TextFrame> {
    if doc.object_locked(id) {
        return None;
    }
    let original = doc.object(id)?;
    let LayoutObject::Shape { path, .. } = &original.object else {
        return None;
    };
    let bounds = original.bounds;
    if !crate::authoring::path_can_be_filled(path)
        || !path.is_finite()
        || !(bounds.width > 0.0 && bounds.height > 0.0)
        || !(bounds.width.is_finite() && bounds.height.is_finite())
    {
        return None;
    }
    let mut outline = path.clone();
    outline
        .subpaths
        .retain(|sub| sub.closed && sub.points.len() >= 2);
    let shaped = !is_box(&outline, bounds.width, bounds.height);
    outline.map_points(|p| Point::new(p.x / bounds.width, p.y / bounds.height));
    let story = StoryId(doc.stories.len() as u32);
    let mut changed = original.clone();
    changed.appearance.paint = original.appearance.paint.over(&original.legacy_paint());
    changed.appearance.outline = shaped.then_some(outline);
    changed.object = LayoutObject::TextFrame {
        balance_columns: Some(doc.balance_columns_default),
        footnotes: Default::default(),
        story,
        text_path: None,
        columns: 1,
        gutter: 0.0,
        insets: Insets::ZERO,
        overflow: FrameOverflow::Clip,
    };
    let edits = vec![
        LayoutEdit::AddedStory {
            id: story.0,
            story: crate::edit::snapshot_story(&crate::Story::new()),
        },
        LayoutEdit::ObjectChanged {
            id: id.0,
            before: crate::edit::snapshot_object(original),
            after: crate::edit::snapshot_object(&changed),
        },
    ];
    history
        .apply(doc, LayoutEdit::Batch { edits })
        .then_some(TextFrame { object: id, story })
}
