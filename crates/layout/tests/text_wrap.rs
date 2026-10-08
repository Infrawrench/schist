use schist_layout::text_wrap::{self, ContourType, TextWrap, WrapMode, WrapPreferences, WrapSide};
use schist_layout::{
    affine, authoring, authoring::ShapeKind, blank_a4, compose::compose_story, ComposedLine,
    ComposedThread, History, Insets, LayoutDocument, LayoutObject, ObjectId, ParagraphStyle, Point,
    Rect, Spread, Story, StoryId, WritingMode,
};

const TEXT: &str =
    "Wrapped type keeps its reading order while a picture pushes the measure aside, \
and every line that meets the obstacle finds the room that remains on either side of it.";
const FRAME: Rect = Rect::new(40.0, 40.0, 400.0, 560.0);

fn document(paragraphs: usize) -> (LayoutDocument, ObjectId) {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 0, FRAME).unwrap();
    let mut story = Story::new();
    for _ in 0..paragraphs {
        story.push_paragraph(TEXT, "Body");
    }
    doc.stories[frame.story.0 as usize] = story;
    (doc, frame.object)
}

fn wrap(mode: WrapMode, offset: f32) -> TextWrap {
    TextWrap {
        mode,
        offsets: Insets::uniform(offset),
        ..Default::default()
    }
}

fn obstacle(doc: &mut LayoutDocument, kind: ShapeKind, rect: Rect, wrap: TextWrap) -> ObjectId {
    let id = authoring::shape(
        doc,
        &mut History::default(),
        0,
        rect,
        kind,
        authoring::Paint::none(),
    )
    .unwrap();
    object_mut(doc, id).appearance.text_wrap = Some(wrap);
    id
}

fn object_mut(doc: &mut LayoutDocument, id: ObjectId) -> &mut schist_layout::PlacedObject {
    doc.objects.iter_mut().find(|o| o.id == id).unwrap()
}

fn flow(doc: &LayoutDocument) -> ComposedThread {
    compose_story(doc, StoryId(0))
}

fn lines(doc: &LayoutDocument) -> Vec<ComposedLine> {
    flow(doc).lines().cloned().collect()
}

fn overlaps(a: (f32, f32), b: (f32, f32)) -> bool {
    a.0 < b.1 - 0.01 && b.0 < a.1 - 0.01
}

fn vertical(line: &ComposedLine) -> (f32, f32) {
    (line.bounds.y, line.bounds.bottom())
}

fn horizontal(line: &ComposedLine) -> (f32, f32) {
    (line.bounds.x, line.bounds.right())
}

fn zone(rect: Rect, offset: f32) -> Rect {
    Rect::new(
        rect.x - offset,
        rect.y - offset,
        rect.width + 2.0 * offset,
        rect.height + 2.0 * offset,
    )
}

fn assert_clear(lines: &[ComposedLine], zone: Rect) {
    for line in lines {
        assert!(
            !(overlaps(vertical(line), (zone.y, zone.bottom()))
                && overlaps(horizontal(line), (zone.x, zone.right()))),
            "line {:?} enters {zone:?}",
            line.bounds
        );
    }
}

/// Source text is placed once, in order, and nothing is lost.
fn assert_complete(doc: &LayoutDocument) {
    let flow = flow(doc);
    assert!(!flow.has_overflow());
    let mut end = 0;
    for line in flow.lines() {
        assert!(line.start >= end, "lines out of order");
        end = line.end;
    }
    assert_eq!(flow.frames[0].consumed_to, doc.stories[0].text().len());
}

#[test]
fn bounding_box_wrap_uses_both_sides_without_entering_the_offset_box() {
    let rect = Rect::new(190.0, 150.0, 100.0, 100.0);
    for offset in [0.0, 6.0, 18.0] {
        let (mut doc, _) = document(4);
        let plain = lines(&doc);
        obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            wrap(WrapMode::BoundingBox, offset),
        );
        let wrapped = lines(&doc);
        let zone = zone(rect, offset);
        assert_clear(&wrapped, zone);
        assert_complete(&doc);
        let beside: Vec<_> = wrapped
            .iter()
            .filter(|l| overlaps(vertical(l), (zone.y, zone.bottom())))
            .collect();
        assert!(beside.iter().any(|l| l.bounds.right() <= zone.x + 0.01));
        assert!(beside.iter().any(|l| l.bounds.x >= zone.right() - 0.01));
        assert!(wrapped.len() > plain.len());
        // Above the object composition is unchanged.
        for (a, b) in plain.iter().zip(&wrapped) {
            if a.bounds.bottom() > zone.y {
                break;
            }
            assert_eq!(a.bounds, b.bounds);
            assert_eq!((a.start, a.end), (b.start, b.end));
        }
    }
}

