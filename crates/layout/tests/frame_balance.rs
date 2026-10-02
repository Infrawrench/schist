use schist_layout::{
    authoring, blank_a4, compose::compose_story, frame_text, object_styles, History, LayoutObject,
    ObjectStyle, Rect, Story, StoryDirection,
};

fn document(count: usize) -> schist_layout::LayoutDocument {
    let mut doc = blank_a4();
    for _ in 0..count {
        authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 30.0, 440.0, 600.0),
        )
        .unwrap();
    }
    doc
}

#[test]
fn optional_balancing_changes_column_occupancy_without_changing_source_or_consumption() {
    for count in 2..=4 {
        for direction in [StoryDirection::LeftToRight, StoryDirection::RightToLeft] {
            for balanced in [false, true] {
                let mut doc = document(1);
                let id = doc.objects[0].id;
                let LayoutObject::TextFrame {
                    columns,
                    gutter,
                    balance_columns,
                    ..
                } = &mut doc.objects[0].object
                else {
                    panic!()
                };
                *columns = count;
                *gutter = 10.0;
                *balance_columns = Some(balanced);
                let mut story = Story::new();
                story.prefs.direction = direction;
                for _ in 0..count * 2 {
                    story.push_paragraph("Short text.", "Body");
                }
                doc.stories[0] = story;
                let before = doc.clone();
                let flow = compose_story(&doc, schist_layout::StoryId(0));
                assert!(!flow.has_overflow());
                assert_eq!(flow.frames[0].object, id);
                assert_eq!(flow.frames[0].consumed_to, doc.stories[0].text().len());
                let mut positions = std::collections::BTreeSet::new();
                for line in flow.lines() {
                    positions.insert(line.bounds.x.round() as i32);
                }
                assert_eq!(positions.len(), if balanced { count as usize } else { 1 });
                let first = flow.lines().next().unwrap().bounds.x.round();
                assert_eq!(
                    first,
                    if direction == StoryDirection::LeftToRight {
                        *positions.first().unwrap() as f32
                    } else {
                        *positions.last().unwrap() as f32
                    }
                );
                assert_eq!(doc, before);
            }
        }
    }
}

#[test]
fn style_categories_local_values_and_detaching_preserve_effective_balance() {
    for category in [None, Some(false), Some(true)] {
        for inherited in [false, true] {
            for local in [None, Some(false), Some(true)] {
                let mut doc = document(1);
                doc.styles.objects.extend([
                    ObjectStyle {
                        name: "Base".into(),
                        enable_text_frame_general: Some(true),
                        balance_columns: Some(inherited),
                        ..Default::default()
                    },
                    ObjectStyle {
                        name: "Child".into(),
                        based_on: Some("Base".into()),
                        enable_text_frame_general: category,
                        ..Default::default()
                    },
                ]);
                doc.objects[0].appearance.style = Some("Child".into());
                let LayoutObject::TextFrame {
                    balance_columns, ..
                } = &mut doc.objects[0].object
                else {
                    panic!()
                };
                *balance_columns = local;
                let expected = local.unwrap_or(category != Some(false) && inherited);
                assert_eq!(doc.styles.frame_balance(&doc.objects[0]), expected);
                let before = doc.clone();
                let id = doc.objects[0].id;
                let mut history = History::default();
                assert!(object_styles::apply_style(
                    &mut doc,
                    &mut history,
                    &[id],
                    None
                ));
                assert_eq!(doc.styles.frame_balance(&doc.objects[0]), expected);
                assert!(history.undo(&mut doc));
                assert_eq!(doc, before);
                object_styles::apply_style(&mut doc, &mut history, &[id], Some("Child"));
                assert_eq!(
                    doc.styles.frame_balance(&doc.objects[0]),
                    if category == Some(false) {
                        expected
                    } else {
                        inherited
                    }
                );
            }
        }
    }
}

