//! A text frame's stroke moves its text, as InDesign's PDF of the public
//! paged-media `stroke-inset` sample measures on 200 × 66 pt frames of 10/12
//! text alternating left and right: the text area shrinks on every side by
//! half a centred stroke's weight, an inside stroke's whole weight, nothing
//! for an outside stroke, and nothing for a stroke with no colour, on top of
//! the frame's InsetSpacing. The first baseline moves down the same amount,
//! and the stroke decides whether the last line still fits.
use schist_layout::{
    authoring, blank_a4, compose::compose_story, History, Ink, Insets, LayoutDocument,
    LayoutObject, Paint, Point, Rect, ShapePath, Story, StoryId, StrokeAlignment, SubPath,
};

const WIDTH: f32 = 200.0;
const HEIGHT: f32 = 66.0;
const AT: Point = Point::new(72.0, 60.0);

type Stroke = Option<(f32, StrokeAlignment, bool)>;

fn document(
    stroke: Stroke,
    insets: Insets,
    height: f32,
    lines: usize,
) -> (LayoutDocument, StoryId) {
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
    body.point_size = Some(10.0);
    body.leading = Some(schist_layout::styles::Leading::Points(12.0));
    let mut right = body.clone();
    right.name = "Right".into();
    right.align = Some(schist_layout::styles::Align::Right);
    doc.styles.paragraphs.push(right);
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(AT.x, AT.y, WIDTH, height),
    )
    .unwrap();
    let mut story = Story::new();
    for (index, word) in ["One", "Two", "Three", "Four", "Five", "Six"]
        .iter()
        .take(lines)
        .enumerate()
    {
        if index % 2 == 0 {
            story.push_paragraph(format!("{word} left"), "Body");
        } else {
            story.push_paragraph(format!("{word} right"), "Right");
        }
    }
    doc.stories[frame.story.0 as usize] = story;
    let object = doc
        .objects
        .iter_mut()
        .find(|o| o.id == frame.object)
        .unwrap();
    object.bounds.height = height;
    if let LayoutObject::TextFrame { insets: own, .. } = &mut object.object {
        *own = insets;
    }
    if let Some((width, alignment, visible)) = stroke {
        object.appearance.paint.stroke = Some(if visible {
            Paint::Ink(Ink::black())
        } else {
            Paint::None
        });
        object.appearance.paint.stroke_width = Some(width);
        object.appearance.paint.stroke_alignment = Some(alignment);
    }
    (doc, frame.story)
}

/// Each line's left edge, right edge and baseline, relative to the frame.
fn lines(doc: &LayoutDocument, story: StoryId) -> Vec<(f32, f32, f32)> {
    compose_story(doc, story).frames[0]
        .lines
        .iter()
        .map(|line| {
            (
                line.bounds.x - AT.x,
                line.bounds.right() - AT.x,
                line.baseline - AT.y,
            )
        })
        .collect()
}

fn near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 0.01, "{what}: {a} vs {b}");
}

/// The first baseline of an unstroked frame.
fn first_baseline() -> f32 {
    let (doc, story) = document(None, Insets::ZERO, HEIGHT, 2);
    lines(&doc, story)[0].2
}

#[test]
fn a_frames_stroke_moves_its_text_in() {
    let base = first_baseline();
    for (stroke, inset) in [
        (Some((0.25, StrokeAlignment::Center, true)), 0.125),
        (Some((1.0, StrokeAlignment::Center, true)), 0.5),
        (Some((1.0, StrokeAlignment::Inside, true)), 1.0),
        (Some((6.0, StrokeAlignment::Center, true)), 3.0),
        (Some((6.0, StrokeAlignment::Inside, true)), 6.0),
        (Some((12.0, StrokeAlignment::Outside, true)), 0.0),
        // A weight with no colour draws nothing and moves nothing.
        (Some((6.0, StrokeAlignment::Inside, false)), 0.0),
    ] {
        let (doc, story) = document(stroke, Insets::ZERO, HEIGHT, 2);
        let set = lines(&doc, story);
        let what = format!("{stroke:?}");
        near(set[0].0, inset, &what);
        near(set[0].2, base + inset, &what);
        near(set[1].1, WIDTH - inset, &what);
    }
}