#[test]
fn sides_and_largest_area_choose_one_side_of_the_object() {
    for (x, side, left_used, right_used) in [
        (190.0, WrapSide::LeftSide, true, false),
        (190.0, WrapSide::RightSide, false, true),
        (190.0, WrapSide::BothSides, true, true),
        (90.0, WrapSide::LargestArea, false, true),
        (290.0, WrapSide::LargestArea, true, false),
    ] {
        let rect = Rect::new(x, 150.0, 80.0, 100.0);
        let (mut doc, _) = document(4);
        obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            TextWrap {
                side,
                ..wrap(WrapMode::BoundingBox, 4.0)
            },
        );
        let wrapped = lines(&doc);
        let zone = zone(rect, 4.0);
        assert_clear(&wrapped, zone);
        assert_complete(&doc);
        let beside: Vec<_> = wrapped
            .iter()
            .filter(|l| overlaps(vertical(l), (zone.y, zone.bottom())))
            .collect();
        assert!(!beside.is_empty());
        assert_eq!(
            beside.iter().any(|l| l.bounds.right() <= zone.x + 0.01),
            left_used,
            "{side:?} at {x}"
        );
        assert_eq!(
            beside.iter().any(|l| l.bounds.x >= zone.right() - 0.01),
            right_used,
            "{side:?} at {x}"
        );
    }
}

#[test]
fn spine_sides_follow_the_page_position_in_a_facing_spread() {
    for page in [0, 1] {
        for (side, toward) in [
            (WrapSide::SideTowardsSpine, true),
            (WrapSide::SideAwayFromSpine, false),
        ] {
            let mut doc = blank_a4();
            doc.pages.push(doc.pages[0].clone());
            doc.facing_pages = true;
            doc.spreads = vec![Spread {
                pages: vec![0, 1],
                binding_location: Some(1),
                gutter: 0.0,
                origin: Point::ZERO,
            }];
            let frame =
                authoring::text_frame(&mut doc, &mut History::default(), page, FRAME).unwrap();
            let mut story = Story::new();
            for _ in 0..4 {
                story.push_paragraph(TEXT, "Body");
            }
            doc.stories[frame.story.0 as usize] = story;
            let rect = Rect::new(190.0, 150.0, 80.0, 100.0);
            let id = authoring::rectangle(
                &mut doc,
                &mut History::default(),
                page,
                rect,
                authoring::Paint::none(),
            )
            .unwrap();
            object_mut(&mut doc, id).appearance.text_wrap = Some(TextWrap {
                side,
                ..wrap(WrapMode::BoundingBox, 0.0)
            });
            let wrapped = lines(&doc);
            assert_clear(&wrapped, rect);
            let beside: Vec<_> = wrapped
                .iter()
                .filter(|l| overlaps(vertical(l), (rect.y, rect.bottom())))
                .collect();
            // The spine is right of the left page and left of the right page.
            let spine_right = page == 0;
            let text_right = toward == spine_right;
            assert!(!beside.is_empty());
            assert!(
                beside.iter().all(|l| if text_right {
                    l.bounds.x >= rect.right() - 0.01
                } else {
                    l.bounds.right() <= rect.x + 0.01
                }),
                "page {page} {side:?}"
            );
        }
    }
}

#[test]
fn jump_object_resumes_below_and_abut_keeps_the_leading_rhythm() {
    let rect = Rect::new(120.0, 150.0, 60.0, 47.0);
    for abut in [true, false] {
        let (mut doc, _) = document(4);
        doc.text_wrap_preferences.abut = abut;
        obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            wrap(WrapMode::JumpObject, 3.0),
        );
        let wrapped = lines(&doc);
        let zone = zone(rect, 3.0);
        assert_complete(&doc);
        for line in &wrapped {
            assert!(!overlaps(vertical(line), (zone.y, zone.bottom())));
        }
        let first = &wrapped[0];
        let below = wrapped
            .iter()
            .find(|l| l.bounds.y >= zone.bottom() - 0.01)
            .unwrap();
        if abut {
            let steps = (below.bounds.y - first.bounds.y) / first.advance;
            assert!((steps - steps.round()).abs() < 0.01, "{steps}");
            assert!(below.bounds.y - zone.bottom() < first.advance + 0.01);
        } else {
            assert!((below.bounds.y - zone.bottom()).abs() < 0.01);
        }
    }
}