#[test]
fn balance_selection_and_creation_defaults_are_single_atomic_undo_steps() {
    for count in [1, 3, 8] {
        let mut doc = document(count);
        let before = doc.clone();
        let ids: Vec<_> = doc.objects.iter().map(|o| o.id).collect();
        let mut history = History::default();
        let mut duplicate_ids = ids.clone();
        duplicate_ids.extend(&ids);
        assert!(frame_text::set_balance(
            &mut doc,
            &mut history,
            &duplicate_ids,
            Some(true)
        ));
        assert_eq!(history.undo_depth(), 1);
        assert!(!frame_text::set_balance(
            &mut doc,
            &mut history,
            &ids,
            Some(true)
        ));
        assert!(doc.objects.iter().all(|o| doc.styles.frame_balance(o)));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        doc.objects[count - 1].locked = true;
        let locked = doc.clone();
        assert!(!frame_text::set_balance(
            &mut doc,
            &mut history,
            &ids,
            Some(true)
        ));
        assert_eq!(doc, locked);
        assert_eq!(history.undo_depth(), 0);
        doc.objects[count - 1].locked = false;
        assert!(frame_text::set_default(&mut doc, &mut history, true));
        assert_eq!(history.undo_depth(), 1);
        assert_eq!(doc.objects, before.objects);
        authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 100.0))
            .unwrap();
        assert!(doc.styles.frame_balance(doc.objects.last().unwrap()));
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
    }
}

#[test]
fn old_snapshots_preserve_balancing_while_new_frames_use_explicit_defaults() {
    let doc = document(1);
    assert!(!doc.styles.frame_balance(&doc.objects[0]));
    let mut json = serde_json::to_value(&doc.objects[0]).unwrap();
    json["object"]["TextFrame"]
        .as_object_mut()
        .unwrap()
        .remove("balance_columns");
    let restored: schist_layout::PlacedObject = serde_json::from_value(json).unwrap();
    assert!(doc.styles.frame_balance(&restored));
    let mut cloned = doc.clone();
    let mut history = History::default();
    let id = cloned.objects[0].id;
    frame_text::set_balance(&mut cloned, &mut history, &[id], Some(true));
    let copy = authoring::duplicate(&mut cloned, &mut history, id).unwrap();
    assert!(cloned.styles.frame_balance(cloned.object(copy).unwrap()));
}

#[test]
fn balancing_can_split_paragraphs_without_changing_lines_or_violating_widows() {
    use schist_layout::{styles::Leading, ParagraphStyle, StoryId};
    for columns in [2, 3, 4] {
        for keep in [1, 2, 3] {
            for paragraphs in [1, 3] {
                for direction in [StoryDirection::LeftToRight, StoryDirection::RightToLeft] {
                    let mut doc = document(1);
                    doc.objects[0].bounds =
                        Rect::new(20.0, 30.0, columns as f32 * 170.0 - 10.0, 3000.0);
                    let LayoutObject::TextFrame {
                        columns: count,
                        gutter,
                        ..
                    } = &mut doc.objects[0].object
                    else {
                        panic!()
                    };
                    *count = columns;
                    *gutter = 10.0;
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Balanced body".into(),
                        point_size: Some(12.0),
                        leading: Some(Leading::Points(16.0)),
                        keep_lines: Some(keep),
                        first_line_indent: Some(17.0),
                        ..Default::default()
                    });
                    let mut story = Story::new();
                    story.prefs.direction = direction;
                    for _ in 0..paragraphs {
                        story.push_paragraph("Words and café text flow through the narrow columns without losing a character. ".repeat(8), "Balanced body");
                    }
                    doc.stories[0] = story;
                    let signature = |flow: &schist_layout::ComposedThread| {
                        flow.lines()
                            .map(|line| {
                                (
                                    line.start,
                                    line.end,
                                    line.is_paragraph_end,
                                    line.bounds.width,
                                )
                            })
                            .collect::<Vec<_>>()
                    };
                    let sequential = compose_story(&doc, StoryId(0));
                    assert!(!sequential.has_overflow());
                    let expected = signature(&sequential);
                    if let LayoutObject::TextFrame {
                        balance_columns, ..
                    } = &mut doc.objects[0].object
                    {
                        *balance_columns = Some(true);
                    }
                    let before = doc.clone();
                    let balanced = compose_story(&doc, StoryId(0));
                    assert!(!balanced.has_overflow());
                    assert_eq!(
                        signature(&balanced),
                        expected,
                        "balancing changed the shaped line boundaries"
                    );
                    let mut per_column = std::collections::BTreeMap::<i32, usize>::new();
                    let mut segments = std::collections::BTreeMap::<(i32, usize), usize>::new();
                    let offsets = doc.stories[0].point_offsets();
                    for line in balanced.lines() {
                        let x = line.inline_origin.round() as i32;
                        *per_column.entry(x).or_default() += 1;
                        let paragraph = offsets
                            .partition_point(|start| *start <= line.start)
                            .saturating_sub(1);
                        *segments.entry((x, paragraph)).or_default() += 1;
                    }
                    assert_eq!(
                        per_column.len(),
                        columns as usize,
                        "a splittable paragraph left columns empty"
                    );
                    assert!(
                        per_column.values().max().unwrap() - per_column.values().min().unwrap()
                            <= keep,
                        "lopsided columns: {per_column:?}"
                    );
                    assert!(
                        segments.values().all(|lines| *lines >= keep),
                        "a split stranded too few lines: {segments:?}"
                    );
                    assert_eq!(doc, before);
                }
            }
        }
    }
}

