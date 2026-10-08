//! Auto-sized text frames fit their text about their reference point.
//! InDesign's PDF of the public paged-media `text-autosize` sample grows a
//! 240 × 40 pt HeightOnly frame anchored at its top left, holding twelve
//! one-line 11 pt paragraphs under a centred 1 pt stroke, until its bottom is
//! 0.5 pt below its last baseline: the stroke's reach.
use schist_layout::{
    authoring,
    auto_size::{self, AutoSize, AutoSizing, ReferencePoint},
    blank_a4,
    compose::compose_story,
    History, Ink, LayoutDocument, ObjectId, Paint, Rect, Story, StoryId,
};

const FRAME: Rect = Rect::new(36.0, 60.0, 240.0, 40.0);

fn document(
    lines: usize,
    frame: Rect,
    auto: Option<AutoSize>,
) -> (LayoutDocument, ObjectId, StoryId) {
    let mut doc = blank_a4();
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.family = Some("IBM Plex Sans".into());
    body.point_size = Some(11.0);
    body.leading = Some(schist_layout::styles::Leading::Points(13.2));
    let made = authoring::text_frame(&mut doc, &mut History::default(), 0, frame).unwrap();
    let mut story = Story::new();
    for line in 0..lines {
        story.push_paragraph(format!("Headline line {line}"), "Body");
    }
    doc.stories[made.story.0 as usize] = story;
    let object = doc
        .objects
        .iter_mut()
        .find(|o| o.id == made.object)
        .unwrap();
    object.bounds = frame;
    object.appearance.paint.stroke = Some(Paint::Ink(Ink::black()));
    object.appearance.paint.stroke_width = Some(1.0);
    object.appearance.auto_size = auto;
    (doc, made.object, made.story)
}

fn height_only(reference: ReferencePoint) -> Option<AutoSize> {
    Some(AutoSize {
        sizing: AutoSizing::HeightOnly,
        reference,
        minimum_height: None,
        minimum_width: None,
        no_line_breaks: false,
    })
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: {a} vs {b}");
}

#[test]
fn a_height_only_frame_grows_to_its_last_baseline_and_stroke() {
    let (mut doc, id, story) = document(12, FRAME, height_only(ReferencePoint::TopLeft));
    assert!(
        compose_story(&doc, story).has_overflow(),
        "undersized at first"
    );
    let edits = auto_size::refit(&mut doc);
    assert_eq!(edits.len(), 1);
    let bounds = doc.object(id).unwrap().bounds;
    let composed = compose_story(&doc, story);
    assert!(!composed.has_overflow());
    let lines = &composed.frames[0].lines;
    assert_eq!(lines.len(), 12);
    // The top left stays; the bottom is the stroke's reach below the last
    // baseline, and the text starts that reach in.
    near(bounds.x, FRAME.x, "x");
    near(bounds.y, FRAME.y, "y");
    near(bounds.width, FRAME.width, "width");
    near(bounds.bottom(), lines[11].baseline + 0.5, "bottom");
    near(lines[0].bounds.x, FRAME.x + 0.5, "text inset");
    // Fitted, it stays put.
    assert!(auto_size::refit(&mut doc).is_empty());
}

#[test]
fn frames_shrink_about_their_reference_point() {
    let tall = Rect::new(36.0, 60.0, 240.0, 400.0);
    let fitted = |reference| {
        let (mut doc, id, _) = document(3, tall, height_only(reference));
        auto_size::refit(&mut doc);
        doc.object(id).unwrap().bounds
    };
    let top = fitted(ReferencePoint::Top);
    assert!(top.height < 60.0, "{top:?}");
    near(top.y, tall.y, "top stays");
    let bottom = fitted(ReferencePoint::BottomRight);
    near(bottom.height, top.height, "same fit");
    near(bottom.bottom(), tall.bottom(), "bottom stays");
    let centre = fitted(ReferencePoint::Center);
    near(
        centre.y + centre.height / 2.0,
        tall.y + tall.height / 2.0,
        "centre stays",
    );
}