#[test]
fn next_column_wrap_moves_text_to_the_next_column() {
    let (mut doc, frame) = document(6);
    let LayoutObject::TextFrame {
        columns, gutter, ..
    } = &mut object_mut(&mut doc, frame).object
    else {
        panic!()
    };
    *columns = 2;
    *gutter = 20.0;
    let rect = Rect::new(80.0, 200.0, 60.0, 40.0);
    obstacle(
        &mut doc,
        ShapeKind::Rectangle,
        rect,
        wrap(WrapMode::NextColumn, 0.0),
    );
    let wrapped = lines(&doc);
    let first_column: Vec<_> = wrapped.iter().filter(|l| l.bounds.x < 240.0).collect();
    let second_column: Vec<_> = wrapped.iter().filter(|l| l.bounds.x >= 240.0).collect();
    assert!(!first_column.is_empty() && !second_column.is_empty());
    assert!(first_column
        .iter()
        .all(|l| l.bounds.bottom() <= rect.y + 0.01));
    assert!(first_column.last().unwrap().end <= second_column[0].start);
}

/// The widest half-chord of a circle over a band.
fn half_chord(center: Point, radius: f32, top: f32, bottom: f32) -> Option<f32> {
    let d = if (top..=bottom).contains(&center.y) {
        0.0
    } else {
        (center.y - top).abs().min((center.y - bottom).abs())
    };
    (d < radius).then(|| (radius * radius - d * d).sqrt())
}

/// The narrowest half-chord over a band, for text inside a circle.
fn inner_chord(center: Point, radius: f32, top: f32, bottom: f32) -> Option<f32> {
    let d = (center.y - top).abs().max((center.y - bottom).abs());
    (d < radius).then(|| (radius * radius - d * d).sqrt())
}

#[test]
fn contour_wrap_follows_the_outline_closer_than_its_box() {
    let rect = Rect::new(180.0, 140.0, 120.0, 120.0);
    let center = Point::new(240.0, 200.0);
    for contour in [
        None,
        Some(ContourType::SameAsClipping),
        Some(ContourType::GraphicFrame),
    ] {
        for offset in [0.0, 8.0] {
            let (mut doc, _) = document(6);
            obstacle(
                &mut doc,
                ShapeKind::Ellipse,
                rect,
                TextWrap {
                    contour,
                    ..wrap(WrapMode::Contour, offset)
                },
            );
            let wrapped = lines(&doc);
            assert_complete(&doc);
            // Flattening inscribes the circle within 0.25pt.
            let radius = 60.0 + offset - 0.3;
            for line in &wrapped {
                if let Some(half) = half_chord(center, radius, line.bounds.y, line.bounds.bottom())
                {
                    assert!(
                        !overlaps(horizontal(line), (center.x - half, center.x + half)),
                        "{:?} enters the contour",
                        line.bounds
                    );
                }
            }
            let boxed = zone(rect, offset);
            assert!(
                wrapped
                    .iter()
                    .any(|l| overlaps(vertical(l), (boxed.y, boxed.bottom()))
                        && overlaps(horizontal(l), (boxed.x, boxed.right()))),
                "contour wrap behaves like a bounding box"
            );
            assert!(!flow(&doc).frames[0].wrap.approximated);
        }
    }
    let (mut doc, _) = document(2);
    obstacle(
        &mut doc,
        ShapeKind::Ellipse,
        rect,
        TextWrap {
            contour: Some(ContourType::DetectEdges),
            ..wrap(WrapMode::Contour, 0.0)
        },
    );
    assert!(flow(&doc).frames[0].wrap.approximated);
}

