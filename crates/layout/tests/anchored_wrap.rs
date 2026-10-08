//! Items at custom positions wrap the lines after their anchor's line, as
//! InDesign's help and public tutorials describe: never that line, nor the
//! lines before it.
use schist_layout::anchored::{
    AnchorPoint, AnchoredItem, AnchoredPosition, HorizontalAlignment, HorizontalReference,
    Placement, VerticalAlignment, VerticalReference,
};
use schist_layout::{
    authoring,
    authoring::ShapeKind,
    blank_a4,
    compose::compose_story,
    geometry::Insets,
    text_wrap::{TextWrap, WrapMode},
    ComposedLine, History, Ink, LayoutDocument, Rect, Story, StoryId, StoryStructure,
};

const FRAME: Rect = Rect::new(40.0, 40.0, 300.0, 600.0);
const WORDS: &str = "Words fill this frame line after line so that the wrap can be seen \
                     beside the anchored item as the story flows past it and on below it. ";

/// A story with an item anchored mid-paragraph at the frame's left edge, its
/// top on the anchor line's baseline, 80 × 100 pt, wrapping with `mode`.
fn document(mode: Option<WrapMode>) -> LayoutDocument {
    let mut doc = blank_a4();
    doc.inks.push(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0]));
    let mut scratch = doc.clone();
    let shape = authoring::shape(
        &mut scratch,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 80.0, 100.0),
        ShapeKind::Rectangle,
        authoring::Paint::filled("Cyan"),
    )
    .unwrap();
    let mut object = scratch.objects.into_iter().find(|o| o.id == shape).unwrap();
    object.appearance.text_wrap = mode.map(|mode| TextWrap {
        mode,
        offsets: Insets {
            top: 0.0,
            right: 6.0,
            bottom: 6.0,
            left: 0.0,
        },
        ..Default::default()
    });
    let frame = authoring::text_frame(&mut doc, &mut History::default(), 0, FRAME).unwrap();
    let before = WORDS.repeat(2);
    let mut story = Story::from_text(format!("{before}{}", WORDS.repeat(6)), "Body");
    story.structures.push(StoryStructure {
        at: Some(before.len()),
        kind: "Rectangle".into(),
        payload: "<Rectangle />".into(),
        control: None,
        footnote: None,
        table: None,
        anchored: Some(Box::new(AnchoredItem {
            position: AnchoredPosition::Anchored,
            y_offset: 0.0,
            placement: Placement {
                anchor_point: AnchorPoint::TopLeft,
                horizontal_reference: HorizontalReference::TextFrame,
                horizontal_alignment: HorizontalAlignment::Left,
                vertical_reference: VerticalReference::LineBaseline,
                vertical_alignment: VerticalAlignment::Top,
                ..Default::default()
            },
            object,
            members: Vec::new(),
        })),
    });
    doc.stories[frame.story.0 as usize] = story;
    doc
}

fn lines(doc: &LayoutDocument) -> (Vec<ComposedLine>, usize) {
    let lines: Vec<_> = compose_story(doc, StoryId(0)).lines().cloned().collect();
    let owner = lines
        .iter()
        .position(|l| l.projected.as_ref().is_some_and(|p| !p.anchored.is_empty()))
        .unwrap();
    (lines, owner)
}

#[test]
fn only_the_lines_after_the_anchor_line_wrap_around_the_item() {
    let doc = document(Some(WrapMode::BoundingBox));
    let (lines, owner) = lines(&doc);
    assert!(owner > 1);
    let item_bottom = lines[owner].baseline + 100.0 + 6.0;
    // The anchor's line and every line before it keep the whole measure.
    for line in &lines[..=owner] {
        assert!((line.bounds.x - FRAME.x).abs() < 0.01, "{:?}", line.bounds);
    }
    // Lines beside the item start after it and its offset.
    let beside: Vec<_> = lines[owner + 1..]
        .iter()
        .filter(|l| l.bounds.y < item_bottom)
        .collect();
    assert!(beside.len() >= 3);
    for line in &beside {
        assert!(line.bounds.x >= FRAME.x + 86.0 - 0.01, "{:?}", line.bounds);
    }
    // Below it, the measure is whole again.
    let below: Vec<_> = lines[owner + 1..]
        .iter()
        .filter(|l| l.bounds.y >= item_bottom)
        .collect();
    assert!(!below.is_empty());
    for line in below {
        assert!((line.bounds.x - FRAME.x).abs() < 0.01, "{:?}", line.bounds);
    }
    let flow = compose_story(&doc, StoryId(0));
    assert!(!flow.frames[0].wrap.ignored);
    // Composition settles: a second pass is identical.
    assert_eq!(lines, self::lines(&doc).0);
}

#[test]
fn without_wrap_nothing_moves_and_a_jump_skips_past_the_item() {
    let (plain, owner) = lines(&document(None));
    assert!(plain.iter().all(|l| (l.bounds.x - FRAME.x).abs() < 0.01));
    let (jumped, again) = lines(&document(Some(WrapMode::JumpObject)));
    assert_eq!(again, owner);
    assert_eq!(jumped[..=owner], plain[..=owner]);
    let item_bottom = jumped[owner].baseline + 100.0 + 6.0;
    assert!(jumped[owner + 1].bounds.y >= item_bottom - 0.01);
}
