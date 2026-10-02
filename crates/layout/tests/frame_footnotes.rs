use schist_layout::{
    authoring, compose::compose_story, footnotes::*, object_styles, History, LayoutDocument,
    LayoutObject, ObjectId, ObjectStyle, Rect, Story, StoryStructure,
};

fn document(count: usize) -> LayoutDocument {
    let mut doc = schist_layout::blank_a4();
    doc.footnotes = FootnoteOptions {
        no_splitting: Some(true),
        first_baseline: Some(FootnoteFirstBaseline::Ascent),
        end_of_story: Some(true),
        spacer: Some(3.0),
        space_between: Some(4.0),
        ..Default::default()
    };
    for _ in 0..count {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 40.0, 180.0, 240.0),
        )
        .unwrap();
        let mut story = Story::from_text("Body", "Body");
        for at in [0, 4] {
            story.structures.push(StoryStructure {
                at: Some(at),
                kind: "Footnote".into(),
                payload: "source".into(),
                footnote: Some(FootnoteBody {
                    story: Story::from_text("A note", "Body"),
                    markers: vec![],
                    reference_paragraph_style: "Body".into(),
                    reference_character_style: String::new(),
                }),
            });
        }
        doc.stories[frame.story.0 as usize] = story;
    }
    doc
}

#[test]
fn frame_overrides_inherit_styles_preserve_disabled_values_and_control_note_spacing() {
    for category in [None, Some(false), Some(true)] {
        for enabled in [None, Some(false), Some(true)] {
            let mut doc = document(1);
            doc.styles.objects.push(ObjectStyle {
                name: "Base".into(),
                enable_footnotes: Some(true),
                footnotes: FrameFootnotes {
                    enabled: Some(true),
                    straddle: Some(false),
                    spacer: Some(15.0),
                    space_between: Some(9.0),
                },
                ..Default::default()
            });
            doc.styles.objects.push(ObjectStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                enable_footnotes: category,
                footnotes: FrameFootnotes {
                    spacer: Some(21.0),
                    ..Default::default()
                },
                ..Default::default()
            });
            doc.objects[0].appearance.style = Some("Child".into());
            let LayoutObject::TextFrame {
                footnotes, story, ..
            } = &mut doc.objects[0].object
            else {
                panic!()
            };
            let story = *story;
            *footnotes = FrameFootnotes {
                enabled,
                space_between: Some(12.0),
                ..Default::default()
            };
            let inherited = category != Some(false);
            let active = enabled.unwrap_or(inherited);
            let effective = doc.styles.frame_footnotes(&doc.objects[0]);
            assert_eq!(effective.spacer, inherited.then_some(21.0));
            assert_eq!(effective.space_between, Some(12.0));
            let saved = doc.clone();
            let frame = compose_story(&doc, story).frames.remove(0);
            assert!(!frame.lost);
            assert_eq!(frame.footnotes.len(), 2);
            let body_bottom = frame
                .lines
                .iter()
                .map(|l| l.bounds.bottom())
                .fold(0.0, f32::max);
            let spacer = if active && inherited { 21.0 } else { 3.0 };
            let between = if active { 12.0 } else { 4.0 };
            assert!((frame.footnotes[0].bounds.y - body_bottom - spacer).abs() < 0.001);
            assert!(
                (frame.footnotes[1].bounds.y - frame.footnotes[0].bounds.bottom() - between).abs()
                    < 0.001
            );
            assert_eq!(doc, saved);
        }
    }
}

