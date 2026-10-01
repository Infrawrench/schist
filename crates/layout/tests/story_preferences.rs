use schist_layout::{
    authoring, blank_a4,
    compose::{compose_thread, InsetsLike},
    edit::snapshot_story,
    FrameOverflow, History, LayoutEdit, ObjectId, Rect, Story, StoryDirection, StoryOrientation,
    StoryPreferences,
};

#[test]
fn story_direction_reverses_column_progression_without_changing_text_or_bidi() {
    for count in 2..5 {
        for balanced in [false, true] {
            let mut doc = blank_a4();
            let mut story = Story::new();
            if balanced {
                for _ in 0..count {
                    story.push_paragraph("Short paragraph.", "Body");
                }
            } else {
                story.push_paragraph(
                    "Words to flow through columns and frames. ".repeat(50),
                    "Body",
                );
            }
            let id = doc.add_story(story);
            let frames: Vec<_> = (0..10)
                .map(|_| {
                    (
                        ObjectId::next(),
                        Rect::new(10.0, 20.0, 440.0, 220.0),
                        FrameOverflow::Thread,
                        count,
                        10.0,
                        InsetsLike::default(),
                    )
                })
                .collect();
            let left = compose_thread(&doc, id, &frames);
            doc.story_mut(id).prefs.direction = StoryDirection::RightToLeft;
            let right = compose_thread(&doc, id, &frames);
            assert!(!left.has_overflow() && !right.has_overflow());
            assert_eq!(left.lines().count(), right.lines().count());
            for (a, b) in left.lines().zip(right.lines()) {
                assert_eq!(
                    (a.start, a.end, a.bounds.y, a.bounds.width),
                    (b.start, b.end, b.bounds.y, b.bounds.width)
                );
                assert!((a.bounds.x + b.bounds.right() - 460.0).abs() < 0.001);
                let spec = schist_layout::compose::line_spec(b, doc.story(id).unwrap(), &doc);
                assert_eq!(
                    spec.direction,
                    schist_text_engine::ParagraphDirection::LeftToRight
                );
            }
        }
    }
}

#[test]
fn text_edits_and_history_retain_preferences_and_legacy_stories_default_safely() {
    for direction in [StoryDirection::LeftToRight, StoryDirection::RightToLeft] {
        for orientation in [StoryOrientation::Horizontal, StoryOrientation::Vertical] {
            let mut doc = blank_a4();
            let id = doc.add_story(Story::from_text("éclair text", "Body"));
            doc.story_mut(id).prefs = StoryPreferences {
                direction,
                orientation,
            };
            let mut history = History::default();
            let before = doc.clone();
            assert!(authoring::replace_text(
                &mut doc,
                &mut history,
                id,
                2..6,
                "中\nword"
            ));
            assert_eq!(
                doc.story(id).unwrap().prefs,
                before.story(id).unwrap().prefs
            );
            assert_eq!(history.undo_depth(), 1);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
            let mut modified = doc.story(id).unwrap().clone();
            modified.prefs.direction = if direction == StoryDirection::LeftToRight {
                StoryDirection::RightToLeft
            } else {
                StoryDirection::LeftToRight
            };
            assert!(history.apply(
                &mut doc,
                LayoutEdit::StoryChanged {
                    id: id.0,
                    before: snapshot_story(before.story(id).unwrap()),
                    after: snapshot_story(&modified)
                }
            ));
            assert_eq!(doc.story(id).unwrap(), &modified);
            assert!(history.undo(&mut doc));
            assert_eq!(doc, before);
        }
    }
    let legacy: Story = serde_json::from_str(r#"{"points":[],"ranges":[]}"#).unwrap();
    assert_eq!(legacy.prefs, StoryPreferences::default());
}
