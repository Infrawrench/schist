use schist_layout::{
    authoring, blank_a4, compose,
    lists::{ListKind, ListStyle, ListTab, MarkerAlignment},
    CharacterStyle, History, Rect, Story, StoryPoint, StyleRange,
};

fn document() -> schist_layout::LayoutDocument {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = blank_a4();
    let style = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Body")
        .unwrap();
    style.family = Some("IBM Plex Sans".into());
    style.point_size = Some(12.0);
    style.left_indent = Some(24.0);
    style.first_line_indent = Some(-24.0);
    style.list = ListStyle {
        kind: Some(ListKind::Numbered),
        ..Default::default()
    };
    doc
}

fn paragraphs(text: &str) -> Story {
    Story {
        points: text
            .split('\n')
            .map(|text| StoryPoint::Paragraph {
                text: text.into(),
                style: "Body".into(),
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn generated_numbers_are_once_per_paragraph_and_stable_across_reflow_and_threads() {
    for width in [90.0, 130.0, 300.0] {
        for height in [55.0, 110.0, 300.0] {
            let mut doc = document();
            let text = (0..12)
                .map(|_| "A long list item with é and several words.")
                .collect::<Vec<_>>()
                .join("\n");
            let story = doc.add_story(paragraphs(&text));
            let before = doc.story(story).unwrap().clone();
            let frames = (0..50)
                .map(|_| {
                    (
                        schist_layout::ObjectId::next(),
                        Rect::new(10.0, 20.0, width, height),
                        schist_layout::FrameOverflow::Thread,
                        1,
                        0.0,
                        compose::InsetsLike::default(),
                    )
                })
                .collect::<Vec<_>>();
            let flow = compose::compose_thread(&doc, story, &frames);
            assert!(!flow.has_overflow(), "{width}/{height}");
            let offsets = before.point_offsets();
            let markers = flow
                .lines()
                .filter(|l| l.generated.is_some())
                .collect::<Vec<_>>();
            assert_eq!(markers.len(), 12, "{width}/{height}");
            for (i, marker) in markers.into_iter().enumerate() {
                assert_eq!((marker.start, marker.end), (offsets[i], offsets[i]));
                assert_eq!(
                    marker.generated.as_ref().unwrap().spec.text,
                    format!("{}.", i + 1)
                );
                let body = flow
                    .lines()
                    .find(|l| l.generated.is_none() && l.start == marker.start)
                    .unwrap();
                assert!(body.bounds.x >= marker.bounds.right());
                assert!((marker.baseline - body.baseline).abs() < 0.001);
            }
            assert_eq!(flow.frames.last().unwrap().consumed_to, text.len());
            assert_eq!(doc.story(story).unwrap(), &before);
            for line in flow.lines().filter(|l| l.generated.is_none()) {
                for (position, _) in
                    schist_text_engine::insertion_points(&compose::line_spec(line, &before, &doc))
                {
                    assert!(text.is_char_boundary(line.start + position.byte));
                    assert!(line.start + position.byte <= line.end);
                }
            }
        }
    }
}

#[test]
fn sequences_follow_named_list_identity_and_explicit_restarts_not_style_identity() {
    let mut doc = document();
    for (name, key, restart, start) in [
        ("A", "a", false, 3),
        ("Alias", "a", false, 50),
        ("B", "b", false, 7),
        ("Restart", "a", true, 19),
    ] {
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: name.into(),
            based_on: Some("Body".into()),
            list: ListStyle {
                list: Some(key.into()),
                start: Some(start),
                continue_numbering: Some(!restart),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    doc.styles.add_paragraph(schist_layout::ParagraphStyle {
        name: "Nested".into(),
        based_on: Some("A".into()),
        list: ListStyle {
            level: Some(2),
            ..Default::default()
        },
        ..Default::default()
    });
    let styles = [
        "A", "Nested", "B", "Alias", "Nested", "A", "Restart", "B", "Alias",
    ];
    let story = Story {
        points: styles
            .iter()
            .map(|s| StoryPoint::Paragraph {
                text: "word".into(),
                style: (*s).into(),
            })
            .collect(),
        ..Default::default()
    };
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 200.0, 600.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = story;
    let flow = compose::compose_story(&doc, frame.story);
    let actual = flow
        .lines()
        .filter_map(|l| l.generated.as_ref().map(|g| g.spec.text.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        ["3.", "3.", "7.", "4.", "1.", "5.", "19.", "8.", "20."]
    );
}

#[test]
fn markers_inherit_the_first_character_then_explicit_style_and_preserve_baselines() {
    for kind in [ListKind::Bullet, ListKind::Numbered] {
        for alignment in [
            MarkerAlignment::Left,
            MarkerAlignment::Center,
            MarkerAlignment::Right,
        ] {
            let mut doc = document();
            doc.styles.add_character(CharacterStyle {
                name: "First".into(),
                point_size: Some(20.0),
                fill: Some(schist_layout::Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
                ..Default::default()
            });
            doc.styles.add_character(CharacterStyle {
                name: "Marker".into(),
                tracking: Some(150.0),
                ..Default::default()
            });
            let body = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Body")
                .unwrap();
            body.list.kind = Some(kind);
            body.list.bullet_alignment = Some(alignment);
            body.list.numbering_alignment = Some(alignment);
            body.list.bullet_character_style = Some("Marker".into());
            body.list.numbering_character_style = Some("Marker".into());
            body.list.tabs = Some(vec![ListTab {
                position: 50.0,
                alignment: "LeftAlign".into(),
                alignment_character: ".".into(),
                leader: String::new(),
            }]);
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(30.0, 20.0, 200.0, 100.0),
            )
            .unwrap();
            let mut story = Story::from_text("é text", "Body");
            story.ranges.push(StyleRange::new(0, 2, "First"));
            doc.stories[frame.story.0 as usize] = story;
            let flow = compose::compose_story(&doc, frame.story);
            let marker = flow.lines().find(|l| l.generated.is_some()).unwrap();
            let body = flow.lines().find(|l| l.generated.is_none()).unwrap();
            let generated = marker.generated.as_ref().unwrap();
            assert_eq!(generated.character.point_size, Some(20.0));
            assert_eq!(generated.character.tracking, Some(150.0));
            assert_eq!(generated.character.fill.as_ref().unwrap().name, "Cyan");
            assert_eq!(generated.spec.size, 20.0);
            assert_eq!(body.bounds.x, 80.0);
            let anchor = match alignment {
                MarkerAlignment::Left => marker.bounds.x,
                MarkerAlignment::Center => marker.bounds.x + marker.bounds.width / 2.0,
                MarkerAlignment::Right => marker.bounds.right(),
            };
            assert!((anchor - 30.0).abs() < 0.001);
            let metrics = schist_text_engine::measure(&generated.spec).unwrap();
            assert!((marker.bounds.y + metrics.first_baseline - body.baseline).abs() < 0.001);
        }
    }
}

#[test]
fn empty_paragraphs_have_markers_and_insufficient_marker_room_is_overset() {
    let mut doc = document();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 200.0, 400.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = paragraphs("\nword\n\n");
    let flow = compose::compose_story(&doc, frame.story);
    assert_eq!(
        flow.lines()
            .filter_map(|l| l.generated.as_ref().map(|g| g.spec.text.clone()))
            .collect::<Vec<_>>(),
        ["1.", "2.", "3.", "4."]
    );
    for pair in flow.frames[0].lines.as_chunks::<2>().0 {
        assert!(pair[0].generated.is_some());
        assert!(pair[1].bounds.x >= pair[0].bounds.right());
        assert_eq!(pair[1].bounds.x, 24.0);
    }
    for content in ["word", ""] {
        doc.stories[frame.story.0 as usize] = Story::from_text(content, "Body");
        doc.objects[0].bounds.width = 23.0;
        let flow = compose::compose_story(&doc, frame.story);
        assert!(flow.has_overflow(), "{content:?}");
        assert_eq!(flow.lines().count(), 0);
        doc.objects[0].bounds.width = 200.0;
        let flow = compose::compose_story(&doc, frame.story);
        assert!(!flow.has_overflow());
        assert_eq!(flow.lines().filter(|l| l.generated.is_some()).count(), 1);
    }
}

#[test]
fn renaming_marker_styles_updates_every_reference_as_one_reversible_operation() {
    let mut doc = document();
    doc.styles.add_character(CharacterStyle {
        name: "Marker".into(),
        point_size: Some(20.0),
        ..Default::default()
    });
    for index in 0..8 {
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: format!("List {index}"),
            list: ListStyle {
                bullet_character_style: Some("Marker".into()),
                numbering_character_style: Some("Marker".into()),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let before = doc.clone();
    let mut history = History::default();
    assert!(schist_layout::properties::rename_style(
        &mut doc,
        &mut history,
        false,
        "Marker",
        "Renamed"
    ));
    assert_eq!(history.undo_depth(), 1);
    for paragraph in doc
        .styles
        .paragraphs
        .iter()
        .filter(|p| p.name.starts_with("List "))
    {
        assert_eq!(
            paragraph.list.bullet_character_style.as_deref(),
            Some("Renamed")
        );
        assert_eq!(
            paragraph.list.numbering_character_style.as_deref(),
            Some("Renamed")
        );
    }
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn every_level_restarts_on_parent_events_and_reflows_without_changing_source() {
    for depth in 2..=9 {
        for restart in [true, false] {
            let mut doc = document();
            for level in 1..=depth {
                doc.styles.add_paragraph(schist_layout::ParagraphStyle {
                    name: format!("Level {level}"),
                    based_on: Some("Body".into()),
                    list: ListStyle {
                        level: Some(level),
                        apply_restart_policy: Some(restart),
                        expression: Some(
                            (1..level).map(|v| format!("^{v}.")).collect::<String>() + "^#^t",
                        ),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
            doc.styles.add_paragraph(schist_layout::ParagraphStyle {
                name: "Repeat parent".into(),
                based_on: Some("Level 1".into()),
                list: ListStyle {
                    continue_numbering: Some(false),
                    start: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            });
            let mut story = Story::default();
            let mut expected = Vec::new();
            let mut counts = vec![0; depth as usize];
            for (pass, level) in
                (0..3).flat_map(|pass| (1..=depth).chain([depth]).map(move |level| (pass, level)))
            {
                if level == 1 && pass > 0 {
                    counts[0] = 0;
                }
                if restart {
                    counts[level as usize..].fill(0);
                }
                counts[level as usize - 1] += 1;
                expected.push(
                    counts[..level as usize]
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join("."),
                );
                story.points.push(StoryPoint::Paragraph {
                    text: if level % 2 == 0 { "" } else { "é item" }.into(),
                    style: if level == 1 && pass > 0 {
                        "Repeat parent".into()
                    } else {
                        format!("Level {level}")
                    },
                });
            }
            let before = story.clone();
            let id = doc.add_story(story);
            for columns in [1, 3] {
                let frames = (0..30)
                    .map(|_| {
                        (
                            schist_layout::ObjectId::next(),
                            Rect::new(15.0, 20.0, 900.0, 100.0),
                            schist_layout::FrameOverflow::Thread,
                            columns,
                            10.0,
                            compose::InsetsLike::default(),
                        )
                    })
                    .collect::<Vec<_>>();
                let flow = compose::compose_thread(&doc, id, &frames);
                assert!(
                    !flow.has_overflow(),
                    "depth={depth},restart={restart},columns={columns}"
                );
                let actual = flow
                    .lines()
                    .filter_map(|l| l.generated.as_ref().map(|g| g.spec.text.clone()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual, expected,
                    "depth={depth},restart={restart},columns={columns}"
                );
                assert_eq!(doc.story(id).unwrap(), &before);
            }
        }
    }
}

#[test]
fn default_sequence_aliases_share_counters_but_display_names_never_merge_ids() {
    use schist_layout::{list_counters::StoryCounters, lists::NumberingList};
    let mut doc = document();
    for id in ["NumberingList/$ID/[Default]", "a", "b"] {
        doc.styles.numbering_lists.push(NumberingList {
            id: id.into(),
            name: "same".into(),
            ..Default::default()
        });
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: id.into(),
            based_on: Some("Body".into()),
            list: ListStyle {
                list: Some(id.into()),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let story = Story {
        points: [
            "Body",
            "a",
            "NumberingList/$ID/[Default]",
            "b",
            "Body",
            "a",
            "b",
        ]
        .into_iter()
        .map(|style| StoryPoint::Paragraph {
            text: "x".into(),
            style: style.into(),
        })
        .collect(),
        ..Default::default()
    };
    let counters = StoryCounters::new(&doc, &story);
    let plans = schist_layout::list_composition::MarkerPlans::new(&doc, &story);
    for (at, expected) in story
        .point_offsets()
        .into_iter()
        .zip(["1.", "1.", "2.", "1.", "3.", "2.", "2."])
    {
        assert_eq!(counters.issue(at), None);
        assert_eq!(plans.spec(at).unwrap().text, expected);
    }
    // A fresh call observes edits; there is no document-global stale cache.
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .list
        .start = Some(7);
    let updated = schist_layout::list_composition::MarkerPlans::new(&doc, &story);
    assert_eq!(plans.spec(0).unwrap().text, "1.");
    assert_eq!(updated.spec(0).unwrap().text, "7.");
    doc.styles.numbering_lists[0].across_stories = true;
    let restricted = schist_layout::list_composition::MarkerPlans::new(&doc, &story);
    for (at, is_default) in story
        .point_offsets()
        .into_iter()
        .zip([true, false, true, false, true, false, false])
    {
        assert_eq!(restricted.spec(at).is_none(), is_default);
    }
}

#[test]
fn missing_stale_or_unsupported_parent_counters_are_diagnosed_without_guessing() {
    use schist_layout::{
        list_counters::StoryCounters, list_numbering::CounterFormat, lists::RestartPolicy,
    };
    let mut doc = document();
    for (name, level, expression, format) in [
        ("Parent", 1, "^#^t", CounterFormat::UpperRoman),
        ("Child", 2, "^1.^#^t", CounterFormat::LowerLetters),
        (
            "Grandchild",
            3,
            "^1.^2.^#^t",
            CounterFormat::SingleLeadingZeros,
        ),
    ] {
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: name.into(),
            based_on: Some("Body".into()),
            list: ListStyle {
                level: Some(level),
                expression: Some(expression.into()),
                format: Some(format.native()),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let mut story = Story::default();
    for style in [
        "Child",
        "Parent",
        "Child",
        "Grandchild",
        "Parent",
        "Grandchild",
        "Child",
        "Grandchild",
    ] {
        story.points.push(StoryPoint::Paragraph {
            text: "x".into(),
            style: style.into(),
        });
    }
    let plans = schist_layout::list_composition::MarkerPlans::new(&doc, &story);
    let counters = StoryCounters::new(&doc, &story);
    for ((at, marker), issue) in story
        .point_offsets()
        .into_iter()
        .zip([
            None,
            Some("I"),
            Some("I.a"),
            Some("I.a.01"),
            Some("II"),
            None,
            Some("II.a"),
            Some("II.a.01"),
        ])
        .zip([true, false, false, false, false, true, false, false])
    {
        assert_eq!(plans.spec(at).map(|s| s.text.as_str()), marker);
        assert_eq!(
            counters.issue(at),
            issue.then_some("NumberingExpression.MissingLevel")
        );
    }
    // An explicit child restart wins over any parent event.
    let child = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Child")
        .unwrap();
    child.list.continue_numbering = Some(false);
    child.list.start = Some(7);
    let restarted = schist_layout::list_composition::MarkerPlans::new(&doc, &story);
    for (index, expected) in [(2, "I.g"), (6, "II.g")] {
        assert_eq!(
            restarted.spec(story.point_offsets()[index]).unwrap().text,
            expected
        );
    }
    for policy in ["AfterSpecificLevel", "RangeOfLevels", "unknown"] {
        let child = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Child")
            .unwrap();
        child.list.restart_policy = Some(RestartPolicy {
            policy: policy.into(),
            lower: 1,
            upper: 2,
        });
        child.list.apply_restart_policy = Some(true);
        let counters = StoryCounters::new(&doc, &story);
        assert_eq!(
            counters.issue(story.point_offsets()[2]),
            Some("NumberingRestartPolicies")
        );
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Child")
            .unwrap()
            .list
            .apply_restart_policy = Some(false);
        let counters = StoryCounters::new(&doc, &story);
        assert_eq!(counters.issue(story.point_offsets()[2]), None);
    }
}
