//! What the pointer means on the pasteboard.
//!
//! Hit-testing lives in the editor and not in `schist-layout` because the
//! answer is a question about the interaction, not about the document: a
//! locked object is still hit, because clicking a locked object and having
//! nothing happen is a bug, whereas a caller that wants only the
//! selectable ones says so.
//!
//! The topmost object wins, which is the order a reader sees them in, and
//! the plan carries each frame's object id, so this is an exact answer
//! rather than a guess from geometry.

use schist_layout::pasteboard::{Display, PagePlan, Pasteboard};
use schist_layout::{ObjectId, PlacedObject, Point, Rect};

/// What is under a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// Nothing.
    Nothing,
    /// A page's own area, with no object on it.
    Page { page: usize },
    /// An object, and where in it the point landed.
    Object {
        object: ObjectId,
        /// The object's box, in pasteboard points, for a rubber band.
        bounds: Rect,
        /// The point, in pasteboard points, which is what a drag's
        /// rectangle is measured from.
        at: Point,
        /// The object comes from a parent page, so selecting it means
        /// overriding it.
        inherited: bool,
        locked: bool,
    },
}

impl Hit {
    /// The object hit, if one was.
    pub fn object(self) -> Option<ObjectId> {
        match self {
            Hit::Object { object, .. } => Some(object),
            _ => None,
        }
    }

    /// Whether this is the same object as another hit, whatever else
    /// differs.
    pub fn same_object(self, other: Hit) -> bool {
        self.object().is_some() && self.object() == other.object()
    }
}

/// The object under a pasteboard point.
///
/// Front to back: a later object in the plan is drawn on top, so it is
/// the one picked.
pub fn hit_test(plan: &Pasteboard, at: Point) -> Hit {
    plan.objects()
        .rev()
        .find_map(|display| hit_display(display, at))
        .or_else(|| {
            plan.pages
                .iter()
                .rev()
                .find(|page| page.page.trim.contains(at))
                .map(|page| Hit::Page {
                    page: page.page.page,
                })
        })
        .unwrap_or(Hit::Nothing)
}

fn hit_display(display: &Display, at: Point) -> Option<Hit> {
    let (object, rect, transform, inherited, locked) = display.frame()?;
    let inverse = transform.invert()?;
    let local = schist_layout::affine::point(inverse, at);
    if hit_bounds(rect).contains(local) {
        return Some(Hit::Object {
            object,
            bounds: schist_layout::affine::bounds(transform, rect),
            at,
            inherited,
            locked,
        });
    }
    None
}

/// A zero-width line is still pickable, without changing saved geometry.
fn hit_bounds(rect: Rect) -> Rect {
    // Forward/inverse f32 affines can move an exact edge point a few ULPs
    // outside. Keep a caret on the first/last baseline point selectable;
    // this is numerical padding, independent of the six-pixel line hit area.
    let magnitude = [rect.x, rect.y, rect.right(), rect.bottom()]
        .into_iter()
        .map(f32::abs)
        .fold(1.0, f32::max);
    let epsilon = (4.0 * f32::EPSILON * magnitude).max(0.001);
    let width = rect.width.max(6.0) + 2.0 * epsilon;
    let height = rect.height.max(6.0) + 2.0 * epsilon;
    Rect::new(
        rect.x - (width - rect.width) / 2.0,
        rect.y - (height - rect.height) / 2.0,
        width,
        height,
    )
}