#[test]
fn inverse_contour_keeps_text_inside_the_outline() {
    let rect = Rect::new(120.0, 120.0, 240.0, 240.0);
    let center = Point::new(240.0, 240.0);
    for offset in [0.0, 6.0] {
        let (mut doc, _) = document(6);
        obstacle(
            &mut doc,
            ShapeKind::Ellipse,
            rect,
            TextWrap {
                inverse: true,
                ..wrap(WrapMode::Contour, offset)
            },
        );
        let flow = flow(&doc);
        let placed: Vec<_> = flow.lines().collect();
        assert!(placed.len() > 3);
        for line in &placed {
            // A four-arc Bézier circle bulges up to 0.03% beyond its radius.
            let half = inner_chord(
                center,
                120.0 - offset + 0.05,
                line.bounds.y,
                line.bounds.bottom(),
            )
            .expect("line inside the circle's height");
            assert!(line.bounds.x >= center.x - half - 0.01, "{:?}", line.bounds);
            assert!(
                line.bounds.right() <= center.x + half + 0.01,
                "{:?}",
                line.bounds
            );
        }
        // Text the circle cannot hold is overset, not placed outside it.
        assert!(flow.has_overflow());
    }
}

/// A change that must make a wrap stop affecting the frame.
type Variant = dyn Fn(&mut LayoutDocument, ObjectId);

#[test]
fn ignore_wrap_layers_hidden_objects_and_none_mode_compose_as_unwrapped() {
    let rect = Rect::new(190.0, 150.0, 100.0, 100.0);
    let (base, frame) = document(4);
    let plain = lines(&base);
    let variants: Vec<Box<Variant>> = vec![
        Box::new(move |doc, _| object_mut(doc, frame).appearance.ignore_wrap = true),
        Box::new(move |doc, _| {
            let layer = doc.object_layer(frame);
            schist_layout::structure::change_layer(doc, &mut History::default(), layer, |l| {
                l.ignore_wrap = true
            });
        }),
        Box::new(|doc, id| object_mut(doc, id).hidden = true),
        Box::new(|doc, id| object_mut(doc, id).appearance.text_wrap = Some(TextWrap::default())),
        // Beneath the frame in stacking order, with wrap restricted to text beneath.
        Box::new(|doc, id| {
            doc.text_wrap_preferences.only_beneath = true;
            let index = doc.objects.iter().position(|o| o.id == id).unwrap();
            let object = doc.objects.remove(index);
            doc.objects.insert(0, object);
        }),
    ];
    for variant in variants {
        let mut doc = base.clone();
        let id = obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            wrap(WrapMode::BoundingBox, 0.0),
        );
        assert_ne!(lines(&doc), plain);
        variant(&mut doc, id);
        assert_eq!(lines(&doc), plain);
    }
    // Above the frame, a restricted wrap still applies.
    let mut doc = base.clone();
    doc.text_wrap_preferences.only_beneath = true;
    obstacle(
        &mut doc,
        ShapeKind::Rectangle,
        rect,
        wrap(WrapMode::BoundingBox, 0.0),
    );
    assert_clear(&lines(&doc), rect);
}

#[test]
fn master_only_parent_items_do_not_wrap_document_pages() {
    for master_only in [false, true] {
        let (mut doc, _) = document(4);
        let plain = lines(&doc);
        let rect = Rect::new(190.0, 150.0, 100.0, 100.0);
        let id = obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            TextWrap {
                master_only,
                ..wrap(WrapMode::BoundingBox, 0.0)
            },
        );
        let index = doc.objects.iter().position(|o| o.id == id).unwrap();
        let object = doc.objects.remove(index);
        doc.parents.push(schist_layout::ParentPage {
            name: "A".into(),
            applied_to: vec![0],
            based_on: None,
            hidden: false,
            sheets: vec![schist_layout::parents::ParentSheet {
                source: None,
                page: doc.pages[0].clone(),
                origin: Point::ZERO,
            }],
            placements: Vec::new(),
            objects: vec![schist_layout::ParentObject {
                object,
                overridden_on: Vec::new(),
            }],
        });
        let wrapped = lines(&doc);
        if master_only {
            assert_eq!(wrapped, plain);
        } else {
            assert_clear(&wrapped, rect);
            assert_ne!(wrapped, plain);
        }
    }
}

