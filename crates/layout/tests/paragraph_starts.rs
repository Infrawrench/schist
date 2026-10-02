use schist_layout::{
    authoring, compose, numbering::Section, styles::ParagraphStart, FrameOverflow, History, Insets,
    LayoutDocument, LayoutObject, Page, ParagraphStyle, Rect, Story, StoryId, StoryPoint,
};

const POLICIES: [ParagraphStart; 6] = [
    ParagraphStart::Anywhere,
    ParagraphStart::NextColumn,
    ParagraphStart::NextFrame,
    ParagraphStart::NextPage,
    ParagraphStart::NextOddPage,
    ParagraphStart::NextEvenPage,
];

fn document(columns: u16) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::a4(); 4]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Starts".into(),
        based_on: Some("Body".into()),
        ..Default::default()
    });
    let mut ids = Vec::new();
    for page in [0, 0, 1, 1, 2, 2, 3, 3] {
        let created = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(0.0, 0.0, 480.0, 600.0),
        )
        .unwrap();
        let placed = doc
            .objects
            .iter_mut()
            .find(|o| o.id == created.object)
            .unwrap();
        if let LayoutObject::TextFrame {
            story,
            columns: count,
            insets,
            overflow,
            balance_columns,
            ..
        } = &mut placed.object
        {
            *story = StoryId(0);
            *count = columns;
            *insets = Insets::ZERO;
            *overflow = FrameOverflow::Thread;
            *balance_columns = Some(false);
        }
        ids.push(created.object);
    }
    doc.thread_order = vec![(StoryId(0), ids)];
    doc
}

#[test]
fn initial_and_already_reached_boundaries_do_not_add_blank_columns_or_frames() {
    for columns in [1, 2, 3] {
        for before in [false, true] {
            for forced in [
                None,
                Some(StoryPoint::ColumnBreak),
                Some(StoryPoint::FrameBreak),
                Some(StoryPoint::PageBreak),
            ] {
                for policy in POLICIES {
                    let mut doc = document(columns);
                    doc.styles
                        .paragraphs
                        .iter_mut()
                        .find(|s| s.name == "Starts")
                        .unwrap()
                        .start_paragraph = Some(policy);
                    let mut story = Story::default();
                    if before {
                        story.push_paragraph("Before", "Body");
                    }
                    if let Some(point) = &forced {
                        story.points.push(point.clone());
                    }
                    let (start, _) = story.push_paragraph("café chapter", "Starts");
                    doc.stories[0] = story;
                    let original = doc.clone();
                    let flow = compose::compose_story(&doc, StoryId(0));
                    let initial = !before && forced.is_none();
                    let moved_column = before || forced.is_some();
                    let forced_frame =
                        matches!(forced, Some(StoryPoint::FrameBreak | StoryPoint::PageBreak));
                    let forced_page = matches!(forced, Some(StoryPoint::PageBreak));
                    let (frame, column) = match policy {
                        ParagraphStart::Anywhere => match forced {
                            Some(StoryPoint::ColumnBreak) if columns > 1 => (0, 1),
                            Some(StoryPoint::ColumnBreak | StoryPoint::FrameBreak) => (1, 0),
                            Some(StoryPoint::PageBreak) => (2, 0),
                            _ => (0, 0),
                        },
                        ParagraphStart::NextColumn if initial => (0, 0),
                        ParagraphStart::NextColumn if forced_page => (2, 0),
                        ParagraphStart::NextColumn if forced_frame || columns == 1 => (1, 0),
                        ParagraphStart::NextColumn => (0, usize::from(moved_column)),
                        ParagraphStart::NextFrame if initial => (0, 0),
                        ParagraphStart::NextFrame if forced_page => (2, 0),
                        ParagraphStart::NextFrame => (1, 0),
                        ParagraphStart::NextPage if initial => (0, 0),
                        ParagraphStart::NextPage => (2, 0),
                        ParagraphStart::NextOddPage if initial => (0, 0),
                        ParagraphStart::NextOddPage => (4, 0),
                        ParagraphStart::NextEvenPage => (2, 0),
                    };
                    let after: Vec<_> = flow
                        .frames
                        .iter()
                        .enumerate()
                        .flat_map(|(frame, result)| {
                            result
                                .lines
                                .iter()
                                .filter(|l| l.start >= start)
                                .map(move |l| (frame, l))
                        })
                        .collect();
                    assert_eq!(after.len(), 1, "{policy:?}, {forced:?}, {before}");
                    assert_eq!(
                        after[0].0, frame,
                        "{policy:?}, {forced:?}, before={before}, columns={columns}"
                    );
                    assert!(
                        (after[0].1.bounds.x - column as f32 * 480.0 / columns as f32).abs() < 0.01,
                        "{policy:?}, {forced:?}, {before}"
                    );
                    assert_eq!(
                        doc.stories[0].slice(after[0].1.start, after[0].1.end),
                        "café chapter"
                    );
                    assert!(!flow.has_overflow());
                    assert_eq!(doc, original);
                }
            }
        }
    }
}

