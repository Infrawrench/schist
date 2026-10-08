use schist_layout::{
    blank_a4, compose::InsetsLike, compose_thread, paragraph_keeps::ParagraphKeeps,
    styles::Leading, FrameOverflow, LayoutDocument, ObjectId, ParagraphStyle, Rect, Story, StoryId,
};

fn flow(doc: &LayoutDocument, story: StoryId, height: f32) -> schist_layout::ComposedThread {
    compose_thread(
        doc,
        story,
        &[
            (
                ObjectId::next(),
                Rect::new(0.0, 0.0, 150.0, height),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            ),
            (
                ObjectId::next(),
                Rect::new(200.0, 0.0, 150.0, 2000.0),
                FrameOverflow::Clip,
                1,
                0.0,
                InsetsLike::default(),
            ),
        ],
    )
}

#[test]
fn adjacent_paragraph_keeps_move_only_the_smallest_legal_suffix() {
    for next in 0..=5 {
        for previous in [false, true] {
            for (enabled, all, first, last) in [
                (false, false, 3, 3),
                (true, false, 2, 3),
                (true, true, 2, 2),
            ] {
                let mut doc = blank_a4();
                for (name, keeps) in [
                    (
                        "Before",
                        ParagraphKeeps {
                            next: Some(next),
                            enabled: Some(enabled),
                            all: Some(all),
                            first: Some(first),
                            last: Some(last),
                            ..Default::default()
                        },
                    ),
                    (
                        "After",
                        ParagraphKeeps {
                            previous: Some(previous),
                            enabled: Some(false),
                            ..Default::default()
                        },
                    ),
                ] {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: name.into(),
                        point_size: Some(12.0),
                        leading: Some(Leading::Points(16.0)),
                        keeps,
                        ..Default::default()
                    });
                }
                let mut story = Story::from_text("café\na\nb\nc\nd\ne", "Before");
                story.push_paragraph("一\nf\ng\nh\ni\nj\nk\nl", "After");
                let id = doc.add_story(story);
                let original = doc.clone();
                let full = flow(&doc, id, 2000.0);
                let lines = &full.frames[0].lines;
                assert_eq!(lines.len(), 14);
                for room in 6..14 {
                    let height = lines[..room]
                        .iter()
                        .map(|l| l.bounds.bottom())
                        .fold(0.0, f32::max)
                        + 0.01;
                    let actual = flow(&doc, id, height);
                    let need = next.max(usize::from(previous));
                    let expected = if room - 6 < need {
                        if !enabled {
                            5
                        } else if all {
                            0
                        } else {
                            3
                        }
                    } else {
                        room
                    };
                    assert_eq!(actual.frames[0].lines.len(), expected,
                        "next={next}, previous={previous}, enabled={enabled}, all={all}, room={room}");
                    assert!(!actual.has_overflow());
                    assert_eq!(
                        actual.lines().map(|l| (l.start, l.end)).collect::<Vec<_>>(),
                        lines.iter().map(|l| (l.start, l.end)).collect::<Vec<_>>()
                    );
                    assert_eq!(doc, original);
                }
            }
        }
    }
}

#[test]
fn independent_keep_properties_inherit_and_old_snapshots_remain_readable() {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Base".into(),
        keep_lines: Some(4),
        keep_with_next: Some(true),
        keeps: ParagraphKeeps {
            all: Some(true),
            previous: Some(true),
            ..Default::default()
        },
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Child".into(),
        based_on: Some("Base".into()),
        keeps: ParagraphKeeps {
            enabled: Some(false),
            first: Some(2),
            next: Some(0),
            previous: Some(false),
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(
        doc.styles.resolve_paragraph("Child").keeps,
        ParagraphKeeps {
            enabled: Some(false),
            all: Some(true),
            first: Some(2),
            last: Some(4),
            next: Some(0),
            previous: Some(false),
        }
    );
    let mut snapshot = serde_json::to_value(&doc.styles).unwrap();
    for style in snapshot["paragraphs"].as_array_mut().unwrap() {
        style.as_object_mut().unwrap().remove("keeps");
    }
    let restored: schist_layout::StyleSet = serde_json::from_value(snapshot).unwrap();
    assert_eq!(
        restored.resolve_paragraph("Child").keeps,
        ParagraphKeeps::from_legacy(Some(true), Some(4))
    );
}

#[test]
fn native_bindings_cross_orientation_changes_without_stranding_a_heading() {
    use schist_layout::WritingMode;
    let modes = [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ];
    for before in modes {
        for after in modes.into_iter().filter(|m| *m != before) {
            for previous in [false, true] {
                let mut doc = blank_a4();
                for (name, mode, size, keeps) in [
                    ("Fill", before, 11.0, ParagraphKeeps::default()),
                    (
                        "Heading",
                        before,
                        11.0,
                        ParagraphKeeps {
                            next: Some(if previous { 0 } else { 5 }),
                            ..Default::default()
                        },
                    ),
                    (
                        "Body",
                        after,
                        130.0,
                        ParagraphKeeps {
                            previous: Some(previous),
                            ..Default::default()
                        },
                    ),
                ] {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: name.into(),
                        writing_mode: Some(mode),
                        point_size: Some(size),
                        leading: Some(Leading::Points(size + 4.0)),
                        keeps,
                        ..Default::default()
                    });
                }
                let mut story = Story::from_text("f", "Fill");
                story.push_paragraph("h", "Heading");
                story.push_paragraph("b", "Body");
                let id = doc.add_story(story);
                let frames: Vec<_> = [100.0, 300.0]
                    .into_iter()
                    .map(|size| {
                        (
                            ObjectId::next(),
                            Rect::new(0.0, 0.0, size, size),
                            FrameOverflow::Thread,
                            1,
                            0.0,
                            InsetsLike::default(),
                        )
                    })
                    .collect();
                let actual = compose_thread(&doc, id, &frames);
                assert!(!actual.has_overflow());
                assert_eq!(actual.frames[0].lines.len(), 1);
                assert_eq!(actual.frames[0].lines[0].paragraph_style, "Fill");
                assert_eq!(
                    actual.frames[1]
                        .lines
                        .iter()
                        .map(|l| l.paragraph_style.as_str())
                        .collect::<Vec<_>>(),
                    ["Heading", "Body"]
                );
            }
        }
    }
}