#[test]
fn frame_settings_are_atomic_for_any_selection_and_survive_duplicate_and_style_detach() {
    for count in [1, 3, 8] {
        let mut doc = document(count);
        let ids: Vec<_> = doc.objects.iter().map(|o| o.id).collect();
        let options = FrameFootnotes {
            enabled: Some(false),
            spacer: Some(17.0),
            space_between: Some(11.0),
            straddle: Some(false),
        };
        let original = doc.clone();
        let mut history = History::default();
        let mut invalid = ids.clone();
        invalid.push(ObjectId(u32::MAX));
        assert!(!set_frame_options(
            &mut doc,
            &mut history,
            &invalid,
            options.clone()
        ));
        for value in [-1.0, f32::NAN, f32::INFINITY, 865.0] {
            assert!(!set_frame_options(
                &mut doc,
                &mut history,
                &ids,
                FrameFootnotes {
                    spacer: Some(value),
                    ..options.clone()
                }
            ));
        }
        assert_eq!(doc, original);
        assert_eq!(history.undo_depth(), 0);
        let mut repeated = ids.clone();
        repeated.extend(&ids);
        assert!(set_frame_options(
            &mut doc,
            &mut history,
            &repeated,
            options.clone()
        ));
        assert_eq!(history.undo_depth(), 1);
        let changed = doc.clone();
        assert!(!set_frame_options(
            &mut doc,
            &mut history,
            &ids,
            options.clone()
        ));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, changed);
        let copy = authoring::duplicate(&mut doc, &mut history, ids[0]).unwrap();
        assert_eq!(
            doc.styles.frame_footnotes(doc.object(copy).unwrap()),
            options
        );
        assert!(history.undo(&mut doc));
        assert_eq!(doc, changed);
        doc.styles.objects.push(ObjectStyle {
            name: "Notes".into(),
            enable_footnotes: Some(true),
            footnotes: FrameFootnotes {
                enabled: Some(true),
                ..options.clone()
            },
            ..Default::default()
        });
        let before_style = doc.clone();
        let depth = history.undo_depth();
        assert!(object_styles::apply_style(
            &mut doc,
            &mut history,
            &ids,
            Some("Notes")
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        let styled = doc.clone();
        assert!(object_styles::apply_style(
            &mut doc,
            &mut history,
            &ids,
            None
        ));
        for (object, before) in doc.objects.iter().zip(&styled.objects) {
            assert_eq!(
                doc.styles.frame_footnotes(object),
                styled.styles.frame_footnotes(before)
            );
        }
        assert!(history.undo(&mut doc));
        assert_eq!(doc, styled);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before_style);
    }
}

