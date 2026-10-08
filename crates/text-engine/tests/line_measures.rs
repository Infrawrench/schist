use schist_text_engine::{line_spans, line_spans_with_widths, TextSpec};

#[test]
fn repeated_advances_do_not_accumulate_spacing_error_after_a_styled_prefix() {
    use schist_text_engine::{measure, ParagraphDirection, StyleRun};
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for size in [9.0, 12.0, 18.0, 20.5, 27.0] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            let unit = TextSpec {
                text: ".".into(),
                family: "IBM Plex Sans".into(),
                size,
                direction,
                ..Default::default()
            };
            let advance = measure(&unit).unwrap().width;
            for count in [1, 8, 32, 128, 512] {
                for offset in [0.0, 70.0, 1000.25] {
                    let spec = TextSpec {
                        text: format!("\u{200b}{}", ".".repeat(count)),
                        runs: vec![StyleRun {
                            start: 0,
                            end: 3,
                            tracking: Some(offset),
                            ..Default::default()
                        }],
                        ..unit.clone()
                    };
                    let expected = (f64::from(offset) + f64::from(advance) * count as f64) as f32;
                    let actual = measure(&spec).unwrap().width;
                    assert!(
                        (actual - expected).abs() <= 2.0 * f32::EPSILON * expected.max(1.0),
                        "{size}/{direction:?}/{count}/{offset}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn successive_measures_preserve_source_ranges_and_repeat_the_final_width() {
    for text in [
        "a few short words to wrap well ".repeat(12),
        "é au thé et café 世界 ".repeat(12),
        "عالم نص عربي نص ".repeat(12),
    ] {
        let spec = TextSpec {
            text,
            size: 11.0,
            wrap_width: Some(90.0),
            ..Default::default()
        };
        assert_eq!(line_spans(&spec), line_spans_with_widths(&spec, &[]));
        assert_eq!(line_spans(&spec), line_spans_with_widths(&spec, &[90.0]));
        for widths in [
            &[50.0, 90.0][..],
            &[60.0, 70.0, 90.0][..],
            &[100.0, 65.0][..],
        ] {
            let lines = line_spans_with_widths(&spec, widths);
            assert!(lines.len() > widths.len());
            let mut cursor = 0;
            for (i, line) in lines.iter().enumerate() {
                assert!(
                    spec.text.is_char_boundary(line.start) && spec.text.is_char_boundary(line.end)
                );
                assert!(spec.text[cursor..line.start]
                    .chars()
                    .all(char::is_whitespace));
                assert!(line.width <= widths[i.min(widths.len() - 1)] + 0.001);
                cursor = line.end;
            }
            assert!(spec.text[cursor..].chars().all(char::is_whitespace));
        }
        for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(line_spans_with_widths(&spec, &[width]).is_empty());
        }
    }
}
