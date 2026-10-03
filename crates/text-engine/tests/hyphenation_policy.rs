use schist_text_engine::{
    line_spans, rasterize, HyphenationPolicy, ParagraphDirection, TextSpec, WritingMode,
};

fn spec(text: &str, direction: ParagraphDirection, writing_mode: WritingMode) -> TextSpec {
    static FONT: std::sync::Once = std::sync::Once::new();
    FONT.call_once(|| {
        schist_text_engine::add_font_data(
            include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
        )
    });
    TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size: 18.0,
        direction,
        writing_mode,
        ..Default::default()
    }
}
fn cases() -> impl Iterator<Item = (ParagraphDirection, WritingMode)> {
    [
        ParagraphDirection::Auto,
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ]
    .into_iter()
    .flat_map(|d| {
        [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ]
        .into_iter()
        .map(move |a| (d, a))
    })
}
fn candidate(d: ParagraphDirection, a: WritingMode) -> TextSpec {
    let mut s = spec("a extensive b", d, a);
    s.wrap_width = Some(line_spans(&spec("a ex-", d, a))[0].width + 0.25);
    s.hyphenation_breaks = vec![4, 7];
    assert!(line_spans(&s)[0].generated_hyphen, "baseline {d:?}/{a:?}");
    s
}

#[test]
fn zone_uses_actual_end_whitespace_and_larger_zones_only_remove_the_first_hyphen() {
    for (d, a) in cases() {
        let mut s = candidate(d, a);
        let width = s.wrap_width.unwrap();
        let plain = line_spans(&spec("a", d, a))[0].width;
        let threshold = width - plain;
        for (zone, expected) in [
            (0.0, true),
            (threshold - 0.01, true),
            (threshold + 0.01, false),
            (width * 2.0, false),
        ] {
            s.hyphenation_policy.zone = zone;
            let line = line_spans(&s)[0];
            assert_eq!(
                line.generated_hyphen, expected,
                "{d:?}/{a:?} zone {zone}, width {width}, plain {plain}"
            );
            assert_eq!(line.end, if expected { 4 } else { 2 });
        }
    }
}

#[test]
fn increasing_hyphen_weight_only_moves_the_first_line_toward_a_whole_word_break() {
    for (d, a) in cases() {
        let mut s = candidate(d, a);
        let mut stopped = false;
        for weight in 0..=100 {
            s.hyphenation_policy.weight = weight;
            let line = line_spans(&s)[0];
            assert!(
                !(stopped && line.generated_hyphen),
                "weight {weight}: {d:?}/{a:?}"
            );
            stopped |= !line.generated_hyphen;
        }
        assert!(stopped);
        s.text = "extensive".into();
        s.hyphenation_breaks = vec![2, 5];
        s.wrap_width = Some(line_spans(&spec("ex-", d, a))[0].width + 0.25);
        // Even maximum preference permits a necessary break when no whole word fits.
        assert!(line_spans(&s)[0].generated_hyphen);
    }
}

#[test]
fn consecutive_limits_include_carried_lines_and_reset_after_an_unhyphenated_line() {
    for (d, a) in cases() {
        for limit in 0..=3 {
            for carried in 0..=4 {
                let mut s = spec("ababababab abc ababababab", d, a);
                s.wrap_width = Some(line_spans(&spec("ab-", d, a))[0].width + 0.25);
                s.hyphenation_breaks = (2..10).step_by(2).chain((16..24).step_by(2)).collect();
                s.hyphenation_policy = HyphenationPolicy {
                    consecutive_limit: limit,
                    preceding_hyphens: carried,
                    ..Default::default()
                };
                let lines = line_spans(&s);
                let mut run = carried;
                for line in &lines {
                    if line.generated_hyphen {
                        run += 1;
                        assert!(limit == 0 || run <= limit, "{limit}/{carried} {d:?}/{a:?}");
                    } else {
                        run = 0;
                    }
                }
                if limit == 0 {
                    assert!(lines.iter().filter(|l| l.generated_hyphen).count() >= 6);
                }
                assert!(lines.iter().any(|l| l.start >= 14 && l.generated_hyphen));
            }
        }
    }
}