#[test]
fn page_parity_uses_numbering_sections_and_skips_all_frames_on_unsuitable_pages() {
    for initial in [false, true] {
        for start_number in [1, 2, 4, 5] {
            for restart in [None, Some(10), Some(11)] {
                for policy in [ParagraphStart::NextOddPage, ParagraphStart::NextEvenPage] {
                    let mut doc = document(2);
                    doc.pages[0].section = Some(Section {
                        start: start_number,
                        continue_numbering: false,
                        ..Default::default()
                    });
                    doc.pages[1].section = restart.map(|start| Section {
                        start,
                        continue_numbering: false,
                        ..Default::default()
                    });
                    doc.styles
                        .paragraphs
                        .iter_mut()
                        .find(|s| s.name == "Starts")
                        .unwrap()
                        .start_paragraph = Some(policy);
                    let mut story = Story::default();
                    if !initial {
                        story.push_paragraph("Before", "Body");
                    }
                    let (start, _) = story.push_paragraph("café chapter", "Starts");
                    doc.stories[0] = story;
                    let parity = u32::from(policy == ParagraphStart::NextOddPage);
                    let page = (usize::from(!initial)..4)
                        .find(|page| doc.page_number_value(*page) % 2 == parity)
                        .unwrap();
                    let flow = compose::compose_story(&doc, StoryId(0));
                    let after: Vec<_> = flow
                        .frames
                        .iter()
                        .enumerate()
                        .flat_map(|(i, f)| {
                            f.lines
                                .iter()
                                .filter(|l| l.start >= start)
                                .map(move |l| (i, l))
                        })
                        .collect();
                    assert_eq!(after.len(), 1);
                    assert_eq!(
                        after[0].0,
                        page * 2,
                        "{policy:?}, initial={initial}, start={start_number}, restart={restart:?}"
                    );
                    assert!(!flow.has_overflow());
                }
            }
        }
    }
}

#[test]
fn start_constraints_inherit_reset_independently_and_old_snapshots_still_load() {
    let mut doc = document(1);
    for policy in POLICIES {
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Body")
            .unwrap()
            .start_paragraph = Some(policy);
        assert_eq!(
            doc.styles.resolve_paragraph("Starts").start_paragraph,
            Some(policy)
        );
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Starts")
            .unwrap()
            .start_paragraph = Some(ParagraphStart::Anywhere);
        assert_eq!(
            doc.styles.resolve_paragraph("Starts").start_paragraph,
            Some(ParagraphStart::Anywhere)
        );
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Starts")
            .unwrap()
            .start_paragraph = None;
    }
    let mut json = serde_json::to_value(&doc.styles).unwrap();
    for style in json["paragraphs"].as_array_mut().unwrap() {
        style.as_object_mut().unwrap().remove("start_paragraph");
    }
    let old: schist_layout::StyleSet = serde_json::from_value(json).unwrap();
    assert_eq!(old.resolve_paragraph("Starts").start_paragraph, None);
}

