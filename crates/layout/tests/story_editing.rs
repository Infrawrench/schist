use schist_layout::{Story, StoryPoint, StyleRange};

#[test]
fn every_utf8_selection_can_be_replaced_deleted_or_split_without_corrupting_ranges() {
    let mut story = Story::from_text("héllö", "Heading");
    story.push_paragraph("世界 tail", "Body");
    let original = story.text();
    story.ranges = vec![
        StyleRange::new(0, 3, "Bold"),
        StyleRange::new(8, 14, "Italic"),
    ];
    let boundaries: Vec<_> = original
        .char_indices()
        .map(|(i, _)| i)
        .chain([original.len()])
        .collect();
    for start in &boundaries {
        for end in boundaries.iter().filter(|end| *end >= start) {
            for inserted in ["", "é", "漢字\nline"] {
                let mut expected = original.clone();
                expected.replace_range(*start..*end, inserted);
                let edited = story.replace_text(*start..*end, inserted, "Body").unwrap();
                assert_eq!(edited.text(), expected, "{start}..{end} / {inserted}");
                for range in &edited.ranges {
                    assert!(range.start < range.end && range.end <= expected.len());
                    assert!(
                        expected.is_char_boundary(range.start)
                            && expected.is_char_boundary(range.end)
                    );
                }
                for pair in edited.ranges.windows(2) {
                    assert!(pair[0].end <= pair[1].start);
                }
            }
        }
    }
}

#[test]
fn typing_keeps_unedited_paragraph_styles_and_structural_points() {
    let story = Story {
        prefs: Default::default(),
        structures: Vec::new(),
        points: vec![
            StoryPoint::Paragraph {
                text: "Title".into(),
                style: "Heading".into(),
            },
            StoryPoint::FrameBreak,
            StoryPoint::Other {
                kind: "Anchor".into(),
                payload: "preserve".into(),
            },
            StoryPoint::Paragraph {
                text: "Body".into(),
                style: "Body style".into(),
            },
        ],
        ranges: vec![StyleRange::new(6, 10, "Italic")],
    };
    let edited = story.replace_text(7..8, "é", "Default").unwrap();
    assert_eq!(edited.points[..3], story.points[..3]);
    assert!(
        matches!(&edited.points[3],StoryPoint::Paragraph {text,style} if text=="Bédy" && style=="Body style")
    );
    assert!(
        story.replace_text(0..10, "", "Default").is_none(),
        "unsupported structure must not disappear"
    );
}

#[test]
fn deleting_the_last_character_really_empties_the_story_and_undo_restores_it() {
    use schist_layout::{authoring, blank_a4, History, Rect};
    let mut doc = blank_a4();
    let mut history = History::default();
    let frame =
        authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0)).unwrap();
    authoring::set_text(&mut doc, &mut history, frame.story, "é");
    let before = doc.clone();
    assert!(authoring::set_text(&mut doc, &mut history, frame.story, ""));
    assert_eq!(doc.story(frame.story).unwrap().text(), "");
    history.undo(&mut doc);
    assert_eq!(doc, before);
}
