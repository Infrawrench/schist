use schist_layout::{
    inline_text::{Insertion, Projection},
    Story, StoryPoint, StoryStructure, StyleRange,
};

#[test]
fn generated_inline_text_preserves_every_source_boundary_and_style() {
    for source in ["", "é界 🦀", "é\n\n界\n", "abc def"] {
        let mut original = Story::new();
        for paragraph in source.split('\n') {
            original.push_paragraph(paragraph, "Body");
        }
        let boundaries: Vec<_> = source
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(source.len()))
            .collect();
        for at in &boundaries {
            let mut story = original.clone();
            story.ranges = vec![StyleRange::new(0, source.len(), "Local")];
            story.structures.push(StoryStructure {
                control: None,
                at: Some(*at),
                kind: "Footnote".into(),
                payload: "raw".into(),
                footnote: None,
            });
            let before = story.clone();
            let projected = Projection::new(
                &story,
                ["4", "❖", "９"]
                    .into_iter()
                    .map(|text| Insertion {
                        at: *at,
                        text: text.into(),
                        style: format!("Marker-{text}"),
                    })
                    .collect(),
            )
            .unwrap();
            assert_eq!(story, before);
            let text = projected.story.text();
            assert_eq!(text, format!("{}4❖９{}", &source[..*at], &source[*at..]));
            for boundary in &boundaries {
                assert_eq!(
                    projected
                        .positions
                        .source(projected.positions.before(*boundary)),
                    *boundary
                );
                assert_eq!(
                    projected
                        .positions
                        .source(projected.positions.after(*boundary)),
                    *boundary
                );
            }
            for byte in 0..=text.len() + 2 {
                assert!(source.is_char_boundary(projected.positions.source(byte)));
            }
            for span in &projected.positions.generated {
                for byte in span.start..=span.end {
                    assert_eq!(projected.positions.source(byte), *at);
                }
            }
            let generated_range = projected.positions.before(*at)..projected.positions.after(*at);
            for (byte, _) in text.char_indices() {
                let style = projected
                    .story
                    .ranges
                    .iter()
                    .find(|r| r.start <= byte && byte < r.end)
                    .unwrap();
                assert_eq!(
                    style.style.starts_with("Marker-"),
                    generated_range.contains(&byte)
                );
            }
            assert_eq!(
                projected.story.structures[0].at,
                Some(projected.positions.after(*at))
            );
            for start in text
                .char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(text.len()))
            {
                let line = projected.positions.line(&text, start, text.len()).unwrap();
                for visual in 0..=text.len() - start {
                    assert_eq!(
                        line.source(visual),
                        projected.positions.source(start + visual)
                    );
                }
                for boundary in &boundaries {
                    let visual = line.visual(*boundary);
                    assert!(text[start..].is_char_boundary(visual));
                    if *boundary >= projected.positions.source(start) {
                        assert_eq!(line.source(visual), *boundary);
                    }
                }
            }
        }
    }
}

#[test]
fn projection_keeps_breaks_and_orders_coincident_insertions_without_guessing_anchors() {
    let source = Story {
        points: vec![
            StoryPoint::Paragraph {
                text: "é".into(),
                style: "A".into(),
            },
            StoryPoint::FrameBreak,
            StoryPoint::Paragraph {
                text: "界".into(),
                style: "B".into(),
            },
        ],
        ..Default::default()
    };
    let insertion = |at, text: &str| Insertion {
        at,
        text: text.into(),
        style: "Number".into(),
    };
    let projected = Projection::new(
        &source,
        vec![insertion(2, "2"), insertion(0, "1"), insertion(2, "3")],
    )
    .unwrap();
    assert_eq!(projected.story.points[0].text(), "1é23");
    assert_eq!(projected.story.points[1], StoryPoint::FrameBreak);
    assert_eq!(projected.story.points[2], source.points[2]);
    for at in [1, 4, usize::MAX] {
        assert!(Projection::new(&source, vec![insertion(at, "1")]).is_none());
    }
    for text in ["\n", "\r", "a\nb"] {
        assert!(Projection::new(&source, vec![insertion(0, text)]).is_none());
    }
    let mut invalid = source.clone();
    invalid.ranges.push(StyleRange::new(1, 2, "Broken"));
    assert!(Projection::new(&invalid, vec![]).is_none());
    assert!(Projection::new(&Story::new(), vec![insertion(0, "1")]).is_none());
}
