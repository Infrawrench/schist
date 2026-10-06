use schist_layout::anchored::{self, AnchoredItem, AnchoredPosition};
use schist_layout::{
    affine, authoring, authoring::ShapeKind, blank_a4, compose::compose_story, styles::Leading,
    ComposedLine, History, Ink, LayoutDocument, ObjectId, ParagraphStyle, PlacedObject, Point,
    Rect, Story, StoryId, StoryStructure, WritingMode,
};

const BEFORE: &str = "Inline art sits ";
const AFTER: &str = " in the line and moves with the words around it as they wrap.";

/// A filled rectangle or ellipse of `width` × `height`, as an anchored item.
fn item(width: f32, height: f32, y_offset: f32, position: AnchoredPosition) -> AnchoredItem {
    let mut scratch = blank_a4();
    scratch.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    let id = authoring::shape(
        &mut scratch,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, width, height),
        ShapeKind::Rectangle,
        authoring::Paint::filled("Cyan"),
    )
    .unwrap();
    let object = scratch.objects.into_iter().find(|o| o.id == id).unwrap();
    AnchoredItem {
        position,
        y_offset,
        object,
        members: Vec::new(),
        placement: Default::default(),
    }
}

fn anchored(at: usize, item: AnchoredItem) -> StoryStructure {
    StoryStructure {
        at: Some(at),
        kind: "Rectangle".into(),
        payload: "<Rectangle />".into(),
        control: None,
        footnote: None,
        table: None,
        anchored: Some(Box::new(item)),
    }
}

/// A frame holding one paragraph of BEFORE + item + AFTER per item.
fn document(
    items: Vec<AnchoredItem>,
    frame: Rect,
    style: Option<ParagraphStyle>,
) -> (LayoutDocument, ObjectId) {
    let mut doc = blank_a4();
    let id = authoring::text_frame(&mut doc, &mut History::default(), 0, frame).unwrap();
    let name = style.as_ref().map_or("Body".to_owned(), |s| s.name.clone());
    if let Some(style) = style {
        doc.styles.add_paragraph(style);
    }
    let mut story = Story::new();
    for item in items {
        let (start, _) = story.push_paragraph(format!("{BEFORE}{AFTER}"), name.clone());
        story.structures.push(anchored(start + BEFORE.len(), item));
    }
    doc.stories[id.story.0 as usize] = story;
    (doc, id.object)
}

fn lines(doc: &LayoutDocument) -> Vec<ComposedLine> {
    compose_story(doc, StoryId(0)).lines().cloned().collect()
}

fn frame_of(doc: &LayoutDocument, id: ObjectId) -> PlacedObject {
    doc.object(id).unwrap().clone()
}

fn placements(doc: &LayoutDocument, id: ObjectId) -> Vec<PlacedObject> {
    let composed = lines(doc);
    anchored::placements(doc, &doc.stories[0], &frame_of(doc, id), &composed)
}

const FRAME: Rect = Rect::new(40.0, 40.0, 420.0, 500.0);
/// Narrow enough that each paragraph takes several lines.
const NARROW: Rect = Rect::new(40.0, 40.0, 200.0, 500.0);