#[test]
fn manual_source_hyphens_keep_priority_over_all_automatic_restrictions() {
    for (d, a) in cases() {
        let mut s = spec("hy\u{ad}phenation abab", d, a);
        s.wrap_width = Some(line_spans(&spec("hy-", d, a))[0].width + 0.25);
        s.hyphenation_breaks = vec![s.text.len() - 2];
        s.hyphenation_policy = HyphenationPolicy {
            consecutive_limit: 1,
            preceding_hyphens: usize::MAX,
            zone: 10000.0,
            weight: 100,
        };
        let first = line_spans(&s)[0];
        assert!(first.discretionary_hyphen && !first.generated_hyphen);
        assert_eq!(first.end, "hy\u{ad}".len());
    }
}

#[test]
fn explicit_newlines_reset_carried_consecutive_history() {
    for (d, a) in cases() {
        let mut s = spec("ababab\nababab", d, a);
        s.wrap_width = Some(line_spans(&spec("ab-", d, a))[0].width + 0.25);
        s.hyphenation_breaks = vec![2, 4, 9, 11];
        s.hyphenation_policy = HyphenationPolicy {
            consecutive_limit: 1,
            preceding_hyphens: 1,
            ..Default::default()
        };
        let lines = line_spans(&s);
        assert!(!lines[0].generated_hyphen);
        let next = lines.iter().find(|line| line.start == 7).unwrap();
        assert!(next.generated_hyphen, "{d:?}/{a:?}");
    }
}

#[test]
fn generated_policy_is_transient_and_does_not_change_unwrapped_ink() {
    for (d, a) in cases() {
        let reference = spec("extensive", d, a);
        let mut s = reference.clone();
        s.hyphenation_breaks = vec![2, 5];
        s.hyphenation_policy = HyphenationPolicy {
            consecutive_limit: 3,
            preceding_hyphens: 2,
            zone: 12.5,
            weight: 75,
        };
        let actual = rasterize(&s).unwrap();
        let expected = rasterize(&reference).unwrap();
        assert_eq!(actual.bounds, expected.bounds);
        assert_eq!(actual.coverage, expected.coverage);
        let saved = serde_json::to_string(&s).unwrap();
        let restored: TextSpec = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, reference);
    }
}

#[test]
fn variable_measures_match_continuations_with_their_actual_preceding_hyphen_count() {
    for (direction, axis) in cases() {
        let mut original = spec(
            "a extensive probability a reorganization extensive probability",
            direction,
            axis,
        );
        original.hyphenation_breaks = original
            .text
            .char_indices()
            .filter_map(|(at, c)| {
                (at > 0
                    && at % 2 == 0
                    && c.is_alphabetic()
                    && original.text.as_bytes()[at - 1].is_ascii_alphabetic())
                .then_some(at)
            })
            .collect();
        original.hyphenation_policy = HyphenationPolicy {
            consecutive_limit: 2,
            zone: 5.0,
            weight: 25,
            ..Default::default()
        };
        let widths = [
            line_spans(&spec("a ex-", direction, axis))[0].width + 0.25,
            80.0,
            47.0,
            140.0,
        ];
        let measures = widths.map(|width| schist_text_engine::InlineMeasure { width, start: 0.0 });
        let lines = schist_text_engine::line_spans_with_measures(&original, &measures);
        let mut preceding = 0;
        for (index, line) in lines.iter().enumerate() {
            let mut continuation = original.clone();
            continuation.text = original.text[line.start..].into();
            continuation.hyphenation_breaks = original
                .hyphenation_breaks
                .iter()
                .copied()
                .filter(|at| *at > line.start)
                .map(|at| at - line.start)
                .collect();
            continuation.hyphenation_policy.preceding_hyphens = preceding;
            continuation.wrap_width = Some(*widths.get(index).unwrap_or(widths.last().unwrap()));
            let actual = line_spans(&continuation)[0];
            assert_eq!(
                actual.end + line.start,
                line.end,
                "{direction:?}/{axis:?}/{index}"
            );
            assert_eq!(actual.generated_hyphen, line.generated_hyphen);
            assert!((actual.width - line.width).abs() < 0.001);
            preceding = if line.generated_hyphen {
                preceding + 1
            } else {
                0
            };
        }
    }
}
