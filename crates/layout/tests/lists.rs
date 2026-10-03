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
fn native_marker_tabs_choose_an_ahead_hanging_indent_before_a_later_explicit_stop() {
    use schist_layout::list_composition::GeneratedRole;
    // Public InDesign 20 list-markers.pdf: observed body origins after
    // removing the common cXX word's 0.125 pt PDF bearing. These cases are
    // insensitive to the marker's precise font advance; no font agreement
    // is inferred from this numeric placement comparison.
    for (case, kind, indent, first, stop, expected) in [
        (0, ListKind::Bullet, 18.0, -18.0, None, 18.0),
        (1, ListKind::Bullet, 18.0, -18.0, None, 18.0),
        (3, ListKind::Bullet, 0.0, 0.0, None, 36.0),
        (4, ListKind::Bullet, 18.0, 0.0, None, 36.0),
        (5, ListKind::Bullet, 50.0, -20.0, None, 50.0),
        (6, ListKind::Bullet, 50.0, -50.0, Some(30.0), 30.0),
        (7, ListKind::Bullet, 30.0, -30.0, Some(60.0), 30.0),
        (8, ListKind::Numbered, 18.0, -18.0, None, 18.0),
        (10, ListKind::Numbered, 50.0, -50.0, Some(10.0), 10.0),
        (11, ListKind::Numbered, 6.0, -6.0, None, 36.0),
        (13, ListKind::Bullet, 18.0, -18.0, Some(2.0), 18.0),
    ] {
        let mut doc = document();
        let style = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Body")
            .unwrap();
        style.point_size = Some(10.0);
        style.left_indent = Some(indent);
        style.first_line_indent = Some(first);
        style.list.kind = Some(kind);
        style.list.tabs = stop.map(|position| {
            vec![ListTab {
                position,
                alignment: "LeftAlign".into(),
                alignment_character: ".".into(),
                leader: ".".into(),
            }]
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(72.0, 72.0, 200.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] =
            paragraphs(&format!("c{case:02} one\nc{case:02} two"));
        let flow = compose::compose_story(&doc, frame.story);
        assert!(!flow.has_overflow());
        for body in flow.lines().filter(|l| l.generated.is_none()) {
            assert!(
                (body.bounds.x - body.inline_origin - expected).abs() < 0.001,
                "case {case}: {} != {expected}",
                body.bounds.x - body.inline_origin
            );
        }
        let leaders = flow
            .lines()
            .filter(|l| {
                l.generated
                    .as_ref()
                    .is_some_and(|g| g.role == GeneratedRole::Leader)
            })
            .count();
        assert_eq!(
            leaders,
            if stop == Some(expected) { 2 } else { 0 },
            "case {case}: a virtual/passed stop cannot borrow a leader"
        );
    }
}

#[test]
fn marker_leaders_add_only_selected_gap_paint_without_changing_counters_or_source_carets() {
    use schist_layout::list_composition::GeneratedRole;
    for kind in [ListKind::Bullet, ListKind::Numbered] {
        for alignment in [
            MarkerAlignment::Left,
            MarkerAlignment::Center,
            MarkerAlignment::Right,
        ] {
            for leader in [".", ". ", "fi"] {
                for (positions, legacy) in [
                    (vec![0.0, 120.0], None),
                    (vec![0.0, 0.0], None),
                    (vec![120.0], Some(5.0)),
                ] {
                    let mut doc = document();
                    let style = doc
                        .styles
                        .paragraphs
                        .iter_mut()
                        .find(|p| p.name == "Body")
                        .unwrap();
                    style.left_indent = Some(0.0);
                    style.first_line_indent = Some(0.0);
                    style.list.kind = Some(kind);
                    style.list.bullet_alignment = Some(alignment);
                    style.list.numbering_alignment = Some(alignment);
                    style.list.legacy_gap = legacy;
                    style.list.tabs = Some(
                        positions
                            .iter()
                            .map(|position| ListTab {
                                position: *position,
                                alignment: "LeftAlign".into(),
                                alignment_character: ".".into(),
                                leader: leader.into(),
                            })
                            .collect(),
                    );
                    let frame = authoring::text_frame(
                        &mut doc,
                        &mut History::default(),
                        0,
                        Rect::new(30.25, 20.5, 260.0, 400.0),
                    )
                    .unwrap();
                    doc.stories[frame.story.0 as usize] =
                        paragraphs("é first paragraph with words\nsecond paragraph");
                    let original = doc.stories.clone();
                    let actual = compose::compose_story(&doc, frame.story);
                    let mut plain = doc.clone();
                    for tab in plain
                        .styles
                        .paragraphs
                        .iter_mut()
                        .find(|p| p.name == "Body")
                        .unwrap()
                        .list
                        .tabs
                        .as_mut()
                        .unwrap()
                    {
                        tab.leader.clear();
                    }
                    let expected = compose::compose_story(&plain, frame.story);
                    let capture =
                        |flow: &compose::ComposedThread, doc: &schist_layout::LayoutDocument| {
                            flow.lines()
                                .filter(|line| {
                                    line.generated
                                        .as_ref()
                                        .is_none_or(|g| g.role == GeneratedRole::Marker)
                                })
                                .map(|line| {
                                    let spec = compose::line_spec(
                                        line,
                                        doc.story(frame.story).unwrap(),
                                        doc,
                                    );
                                    let ink = schist_text_engine::rasterize(&spec).unwrap();
                                    (
                                        line.start,
                                        line.end,
                                        line.bounds,
                                        spec.text.clone(),
                                        schist_text_engine::insertion_points(&spec),
                                        ink.bounds,
                                        ink.coverage,
                                    )
                                })
                                .collect::<Vec<_>>()
                        };
                    assert_eq!(capture(&actual, &doc), capture(&expected, &plain));
                    let markers = actual
                        .lines()
                        .filter(|l| {
                            l.generated
                                .as_ref()
                                .is_some_and(|g| g.role == GeneratedRole::Marker)
                        })
                        .count();
                    let leaders = actual
                        .lines()
                        .filter(|l| {
                            l.generated
                                .as_ref()
                                .is_some_and(|g| g.role == GeneratedRole::Leader)
                        })
                        .count();
                    assert_eq!(markers, 2);
                    assert_eq!(
                        leaders,
                        if positions.last() == Some(&120.0) && legacy.is_none() {
                            2
                        } else {
                            0
                        }
                    );
                    for line in actual.lines().filter(|l| {
                        l.generated
                            .as_ref()
                            .is_some_and(|g| g.role == GeneratedRole::Leader)
                    }) {
                        let spec = compose::line_spec(line, doc.story(frame.story).unwrap(), &doc);
                        assert!(schist_text_engine::rasterize(&spec)
                            .unwrap()
                            .coverage
                            .iter()
                            .any(|v| *v > 0));
                    }
                    assert_eq!(doc.stories, original);
                }
            }
        }
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
fn unsupported_rtl_markers_are_diagnosed_without_painting_at_the_wrong_column_edge() {
    use schist_layout::styles::ParagraphDirection;
    for direction in [
        None,
        Some(ParagraphDirection::LeftToRight),
        Some(ParagraphDirection::RightToLeft),
        Some(ParagraphDirection::Auto),
    ] {
        for text in ["A list item", "אב list item"] {
            let mut doc = document();
            doc.styles
                .paragraphs
                .iter_mut()
                .find(|p| p.name == "Body")
                .unwrap()
                .direction = direction;
            let original = paragraphs(text);
            let paragraph = doc.styles.resolve_paragraph("Body");
            let rtl = direction == Some(ParagraphDirection::RightToLeft)
                || matches!(direction, None | Some(ParagraphDirection::Auto))
                    && text.starts_with('א');
            assert_eq!(
                schist_layout::list_composition::unsupported_paragraph(&paragraph, text).is_empty(),
                !rtl
            );
            let markers = schist_layout::list_composition::MarkerPlans::new(&doc, &original);
            assert_eq!(markers.spec(0).is_some(), !rtl);
            let id = doc.add_story(original.clone());
            let flow = compose::compose_thread(
                &doc,
                id,
                &[(
                    schist_layout::ObjectId::next(),
                    Rect::new(10.0, 20.0, 300.0, 100.0),
                    schist_layout::FrameOverflow::Thread,
                    1,
                    0.0,
                    compose::InsetsLike::default(),
                )],
            );
            assert!(!flow.has_overflow());
            assert_eq!(
                flow.lines().filter(|line| line.generated.is_some()).count(),
                usize::from(!rtl)
            );
            assert_eq!(doc.story(id).unwrap(), &original);
            let mut inactive = paragraph;
            inactive.list.kind = Some(ListKind::None);
            assert!(
                schist_layout::list_composition::unsupported_paragraph(&inactive, text).is_empty()
            );
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
            // The ahead 24 pt hanging indent precedes the explicit 50 pt
            // stop, as observed in native list-marker case c07.
            assert_eq!(body.bounds.x, 54.0);
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

fn cross_story_document(
    count: usize,
) -> (schist_layout::LayoutDocument, Vec<schist_layout::ObjectId>) {
    use schist_layout::lists::NumberingList;
    let mut doc = document();
    doc.styles.numbering_lists.push(NumberingList {
        id: "shared".into(),
        name: "Shared".into(),
        across_stories: true,
        ..Default::default()
    });
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .list
        .list = Some("shared".into());
    let mut ids = Vec::new();
    for _ in 0..count {
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(72.0, 72.0, 200.0, 200.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = paragraphs("x\nx");
        ids.push(frame.object);
    }
    (doc, ids)
}

fn object_markers(doc: &schist_layout::LayoutDocument, id: schist_layout::ObjectId) -> Vec<String> {
    let schist_layout::LayoutObject::TextFrame { story, .. } = doc.object(id).unwrap().object
    else {
        panic!("text frame")
    };
    let story = doc.story(story).unwrap();
    let plans = schist_layout::list_composition::MarkerPlans::new(doc, story);
    story
        .point_offsets()
        .iter()
        .filter_map(|at| plans.spec(*at).map(|spec| spec.text.clone()))
        .collect()
}

#[test]
fn cross_story_counters_follow_creation_not_paint_story_indices_or_geometry() {
    for count in [2, 3, 7] {
        let (mut doc, ids) = cross_story_document(count);
        for mutation in 0..3 {
            if mutation == 1 {
                doc.objects.reverse();
            }
            if mutation == 2 {
                doc.stories.reverse();
                for object in &mut doc.objects {
                    if let schist_layout::LayoutObject::TextFrame { story, .. } = &mut object.object
                    {
                        story.0 = count as u32 - 1 - story.0;
                    }
                    object.bounds.x += 10.0;
                    object.bounds.y -= 30.0;
                }
            }
            for (index, id) in ids.iter().enumerate() {
                assert_eq!(
                    object_markers(&doc, *id),
                    vec![format!("{}.", index * 2 + 1), format!("{}.", index * 2 + 2)]
                );
            }
        }
        let before = doc.clone();
        let mut history = History::default();
        assert!(authoring::delete_all(&mut doc, &mut history, &ids[..1]));
        // Unplaced tombstone stories do not contribute to the live list.
        assert_eq!(object_markers(&doc, ids[1]), vec!["1.", "2."]);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert_eq!(object_markers(&doc, ids[1]), vec!["3.", "4."]);
    }
}

#[test]
fn higher_level_restarts_use_monotonic_events_across_story_byte_offset_resets() {
    for level in [2, 4, 9] {
        let (mut doc, ids) = cross_story_document(3);
        for (name, depth, restart, expression) in [
            ("Root", 1, false, "^#."),
            ("Reset", 1, true, "^#."),
            ("Child", level, false, "^1.^#."),
        ] {
            doc.styles.add_paragraph(schist_layout::ParagraphStyle {
                name: name.into(),
                based_on: Some("Body".into()),
                list: ListStyle {
                    level: Some(depth),
                    continue_numbering: Some(!restart),
                    expression: Some(expression.into()),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        doc.stories[0].points = vec![
            StoryPoint::Paragraph {
                style: "Root".into(),
                text: "long source before child".repeat(10),
            },
            StoryPoint::Paragraph {
                style: "Child".into(),
                text: "x".into(),
            },
        ];
        doc.stories[1].points = vec![
            StoryPoint::Paragraph {
                style: "Reset".into(),
                text: "x".into(),
            },
            StoryPoint::Paragraph {
                style: "Child".into(),
                text: "x".into(),
            },
        ];
        doc.stories[2].points = vec![StoryPoint::Paragraph {
            style: "Child".into(),
            text: "x".into(),
        }];
        assert_eq!(object_markers(&doc, ids[0]), vec!["1.", "1.1."]);
        assert_eq!(object_markers(&doc, ids[1]), vec!["1.", "1.1."]);
        assert_eq!(object_markers(&doc, ids[2]), vec!["1.2."]);
    }
}

#[test]
fn uncertain_cross_story_order_suppresses_only_its_sequence_without_guessing() {
    use schist_layout::{list_counters::StoryCounters, lists::NumberingList};
    let (base, ids) = cross_story_document(2);
    for mutation in 0..7 {
        let mut doc = base.clone();
        let issue = match mutation {
            0 => {
                doc.creation_order.clear();
                "UnknownCreationOrder"
            }
            1 => {
                doc.creation_order.push(ids[0]);
                "UnknownCreationOrder"
            }
            2 => {
                let mut extra = doc.objects[0].clone();
                extra.id = schist_layout::ObjectId(u32::MAX);
                doc.add_object(extra);
                "ThreadedFrames"
            }
            3 => {
                doc.objects[1].page = usize::MAX;
                "PageOrder"
            }
            4 => {
                if let schist_layout::LayoutObject::TextFrame { story, .. } =
                    &mut doc.objects[0].object
                {
                    story.0 = u32::MAX;
                }
                "MissingFrame"
            }
            5 => {
                doc.styles
                    .numbering_lists
                    .push(doc.styles.numbering_lists[0].clone());
                "AmbiguousList"
            }
            6 => {
                doc.styles.numbering_lists[0].across_documents = true;
                "ContinueNumbersAcrossDocuments"
            }
            _ => unreachable!(),
        };
        doc.styles.numbering_lists.push(NumberingList {
            id: "independent".into(),
            ..Default::default()
        });
        doc.styles.add_paragraph(schist_layout::ParagraphStyle {
            name: "Independent".into(),
            based_on: Some("Body".into()),
            list: ListStyle {
                list: Some("independent".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        doc.stories[0].push_paragraph("separate", "Independent");
        let story = &doc.stories[0];
        let counters = StoryCounters::new(&doc, story);
        assert!(
            counters.issue(0).unwrap().ends_with(issue),
            "{mutation}: {:?}",
            counters.issue(0)
        );
        let plans = schist_layout::list_composition::MarkerPlans::new(&doc, story);
        let valid: Vec<_> = story
            .point_offsets()
            .into_iter()
            .filter_map(|at| plans.spec(at).map(|spec| spec.text.as_str()))
            .collect();
        assert_eq!(valid, vec!["1."]);
    }
    let copied = base.stories[0].clone();
    assert_eq!(
        StoryCounters::new(&base, &copied).issue(0),
        Some("ContinueNumbersAcrossStories.UnknownStory")
    );
}