#[test]
fn an_empty_paragraph_still_reaches_its_required_numbered_page() {
    for number in [1, 2] {
        for policy in [ParagraphStart::NextOddPage, ParagraphStart::NextEvenPage] {
            let mut doc = document(2);
            doc.pages[0].section = Some(Section {
                start: number,
                continue_numbering: false,
                ..Default::default()
            });
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Starts")
                .unwrap()
                .start_paragraph = Some(policy);
            doc.stories[0] = Story::from_text("", "Starts");
            let original = doc.clone();
            let parity = u32::from(policy == ParagraphStart::NextOddPage);
            let page = (0..4)
                .find(|page| doc.page_number_value(*page) % 2 == parity)
                .unwrap();
            let flow = compose::compose_story(&doc, StoryId(0));
            let occupied: Vec<_> = flow
                .frames
                .iter()
                .enumerate()
                .filter(|(_, f)| !f.lines.is_empty())
                .collect();
            assert_eq!(occupied.len(), 1, "{policy:?}, start={number}");
            assert_eq!(occupied[0].0, page * 2);
            assert_eq!(occupied[0].1.lines[0].start, 0);
            assert_eq!(occupied[0].1.lines[0].end, 0);
            assert!(!flow.has_overflow());
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn unavailable_destinations_leave_overset_text_without_consuming_its_first_character() {
    for overflow in [FrameOverflow::Clip, FrameOverflow::Thread] {
        for policy in [
            ParagraphStart::NextPage,
            ParagraphStart::NextOddPage,
            ParagraphStart::NextEvenPage,
        ] {
            let mut doc = document(3);
            for object in &mut doc.objects {
                object.page = 0;
            }
            if let LayoutObject::TextFrame { overflow: mode, .. } = &mut doc.objects[0].object {
                *mode = overflow;
            }
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Starts")
                .unwrap()
                .start_paragraph = Some(policy);
            let mut story = Story::from_text("Before", "Body");
            let (start, _) = story.push_paragraph("café chapter", "Starts");
            doc.stories[0] = story;
            let original = doc.clone();
            let flow = compose::compose_story(&doc, StoryId(0));
            assert!(flow.has_overflow());
            assert_eq!(flow.lines().count(), 1);
            // Clipped threads retain empty addressable tail frames. The
            // clipping frame, not the last unused port, owns the lost text.
            let terminal = if overflow == FrameOverflow::Clip {
                0
            } else {
                flow.frames.len() - 1
            };
            assert!(flow.frames[terminal].lost);
            assert!(flow.frames[terminal + 1..]
                .iter()
                .all(|frame| !frame.lost && !frame.passed_on));
            assert!(flow.frames.iter().all(|f| f.consumed_to == start));
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn reaching_a_frame_start_preserves_balancing_and_overrides_adjacent_line_keeps() {
    for columns in [2, 3] {
        for policy in [
            ParagraphStart::NextFrame,
            ParagraphStart::NextPage,
            ParagraphStart::NextOddPage,
            ParagraphStart::NextEvenPage,
        ] {
            let mut doc = document(columns);
            for object in &mut doc.objects {
                if let LayoutObject::TextFrame {
                    balance_columns, ..
                } = &mut object.object
                {
                    *balance_columns = Some(true);
                }
            }
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Body")
                .unwrap()
                .keeps
                .next = Some(5);
            let style = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Starts")
                .unwrap();
            style.start_paragraph = Some(policy);
            style.keeps.previous = Some(true);
            style.keeps.enabled = Some(false);
            style.leading = Some(schist_layout::styles::Leading::Points(16.0));
            let mut story = Story::from_text("Before", "Body");
            let (start, _) = story.push_paragraph("café\na\nb\nc\nd\ne\nf\ng\nh", "Starts");
            doc.stories[0] = story;
            let flow = compose::compose_story(&doc, StoryId(0));
            assert!(!flow.has_overflow());
            assert_eq!(flow.frames[0].lines.len(), 1, "{policy:?}");
            let frame = flow
                .frames
                .iter()
                .find(|f| f.lines.iter().any(|l| l.start >= start))
                .unwrap();
            assert_eq!(frame.lines.len(), 9);
            let counts: Vec<_> = (0..columns)
                .map(|col| {
                    frame
                        .lines
                        .iter()
                        .filter(|l| {
                            (l.bounds.x - f32::from(col) * 480.0 / f32::from(columns)).abs() < 0.01
                        })
                        .count()
                })
                .collect();
            assert!(
                counts.iter().all(|count| *count > 0),
                "{policy:?}: {counts:?}"
            );
            assert!(
                counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1,
                "{policy:?}: {counts:?}"
            );
        }
    }
}

#[test]
fn starts_follow_logical_columns_and_do_not_restart_on_writing_mode_changes() {
    use schist_layout::{styles::WritingMode, StoryDirection};
    for reverse in [false, true] {
        for vertical in [false, true] {
            for policy in POLICIES {
                let mut doc = document(3);
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Starts")
                    .unwrap();
                style.start_paragraph = Some(policy);
                style.writing_mode = Some(if vertical {
                    WritingMode::VerticalRightToLeft
                } else {
                    WritingMode::Horizontal
                });
                let mut story = Story::from_text("Before", "Body");
                story.prefs.direction = if reverse {
                    StoryDirection::RightToLeft
                } else {
                    StoryDirection::LeftToRight
                };
                let (start, _) = story.push_paragraph("café", "Starts");
                doc.stories[0] = story;
                let flow = compose::compose_story(&doc, StoryId(0));
                assert!(!flow.has_overflow());
                let after: Vec<_> = flow
                    .frames
                    .iter()
                    .enumerate()
                    .flat_map(|(i, f)| {
                        f.lines
                            .iter()
                            .filter(|l| l.start >= start)
                            .map(move |l| (i, l))
                    })
                    .collect();
                assert_eq!(after.len(), 1);
                let frame = match policy {
                    ParagraphStart::NextFrame => 1,
                    ParagraphStart::NextPage | ParagraphStart::NextEvenPage => 2,
                    ParagraphStart::NextOddPage => 4,
                    _ => 0,
                };
                assert_eq!(
                    after[0].0, frame,
                    "{policy:?}, reverse={reverse}, vertical={vertical}"
                );
                assert_eq!(
                    doc.stories[0].slice(after[0].1.start, after[0].1.end),
                    "café"
                );
                if !vertical {
                    let col = if policy == ParagraphStart::NextColumn {
                        1
                    } else if reverse {
                        2
                    } else {
                        0
                    };
                    assert!((after[0].1.bounds.x - col as f32 * 160.0).abs() < 0.01);
                }
            }
        }
    }
}

#[test]
fn path_threads_obey_starts_without_losing_the_character_after_a_break() {
    use schist_layout::{text_path::PathText, Point, ShapePath, SubPath};
    for policy in POLICIES {
        for forced in [false, true] {
            let mut doc = document(1);
            for object in &mut doc.objects {
                if let LayoutObject::TextFrame { text_path, .. } = &mut object.object {
                    *text_path = Some(PathText {
                        path: ShapePath {
                            subpaths: vec![SubPath {
                                points: vec![Point::ZERO, Point::new(480.0, 0.0)],
                                handles: Vec::new(),
                                closed: false,
                            }],
                            even_odd: false,
                        },
                        start: 0.0,
                        end: None,
                    });
                }
            }
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Starts")
                .unwrap()
                .start_paragraph = Some(policy);
            let mut story = Story::from_text("Before", "Body");
            if forced {
                story.points.push(StoryPoint::FrameBreak);
            }
            let (start, _) = story.push_paragraph("café", "Starts");
            doc.stories[0] = story;
            let original = doc.clone();
            let flow = compose::compose_story(&doc, StoryId(0));
            let after: Vec<_> = flow
                .frames
                .iter()
                .enumerate()
                .flat_map(|(i, f)| {
                    f.lines
                        .iter()
                        .filter(|l| l.start >= start)
                        .map(move |l| (i, l))
                })
                .collect();
            assert_eq!(after.len(), 1);
            let frame = match policy {
                ParagraphStart::NextPage | ParagraphStart::NextEvenPage => 2,
                ParagraphStart::NextOddPage => 4,
                _ => 1,
            };
            assert_eq!(after[0].0, frame, "{policy:?}, forced={forced}");
            assert_eq!(
                doc.stories[0].slice(after[0].1.start, after[0].1.end),
                "café"
            );
            assert!(!flow.has_overflow());
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn main_paragraph_starts_preserve_whole_and_split_notes_in_independent_or_spanning_columns() {
    use schist_layout::footnotes::{FootnoteBody, FootnoteFirstBaseline, FootnoteMarkerPosition};
    for splitting in [false, true] {
        for spanning in [false, true] {
            for policy in POLICIES {
                let mut doc = document(2);
                doc.footnotes.no_splitting = Some(!splitting);
                doc.footnotes.straddle = Some(spanning);
                doc.footnotes.first_baseline = Some(FootnoteFirstBaseline::Ascent);
                doc.footnotes.marker_position = Some(FootnoteMarkerPosition::Normal);
                doc.footnotes.rule.on = Some(false);
                doc.footnotes.continuing_rule.on = Some(false);
                doc.footnotes.spacer = Some(4.0);
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Note".into(),
                    point_size: Some(9.0),
                    ..Default::default()
                });
                doc.styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Starts")
                    .unwrap()
                    .start_paragraph = Some(policy);
                let mut story = Story::from_text("Before", "Body");
                let (start, _) = story.push_paragraph("café", "Starts");
                let anchor = story.text_len();
                story.structures.push(schist_layout::StoryStructure {
                    at: Some(anchor),
                    kind: "Footnote".into(),
                    payload: "retained source".into(),
                    footnote: Some(FootnoteBody {
                        story: Story::from_text("note é", "Note"),
                        markers: Vec::new(),
                        reference_paragraph_style: "Starts".into(),
                        reference_character_style: "Default".into(),
                    }),
                });
                doc.stories[0] = story;
                if splitting {
                    for object in &mut doc.objects {
                        object.bounds.height = 80.0;
                    }
                    doc.stories[0].structures[0]
                        .footnote
                        .as_mut()
                        .unwrap()
                        .story = Story::from_text(
                        (0..24)
                            .map(|i| format!("note é{i}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        "Note",
                    );
                }
                let expected = doc.stories[0].structures[0]
                    .footnote
                    .as_ref()
                    .unwrap()
                    .story
                    .text()
                    .replace('\n', "");
                let original = doc.clone();
                let flow = compose::compose_story(&doc, StoryId(0));
                assert!(!flow.has_overflow(), "{policy:?}/{splitting}/{spanning}");
                let frame = match policy {
                    ParagraphStart::NextFrame => 1,
                    ParagraphStart::NextPage | ParagraphStart::NextEvenPage => 2,
                    ParagraphStart::NextOddPage => 4,
                    _ => 0,
                };
                let after = flow.frames[frame]
                    .lines
                    .iter()
                    .find(|l| l.start == start)
                    .unwrap();
                assert_eq!(doc.stories[0].slice(after.start, after.end), "café");
                let notes: Vec<_> = flow
                    .frames
                    .iter()
                    .enumerate()
                    .flat_map(|(i, f)| f.footnotes.iter().map(move |n| (i, n)))
                    .collect();
                if splitting {
                    assert!(notes.len() > 1);
                } else {
                    assert_eq!(notes.len(), 1);
                }
                assert!(notes.iter().all(|(i, n)| *i >= frame && n.anchor == anchor));
                assert_eq!(notes[0].0, frame);
                assert_eq!(notes[0].1.anchor, anchor);
                let text: String = notes
                    .iter()
                    .flat_map(|(_, n)| &n.lines)
                    .map(|l| compose::line_spec(l, &doc.stories[0], &doc).text)
                    .collect();
                assert_eq!(text, expected);
                assert_eq!(doc, original);
            }
        }
    }
}
