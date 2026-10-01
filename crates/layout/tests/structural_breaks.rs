use schist_layout::{
    authoring, blank_a4, compose, FrameOverflow, History, LayoutObject, Page, Rect, Story,
    StoryPoint,
};

#[test]
fn every_structural_break_preserves_the_next_unicode_character_and_destination() {
    for columns in [1, 2, 4] {
        for kind in [
            StoryPoint::ColumnBreak,
            StoryPoint::FrameBreak,
            StoryPoint::PageBreak,
        ] {
            for text in ["é after", "中 after", "😀 after"] {
                let mut doc = blank_a4();
                doc.pages.push(Page::a4());
                let mut history = History::default();
                let mut ids = Vec::new();
                let mut sid = None;
                for page in [0, 0, 1] {
                    let created = authoring::text_frame(
                        &mut doc,
                        &mut history,
                        page,
                        Rect::new(0.0, 0.0, 500.0, 700.0),
                    )
                    .unwrap();
                    let shared = *sid.get_or_insert(created.story);
                    let placed = doc
                        .objects
                        .iter_mut()
                        .find(|object| object.id == created.object)
                        .unwrap();
                    if let LayoutObject::TextFrame {
                        story,
                        columns: count,
                        overflow,
                        ..
                    } = &mut placed.object
                    {
                        *story = shared;
                        *count = columns;
                        *overflow = FrameOverflow::Thread;
                    }
                    ids.push(created.object);
                }
                let sid = sid.unwrap();
                let mut story = Story::from_text("Before", "Body");
                story.points.push(kind.clone());
                let (start, _) = story.push_paragraph(text, "Body");
                doc.stories[sid.0 as usize] = story;
                doc.thread_order = vec![(sid, ids.clone())];
                let result = compose::compose_story(&doc, sid);
                assert!(!result.has_overflow());
                let target = match kind {
                    StoryPoint::ColumnBreak if columns > 1 => 0,
                    StoryPoint::ColumnBreak | StoryPoint::FrameBreak => 1,
                    _ => 2,
                };
                let line = result.frames[target]
                    .lines
                    .iter()
                    .find(|line| line.start >= start)
                    .unwrap();
                assert_eq!(line.start, start);
                assert_eq!(
                    doc.stories[sid.0 as usize].slice(line.start, line.end),
                    text
                );
                if target == 0 {
                    assert!(line.bounds.x > 0.0);
                }
                if target == 2 {
                    assert!(result.frames[1].lines.is_empty());
                }
                let recovered: String = result
                    .lines()
                    .map(|line| doc.stories[sid.0 as usize].slice(line.start, line.end))
                    .collect();
                assert_eq!(recovered, format!("Before{text}"));
            }
        }
    }
}

#[test]
fn blank_paragraphs_reserve_lines_and_balanced_columns_never_overlap() {
    for columns in [1, 2, 3] {
        for blank_count in 0..5 {
            let mut doc = blank_a4();
            let created = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(0.0, 0.0, 500.0, 700.0),
            )
            .unwrap();
            let placed = doc
                .objects
                .iter_mut()
                .find(|o| o.id == created.object)
                .unwrap();
            if let LayoutObject::TextFrame { columns: count, .. } = &mut placed.object {
                *count = columns;
            }
            let mut story = Story::default();
            for index in 0..6 {
                story.push_paragraph(format!("Paragraph {index}"), "Body");
                if index < 5 {
                    for _ in 0..blank_count {
                        story.push_paragraph("", "Body");
                    }
                }
            }
            doc.stories[created.story.0 as usize] = story;
            let thread = compose::compose_story(&doc, created.story);
            assert!(!thread.has_overflow());
            assert_eq!(thread.lines().count(), 6 + blank_count * 5);
            for column in compose::columns(Rect::new(0.0, 0.0, 500.0, 700.0), columns, 0.0) {
                let lines: Vec<_> = thread.lines().filter(|l| l.bounds.x == column.x).collect();
                for pair in lines.windows(2) {
                    assert!(
                        pair[0].bounds.bottom() <= pair[1].bounds.y + 0.01,
                        "{pair:?}"
                    );
                }
            }
        }
    }
}
