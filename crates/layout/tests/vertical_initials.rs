use schist_layout::{
    blank_a4,
    compose::{compose_thread, line_spec, InsetsLike},
    FrameOverflow, ObjectId, ParagraphDirection, ParagraphStyle, Rect, Story, WritingMode,
};

#[test]
fn vertical_initials_paint_each_source_grapheme_once_and_reserve_their_actual_ink() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    for writing in [
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for prefix in ["A", "日", "E\u{301}"] {
                for chars in [1, 2] {
                    for count in [2, 3, 4] {
                        let mut doc = blank_a4();
                        doc.styles.add_paragraph(ParagraphStyle {
                            name: "Initial".into(),
                            family: Some("Noto Sans CJK JP".into()),
                            point_size: Some(11.0),
                            leading: Some(schist_layout::styles::Leading::Points(14.0)),
                            writing_mode: Some(writing),
                            direction: Some(direction),
                            drop_caps_lines: Some(count),
                            drop_caps_characters: Some(chars),
                            keep_lines: Some(1),
                            ..Default::default()
                        });
                        let text = format!(
                            "{prefix}本 {}",
                            "日本語 body text wraps around an initial. ".repeat(12)
                        );
                        let mut source = Story::from_text(&text, "Initial");
                        source.push_paragraph(&text, "Initial");
                        let before = source.clone();
                        let offsets = source.point_offsets();
                        let len = source.text_len();
                        let id = doc.add_story(source);
                        let frames: Vec<_> = (0..20)
                            .map(|_| {
                                (
                                    ObjectId::next(),
                                    Rect::new(20.0, 30.0, 130.0, 200.0),
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
                            "{writing:?} {direction:?} {prefix} {chars} {count}"
                        );
                        let initials: Vec<_> = thread
                            .lines()
                            .filter(|line| line.initial.is_some())
                            .collect();
                        assert_eq!(
                            initials.iter().map(|line| line.start).collect::<Vec<_>>(),
                            offsets
                        );
                        let mut covered = vec![0; len];
                        for line in thread.lines() {
                            for byte in &mut covered[line.start..line.end] {
                                *byte += 1;
                            }
                        }
                        for (i, byte) in before.text().bytes().enumerate() {
                            if byte != b'\n' {
                                assert_eq!(covered[i], 1, "source byte {i}");
                            }
                        }
                        for frame in &thread.frames {
                            for (i, line) in frame.lines.iter().enumerate() {
                                let Some(initial) = line.initial else {
                                    continue;
                                };
                                assert!(initial.scale > 1.0);
                                let spec = line_spec(line, doc.story(id).unwrap(), &doc);
                                let raster = schist_text_engine::rasterize(&spec).unwrap();
                                let actual = [
                                    line.bounds.x + raster.bounds.left as f32,
                                    line.bounds.y + raster.bounds.top as f32,
                                    line.bounds.x + raster.bounds.right as f32,
                                    line.bounds.y + raster.bounds.bottom as f32,
                                ];
                                let planned = [
                                    initial.ink.x,
                                    initial.ink.y,
                                    initial.ink.right(),
                                    initial.ink.bottom(),
                                ];
                                for (a, b) in actual.into_iter().zip(planned) {
                                    assert!((a - b).abs() < 2.0, "{writing:?}: {a}, {b}");
                                }
                                for body in &frame.lines[i + 1..i + 1 + count] {
                                    assert!(body.drop_cap);
                                    assert!(
                                        body.bounds.y > initial.ink.bottom(),
                                        "{writing:?}: body {:?} vs initial {:?}",
                                        body.bounds,
                                        initial.ink
                                    );
                                }
                                let last = &frame.lines[i + count];
                                let edge = if writing == WritingMode::VerticalRightToLeft {
                                    initial.ink.x
                                } else {
                                    initial.ink.right()
                                };
                                assert!(
                                    (edge - last.baseline).abs() < 0.001,
                                    "{writing:?}: edge {edge}, baseline {}",
                                    last.baseline
                                );
                            }
                        }
                        assert_eq!(doc.story(id).unwrap(), &before);
                    }
                }
            }
        }
    }
}

#[test]
fn vertical_initials_and_their_columns_move_together_or_remain_overset() {
    for writing in [
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for count in [2, 3, 4] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Initial".into(),
                point_size: Some(11.0),
                leading: Some(schist_layout::styles::Leading::Points(14.0)),
                writing_mode: Some(writing),
                drop_caps_lines: Some(count),
                keep_lines: Some(1),
                ..Default::default()
            });
            let id = doc.add_story(Story::from_text(
                "A paragraph with many short words to fill columns. ".repeat(10),
                "Initial",
            ));
            let frames = [
                (
                    ObjectId::next(),
                    Rect::new(10.0, 20.0, (count - 1) as f32 * 14.0, 200.0),
                    FrameOverflow::Thread,
                    1,
                    0.0,
                    InsetsLike::default(),
                ),
                (
                    ObjectId::next(),
                    Rect::new(10.0, 20.0, 1000.0, 200.0),
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
}

#[test]
fn initial_font_size_is_independent_of_frame_position() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansJP-Schist.otf").to_vec(),
    );
    for writing in [
        WritingMode::Horizontal,
        WritingMode::VerticalLeftToRight,
        WritingMode::VerticalRightToLeft,
    ] {
        for prefix in ["É", "W", "E\u{301}"] {
            let mut doc = blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Initial".into(),
                family: Some("Noto Sans CJK JP".into()),
                point_size: Some(12.0),
                leading: Some(schist_layout::styles::Leading::Points(20.0)),
                writing_mode: Some(writing),
                drop_caps_lines: Some(3),
                keep_lines: Some(1),
                ..Default::default()
            });
            let id = doc.add_story(Story::from_text(
                format!("{prefix}alpha\u{2028}bravo\u{2028}cello\u{2028}delta"),
                "Initial",
            ));
            let mut expected = None;
            for offset in [0.0, 0.125, 25.0, 200.0, 1024.0, 8192.0] {
                let thread = compose_thread(
                    &doc,
                    id,
                    &[(
                        ObjectId::next(),
                        Rect::new(offset, offset, 180.0, 180.0),
                        FrameOverflow::Thread,
                        1,
                        0.0,
                        InsetsLike::default(),
                    )],
                );
                let initial = thread.lines().find_map(|line| line.initial).unwrap();
                assert_eq!(
                    *expected.get_or_insert(initial.scale),
                    initial.scale,
                    "{writing:?}/{prefix}/{offset}"
                );
            }
        }
    }
}
