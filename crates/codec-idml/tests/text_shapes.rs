use schist_codec_idml::{export, import};
use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, compose::compose_story, text_shape, History,
    LayoutDocument, Rect, ShapePath, Story, StoryId,
};

const TEXT: &str = "A shaped frame saved and reopened composes the same lines inside the same \
outline, because the outline travels as the frame's own path geometry.";

fn document(path: Option<ShapePath>) -> LayoutDocument {
    let mut doc = blank_a4();
    let mut history = History::default();
    let id = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(80.0, 90.0, 320.0, 240.0),
        ShapeKind::Ellipse,
        authoring::Paint::none(),
    )
    .unwrap();
    let frame = text_shape::attach(&mut doc, &mut history, id).unwrap();
    if let Some(path) = path {
        doc.objects[0].appearance.outline = Some(path);
    }
    let mut story = Story::new();
    for _ in 0..5 {
        story.push_paragraph(TEXT, "Body");
    }
    doc.stories[frame.story.0 as usize] = story;
    doc
}

fn bounds(doc: &LayoutDocument) -> Vec<(usize, usize, Rect)> {
    compose_story(doc, StoryId(0))
        .lines()
        .map(|l| (l.start, l.end, l.bounds))
        .collect()
}

fn close(a: &[(usize, usize, Rect)], b: &[(usize, usize, Rect)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.0 == y.0
                && x.1 == y.1
                && (x.2.x - y.2.x).abs() < 0.01
                && (x.2.y - y.2.y).abs() < 0.01
                && (x.2.width - y.2.width).abs() < 0.01
        })
}

/// An ellipse traced the other way round: under the nonzero rule it is a hole
/// in an outline traced forwards.
fn reversed(mut path: ShapePath) -> ShapePath {
    for sub in &mut path.subpaths {
        sub.points.reverse();
        sub.handles.reverse();
        for handles in &mut sub.handles {
            std::mem::swap(&mut handles.incoming, &mut handles.outgoing);
        }
    }
    path
}

fn ring(even_odd: bool) -> ShapePath {
    let mut ring = ShapePath::ellipse(1.0, 1.0);
    let mut hole = ShapePath::ellipse(0.3, 0.3);
    hole.map_points(|p| schist_layout::Point::new(p.x + 0.35, p.y + 0.35));
    if !even_odd {
        hole = reversed(hole);
    }
    ring.subpaths.extend(hole.subpaths);
    ring.even_odd = even_odd;
    ring
}

#[test]
fn shaped_frames_compose_the_same_lines_after_repeated_saves() {
    for path in [None, Some(ring(false))] {
        let doc = document(path);
        let expected = bounds(&doc);
        assert!(!expected.is_empty());
        let mut current = doc.clone();
        for _ in 0..2 {
            let written = export::write(&current);
            assert!(
                written
                    .warnings
                    .iter()
                    .all(|w| !w.contains(&current.objects[0].name)),
                "{:?}",
                written.warnings
            );
            let imported = import::read(&written.bytes).unwrap();
            current = imported.document;
            assert!(current.objects[0].appearance.outline.is_some());
            assert!(close(&bounds(&current), &expected));
        }
    }
    // An even-odd hole composes like the nonzero one, but saving it warns: the
    // native frame geometry cannot carry the rule.
    let even_odd = document(Some(ring(true)));
    assert!(close(
        &bounds(&even_odd),
        &bounds(&document(Some(ring(false))))
    ));
    let written = export::write(&even_odd);
    assert!(written.warnings.contains(&schist_i18n::tf!(
        "design.idml_even_odd",
        name = even_odd.objects[0].name
    )));
}
