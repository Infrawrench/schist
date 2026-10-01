use schist_layout::{
    blank_a4,
    compose::{compose_thread, spec_for, InsetsLike},
    styles::Leading,
    CharacterStyle, FrameOverflow, ObjectId, ParagraphStyle, Rect, Story, WritingMode,
};

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}

#[test]
fn auto_fixed_and_inherited_leading_resolve_per_nominal_run_without_double_scaling() {
    for parent in [Leading::Auto, Leading::Points(27.0)] {
        for child in [
            None,
            Some(Leading::Auto),
            Some(Leading::Points(0.0)),
            Some(Leading::Points(18.0)),
        ] {
            for percentage in [None, Some(0.0), Some(150.0), Some(500.0)] {
                let mut doc = blank_a4();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Parent".into(),
                    point_size: Some(20.0),
                    leading: Some(parent),
                    auto_leading: percentage,
                    ..Default::default()
                });
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Child".into(),
                    based_on: Some("Parent".into()),
                    leading: child,
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Large".into(),
                    point_size: Some(40.0),
                    ..Default::default()
                });
                let mut story = Story::from_text("aé中z", "Child");
                story.apply_style(1, 6, "Large");
                let requested = child.unwrap_or(parent);
                for local in [None, Some(Leading::Auto), Some(Leading::Points(31.0))] {
                    doc.styles
                        .characters
                        .iter_mut()
                        .find(|s| s.name == "Large")
                        .unwrap()
                        .leading = local;
                    let spec = spec_for(&story, 0, 7, &doc.styles, "Child", "Default", 400.0);
                    for byte in [0, 1, 3, 6] {
                        let ranged = (1..6).contains(&byte);
                        let size = if ranged { 40.0 } else { 20.0 };
                        let leading = if ranged {
                            local.unwrap_or(requested)
                        } else {
                            requested
                        };
                        close(
                            spec.style_at(byte).leading.unwrap(),
                            leading.points(size, percentage).unwrap(),
                        );
                    }
                }
                let saved = serde_json::to_value(&doc.styles).unwrap();
                assert_eq!(
                    serde_json::from_value::<schist_layout::StyleSet>(saved).unwrap(),
                    doc.styles
                );
            }
        }
    }
    assert_eq!(
        serde_json::from_str::<Leading>("13.5").unwrap(),
        Leading::Points(13.5)
    );
    assert_eq!(
        serde_json::to_string(&Leading::Points(13.5)).unwrap(),
        "13.5"
    );
}

#[test]
fn incoming_leading_controls_mixed_size_baselines_across_soft_and_paragraph_breaks() {
    for writing in [
        WritingMode::Horizontal,
        WritingMode::VerticalRightToLeft,
        WritingMode::VerticalLeftToRight,
    ] {
        for leading in [0.0, 5.0, 40.0, 120.0] {
            for paragraphs in [false, true] {
                let mut doc = blank_a4();
                doc.styles.add_paragraph(ParagraphStyle {
                    name: "Plain".into(),
                    point_size: Some(14.0),
                    leading: Some(Leading::Points(leading)),
                    keep_lines: Some(1),
                    writing_mode: Some(writing),
                    ..Default::default()
                });
                doc.styles.add_character(CharacterStyle {
                    name: "Large".into(),
                    point_size: Some(42.0),
                    ..Default::default()
                });
                let mut story = if paragraphs {
                    let mut story = Story::from_text("small", "Plain");
                    for text in ["BIG", "small", "", "small"] {
                        story.push_paragraph(text, "Plain");
                    }
                    story
                } else {
                    Story::from_text("small\nBIG\nsmall\n\nsmall", "Plain")
                };
                story.apply_style(6, 9, "Large");
                let id = doc.add_story(story);
                let composed = compose_thread(
                    &doc,
                    id,
                    &[(
                        ObjectId::next(),
                        Rect::new(0.0, 0.0, 700.0, 700.0),
                        FrameOverflow::Clip,
                        1,
                        0.0,
                        InsetsLike::default(),
                    )],
                );
                assert!(!composed.has_overflow());
                let lines: Vec<_> = composed.lines().collect();
                assert_eq!(lines.len(), 5);
                for pair in lines.windows(2) {
                    close((pair[1].baseline - pair[0].baseline).abs(), leading);
                    close(pair[1].advance, leading);
                }
                // The first baseline follows the type metrics, even at 120pt leading.
                if writing == WritingMode::Horizontal {
                    assert!(lines[0].baseline < 30.0);
                }
            }
        }
    }
}

#[test]
fn large_leading_does_not_prevent_the_first_line_from_fitting_a_threaded_frame() {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Plain".into(),
        point_size: Some(11.0),
        leading: Some(Leading::Points(120.0)),
        keep_lines: Some(1),
        ..Default::default()
    });
    let id = doc.add_story(Story::from_text("first\nsecond\nthird", "Plain"));
    let frames: Vec<_> = (0..3)
        .map(|_| {
            (
                ObjectId::next(),
                Rect::new(0.0, 0.0, 100.0, 30.0),
                FrameOverflow::Thread,
                1,
                0.0,
                InsetsLike::default(),
            )
        })
        .collect();
    let composed = compose_thread(&doc, id, &frames);
    assert!(!composed.has_overflow());
    assert!(composed.frames.iter().all(|f| f.lines.len() == 1));
    for line in composed.lines() {
        close(line.advance, 120.0);
    }
}

#[test]
fn canvas_zoom_scales_absolute_leading_even_for_an_uncomposed_empty_frame() {
    use schist_layout::{authoring, Display, History, PasteboardView};
    for height in [1.0, 200.0] {
        let mut doc = blank_a4();
        doc.default_paragraph_style = "Plain".into();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Plain".into(),
            point_size: Some(20.0),
            leading: Some(Leading::Auto),
            auto_leading: Some(150.0),
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 200.0, height),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("", "Plain");
        for zoom in [0.5, 1.0, 3.0] {
            let board = schist_layout::pasteboard(
                &doc,
                &PasteboardView {
                    scale: zoom,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut count = 0;
            for item in board.objects() {
                if let Display::Text { spec, object, .. } = item {
                    if *object != frame.object {
                        continue;
                    }
                    count += 1;
                    close(spec.leading.unwrap(), 30.0 * zoom);
                    close(spec.size, 20.0 * zoom);
                }
            }
            assert_eq!(count, 1);
        }
    }
}