/// A shape's own anchor point, and which one it is.
///
/// Hit in pasteboard space, like [`hit_test`], because that is the space
/// the plan is drawn in.
///
/// All contours expose their anchors. Handles are hittable only on a
/// selected shape, matching the handles shown by the painter.
pub fn hit_anchor(
    plan: &Pasteboard,
    at: Point,
    within: f32,
    selected: &[ObjectId],
) -> Option<(ObjectId, schist_layout::authoring::PointRef)> {
    use schist_layout::authoring::{PointPart, PointRef};
    for display in plan.objects().rev() {
        let Display::Shape {
            path_editable: true,
            object,
            path,
            inherited: false,
            locked: false,
            ..
        } = display
        else {
            continue;
        };
        let mut best: Option<(f32, PointRef)> = None;
        for (subpath, sub) in path.subpaths.iter().enumerate() {
            for (index, anchor) in sub.points.iter().enumerate() {
                let h = sub.handles_at(index);
                for (part, point) in [
                    (PointPart::Anchor, Some(*anchor)),
                    (PointPart::Incoming, h.incoming),
                    (PointPart::Outgoing, h.outgoing),
                ] {
                    if part != PointPart::Anchor && !selected.contains(object) {
                        continue;
                    }
                    let Some(point) = point else {
                        continue;
                    };
                    let distance = (point.x - at.x).powi(2) + (point.y - at.y).powi(2);
                    if distance <= within * within && best.is_none_or(|(seen, _)| distance < seen) {
                        best = Some((
                            distance,
                            PointRef {
                                subpath,
                                index,
                                part,
                            },
                        ));
                    }
                }
            }
        }
        if let Some((_, point)) = best {
            return Some((*object, point));
        }
    }
    None
}

/// Whether a point is inside any frame, ignoring which.
///
/// This is what a rubber band wants: "did the drag touch anything at all",
/// which is a different question from "what is at this exact point".
pub fn touches_any(plan: &Pasteboard, at: Point) -> bool {
    hit_test(plan, at).object().is_some()
}

/// Every selectable frame's visual bounds, once per object.
pub fn boxes(page: &PagePlan) -> Vec<Rect> {
    let mut seen = std::collections::HashSet::new();
    page.objects
        .iter()
        .filter_map(Display::frame)
        .filter(|(object, ..)| seen.insert(*object))
        .map(|(_, rect, transform, ..)| schist_layout::affine::bounds(transform, rect))
        .collect()
}

/// The ids of every frame on a page, in draw order.
pub fn ids(page: &PagePlan) -> Vec<ObjectId> {
    let mut seen = std::collections::HashSet::new();
    page.objects
        .iter()
        .filter_map(|display| match display {
            Display::Frame { object, .. }
            | Display::EmptyFrame { object, .. }
            | Display::Shape { object, .. }
            | Display::Graphic { object, .. } => Some(*object),
            _ => None,
        })
        .filter(|id| seen.insert(*id))
        .collect()
}

/// Every frame a rectangle touches, for a rubber-band selection.
///
/// Inclusive of anything the band merely clips, which is what a band
/// means in every other drawing program and is the only behaviour that
/// lets you select a frame without landing inside it.
pub fn within(plan: &Pasteboard, band: Rect) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for page in &plan.pages {
        for display in &page.objects {
            let Some((object, rect, transform, false, _)) = display.frame() else {
                continue;
            };
            let rect = schist_layout::affine::bounds(transform, rect);
            if band.intersects(rect) && !out.contains(&object) {
                out.push(object);
            }
        }
    }
    out
}