#[test]
fn an_inline_item_takes_its_width_in_the_line_and_sits_on_the_baseline() {
    let (doc, frame) = document(
        vec![item(30.0, 12.0, 0.0, AnchoredPosition::Inline)],
        FRAME,
        None,
    );
    let composed = lines(&doc);
    let line = composed
        .iter()
        .find(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
        .expect("a line sets the item");
    let [placed] = &placements(&doc, frame)[..] else {
        panic!()
    };
    let bounds = placed.visual_bounds();
    assert!((bounds.width - 30.0).abs() < 0.01);
    assert!((bounds.height - 12.0).abs() < 0.01);
    assert!(
        (bounds.bottom() - line.baseline).abs() < 0.05,
        "{bounds:?} {}",
        line.baseline
    );
    // The text before it ends where the item starts.
    let before = schist_text_engine::measure(&schist_layout::compose::line_spec(
        line,
        &doc.stories[0],
        &doc,
    ))
    .unwrap();
    assert!(bounds.x > line.bounds.x && bounds.right() < line.bounds.x + before.width + 0.5);
    // Source text and anchors are untouched by the projection.
    assert_eq!(doc.stories[0].text(), format!("{BEFORE}{AFTER}"));
    assert!(composed
        .iter()
        .all(|l| l.end <= doc.stories[0].text().len()));
}

#[test]
fn tall_items_raise_their_line_and_offsets_lift_them() {
    let (short, _) = document(
        vec![item(20.0, 8.0, 0.0, AnchoredPosition::Inline)],
        NARROW,
        None,
    );
    let (tall, frame) = document(
        vec![item(20.0, 60.0, 0.0, AnchoredPosition::Inline)],
        NARROW,
        None,
    );
    let first = |doc: &LayoutDocument| lines(doc)[0].clone();
    assert!(first(&tall).baseline > first(&short).baseline + 40.0);
    assert!(lines(&tall)[1].bounds.y > lines(&short)[1].bounds.y + 40.0);
    let [placed] = &placements(&tall, frame)[..] else {
        panic!()
    };
    assert!(placed.visual_bounds().y >= first(&tall).bounds.y - 0.05);
    for lift in [4.0, -3.0] {
        let (doc, frame) = document(
            vec![item(20.0, 8.0, lift, AnchoredPosition::Inline)],
            NARROW,
            None,
        );
        let line = lines(&doc)[0].clone();
        let [placed] = &placements(&doc, frame)[..] else {
            panic!()
        };
        assert!(
            (placed.visual_bounds().bottom() - (line.baseline - lift)).abs() < 0.05,
            "{lift}"
        );
    }
}

/// Native review: Auto leading resolved to points before the engine saw the
/// item, so a tall item past the first line never made room for itself.
#[test]
fn auto_leading_makes_room_for_a_tall_item_and_fixed_leading_keeps_its_step() {
    let step = |leading| {
        let style = ParagraphStyle {
            name: "Lead".into(),
            point_size: Some(12.0),
            leading: Some(leading),
            ..Default::default()
        };
        let (doc, _) = document(
            vec![
                item(20.0, 8.0, 0.0, AnchoredPosition::Inline),
                item(20.0, 60.0, 0.0, AnchoredPosition::Inline),
            ],
            NARROW,
            Some(style),
        );
        let composed = lines(&doc);
        let owner = composed
            .iter()
            .rposition(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
            .unwrap();
        assert!(owner > 1);
        let text = composed[owner + 1].baseline - composed[owner].baseline;
        (
            composed[owner].baseline - composed[owner - 1].baseline,
            text,
        )
    };
    // The item's height plus the text's extra 2.4 pt; the next line steps by
    // the text's own Auto leading.
    let (item, text) = step(Leading::Auto);
    assert!((item - 62.4).abs() < 0.05, "{item}");
    assert!((text - 14.4).abs() < 0.05, "{text}");
    let (item, text) = step(Leading::Points(14.0));
    assert!((item - 14.0).abs() < 0.05 && (text - 14.0).abs() < 0.05);
}

/// InDesign's export of the public paged-media `anchored` sample
/// (corpus/generated/anchored.pdf, pages 1 and 7): 12 pt text under Auto
/// leading with a 60 × 36 pt frame stroked 0.5 pt set inline. Its line steps
/// 38.9 pt from the one above and the next 14.4 pt; the stroked bottom sits on
/// the baseline. A 3 pt bounding-box wrap moves the frame 3 pt right and the
/// following text 6 pt, and leaves the lines where they were.
#[test]
fn inline_items_match_the_public_native_sample() {
    let compose = |wrap: bool| {
        let mut doc = blank_a4();
        doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Lead".into(),
            point_size: Some(12.0),
            leading: Some(Leading::Auto),
            ..Default::default()
        });
        let mut scratch = doc.clone();
        let shape = authoring::shape(
            &mut scratch,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 60.0, 36.0),
            ShapeKind::Rectangle,
            authoring::Paint {
                fill: Some("Cyan".into()),
                stroke: Some("Cyan".into()),
                stroke_width: 0.5,
            },
        )
        .unwrap();
        let mut object = scratch.objects.into_iter().find(|o| o.id == shape).unwrap();
        if wrap {
            object.appearance.text_wrap = Some(schist_layout::text_wrap::TextWrap {
                mode: schist_layout::text_wrap::WrapMode::BoundingBox,
                offsets: schist_layout::geometry::Insets {
                    top: 3.0,
                    right: 3.0,
                    bottom: 3.0,
                    left: 3.0,
                },
                ..Default::default()
            });
        }
        let id = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(67.638, 80.0, 460.0, 720.0),
        )
        .unwrap();
        let mut story = Story::new();
        story.push_paragraph(
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor              incididunt ut labore et dolore magna aliqua.",
            "Lead",
        );
        let (start, _) = story.push_paragraph(format!("{BEFORE}{AFTER}"), "Lead");
        story
            .structures
            .push(anchored(start + BEFORE.len(), AnchoredItem::inline(object)));
        story.push_paragraph("Excepteur sint occaecat cupidatat non proident.", "Lead");
        doc.stories[id.story.0 as usize] = story;
        let composed = lines(&doc);
        let owner = composed
            .iter()
            .position(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
            .unwrap();
        let [placed] = &placements(&doc, id.object)[..] else {
            panic!()
        };
        (composed, owner, placed.visual_bounds())
    };
    let (plain, owner, bounds) = compose(false);
    assert!(owner > 0 && owner + 1 < plain.len());
    let step = |lines: &[ComposedLine], i: usize| lines[i].baseline - lines[i - 1].baseline;
    assert!(
        (step(&plain, owner) - 38.9).abs() < 0.01,
        "{}",
        step(&plain, owner)
    );
    assert!((step(&plain, owner + 1) - 14.4).abs() < 0.01);
    // Geometric bounds: the stroke's outer half reaches the baseline.
    assert!((bounds.height - 36.0).abs() < 0.01 && (bounds.width - 60.0).abs() < 0.01);
    assert!((bounds.bottom() + 0.25 - plain[owner].baseline).abs() < 0.01);
    let (wrapped, again, moved) = compose(true);
    assert_eq!(again, owner);
    assert!(
        (moved.x - bounds.x - 3.0).abs() < 0.01,
        "{moved:?} {bounds:?}"
    );
    assert!((moved.y - bounds.y).abs() < 0.01);
    let width = |lines: &[ComposedLine]| {
        lines[owner].projected.as_ref().unwrap().spec.inline_boxes[0].width
    };
    assert!((width(&wrapped) - width(&plain) - 6.0).abs() < 0.01);
    assert!((wrapped[owner].baseline - plain[owner].baseline).abs() < 0.01);
}

