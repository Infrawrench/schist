use schist_layout::{
    authoring, compose, numbering::Section, FrameOverflow, History, Insets, LayoutDocument,
    LayoutObject, Page, Rect, Story, StoryId, StoryPoint,
};

fn document(path: bool, first: u32, restart: Option<u32>) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::a4(); 7]);
    doc.pages[0].section = Some(Section {
        start: first,
        continue_numbering: false,
        ..Default::default()
    });
    doc.pages[1].section = restart.map(|start| Section {
        start,
        continue_numbering: false,
        ..Default::default()
    });
    let mut ids = Vec::new();
    for frame in 0..14 {
        let created = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            frame / 2,
            Rect::new(0.0, 0.0, 500.0, 700.0),
        )
        .unwrap();
        if let LayoutObject::TextFrame {
            story,
            columns,
            gutter,
            insets,
            balance_columns,
            overflow,
            text_path,
            ..
        } = &mut doc.objects.last_mut().unwrap().object
        {
            *story = StoryId(0);
            *columns = 2;
            *gutter = 0.0;
            *insets = Insets::ZERO;
            *balance_columns = Some(false);
            *overflow = FrameOverflow::Thread;
            if path {
                *text_path = Some(schist_layout::text_path::PathText {
                    path: schist_layout::ShapePath {
                        subpaths: vec![schist_layout::SubPath {
                            points: vec![
                                schist_layout::Point::ZERO,
                                schist_layout::Point::new(500.0, 0.0),
                            ],
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
        ids.push(created.object);
    }
    doc.thread_order = vec![(StoryId(0), ids)];
    doc
}

#[test]
fn each_explicit_break_advances_its_own_container_even_before_an_empty_paragraph() {
    for path in [false, true] {
        for first in [1, 2] {
            for restart in [None, Some(10), Some(11)] {
                for kind in [
                    StoryPoint::ColumnBreak,
                    StoryPoint::FrameBreak,
                    StoryPoint::PageBreak,
                    StoryPoint::OddPageBreak,
                    StoryPoint::EvenPageBreak,
                ] {
                    for count in 1..=3 {
                        for before in [false, true] {
                            for text in ["", "é中😀"] {
                                let mut doc = document(path, first, restart);
                                let mut story = Story::default();
                                if before {
                                    story.push_paragraph("Before", "Body");
                                }
                                story.points.extend(vec![kind.clone(); count]);
                                let (start, _) = story.push_paragraph(text, "Body");
                                doc.stories[0] = story;
                                let original = doc.clone();
                                let (mut frame, mut column) = (0usize, 0usize);
                                for _ in 0..count {
                                    match kind {
                                        StoryPoint::ColumnBreak if !path => {
                                            column += 1;
                                            frame += column / 2;
                                            column %= 2;
                                        }
                                        StoryPoint::ColumnBreak | StoryPoint::FrameBreak => {
                                            frame += 1;
                                            column = 0;
                                        }
                                        _ => {
                                            let next = (frame / 2 + 1..7)
                                                .find(|page| match kind {
                                                    StoryPoint::OddPageBreak => {
                                                        doc.page_number_value(*page) % 2 == 1
                                                    }
                                                    StoryPoint::EvenPageBreak => doc
                                                        .page_number_value(*page)
                                                        .is_multiple_of(2),
                                                    _ => true,
                                                })
                                                .unwrap();
                                            frame = next * 2;
                                            column = 0;
                                        }
                                    }
                                }
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
                                assert_eq!(
                                    after.len(),
                                    1,
                                    "{kind:?}/{count}, path={path}, before={before}, text={text:?}"
                                );
                                assert_eq!(after[0].0, frame, "{kind:?}/{count}, path={path}, before={before}, text={text:?}, first={first}, restart={restart:?}");
                                if !path {
                                    assert!(
                                        (after[0].1.bounds.x - column as f32 * 250.0).abs() < 0.01
                                    );
                                }
                                assert_eq!(
                                    doc.stories[0].slice(after[0].1.start, after[0].1.end),
                                    text
                                );
                                assert!(!flow.has_overflow());
                                assert_eq!(doc, original);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unavailable_numbered_destinations_leave_text_and_blank_paragraphs_overset() {
    for kind in [StoryPoint::OddPageBreak, StoryPoint::EvenPageBreak] {
        for clip in [false, true] {
            for text in ["", "é中😀"] {
                let mut doc = document(false, 1, None);
                for object in &mut doc.objects {
                    object.page = 0;
                }
                if clip {
                    if let LayoutObject::TextFrame { overflow, .. } = &mut doc.objects[0].object {
                        *overflow = FrameOverflow::Clip;
                    }
                }
                let mut story = Story::default();
                story.points.push(kind.clone());
                story.push_paragraph(text, "Body");
                doc.stories[0] = story;
                let original = doc.clone();
                let flow = compose::compose_story(&doc, StoryId(0));
                assert_eq!(flow.lines().count(), 0);
                assert!(flow.has_overflow());
                assert!(flow.frames[if clip { 0 } else { 13 }].lost);
                assert!(flow.frames.iter().all(|f| f.consumed_to == 0));
                assert_eq!(doc, original);
            }
        }
    }
}

#[test]
fn a_clipped_intermediate_frame_stops_pending_page_destinations() {
    use schist_layout::{styles::ParagraphStart, ParagraphStyle};
    for start in [
        ParagraphStart::NextPage,
        ParagraphStart::NextOddPage,
        ParagraphStart::NextEvenPage,
    ] {
        for explicit in [false, true] {
            for clip_index in if start == ParagraphStart::NextOddPage {
                vec![1, 3]
            } else {
                vec![1]
            } {
                for text in ["", "é中😀"] {
                    let mut doc = document(false, 1, None);
                    if let LayoutObject::TextFrame { overflow, .. } =
                        &mut doc.objects[clip_index].object
                    {
                        *overflow = FrameOverflow::Clip;
                    }
                    let mut story = Story::from_text("Before", "Body");
                    if explicit {
                        story.points.push(match start {
                            ParagraphStart::NextOddPage => StoryPoint::OddPageBreak,
                            ParagraphStart::NextEvenPage => StoryPoint::EvenPageBreak,
                            _ => StoryPoint::PageBreak,
                        });
                    }
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Starts".into(),
                        based_on: Some("Body".into()),
                        start_paragraph: (!explicit).then_some(start),
                        ..Default::default()
                    });
                    let (at, _) = story.push_paragraph(text, "Starts");
                    doc.stories[0] = story;
                    let original = doc.clone();
                    let flow = compose::compose_story(&doc, StoryId(0));
                    assert!(
                        flow.frames[clip_index].lost,
                        "{start:?}, explicit={explicit}"
                    );
                    assert!(!flow.frames[clip_index].passed_on);
                    assert_eq!(flow.frames[clip_index].consumed_to, at);
                    assert!(flow.frames[clip_index + 1..]
                        .iter()
                        .all(|f| f.lines.is_empty() && !f.lost && !f.passed_on));
                    assert_eq!(flow.lines().count(), 1);
                    assert_eq!(doc, original);
                }
            }
        }
    }
}

#[test]
fn text_edits_snapshots_and_single_step_undo_preserve_numbered_break_identity() {
    for kind in [StoryPoint::OddPageBreak, StoryPoint::EvenPageBreak] {
        let mut doc = document(false, 1, None);
        let mut story = Story::from_text("éA", "Body");
        story.points.push(kind.clone());
        let (start, end) = story.push_paragraph("中😀", "Body");
        story
            .ranges
            .push(schist_layout::StyleRange::new(start, end, "Default"));
        assert!(kind.is_forced_break());
        assert!(story
            .replace_text(0..story.text_len(), "rewrite", "Body")
            .is_none());
        doc.stories[0] = story;
        let original = doc.clone();
        let mut history = History::default();
        assert!(authoring::replace_text(
            &mut doc,
            &mut history,
            StoryId(0),
            0..2,
            "prefix"
        ));
        let edited = doc.clone();
        assert_eq!(doc.stories[0].points[1], kind);
        assert_eq!(history.undo_depth(), 1);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, edited);
        let snapshot = schist_layout::edit::snapshot_story(&doc.stories[0]);
        let json = serde_json::to_string(&snapshot).unwrap();
        let after = serde_json::from_str(&json).unwrap();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
        assert!(history.apply(
            &mut doc,
            schist_layout::LayoutEdit::StoryChanged {
                id: 0,
                before: schist_layout::edit::snapshot_story(&original.stories[0]),
                after
            }
        ));
        assert_eq!(doc, edited);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
    }
}

#[test]
fn numbered_break_trials_preserve_balanced_main_text_and_whole_or_continued_notes() {
    use schist_layout::{
        footnotes::{FootnoteBody, FootnoteFirstBaseline, FootnoteMarkerPosition},
        styles::Leading,
        ParagraphStyle, StoryStructure,
    };
    for kind in [StoryPoint::OddPageBreak, StoryPoint::EvenPageBreak] {
        for count in [1, 2] {
            for spanning in [false, true] {
                for splitting in [false, true] {
                    let mut doc = document(false, 1, None);
                    for object in &mut doc.objects {
                        object.bounds.height = 100.0;
                        if let LayoutObject::TextFrame {
                            balance_columns, ..
                        } = &mut object.object
                        {
                            *balance_columns = Some(true);
                        }
                    }
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
                        leading: Some(Leading::Points(12.0)),
                        ..Default::default()
                    });
                    let note = (0..if splitting { 14 } else { 3 })
                        .map(|i| format!("note é{i}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut story = Story::from_text("Before", "Body");
                    story.points.extend(vec![kind.clone(); count]);
                    story.push_paragraph("café\na\nb\nc\nd\ne\nf\ng\nh", "Body");
                    let anchor = story.text_len();
                    story.structures.push(StoryStructure {
                        at: Some(anchor),
                        kind: "Footnote".into(),
                        payload: "retained source".into(),
                        footnote: Some(FootnoteBody {
                            story: Story::from_text(&note, "Note"),
                            markers: Vec::new(),
                            reference_paragraph_style: "Body".into(),
                            reference_character_style: "Default".into(),
                        }),
                    });
                    doc.stories[0] = story;
                    let original = doc.clone();
                    let page = if kind == StoryPoint::OddPageBreak {
                        count * 2
                    } else {
                        count * 2 - 1
                    };
                    let flow = compose::compose_story(&doc, StoryId(0));
                    assert!(
                        !flow.has_overflow(),
                        "{kind:?}/{count}/{spanning}/{splitting}"
                    );
                    assert!(flow.frames[1..page * 2]
                        .iter()
                        .all(|f| f.lines.is_empty() && f.footnotes.is_empty()));
                    let text: String = flow
                        .lines()
                        .filter(|l| l.generated.is_none())
                        .map(|l| doc.stories[0].slice(l.start, l.end))
                        .collect();
                    assert_eq!(text, doc.stories[0].text().replace('\n', ""));
                    let notes: Vec<_> = flow.frames.iter().flat_map(|f| &f.footnotes).collect();
                    assert!(!notes.is_empty());
                    assert!(notes.iter().all(|n| n.anchor == anchor));
                    let rendered: String = notes
                        .iter()
                        .flat_map(|n| &n.lines)
                        .map(|l| compose::line_spec(l, &doc.stories[0], &doc).text)
                        .collect();
                    assert_eq!(rendered, note.replace('\n', ""));
                    assert_eq!(doc, original);
                }
            }
        }
    }
}