#[test]
fn a_minimum_height_holds() {
    let (mut doc, id, _) = document(
        1,
        FRAME,
        Some(AutoSize {
            minimum_height: Some(72.0),
            ..height_only(ReferencePoint::TopLeft).unwrap()
        }),
    );
    auto_size::refit(&mut doc);
    near(doc.object(id).unwrap().bounds.height, 72.0, "minimum");
}

#[test]
fn a_width_only_frame_takes_its_longest_line() {
    let (mut doc, id, story) = document(
        3,
        Rect::new(36.0, 60.0, 30.0, 200.0),
        Some(AutoSize {
            sizing: AutoSizing::WidthOnly,
            reference: ReferencePoint::TopLeft,
            minimum_height: None,
            minimum_width: None,
            no_line_breaks: false,
        }),
    );
    auto_size::refit(&mut doc);
    let bounds = doc.object(id).unwrap().bounds;
    near(bounds.height, 200.0, "height kept");
    let composed = compose_story(&doc, story);
    let lines = &composed.frames[0].lines;
    assert_eq!(lines.len(), 3, "no line wraps");
    let longest = lines.iter().map(|l| l.natural_width).fold(0.0, f32::max);
    near(
        bounds.width,
        longest + 1.0,
        "longest line and the stroke's reach",
    );
}

#[test]
fn threaded_proportional_and_empty_frames_keep_their_size() {
    // Proportional fits are kept, not applied.
    let (mut doc, id, _) = document(
        12,
        FRAME,
        Some(AutoSize {
            sizing: AutoSizing::HeightAndWidthProportionally,
            ..height_only(ReferencePoint::TopLeft).unwrap()
        }),
    );
    assert!(auto_size::refit(&mut doc).is_empty());
    assert_eq!(doc.object(id).unwrap().bounds, FRAME);
    // An empty frame has nothing to fit.
    let (mut doc, _, _) = document(0, FRAME, height_only(ReferencePoint::TopLeft));
    assert!(auto_size::refit(&mut doc).is_empty());
    // A threaded frame keeps its size.
    let (mut doc, id, _) = document(12, FRAME, height_only(ReferencePoint::TopLeft));
    let next = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(300.0, 60.0, 240.0, 400.0),
    )
    .unwrap();
    assert!(schist_layout::threading::link(
        &mut doc,
        &mut History::default(),
        id,
        next.object
    ));
    assert!(auto_size::refit(&mut doc).is_empty());
    assert_eq!(doc.object(id).unwrap().bounds, FRAME);
}

#[test]
fn a_fit_undoes_with_the_edit_that_caused_it() {
    let (mut doc, id, story) = document(1, FRAME, height_only(ReferencePoint::TopLeft));
    auto_size::refit(&mut doc);
    let small = doc.object(id).unwrap().bounds;
    let mut history = History::default();
    let text: Vec<String> = (0..12).map(|n| format!("Headline line {n}")).collect();
    assert!(authoring::set_text(
        &mut doc,
        &mut history,
        story,
        text.join("\n")
    ));
    let edits = auto_size::refit(&mut doc);
    assert!(!edits.is_empty());
    history.amend(schist_layout::LayoutEdit::Batch { edits });
    assert!(doc.object(id).unwrap().bounds.height > small.height);
    assert_eq!(history.undo_depth(), 1, "one step");
    assert!(history.undo(&mut doc));
    assert_eq!(doc.object(id).unwrap().bounds, small);
    assert!(!compose_story(&doc, story).has_overflow());
}

