use schist_text_engine::{caret_at, line_spans, StyleRun, TextSpec, WritingMode};

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn nominal_metrics_preserve_baselines_and_line_steps_while_glyphs_and_carets_scale() {
    for text in ["HH HH\nHH", "é fi عربي\né fi عربي"] {
        for writing_mode in [
            WritingMode::Horizontal,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let original = TextSpec {
                text: text.into(),
                size: 30.0,
                writing_mode,
                ..Default::default()
            };
            let old = line_spans(&original);
            for scale in [0.4, 0.583, 1.4] {
                for shift in [-9.0, 0.0, 12.0] {
                    let mut script = original.clone();
                    script.runs.push(StyleRun {
                        start: 0,
                        end: text.len(),
                        size: Some(original.size * scale),
                        metric_size: Some(original.size),
                        baseline_shift: Some(shift),
                        ..Default::default()
                    });
                    let new = line_spans(&script);
                    assert_eq!(old.len(), new.len());
                    for (a, b) in old.iter().zip(new) {
                        close(a.baseline, b.baseline);
                        close(a.height, b.height);
                        close(a.top, b.top);
                    }
                    let a = caret_at(&original, 0).unwrap();
                    let b = caret_at(&script, 0).unwrap();
                    close(b.height, a.height * scale);
                    let selected = schist_text_engine::selection_rects(
                        &script,
                        0..text.chars().next().unwrap().len_utf8(),
                    );
                    assert_eq!(selected.len(), 1);
                    let cell = selected[0];
                    let (sin, cos) = b.angle.sin_cos();
                    let tip = (b.x - sin * b.height, b.top + cos * b.height);
                    if writing_mode == WritingMode::Horizontal {
                        assert!((cell.top as f32 - b.top.min(tip.1)).abs() <= 1.0);
                        assert!((cell.bottom as f32 - b.top.max(tip.1)).abs() <= 1.0);
                    } else {
                        assert!((cell.left as f32 - b.x.min(tip.0)).abs() <= 1.0);
                        assert!((cell.right as f32 - b.x.max(tip.0)).abs() <= 1.0);
                    }

                    if writing_mode == WritingMode::Horizontal {
                        close(b.top, a.top + old[0].baseline * (1.0 - scale) - shift);
                    } else {
                        close(b.x, a.x - a.height * (1.0 - scale) / 2.0 + shift);
                    }
                }
            }
        }
    }
}

#[test]
fn metric_overrides_survive_edits_but_do_not_split_shaping() {
    let mut spec = TextSpec {
        text: "office affine".into(),
        size: 30.0,
        ..Default::default()
    };
    spec.set_feature("liga", true);
    let before = line_spans(&spec);
    let mut whole = spec.clone();
    whole.apply_style(
        0..whole.text.len(),
        &StyleRun {
            metric_size: Some(45.0),
            ..Default::default()
        },
    );
    assert_eq!(whole.style_at(0).metric_size, Some(45.0));
    assert!(line_spans(&whole)[0].height > before[0].height);

    spec.apply_style(
        2..4,
        &StyleRun {
            metric_size: Some(45.0),
            ..Default::default()
        },
    );
    let after = line_spans(&spec);
    close(before[0].width, after[0].width);
    assert!(after[0].height > before[0].height);
    assert_eq!(spec.style_at(2).as_run().metric_size, Some(45.0));
    let json = serde_json::to_value(&spec).unwrap();
    assert_eq!(
        serde_json::from_value::<TextSpec>(json.clone()).unwrap(),
        spec
    );
    let mut old = json;
    for run in old["runs"].as_array_mut().unwrap() {
        run.as_object_mut().unwrap().remove("metric_size");
    }
    let old: TextSpec = serde_json::from_value(old).unwrap();
    assert!(old.runs.iter().all(|r| r.metric_size.is_none()));
    for value in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
        spec.runs[0].metric_size = Some(value);
        assert_eq!(spec.style_at(2).metric_size, None);
        assert_eq!(line_spans(&spec), before);
    }
}

#[test]
fn overlapping_and_disjoint_runs_resolve_the_same_glyphs_at_every_style_boundary() {
    use schist_text_engine::rasterize;
    for writing_mode in [
        WritingMode::Horizontal,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for base_size in [12.0, 20.0, 40.0] {
            let base = StyleRun {
                start: 0,
                end: 8,
                size: Some(base_size),
                metric_size: Some(24.0),
                ..Default::default()
            };
            let child = StyleRun {
                start: 3,
                end: 5,
                size: Some(24.0),
                bold: Some(true),
                ..Default::default()
            };
            let overlapping = TextSpec {
                text: "AB CD EF".into(),
                size: 24.0,
                writing_mode,
                runs: vec![child.clone(), base.clone()],
                ..Default::default()
            };
            let disjoint = TextSpec {
                runs: vec![
                    StyleRun {
                        end: 3,
                        ..base.clone()
                    },
                    child,
                    StyleRun { start: 5, ..base },
                ],
                ..overlapping.clone()
            };
            assert_eq!(line_spans(&overlapping), line_spans(&disjoint));
            let mut normalized = overlapping.clone();
            normalized.normalize_runs();
            assert_eq!(normalized.runs, disjoint.runs);
            normalized.apply_style(
                1..7,
                &StyleRun {
                    underline: Some(true),
                    ..Default::default()
                },
            );
            for byte in 0..8 {
                assert_eq!(
                    normalized.style_at(byte).size,
                    overlapping.style_at(byte).size
                );
                assert_eq!(normalized.style_at(byte).underline, (1..7).contains(&byte));
            }

            let a = rasterize(&overlapping).unwrap();
            let b = rasterize(&disjoint).unwrap();
            assert_eq!(a.bounds, b.bounds);
            assert_eq!(a.coverage, b.coverage);
            assert_eq!(a.colors, b.colors);

            // An explicit plain range masks a later fallback even though the
            // normalized representation can omit that range entirely.
            let mut masked = overlapping.clone();
            masked.runs.insert(
                0,
                StyleRun {
                    start: 1,
                    end: 7,
                    ..Default::default()
                },
            );
            let expected: Vec<_> = (0..8).map(|byte| masked.style_at(byte)).collect();
            masked.normalize_runs();
            assert_eq!(
                (0..8).map(|byte| masked.style_at(byte)).collect::<Vec<_>>(),
                expected
            );
        }
    }
}