#[test]
fn rotated_frames_and_objects_wrap_in_page_space() {
    for frame_rotation in [0.0, 12.0, -30.0] {
        for object_rotation in [0.0, 45.0] {
            let (mut doc, frame) = document(5);
            object_mut(&mut doc, frame).rotation = frame_rotation;
            let rect = Rect::new(200.0, 220.0, 80.0, 80.0);
            let id = obstacle(
                &mut doc,
                ShapeKind::Rectangle,
                rect,
                wrap(WrapMode::BoundingBox, 4.0),
            );
            object_mut(&mut doc, id).rotation = object_rotation;
            let blocked = zone(doc.object(id).unwrap().visual_bounds(), 4.0 - 0.05);
            let transform = doc.object(frame).unwrap().content_transform();
            let wrapped = lines(&doc);
            assert!(!wrapped.is_empty());
            for line in &wrapped {
                // Sample the line box densely in page space.
                for i in 0..=40 {
                    for j in 0..=4 {
                        let p = Point::new(
                            line.bounds.x + line.bounds.width * i as f32 / 40.0,
                            line.bounds.y + line.bounds.height * j as f32 / 4.0,
                        );
                        let p = affine::point(transform, p);
                        assert!(
                            !(p.x > blocked.x
                                && p.x < blocked.right()
                                && p.y > blocked.y
                                && p.y < blocked.bottom()),
                            "frame {frame_rotation} object {object_rotation}: {p:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn spread_artwork_crossing_the_gutter_wraps_the_facing_page() {
    let mut doc = blank_a4();
    let width = doc.pages[0].width;
    doc.pages.push(doc.pages[0].clone());
    doc.facing_pages = true;
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        binding_location: Some(1),
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 1, FRAME).unwrap();
    let mut story = Story::new();
    for _ in 0..4 {
        story.push_paragraph(TEXT, "Body");
    }
    doc.stories[frame.story.0 as usize] = story;
    // Owned by the left page, reaching 150pt into the right one.
    let rect = Rect::new(width - 60.0, 150.0, 210.0, 80.0);
    obstacle(
        &mut doc,
        ShapeKind::Rectangle,
        rect,
        wrap(WrapMode::BoundingBox, 0.0),
    );
    let local = Rect::new(rect.x - width, rect.y, rect.width, rect.height);
    let wrapped = lines(&doc);
    assert_clear(&wrapped, local);
    assert!(wrapped
        .iter()
        .any(|l| overlaps(vertical(l), (local.y, local.bottom()))
            && l.bounds.x >= local.right() - 0.01));
}

#[test]
fn unsupported_text_is_composed_unwrapped_and_reported() {
    let rect = Rect::new(60.0, 60.0, 100.0, 100.0);
    for case in 0..2 {
        let (mut doc, _) = document(2);
        let style = if case == 0 {
            ParagraphStyle {
                name: "Initial".into(),
                drop_caps_lines: Some(3),
                drop_caps_characters: Some(1),
                ..Default::default()
            }
        } else {
            ParagraphStyle {
                name: "Initial".into(),
                writing_mode: Some(WritingMode::VerticalRightToLeft),
                ..Default::default()
            }
        };
        doc.styles.add_paragraph(style);
        doc.stories[0] = Story::from_text(TEXT, "Initial");
        let plain = lines(&doc);
        assert!(!flow(&doc).frames[0].wrap.ignored);
        obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            wrap(WrapMode::BoundingBox, 0.0),
        );
        assert_eq!(lines(&doc), plain);
        assert!(flow(&doc).frames[0].wrap.ignored, "case {case}");
    }
}

#[test]
fn wrap_edits_are_single_undo_steps_and_refuse_locked_or_invalid_input() {
    let (mut doc, frame) = document(2);
    let a = authoring::rectangle(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 50.0, 50.0),
        authoring::Paint::none(),
    )
    .unwrap();
    let b = authoring::rectangle(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(200.0, 100.0, 50.0, 50.0),
        authoring::Paint::none(),
    )
    .unwrap();
    let before = doc.clone();
    let mut history = History::default();
    assert!(text_wrap::edit_wrap(
        &mut doc,
        &mut history,
        &[a, b, a],
        |w| {
            w.mode = WrapMode::Contour;
            w.offsets = Insets::uniform(9.0);
        }
    ));
    assert_eq!(history.undo_depth(), 1);
    for id in [a, b] {
        let wrap = doc
            .object(id)
            .unwrap()
            .appearance
            .text_wrap
            .clone()
            .unwrap();
        assert_eq!(wrap.mode, WrapMode::Contour);
        assert_eq!(wrap.offsets, Insets::uniform(9.0));
    }
    assert!(!text_wrap::edit_wrap(&mut doc, &mut history, &[a], |w| {
        w.mode = WrapMode::Contour;
    }));
    for bad in [f32::NAN, f32::INFINITY, 1e9] {
        assert!(!text_wrap::edit_wrap(&mut doc, &mut history, &[a], |w| {
            w.offsets.top = bad;
        }));
    }
    assert_eq!(history.undo_depth(), 1);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);

    object_mut(&mut doc, b).locked = true;
    let locked = doc.clone();
    assert!(!text_wrap::edit_wrap(
        &mut doc,
        &mut history,
        &[a, b],
        |w| {
            w.mode = WrapMode::BoundingBox;
        }
    ));
    assert_eq!(doc, locked);

    // Resetting to the defaults drops the record entirely.
    assert!(text_wrap::edit_wrap(&mut doc, &mut history, &[a], |w| {
        w.mode = WrapMode::JumpObject;
    }));
    assert!(text_wrap::edit_wrap(&mut doc, &mut history, &[a], |w| {
        *w = TextWrap::default();
    }));
    assert_eq!(doc.object(a).unwrap().appearance.text_wrap, None);

    let before = doc.clone();
    assert!(!text_wrap::set_ignore_wrap(
        &mut doc,
        &mut history,
        &[a],
        true
    ));
    assert!(text_wrap::set_ignore_wrap(
        &mut doc,
        &mut history,
        &[frame],
        true
    ));
    assert!(doc.object(frame).unwrap().appearance.ignore_wrap);
    assert!(!text_wrap::set_ignore_wrap(
        &mut doc,
        &mut history,
        &[frame],
        true
    ));
    let preferences = WrapPreferences {
        only_beneath: true,
        abut: false,
        justify: true,
    };
    assert!(text_wrap::set_preferences(
        &mut doc,
        &mut history,
        preferences
    ));
    assert!(!text_wrap::set_preferences(
        &mut doc,
        &mut history,
        preferences
    ));
    assert_eq!(doc.text_wrap_preferences, preferences);
    assert!(history.undo(&mut doc));
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
}

#[test]
fn wrapped_documents_survive_native_serialization() {
    let (mut doc, frame) = document(1);
    obstacle(
        &mut doc,
        ShapeKind::Ellipse,
        Rect::new(100.0, 100.0, 80.0, 60.0),
        TextWrap {
            side: WrapSide::LargestArea,
            inverse: true,
            master_only: true,
            contour: Some(ContourType::AlphaChannel),
            inside_edges: true,
            contour_path: "Mask \"1\"".into(),
            ..wrap(WrapMode::Contour, 5.0)
        },
    );
    object_mut(&mut doc, frame).appearance.ignore_wrap = true;
    doc.text_wrap_preferences.only_beneath = true;
    let json = serde_json::to_string(&doc).unwrap();
    let back: LayoutDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(back, doc);
    // Older documents without wrap fields load with the published defaults.
    let (plain, _) = document(1);
    let mut value = serde_json::to_value(&plain).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("text_wrap_preferences");
    let back: LayoutDocument = serde_json::from_value(value).unwrap();
    assert_eq!(back.text_wrap_preferences, WrapPreferences::default());
    assert!(back.text_wrap_preferences.abut);
}

/// Native review: a hyphenated, centered paragraph met an inverse ellipse whose
/// narrow top could not hold its first word, and the whole story was overset.
#[test]
fn narrow_intervals_a_word_cannot_fit_are_skipped_not_overflowed() {
    use schist_layout::styles::Align;
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    // No word here fits in 20pt at 12pt.
    let long = "Typesetting paragraphs everywhere demonstrates wrapping behaviour \
        consistently throughout composition engines nowadays.";
    for hyphenate in [false, true] {
        for align in [Align::Left, Align::Center] {
            let style = ParagraphStyle {
                name: "Wrapped".into(),
                family: Some("IBM Plex Sans".into()),
                point_size: Some(12.0),
                language: Some(schist_layout::language::TextLanguage::Tag {
                    tag: "en-US".into(),
                }),
                hyphenate: Some(hyphenate),
                align: Some(align),
                ..Default::default()
            };
            let (mut doc, _) = document(0);
            doc.styles.add_paragraph(style);
            let mut story = Story::new();
            for _ in 0..6 {
                story.push_paragraph(long, "Wrapped");
            }
            doc.stories[0] = story.clone();

            let mut inside = doc.clone();
            let rect = Rect::new(120.0, 120.0, 240.0, 240.0);
            obstacle(
                &mut inside,
                ShapeKind::Ellipse,
                rect,
                TextWrap {
                    inverse: true,
                    ..wrap(WrapMode::Contour, 6.0)
                },
            );
            let placed = lines(&inside);
            assert!(placed.len() > 6, "{hyphenate} {align:?}: {}", placed.len());
            let center = Point::new(240.0, 240.0);
            for line in &placed {
                let half = inner_chord(
                    center,
                    120.0 - 6.0 + 0.05,
                    line.bounds.y,
                    line.bounds.bottom(),
                )
                .expect("line inside the circle's height");
                assert!(line.bounds.x >= center.x - half - 0.01, "{:?}", line.bounds);
                assert!(
                    line.bounds.right() <= center.x + half + 0.01,
                    "{:?}",
                    line.bounds
                );
            }

            // A 20pt gap beside an obstacle takes no word; 140pt on its right does.
            let mut beside = doc.clone();
            let rect = Rect::new(60.0, 150.0, 240.0, 120.0);
            obstacle(
                &mut beside,
                ShapeKind::Rectangle,
                rect,
                wrap(WrapMode::BoundingBox, 0.0),
            );
            let placed = lines(&beside);
            assert_clear(&placed, rect);
            assert!(placed
                .iter()
                .all(|l| !(overlaps(vertical(l), (rect.y, rect.bottom())) && l.bounds.x < rect.x)));
            assert!(placed
                .iter()
                .any(|l| overlaps(vertical(l), (rect.y, rect.bottom()))
                    && l.bounds.x >= rect.right()));
            assert!(!flow(&beside).has_overflow());
        }
    }
}

/// An object style's Text Wrap & Other category gives its items its wrap
/// unless they have their own; based-on styles inherit it, a disabled
/// category gives none, and editing a styled item to no wrap keeps that as
/// its own choice.
#[test]
fn object_styles_give_their_wrap_to_items_without_their_own() {
    let rect = Rect::new(190.0, 150.0, 100.0, 100.0);
    let styled = |enable: Option<bool>, local: Option<TextWrap>| {
        let (mut doc, _) = document(4);
        doc.styles.objects.extend([
            schist_layout::ObjectStyle {
                name: "Wraps".into(),
                enable_text_wrap: enable,
                text_wrap: Some(wrap(WrapMode::BoundingBox, 6.0)),
                ..Default::default()
            },
            schist_layout::ObjectStyle {
                name: "Child".into(),
                based_on: Some("Wraps".into()),
                ..Default::default()
            },
        ]);
        let id = obstacle(
            &mut doc,
            ShapeKind::Rectangle,
            rect,
            wrap(WrapMode::None, 0.0),
        );
        let object = object_mut(&mut doc, id);
        object.appearance.text_wrap = local;
        object.appearance.style = Some("Child".into());
        (doc, id)
    };
    let (doc, _) = styled(Some(true), None);
    assert_clear(&lines(&doc), zone(rect, 6.0));
    assert_complete(&doc);
    let (plain, _) = document(4);
    for (enable, local) in [
        (Some(false), None),
        (None, None),
        (Some(true), Some(wrap(WrapMode::None, 0.0))),
    ] {
        let (doc, _) = styled(enable, local);
        assert_eq!(lines(&doc), lines(&plain), "{enable:?}");
    }
    // Editing a styled item to no wrap stores it; back to the style's wrap
    // returns it to inheriting.
    let (mut doc, id) = styled(Some(true), None);
    let mut history = History::default();
    assert!(text_wrap::edit_wrap(&mut doc, &mut history, &[id], |w| {
        w.mode = WrapMode::None
    }));
    let own = doc.object(id).unwrap().appearance.text_wrap.clone();
    assert_eq!(own.map(|w| w.mode), Some(WrapMode::None));
    assert_eq!(lines(&doc), lines(&plain));
    assert!(text_wrap::edit_wrap(&mut doc, &mut history, &[id], |w| {
        *w = wrap(WrapMode::BoundingBox, 6.0)
    }));
    assert!(doc.object(id).unwrap().appearance.text_wrap.is_none());
    assert_clear(&lines(&doc), zone(rect, 6.0));
}