/// The object under a point in a document, rather than in a plan.
///
/// Used by the panels, which know which page they are listing and do not
/// want to build a plan to answer a question.
pub fn hit_objects<'a>(
    mut objects: impl DoubleEndedIterator<Item = &'a PlacedObject>,
    at: Point,
) -> Option<&'a PlacedObject> {
    // Last, because the document lists back to front like the plan. A
    // reverse scan rather than a filter, so it stops at the first frame
    // found instead of walking the whole page.
    objects.rfind(|object| object.contains(at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_layout::pasteboard::{pasteboard, PasteboardView};
    use schist_layout::{
        blank_a4, LayoutDocument, LayoutObject, ObjectId, Page, Point, Rect, Story,
    };

    #[test]
    fn painted_frames_remain_one_selection_without_exposing_synthetic_anchors() {
        use schist_layout::{authoring, object_styles, History, Ink, ObjectPaint, Paint};
        for graphic in [false, true] {
            let mut doc = blank_a4();
            let mut history = History::default();
            let rect = Rect::new(30.0, 30.0, 80.0, 80.0);
            let id = if graphic {
                authoring::graphic_frame(&mut doc, &mut history, 0, rect, "image.png", false)
                    .unwrap()
            } else {
                authoring::text_frame(&mut doc, &mut history, 0, rect)
                    .unwrap()
                    .object
            };
            object_styles::edit_paint(
                &mut doc,
                &mut history,
                &[id],
                &ObjectPaint {
                    fill: Some(Paint::Ink(Ink::white())),
                    stroke: Some(Paint::Ink(Ink::black())),
                    stroke_width: Some(4.0),
                    ..Default::default()
                },
            );
            let plan = schist_layout::pasteboard(&doc, &Default::default()).unwrap();
            assert_eq!(ids(&plan.pages[0]), vec![id]);
            for display in plan.objects() {
                if let Display::Shape { path, .. } = display {
                    for subpath in &path.subpaths {
                        for point in &subpath.points {
                            assert!(hit_anchor(&plan, *point, 4.0, &[id]).is_none());
                            assert_eq!(hit_test(&plan, *point).object(), Some(id));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn crossover_selection_follows_global_stacking_in_spread_and_single_page_views() {
        use schist_layout::{
            authoring::{self, Paint},
            History, LayerId, Spread,
        };
        for reverse in [false, true] {
            let mut doc = LayoutDocument::new(vec![Page::new("page", 100.0, 100.0); 2]);
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                ..Spread::single(0)
            }];
            doc.layers = vec![LayerId(1), LayerId(0)];
            let order = if reverse { [0, 1] } else { [1, 0] };
            let mut ids = Vec::new();
            for page in order {
                ids.push(
                    authoring::rectangle(
                        &mut doc,
                        &mut History::default(),
                        page,
                        Rect::new(80.0 - page as f32 * 100.0, 20.0, 40.0, 30.0),
                        Paint::filled("Black"),
                    )
                    .unwrap(),
                );
            }
            for top_layer in [false, true] {
                if top_layer {
                    doc.object_layers[0].1 = LayerId(1);
                    doc.object_layers[1].1 = LayerId(0);
                }
                let top = ids[usize::from(!top_layer)];
                for page in [None, Some(0), Some(1)] {
                    let plan = pasteboard(
                        &doc,
                        &PasteboardView {
                            page,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let view = PasteboardView::default();
                    for x in [85.0, 115.0] {
                        let at = view.to_pasteboard(Point::new(x, 30.0));
                        assert_eq!(hit_test(&plan, at).object(), Some(top));
                        assert_eq!(
                            within(&plan, Rect::new(at.x - 1.0, at.y - 1.0, 2.0, 2.0)).len(),
                            2
                        );
                    }
                    let drawn: Vec<_> = plan
                        .objects()
                        .filter_map(|display| match display {
                            Display::Shape { object, .. } => Some(*object),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(drawn.len(), 2, "crossovers must not paint twice");
                    assert_eq!(drawn.last(), Some(&top));
                }
            }
        }
    }

    /// A document with one text frame, whose id is returned.
    fn doc_with_frame() -> (LayoutDocument, ObjectId) {
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text("hello", "Body"));
        let id = ObjectId::next();
        doc.add_object(PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id,
            page: 0,
            bounds: Rect::new(100.0, 100.0, 200.0, 40.0),
            object: LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
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

    fn plan_of(doc: &LayoutDocument) -> Pasteboard {
        pasteboard(
            doc,
            &PasteboardView {
                page: Some(0),
                ..PasteboardView::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn a_point_on_blank_paper_hits_the_page() {
        let plan = plan_of(&blank_a4());
        assert_eq!(
            hit_test(&plan, Point::new(10.0, 10.0)),
            Hit::Page { page: 0 }
        );
    }

    #[test]
    fn a_point_off_the_pasteboard_hits_nothing() {
        let plan = plan_of(&blank_a4());
        assert_eq!(hit_test(&plan, Point::new(-500.0, -500.0)), Hit::Nothing);
    }

    #[test]
    fn a_point_outside_the_trim_but_on_the_sheet_hits_nothing() {
        // The slug is on the sheet but is not the page, so clicking it
        // selects nothing. Otherwise a click near the edge would select a
        // page the reader cannot see.
        let mut page = Page::a4();
        page.bleed = (3.0).into();
        let doc = LayoutDocument::new(vec![page]);
        let plan = plan_of(&doc);
        // The media box is centred on the trim, so the slug is the band
        // of sheet outside it. A point there is on paper but is not the
        // page, so it selects nothing.
        assert_eq!(hit_test(&plan, Point::new(-2.0, -2.0)), Hit::Nothing);
        assert!(matches!(
            hit_test(&plan, Point::new(1.0, 1.0)),
            Hit::Page { .. }
        ));
    }

    #[test]
    fn a_point_on_a_frame_hits_that_object_by_id() {
        let (doc, id) = doc_with_frame();
        let plan = plan_of(&doc);
        let hit = hit_test(&plan, Point::new(150.0, 120.0));
        assert_eq!(hit.object(), Some(id));
        assert!(!matches!(
            hit,
            Hit::Object {
                inherited: true,
                ..
            }
        ));
    }

    #[test]
    fn a_hit_carries_the_frame_and_the_point() {
        let (doc, _) = doc_with_frame();
        let plan = plan_of(&doc);
        let Hit::Object { bounds, at, .. } = hit_test(&plan, Point::new(150.0, 120.0)) else {
            panic!("expected an object");
        };
        assert_eq!(bounds, Rect::new(100.0, 100.0, 200.0, 40.0));
        assert_eq!(at, Point::new(150.0, 120.0));
    }

    #[test]
    fn the_front_object_wins() {
        let (mut doc, back) = doc_with_frame();
        let story = doc.add_story(Story::from_text("front", "Body"));
        let front = ObjectId::next();
        doc.add_object(PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: front,
            page: 0,
            bounds: Rect::new(120.0, 110.0, 200.0, 40.0),
            object: LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Front".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        let plan = plan_of(&doc);
        // A point inside both frames is on the one drawn last.
        assert_eq!(
            hit_test(&plan, Point::new(150.0, 120.0)).object(),
            Some(front)
        );
        // And a point only the back one covers still reaches it.
        assert_eq!(
            hit_test(&plan, Point::new(105.0, 105.0)).object(),
            Some(back)
        );
    }

    #[test]
    fn a_locked_object_is_hit_and_says_so() {
        // Clicking a locked object and having nothing happen is a bug, so
        // the hit reports locked and the caller decides what to do.
        let (mut doc, id) = doc_with_frame();
        // Locked through the document's own list, because a caller has no
        // business reaching past it into a borrowed object.
        if let Some(placed) = doc.objects.iter_mut().find(|o| o.id == id) {
            placed.locked = true;
        }
        let plan = plan_of(&doc);
        assert!(matches!(
            hit_test(&plan, Point::new(150.0, 120.0)),
            Hit::Object { locked: true, .. }
        ));
    }

    #[test]
    fn a_hit_scales_with_the_view() {
        // The same document point is a different pasteboard point at a
        // different zoom, and the hit follows the document.
        let (doc, id) = doc_with_frame();
        let half = pasteboard(
            &doc,
            &PasteboardView {
                page: Some(0),
                scale: 0.5,
                ..PasteboardView::default()
            },
        )
        .unwrap();
        assert_eq!(hit_test(&half, Point::new(75.0, 60.0)).object(), Some(id));
        assert!(matches!(
            hit_test(&half, Point::new(150.0, 120.0)),
            Hit::Page { .. }
        ));
    }

    #[test]
    fn an_empty_frame_is_still_clickable() {
        // A frame with no text has no glyphs to click, so its box is what
        // makes it reachable.
        let mut doc = blank_a4();
        let story = doc.add_story(Story::from_text("", "Body"));
        let id = ObjectId::next();
        doc.add_object(PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id,
            page: 0,
            bounds: Rect::new(100.0, 100.0, 200.0, 40.0),
            object: LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Empty".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        let plan = plan_of(&doc);
        assert_eq!(hit_test(&plan, Point::new(150.0, 120.0)).object(), Some(id));
    }

    #[test]
    fn a_rubber_band_knows_whether_it_touched_anything() {
        let (doc, _) = doc_with_frame();
        let plan = plan_of(&doc);
        assert!(touches_any(&plan, Point::new(150.0, 120.0)));
        assert!(!touches_any(&plan, Point::new(20.0, 20.0)));
    }

    #[test]
    fn a_rubber_band_takes_everything_it_clips() {
        let (mut doc, back) = doc_with_frame();
        let story = doc.add_story(Story::from_text("far", "Body"));
        let far = ObjectId::next();
        doc.add_object(PlacedObject {
            hidden: false,
            appearance: Default::default(),
            id: far,
            page: 0,
            bounds: Rect::new(400.0, 400.0, 50.0, 50.0),
            object: LayoutObject::TextFrame {
                balance_columns: Some(false),
                footnotes: Default::default(),
                text_path: None,
                story,
                columns: 1,
                gutter: 0.0,
                insets: Default::default(),
                overflow: Default::default(),
            },
            rotation: 0.0,
            transform: Default::default(),
            name: "Far".into(),
            locked: false,
            overprint: false,
            transparency: 0.0,
        });
        let plan = plan_of(&doc);
        // A band clipping the near frame's corner catches it even though
        // no part of the band is inside it.
        let band = Rect::new(90.0, 90.0, 20.0, 20.0);
        assert_eq!(within(&plan, band), vec![back]);
        // A band over both takes both.
        assert_eq!(
            within(&plan, Rect::new(0.0, 0.0, 500.0, 500.0)),
            vec![back, far]
        );
        // A band over neither takes neither.
        assert!(within(&plan, Rect::new(600.0, 600.0, 50.0, 50.0)).is_empty());
    }

    #[test]
    fn a_pages_boxes_and_ids_agree() {
        let (doc, id) = doc_with_frame();
        let plan = plan_of(&doc);
        assert_eq!(
            boxes(&plan.pages[0]),
            vec![Rect::new(100.0, 100.0, 200.0, 40.0)]
        );
        assert_eq!(ids(&plan.pages[0]), vec![id]);
    }

    #[test]
    fn a_helpers_hit_agrees_with_the_plan() {
        // The panel path and the pasteboard path must not disagree, or
        // clicking a row selects something other than what was clicked.
        let (doc, id) = doc_with_frame();
        let objects = doc.page_objects(0);
        let found = hit_objects(objects.iter().map(|o| o.as_ref()), Point::new(150.0, 120.0));
        assert_eq!(found.map(|o| o.id), Some(id));
        assert!(hit_objects(
            doc.page_objects(0).iter().map(|o| o.as_ref()),
            Point::new(5.0, 5.0)
        )
        .is_none());
    }

    #[test]
    fn two_hits_of_one_object_are_the_same_object() {
        let (doc, _) = doc_with_frame();
        let plan = plan_of(&doc);
        let a = hit_test(&plan, Point::new(110.0, 105.0));
        let b = hit_test(&plan, Point::new(290.0, 135.0));
        assert!(a.same_object(b));
        // And a hit of paper is not the same object as a hit of a frame.
        assert!(!a.same_object(hit_test(&plan, Point::new(10.0, 10.0))));
    }

    /// A shape on page 0, made the way the editor's tool makes one.
    fn editor_shape(
        document: &mut schist_layout::LayoutDocument,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> ObjectId {
        schist_layout::authoring::rectangle(
            document,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(x, y, width, height),
            schist_layout::authoring::Paint::none(),
        )
        .expect("a shape")
    }

    /// A placed graphic on page 0.
    fn editor_graphic(
        document: &mut schist_layout::LayoutDocument,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> ObjectId {
        schist_layout::authoring::graphic_frame(
            document,
            &mut schist_layout::History::default(),
            0,
            schist_layout::Rect::new(x, y, width, height),
            "missing.png",
            false,
        )
        .expect("a graphic")
    }

    #[test]
    fn a_shape_is_clicked_by_its_box_not_its_outline() {
        // A user clicks the middle of a filled rectangle. Requiring a
        // click on the stroke would make every shape feel hollow.
        let mut document = blank_a4();
        let object = editor_shape(&mut document, 10.0, 10.0, 100.0, 50.0);
        let plan = plan_of(&document);
        assert_eq!(
            hit_test(&plan, Point::new(60.0, 35.0)).object(),
            Some(object),
            "the middle of a shape is the shape"
        );
    }

    #[test]
    fn a_placed_graphic_is_clicked_by_its_box() {
        let mut document = blank_a4();
        let object = editor_graphic(&mut document, 10.0, 10.0, 100.0, 50.0);
        let plan = plan_of(&document);
        assert_eq!(
            hit_test(&plan, Point::new(60.0, 35.0)).object(),
            Some(object)
        );
        assert_eq!(
            hit_test(&plan, Point::new(200.0, 200.0)).object(),
            None,
            "outside it is not it"
        );
    }
}
