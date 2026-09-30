use schist_layout::{
    blank_a4,
    compose::{compose_thread, InsetsLike},
    FrameOverflow, ObjectId, ParagraphStyle, Rect, Story,
};

#[test]
fn a_completely_empty_story_has_no_overset_text_even_in_a_short_frame() {
    let mut doc = blank_a4();
    let id = doc.add_story(Story::from_text("", "Body"));
    for height in [1.0, 10.0, 30.0] {
        let thread = compose_thread(
            &doc,
            id,
            &[(
                ObjectId::next(),
                Rect::new(0.0, 0.0, 100.0, height),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            )],
        );
        assert!(!thread.has_overflow());
        assert!(thread.frames.iter().all(|f| f.consumed_to == 0));
    }
}

#[test]
fn every_empty_paragraph_including_the_terminal_one_reserves_a_line() {
    for first in ["", "text"] {
        for count in 1..6 {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Plain".into(),
                point_size: Some(11.0),
                leading: Some(14.0),
                keep_lines: Some(1),
                ..Default::default()
            });
            let mut story = Story::from_text(first, "Plain");
            for _ in 0..count {
                story.push_paragraph("", "Plain");
            }
            let offsets = story.point_offsets();
            let end = story.text_len();
            let id = doc.add_story(story);
            for height in [14.01, 1000.0] {
                let frames: Vec<_> = (0..count + 1)
                    .map(|_| {
                        (
                            ObjectId::next(),
                            Rect::new(0.0, 0.0, 150.0, height),
                            FrameOverflow::Thread,
                            1,
                            0.0,
                            InsetsLike::default(),
                        )
                    })
                    .collect();
                let thread = compose_thread(&doc, id, &frames);
                assert!(!thread.has_overflow());
                assert_eq!(thread.lines().map(|l| l.start).collect::<Vec<_>>(), offsets);
                assert!(thread.lines().all(|l| l.start <= l.end
                    && l.end <= end
                    && (l.bounds.height - 14.0).abs() < 0.001));
                assert!(thread.frames.iter().all(|f| f.consumed_to <= end));
                if height < 15.0 {
                    assert!(thread.frames.iter().all(|f| f.lines.len() == 1));
                }
            }
            let short = compose_thread(
                &doc,
                id,
                &[(
                    ObjectId::next(),
                    Rect::new(0.0, 0.0, 150.0, count as f32 * 14.0 + 0.01),
                    FrameOverflow::Clip,
                    1,
                    0.0,
                    InsetsLike::default(),
                )],
            );
            assert!(
                short.has_overflow(),
                "the final empty paragraph still needs room"
            );
        }
    }
}