/// A frame of `paragraphs` at `size` pt, set at `area` with a centred 1 pt
/// stroke and `auto`, its `columns` 18 pt apart.
fn set(
    paragraphs: &[String],
    size: f32,
    area: Rect,
    columns: u16,
    auto: AutoSize,
) -> (LayoutDocument, ObjectId, StoryId) {
    let (mut doc, id, story) = document(0, area, Some(auto));
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.point_size = Some(size);
    body.leading = Some(schist_layout::styles::Leading::Points(size * 1.2));
    let mut text = Story::new();
    for paragraph in paragraphs {
        text.push_paragraph(paragraph.clone(), "Body");
    }
    doc.stories[story.0 as usize] = text;
    let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
    if let schist_layout::LayoutObject::TextFrame {
        columns: count,
        gutter,
        ..
    } = &mut object.object
    {
        *count = columns;
        *gutter = 18.0;
    }
    (doc, id, story)
}

#[test]
fn a_height_and_width_frame_narrows_to_its_widest_fragment() {
    // The public `layout` sample's centred frame: 200 × 36 pt at (80, 200),
    // eight paragraphs "Centre grow N" at 12 pt. InDesign narrows it to
    // 29.93 pt about its centre, hyphenating "Cen-tre"; unhyphenated here,
    // it narrows to its widest word.
    let paragraphs: Vec<String> = (0..8).map(|n| format!("Centre grow {n}")).collect();
    let area = Rect::new(80.0, 200.0, 200.0, 36.0);
    let (mut doc, id, story) = set(
        &paragraphs,
        12.0,
        area,
        1,
        AutoSize {
            sizing: AutoSizing::HeightAndWidth,
            ..height_only(ReferencePoint::Center).unwrap()
        },
    );
    auto_size::refit(&mut doc);
    let bounds = doc.object(id).unwrap().bounds;
    assert!(bounds.width < 60.0, "narrowed: {bounds:?}");
    assert!(bounds.height > 100.0, "grown: {bounds:?}");
    near(bounds.x + bounds.width / 2.0, 180.0, "centre across");
    near(bounds.y + bounds.height / 2.0, 218.0, "centre down");
    let composed = compose_story(&doc, story);
    assert!(!composed.has_overflow());
    // Every line fits, the widest only just: any narrower and it would not.
    let lines = &composed.frames[0].lines;
    let widest = lines.iter().map(|l| l.natural_width).fold(0.0, f32::max);
    assert!(lines
        .iter()
        .all(|l| l.natural_width <= l.bounds.width + 0.01));
    assert!(
        (bounds.width - 1.0 - widest).abs() < 0.05,
        "{} vs {widest}",
        bounds.width
    );
    // With no line breaks it takes its longest line instead.
    let (mut doc, id, _) = set(
        &paragraphs,
        12.0,
        area,
        1,
        AutoSize {
            sizing: AutoSizing::HeightAndWidth,
            no_line_breaks: true,
            ..height_only(ReferencePoint::Center).unwrap()
        },
    );
    auto_size::refit(&mut doc);
    assert!(doc.object(id).unwrap().bounds.width > 60.0);
}

#[test]
fn two_columns_grow_to_hold_their_lines_side_by_side() {
    // The sample's two-column frame: 460 × 40 pt, twenty one-line 9 pt
    // paragraphs, grown by InDesign to hold ten in each column.
    let paragraphs: Vec<String> = (0..20)
        .map(|n| format!("Paragraph {n} of the growing column copy."))
        .collect();
    let (mut doc, id, story) = set(
        &paragraphs,
        9.0,
        Rect::new(67.0, 80.0, 460.0, 40.0),
        2,
        height_only(ReferencePoint::TopLeft).unwrap(),
    );
    auto_size::refit(&mut doc);
    let bounds = doc.object(id).unwrap().bounds;
    let composed = compose_story(&doc, story);
    assert!(!composed.has_overflow());
    let lines = &composed.frames[0].lines;
    assert_eq!(lines.len(), 20);
    let left = lines.iter().filter(|l| l.bounds.x < 67.0 + 230.0).count();
    assert_eq!((left, lines.len() - left), (10, 10), "{bounds:?}");
    // The shortest such height: its last baseline and the stroke's reach.
    let last = lines.iter().map(|l| l.baseline).fold(0.0, f32::max);
    assert!(
        (bounds.bottom() - last - 0.5).abs() < 0.05,
        "{bounds:?} {last}"
    );
}