#[test]
fn balancing_a_single_paragraph_keeps_complete_notes_with_their_reference_columns() {
    use schist_layout::{compose::line_spec, footnotes::*, StoryId, StoryStructure};
    for columns in [2, 3] {
        for direction in [StoryDirection::LeftToRight, StoryDirection::RightToLeft] {
            let mut doc = document(1);
            doc.objects[0].bounds = Rect::new(20.0, 30.0, columns as f32 * 170.0 - 10.0, 2000.0);
            let LayoutObject::TextFrame {
                columns: count,
                gutter,
                balance_columns,
                ..
            } = &mut doc.objects[0].object
            else {
                panic!()
            };
            *count = columns;
            *gutter = 10.0;
            *balance_columns = Some(true);
            doc.footnotes = FootnoteOptions {
                no_splitting: Some(true),
                straddle: Some(false),
                first_baseline: Some(FootnoteFirstBaseline::Ascent),
                spacer: Some(8.0),
                ..Default::default()
            };
            doc.footnotes.rule.on = Some(false);
            let unit = "Words and café text flow through columns without losing a character. ";
            let mut story = Story::from_text(unit.repeat(12), "Body");
            story.prefs.direction = direction;
            for index in [2, 5, 8] {
                let mut body = Story::new();
                body.push_paragraph("First note paragraph.", "Body");
                body.push_paragraph("Second note paragraph.", "Body");
                story.structures.push(StoryStructure {
                    at: Some(unit.len() * index),
                    kind: "Footnote".into(),
                    payload: "source".into(),
                    footnote: Some(FootnoteBody {
                        story: body,
                        markers: Vec::new(),
                        reference_paragraph_style: "Body".into(),
                        reference_character_style: String::new(),
                    }),
                });
            }
            doc.stories[0] = story;
            let before = doc.clone();
            let flow = compose_story(&doc, StoryId(0));
            assert!(!flow.has_overflow());
            assert_eq!(flow.frames.len(), 1);
            let frame = &flow.frames[0];
            assert_eq!(frame.footnotes.len(), 3);
            assert_eq!(frame.unrendered_structures, 0);
            assert_eq!(frame.consumed_to, doc.stories[0].text().len());
            let origins = frame
                .lines
                .iter()
                .map(|line| line.inline_origin.round() as i32)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(origins.len(), columns as usize);
            for note in &frame.footnotes {
                let reference = frame
                    .lines
                    .iter()
                    .find(|line| {
                        line_spec(line, &doc.stories[0], &doc)
                            .text
                            .contains(&(note.structure + 1).to_string())
                    })
                    .unwrap();
                assert!((reference.inline_origin - note.bounds.x).abs() < 0.001);
                assert!(frame
                    .lines
                    .iter()
                    .filter(|line| (line.inline_origin - note.bounds.x).abs() < 0.001)
                    .all(|line| line.bounds.bottom() <= note.bounds.y - 8.0 + 0.001));
                let actual = note
                    .lines
                    .iter()
                    .map(|line| line_spec(line, &doc.stories[0], &doc).text)
                    .collect::<Vec<_>>()
                    .join(" ");
                let expected = doc.stories[0].structures[note.structure]
                    .footnote
                    .as_ref()
                    .unwrap()
                    .story
                    .text();
                let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
                assert_eq!(normalize(&actual), normalize(&expected));
            }
            assert_eq!(doc, before);
        }
    }
}