#[test]
fn explicit_destination_breaks_override_both_adjacent_keep_directions() {
    use schist_layout::StoryPoint;
    for kind in [
        StoryPoint::ColumnBreak,
        StoryPoint::FrameBreak,
        StoryPoint::PageBreak,
    ] {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Kept".into(),
            point_size: Some(12.0),
            keeps: ParagraphKeeps {
                next: Some(5),
                previous: Some(true),
                ..Default::default()
            },
            ..Default::default()
        });
        let mut story = Story::from_text("before", "Kept");
        story.points.push(kind);
        story.push_paragraph("after", "Kept");
        let id = doc.add_story(story);
        let actual = flow(&doc, id, 100.0);
        assert_eq!(actual.frames[0].lines.len(), 1);
        assert_eq!(actual.frames[0].lines[0].start, 0);
        assert_eq!(
            doc.story(id).unwrap().slice(
                actual.frames[0].lines[0].start,
                actual.frames[0].lines[0].end
            ),
            "before"
        );
    }
}

#[test]
fn balancing_and_both_reading_orders_preserve_chains_of_native_bindings() {
    for columns in 2..=4 {
        for groups in [1, 3, 5] {
            for rtl in [false, true] {
                let mut doc = blank_a4();
                for (name, next, previous) in [
                    ("A", 0, false),
                    ("B", 5, true),
                    ("C", 2, false),
                    ("Body", 0, false),
                ] {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: name.into(),
                        point_size: Some(12.0),
                        leading: Some(Leading::Points(16.0)),
                        keeps: ParagraphKeeps {
                            next: Some(next),
                            previous: Some(previous),
                            enabled: Some(false),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                let mut story = Story::new();
                for _ in 0..groups {
                    for (text, style) in [
                        ("a", "A"),
                        ("b", "B"),
                        ("c", "C"),
                        ("d\ne\nf\ng\nh\ni\nj\nk", "Body"),
                    ] {
                        story.push_paragraph(text, style);
                    }
                }
                if rtl {
                    story.prefs.direction = schist_layout::StoryDirection::RightToLeft;
                }
                let id = doc.add_story(story);
                let original = doc.clone();
                let actual = compose_thread(
                    &doc,
                    id,
                    &[(
                        ObjectId::next(),
                        Rect::new(0.0, 0.0, columns as f32 * 150.0, 1200.0),
                        FrameOverflow::Clip,
                        columns,
                        10.0,
                        InsetsLike::default(),
                    )],
                );
                assert!(!actual.has_overflow());
                let lines: Vec<_> = actual.lines().collect();
                assert_eq!(lines.len(), groups * 11);
                for chunk in lines.chunks(11) {
                    for line in &chunk[..5] {
                        assert_eq!(line.bounds.x, chunk[0].bounds.x);
                    }
                }
                assert_eq!(doc, original);
            }
        }
    }
}

#[test]
fn keep_rollback_never_separates_an_opening_initial_from_its_inset_lines() {
    for cap_lines in 2..=4 {
        let mut doc = blank_a4();
        for (name, cap, next) in [("Plain", None, 0), ("Initial", Some(cap_lines), 1)] {
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.into(),
                point_size: Some(12.0),
                leading: Some(Leading::Points(16.0)),
                drop_caps_lines: cap,
                drop_caps_characters: Some(1),
                // Ensure the following line lies beyond the initial's ink
                // bounds as well as its final body baseline.
                space_after: cap.map(|_| 40.0),
                keeps: ParagraphKeeps {
                    enabled: Some(false),
                    next: Some(next),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        let mut story = Story::from_text("fill", "Plain");
        story.push_paragraph(format!("Aa{}", "\nb".repeat(cap_lines - 1)), "Initial");
        story.push_paragraph("tail", "Plain");
        let id = doc.add_story(story);
        let full = flow(&doc, id, 2000.0);
        let opening: Vec<_> = full
            .lines()
            .filter(|l| l.paragraph_style == "Initial")
            .collect();
        assert_eq!(opening.len(), cap_lines + 1);
        assert_eq!(opening.iter().filter(|l| l.initial.is_some()).count(), 1);
        let height = opening
            .iter()
            .map(|l| l.bounds.bottom())
            .fold(0.0, f32::max)
            + 0.01;
        let actual = flow(&doc, id, height);
        assert!(!actual.has_overflow());
        assert_eq!(actual.frames[0].lines.len(), 1);
        assert_eq!(actual.frames[0].lines[0].paragraph_style, "Plain");
        assert!(actual.frames[1].lines[0].initial.is_some());
    }
}