#[test]
fn an_item_wider_than_the_room_left_moves_to_the_next_line_whole() {
    let narrow = Rect::new(40.0, 40.0, 150.0, 500.0);
    let (doc, frame) = document(
        vec![item(120.0, 10.0, 0.0, AnchoredPosition::Inline)],
        narrow,
        None,
    );
    let composed = lines(&doc);
    let owner = composed
        .iter()
        .position(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
        .unwrap();
    assert!(owner > 0);
    let [placed] = &placements(&doc, frame)[..] else {
        panic!()
    };
    let bounds = placed.visual_bounds();
    assert!(bounds.x >= narrow.x - 0.05 && bounds.right() <= narrow.right() + 0.05);
}

#[test]
fn items_follow_their_frames_affine() {
    for rotation in [0.0, 17.0, -90.0] {
        let (mut doc, frame) = document(
            vec![item(30.0, 12.0, 0.0, AnchoredPosition::Inline)],
            FRAME,
            None,
        );
        doc.objects
            .iter_mut()
            .find(|o| o.id == frame)
            .unwrap()
            .rotation = rotation;
        let composed = lines(&doc);
        let line = composed
            .iter()
            .find(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
            .unwrap()
            .clone();
        let host = frame_of(&doc, frame);
        let [placed] = &placements(&doc, frame)[..] else {
            panic!()
        };
        // Unrotated, the item's box sits in the frame's own coordinates;
        // every corner then follows the frame's map.
        let (upright, _) = document(
            vec![item(30.0, 12.0, 0.0, AnchoredPosition::Inline)],
            FRAME,
            None,
        );
        let [reference] = &placements(&upright, upright.objects[0].id)[..] else {
            panic!()
        };
        let expected =
            affine::corners(reference.bounds).map(|p| affine::point(host.content_transform(), p));
        let actual =
            affine::corners(placed.bounds).map(|p| affine::point(placed.content_transform(), p));
        for (a, b) in actual.iter().zip(&expected) {
            assert!(
                (a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() < 0.05,
                "{rotation}: {a:?} {b:?}"
            );
        }
        let _ = line;
        assert_eq!(placed.page, host.page);
    }
}

#[test]
fn every_position_renders_and_vertical_text_stays_unrendered() {
    for position in [AnchoredPosition::AboveLine, AnchoredPosition::Anchored] {
        let (doc, frame) = document(vec![item(30.0, 12.0, 0.0, position)], FRAME, None);
        let flow = compose_story(&doc, StoryId(0));
        assert_eq!(flow.frames[0].unrendered_structures, 0, "{position:?}");
        assert_eq!(placements(&doc, frame).len(), 1);
    }
    let vertical = ParagraphStyle {
        name: "Vertical".into(),
        writing_mode: Some(WritingMode::VerticalRightToLeft),
        ..Default::default()
    };
    let (doc, frame) = document(
        vec![item(30.0, 12.0, 0.0, AnchoredPosition::Inline)],
        FRAME,
        Some(vertical),
    );
    assert_eq!(
        compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
        1
    );
    assert!(placements(&doc, frame).is_empty());
    // Inline items in horizontal text count as rendered.
    let (doc, _) = document(
        vec![
            item(30.0, 12.0, 0.0, AnchoredPosition::Inline),
            item(10.0, 40.0, 2.0, AnchoredPosition::Inline),
        ],
        FRAME,
        None,
    );
    assert_eq!(
        compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
        0
    );
}

#[test]
fn the_preview_draws_items_as_part_of_their_frame() {
    let (doc, frame) = document(
        vec![item(30.0, 12.0, 0.0, AnchoredPosition::Inline)],
        FRAME,
        None,
    );
    let plan = schist_layout::pasteboard(
        &doc,
        &schist_layout::PasteboardView {
            scale: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let shapes: Vec<_> = plan
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .filter(|d| matches!(d, schist_layout::Display::Shape { object, .. } if *object == frame))
        .collect();
    assert_eq!(shapes.len(), 1);
    // Display specs are scaled to the view, boxes with the text.
    for scale in [0.48, 2.0] {
        let plan = schist_layout::pasteboard(
            &doc,
            &schist_layout::PasteboardView {
                scale,
                ..Default::default()
            },
        )
        .unwrap();
        let boxed: Vec<_> = plan
            .pages
            .iter()
            .flat_map(|p| &p.objects)
            .filter_map(|d| match d {
                schist_layout::Display::Text { spec, .. } if !spec.inline_boxes.is_empty() => {
                    Some(spec.inline_boxes[0])
                }
                _ => None,
            })
            .collect();
        assert_eq!(boxed.len(), 1);
        assert!((boxed[0].width - 30.0 * scale).abs() < 0.01, "{scale}");
        assert!((boxed[0].ascent - 12.0 * scale).abs() < 0.01, "{scale}");
    }
    let _ = Point::ZERO;
}

#[test]
fn a_groups_members_move_together_and_its_extent_spans_them() {
    let a = item(20.0, 10.0, 0.0, AnchoredPosition::Inline).object;
    let mut b = item(20.0, 30.0, 0.0, AnchoredPosition::Inline).object;
    b.bounds.x += 30.0;
    b.bounds.y += 5.0;
    let mut container = a.clone();
    container.object = schist_layout::LayoutObject::Group {
        children: Vec::new(),
    };
    container.bounds = a.bounds.union(b.bounds);
    let group = AnchoredItem {
        members: vec![a, b],
        ..AnchoredItem::inline(container)
    };
    let extent = group.extent();
    assert!((extent.width - 50.0).abs() < 0.01 && (extent.height - 35.0).abs() < 0.01);
    let (doc, frame) = document(vec![group], FRAME, None);
    let composed = lines(&doc);
    let line = composed
        .iter()
        .find(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
        .unwrap();
    let [first, second] = &placements(&doc, frame)[..] else {
        panic!()
    };
    let (p, q) = (first.visual_bounds(), second.visual_bounds());
    assert!((q.x - p.x - 30.0).abs() < 0.01 && (q.y - p.y - 5.0).abs() < 0.01);
    // The group's lowest edge sits on the baseline.
    assert!((p.bottom().max(q.bottom()) - line.baseline).abs() < 0.05);
    // A hidden member is not drawn but still counts in the extent.
    let mut hidden = doc.clone();
    let item = hidden.stories[0].structures[0].anchored.as_mut().unwrap();
    item.members[1].hidden = true;
    assert_eq!(placements(&hidden, frame).len(), 1);
}