#[test]
fn column_flow_and_balancing_keep_each_whole_note_with_its_reference_in_reading_order() {
    use schist_layout::{FrameOverflow, Insets, StoryDirection, StoryId, StoryPoint};
    for columns in [2, 3] {
        for reverse in [false, true] {
            for (height, balanced) in [55.0, 160.0, 400.0]
                .into_iter()
                .flat_map(|height| [false, true].map(|balanced| (height, balanced)))
            {
                for forced in [false, true] {
                    for below_text in [false, true] {
                        let mut doc = document(1);
                        if let LayoutObject::TextFrame {
                            balance_columns, ..
                        } = &mut doc.objects[0].object
                        {
                            *balance_columns = Some(balanced);
                        }
                        doc.footnotes.straddle = Some(false);
                        doc.footnotes.end_of_story = Some(below_text);
                        let mut story = Story::new();
                        for i in 0..columns * 2 {
                            if forced && i > 0 && i % 2 == 0 {
                                story.points.push(StoryPoint::ColumnBreak);
                            }
                            story.push_paragraph("Alpha béta.", "Body");
                        }
                        if reverse {
                            story.prefs.direction = StoryDirection::RightToLeft;
                        }
                        for (point, at) in story.points.iter().zip(story.point_offsets()) {
                            if matches!(point, StoryPoint::Paragraph { .. }) {
                                story.structures.push(StoryStructure {
                                    at: Some(at + 5),
                                    kind: "Footnote".into(),
                                    payload: "source".into(),
                                    footnote: Some(FootnoteBody {
                                        story: Story::from_text("Note", "Body"),
                                        markers: vec![],
                                        reference_paragraph_style: "Body".into(),
                                        reference_character_style: String::new(),
                                    }),
                                });
                            }
                        }
                        let text = story.text();
                        let count = story.structures.len();
                        doc.stories[0] = story;
                        let source = doc.clone();
                        let first = Rect::new(20.0, 40.0, 180.0, height);
                        // Forced column breaks need actual following columns;
                        // extra height in one column cannot consume a break.
                        let next = Rect::new(20.0, 40.0, 360.0, 800.0);
                        let frames = [
                            (
                                doc.objects[0].id,
                                first,
                                FrameOverflow::Thread,
                                columns,
                                10.0,
                                Insets::ZERO.into(),
                            ),
                            (
                                ObjectId(u32::MAX),
                                next,
                                FrameOverflow::Thread,
                                columns,
                                10.0,
                                Insets::ZERO.into(),
                            ),
                        ];
                        let flow = schist_layout::compose_thread(&doc, StoryId(0), &frames);
                        assert!(
                            !flow.frames.last().unwrap().lost,
                            "columns={columns} height={height} forced={forced}"
                        );
                        assert_eq!(flow.frames.last().unwrap().consumed_to, text.len());
                        let mut seen = std::collections::BTreeSet::new();
                        for (index, frame) in flow.frames.iter().enumerate() {
                            let bounds = if index == 0 { first } else { next };
                            assert_eq!(frame.unrendered_structures, 0);
                            let mut previous_x = None;
                            for note in &frame.footnotes {
                                assert!(seen.insert(note.structure));
                                let number = (note.structure + 1).to_string();
                                let reference = frame
                                    .lines
                                    .iter()
                                    .find(|line| {
                                        schist_layout::compose::line_spec(
                                            line,
                                            &doc.stories[0],
                                            &doc,
                                        )
                                        .text
                                        .contains(&number)
                                    })
                                    .unwrap();
                                assert!(reference.bounds.x + 0.001 >= note.bounds.x);
                                assert!(reference.bounds.right() <= note.bounds.right() + 0.001);
                                let bottom = frame
                                    .lines
                                    .iter()
                                    .filter(|line| {
                                        line.bounds.x + 0.001 >= note.bounds.x
                                            && line.bounds.right() <= note.bounds.right() + 0.001
                                    })
                                    .map(|line| line.bounds.bottom())
                                    .fold(bounds.y, f32::max);
                                assert!(note.bounds.y + 0.001 >= bottom + 3.0);
                                assert!(note.bounds.bottom() <= bounds.bottom() + 0.001);
                                if let Some(x) = previous_x {
                                    assert!(if reverse {
                                        note.bounds.x <= x
                                    } else {
                                        note.bounds.x >= x
                                    });
                                }
                                previous_x = Some(note.bounds.x);
                                if !below_text
                                    && !frame.footnotes.iter().any(|other| {
                                        other.bounds.x == note.bounds.x
                                            && other.bounds.y > note.bounds.y
                                    })
                                {
                                    assert!((note.bounds.bottom() - bounds.bottom()).abs() < 0.001);
                                }
                            }
                        }
                        assert_eq!(seen.len(), count);
                        assert_eq!(doc, source);
                    }
                }
            }
        }
    }
}

#[test]
fn frame_creation_defaults_are_reversible_and_do_not_rewrite_existing_frames() {
    let mut doc = document(2);
    let original = doc.clone();
    let mut history = History::default();
    let defaults = FrameFootnotes {
        enabled: Some(true),
        spacer: Some(32.0),
        space_between: Some(14.0),
        straddle: Some(false),
    };
    assert!(set_frame_defaults(&mut doc, &mut history, defaults.clone()));
    assert_eq!(history.undo_depth(), 1);
    assert_eq!(doc.objects, original.objects);
    assert!(!set_frame_defaults(
        &mut doc,
        &mut history,
        defaults.clone()
    ));
    let before_frame = doc.clone();
    let added = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(20.0, 20.0, 100.0, 200.0),
    )
    .unwrap();
    assert_eq!(
        doc.styles
            .frame_footnotes(doc.object(added.object).unwrap()),
        defaults
    );
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before_frame);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, original);
}
