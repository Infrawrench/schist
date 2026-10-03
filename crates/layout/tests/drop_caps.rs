use schist_layout::{
    blank_a4,
    compose::{compose_thread, line_spec, InsetsLike},
    grid::GridMode,
    FrameOverflow, ObjectId, ParagraphDirection, ParagraphStyle, Rect, Story,
};

#[test]
fn opening_graphemes_are_enlarged_once_and_body_lines_clear_their_actual_ink() {
    for (prefix, direction) in [
        ("A", ParagraphDirection::LeftToRight),
        ("E\u{301}", ParagraphDirection::LeftToRight),
        ("É", ParagraphDirection::RightToLeft),
    ] {
        for characters in [1, 2] {
            for height in [2, 3, 4] {
                for grid in [GridMode::None, GridMode::SnapToGrid] {
                    let mut doc = blank_a4();
                    doc.grids.document.mode = grid;
                    doc.pages[0].margins.top = 0.0;
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Initial".into(),
                        point_size: Some(11.0),
                        leading: Some(schist_layout::styles::Leading::Points(14.0)),
                        drop_caps_lines: Some(height),
                        drop_caps_characters: Some(characters),
                        direction: Some(direction),
                        keep_lines: Some(1),
                        ..Default::default()
                    });
                    let text = format!(
                        "{prefix}fter opening words, {}",
                        "more words to wrap into lines. ".repeat(12)
                    );
                    let mut story = Story::from_text(&text, "Initial");
                    story.push_paragraph(&text, "Initial");
                    let offsets = story.point_offsets();
                    let end = story.text_len();
                    let id = doc.add_story(story);
                    let frames: Vec<_> = (0..20)
                        .map(|_| {
                            (
                                ObjectId::next(),
                                Rect::new(10.0, 20.0, 200.0, 130.0),
                                FrameOverflow::Thread,
                                1,
                                0.0,
                                InsetsLike::default(),
                            )
                        })
                        .collect();
                    let thread = compose_thread(&doc, id, &frames);
                    assert!(
                        !thread.has_overflow(),
                        "{height} lines {characters} characters {grid:?}"
                    );
                    let lines: Vec<_> = thread.lines().collect();
                    let initials: Vec<_> = lines.iter().filter(|l| l.initial.is_some()).collect();
                    assert_eq!(initials.len(), offsets.len());
                    assert_eq!(
                        initials.iter().map(|l| l.start).collect::<Vec<_>>(),
                        offsets
                    );
                    let mut covered = vec![0; end];
                    for line in &lines {
                        assert!(doc.stories[id.0 as usize]
                            .text()
                            .is_char_boundary(line.start));
                        assert!(doc.stories[id.0 as usize].text().is_char_boundary(line.end));
                        for count in &mut covered[line.start..line.end] {
                            *count += 1;
                        }
                    }
                    for (i, byte) in doc.stories[id.0 as usize].text().bytes().enumerate() {
                        if byte != b'\n' {
                            assert_eq!(covered[i], 1, "byte {i}");
                        }
                    }
                    for frame in &thread.frames {
                        for (index, line) in frame.lines.iter().enumerate() {
                            let Some(initial) = line.initial else {
                                continue;
                            };
                            assert!(initial.scale > 1.0);
                            assert!(initial.ink.width > 0.0);
                            let spec = line_spec(line, doc.story(id).unwrap(), &doc);
                            let raster = schist_text_engine::rasterize(&spec).unwrap();
                            assert!(!raster.is_empty());
                            // The actual bitmap encloses the planned outline, independent
                            // of font side bearings and combining marks.
                            assert!(
                                (line.bounds.x + raster.bounds.left as f32 - initial.ink.x).abs()
                                    < 2.0
                            );
                            assert!(
                                (line.bounds.y + raster.bounds.top as f32 - initial.ink.y).abs()
                                    < 2.0
                            );
                            assert!(
                                (raster.bounds.height() as f32 - initial.ink.height).abs() < 3.0
                            );
                            let body = &frame.lines[index + 1..index + 1 + height];
                            assert!(body.iter().all(|l| l.drop_cap));
                            for body in body {
                                if direction == ParagraphDirection::RightToLeft {
                                    assert!(body.bounds.right() < initial.ink.x);
                                } else {
                                    assert!(body.bounds.x > initial.ink.right());
                                }
                            }
                            assert!(
                                (initial.ink.bottom() - body.last().unwrap().baseline).abs()
                                    < 0.001
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_drop_cap_and_its_covered_lines_move_together_or_remain_overset() {
    for height in [2, 3, 4] {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Initial".into(),
            point_size: Some(11.0),
            leading: Some(schist_layout::styles::Leading::Points(14.0)),
            drop_caps_lines: Some(height),
            keep_lines: Some(1),
            ..Default::default()
        });
        let id = doc.add_story(Story::from_text(
            "A paragraph with many short words to fill multiple lines. ".repeat(10),
            "Initial",
        ));
        let frames = [
            (
                ObjectId::next(),
                Rect::new(0.0, 0.0, 200.0, (height - 1) as f32 * 14.0),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            ),
            (
                ObjectId::next(),
                Rect::new(0.0, 0.0, 200.0, 1000.0),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            ),
        ];
        let thread = compose_thread(&doc, id, &frames);
        assert!(thread.frames[0].lines.is_empty());
        assert_eq!(thread.frames[0].consumed_to, 0);
        assert!(thread.frames[1].lines[0].initial.is_some());
        assert!(!thread.has_overflow());
        assert!(compose_thread(&doc, id, &frames[..1]).has_overflow());
    }
}

use schist_layout::drop_caps;

#[test]
fn native_initial_flags_inherit_independently_and_zero_clears_without_enabling_an_initial() {
    for value in [
        None,
        Some(i32::MIN),
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(256),
        Some(i32::MAX),
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            drop_caps_lines: Some(3),
            drop_caps_characters: Some(2),
            drop_caps_detail: Some(3),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            drop_caps_lines: Some(0),
            drop_caps_detail: value,
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Leaf".into(),
            based_on: Some("Child".into()),
            ..Default::default()
        });
        let resolved = doc.styles.resolve_paragraph("Leaf");
        assert_eq!(resolved.drop_caps_detail, value.or(Some(3)));
        assert_eq!(resolved.drop_caps_lines, Some(0));
        assert_eq!(resolved.drop_caps_characters, Some(2));
        assert_eq!(drop_caps::unsupported_detail(&resolved), None);
        assert_eq!(
            doc.styles.resolve_paragraph("Base").drop_caps_detail,
            Some(3)
        );
    }
}

#[test]
fn old_snapshots_keep_legacy_initial_geometry_without_inventing_native_flags() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.paragraphs[0].drop_caps_lines = Some(3);
    let mut snapshot = serde_json::to_value(&doc).unwrap();
    for style in snapshot["styles"]["paragraphs"].as_array_mut().unwrap() {
        style.as_object_mut().unwrap().remove("drop_caps_detail");
    }
    let restored: schist_layout::LayoutDocument = serde_json::from_value(snapshot).unwrap();
    assert_eq!(restored, doc);
    let style = restored
        .styles
        .resolve_paragraph(&restored.styles.paragraphs[0].name);
    assert_eq!(style.drop_caps_detail, None);
    assert_eq!(drop_caps::unsupported_detail(&style), None);
}