#[test]
fn insets_add_to_the_strokes_reach() {
    let base = first_baseline();
    // InDesign's case "inset 2/8/6/10 + 1 centre": left 8.5, right 189.5.
    let (doc, story) = document(
        Some((1.0, StrokeAlignment::Center, true)),
        Insets::new(2.0, 10.0, 6.0, 8.0),
        HEIGHT,
        2,
    );
    let set = lines(&doc, story);
    near(set[0].0, 8.5, "left");
    near(set[1].1, 189.5, "right");
    near(set[0].2, base + 2.5, "baseline");
    // "inset 4 + 6 inside": 10 in.
    let (doc, story) = document(
        Some((6.0, StrokeAlignment::Inside, true)),
        Insets::uniform(4.0),
        HEIGHT,
        2,
    );
    near(lines(&doc, story)[0].0, 10.0, "inside");
}

#[test]
fn the_stroke_decides_whether_the_last_line_fits() {
    // InDesign's "fit 60.3" frames hold five 12 pt lines with 0.3 pt to
    // spare unstroked and under a 0.25 pt centred stroke, which takes 0.25 pt
    // of the frame's height, but four under a 1 pt centred or a 0.25 pt
    // inside stroke. InDesign fits a line by its baseline and Schist by its
    // descent, so the frames here leave the same 0.3 pt past Schist's fit.
    let (tall, story) = document(None, Insets::ZERO, 120.0, 5);
    let fit = compose_story(&tall, story).frames[0].lines[4]
        .bounds
        .bottom()
        - AT.y;
    for (stroke, count) in [
        (None, 5),
        (Some((0.25, StrokeAlignment::Center, true)), 5),
        (Some((1.0, StrokeAlignment::Center, true)), 4),
        (Some((0.25, StrokeAlignment::Inside, true)), 4),
    ] {
        let (doc, story) = document(stroke, Insets::ZERO, fit + 0.3, 6);
        assert_eq!(lines(&doc, story).len(), count, "{stroke:?}");
    }
}

#[test]
fn a_shaped_frame_offsets_its_outline() {
    // A 200 × 66 pt frame with its top-left corner cut by a 30 pt chamfer.
    let outline = ShapePath {
        subpaths: vec![SubPath {
            points: [
                (30.0, 0.0),
                (200.0, 0.0),
                (200.0, 66.0),
                (0.0, 66.0),
                (0.0, 30.0),
            ]
            .iter()
            .map(|(x, y)| Point::new(x / WIDTH, y / HEIGHT))
            .collect(),
            closed: true,
            handles: Vec::new(),
        }],
        even_odd: false,
    };
    // The edge runs x = 30 − y; offset d inward it runs √2·d further in, and
    // a line takes it at its top. InDesign's PDF sets the first and third
    // lines at 30 and 6 pt unstroked, 31 and 7 under a 6 pt centred stroke,
    // 31 and 7 inset 4 pt and 32 and 8 inset 4 pt under that stroke: each
    // of these rounded down to a whole point.
    for (stroke, inset, indesign) in [
        (None, 0.0, [30.0, 6.0]),
        (Some((6.0, StrokeAlignment::Center, true)), 0.0, [31.0, 7.0]),
        (None, 4.0, [31.0, 7.0]),
        (Some((6.0, StrokeAlignment::Center, true)), 4.0, [32.0, 8.0]),
    ] {
        let (mut doc, story) = document(stroke, Insets::uniform(inset), HEIGHT, 3);
        let object = doc
            .objects
            .iter_mut()
            .find(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
            .unwrap();
        object.appearance.outline = Some(outline.clone());
        let set = lines(&doc, story);
        let offset = inset + stroke.map_or(0.0, |(width, _, _)| width / 2.0);
        for (line, whole) in [(0, indesign[0]), (2, indesign[1])] {
            let top = offset + 12.0 * line as f32;
            let edge = 30.0 - top + std::f32::consts::SQRT_2 * offset;
            let what = format!("{stroke:?} inset {inset}, line {line}");
            assert!(
                (set[line].0 - edge).abs() < 0.05,
                "{what}: {} vs {edge}",
                set[line].0
            );
            assert!(
                set[line].0 > whole - 0.01 && set[line].0 < whole + 1.0,
                "{what}"
            );
        }
    }
}
