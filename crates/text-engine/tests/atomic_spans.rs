use schist_text_engine::{
    carets, grapheme_boundaries, line_spans, line_spans_with_widths, ParagraphDirection, StyleRun,
    TextSpec, WritingMode,
};

fn spec(text: &str, direction: ParagraphDirection, writing_mode: WritingMode) -> TextSpec {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size: 14.0,
        direction,
        writing_mode,
        ..Default::default()
    }
}

fn axes() -> [WritingMode; 3] {
    [
        WritingMode::Horizontal,
        WritingMode::VerticalLr,
        WritingMode::VerticalRl,
    ]
}

fn directions() -> [ParagraphDirection; 3] {
    [
        ParagraphDirection::Auto,
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ]
}

#[test]
fn atomic_spans_never_split_across_widths_axes_shapers_or_style_overrides() {
    for direction in directions() {
        for axis in axes() {
            for phrase in [
                "a café name",
                "漢 字 名",
                "שם עם שם",
                "ffi office",
                "de\u{ad}monstration",
            ] {
                let text = format!("Prefix {phrase} suffix tail");
                let mut source = spec(&text, direction, axis);
                let span = "Prefix ".len().."Prefix ".len() + phrase.len();
                source.atomic_spans.push(span.clone());
                source.apply_style(
                    span.clone(),
                    &StyleRun {
                        no_break: Some(false),
                        ..Default::default()
                    },
                );
                let original = source.clone();
                let mut ordinary = source.clone();
                ordinary.atomic_spans.clear();
                assert_eq!(line_spans(&source), line_spans(&ordinary));
                assert_eq!(carets(&source), carets(&ordinary));
                let painted = schist_text_engine::rasterize_with_paints(&source).unwrap();
                let control = schist_text_engine::rasterize_with_paints(&ordinary).unwrap();
                assert_eq!(painted.bounds, control.bounds);
                assert_eq!(painted.coverage, control.coverage);
                assert_eq!(
                    painted.rgba([40, 80, 120, 255]),
                    control.rgba([40, 80, 120, 255])
                );
                for widths in [&[15.0][..], &[60.0], &[100.0], &[30.0, 70.0, 90.0]] {
                    let lines = line_spans_with_widths(&source, widths);
                    assert!(!lines.is_empty(), "{direction:?}/{axis:?}/{phrase}");
                    for line in &lines {
                        assert!(!(span.start < line.start && line.start < span.end));
                        assert!(!(span.start < line.end && line.end < span.end));
                    }
                    assert_eq!(lines.first().unwrap().start, 0);
                    assert_eq!(lines.last().unwrap().end, text.len());
                }
                assert_eq!(source, original);
            }
        }
    }
}

#[test]
fn adjacent_spans_remain_independent_and_still_obey_authored_no_break() {
    for direction in directions() {
        for axis in axes() {
            let mut source = spec("red blue green gold", direction, axis);
            let middle = "red blue ".len();
            source.atomic_spans = vec![0..middle, middle..source.text.len()];
            let separate = line_spans_with_widths(&source, &[1.0]);
            assert_eq!(separate.len(), 2);
            assert_eq!(separate[0].end, middle);
            assert_eq!(separate[1].start, middle);
            source.atomic_spans.reverse();
            assert_eq!(line_spans_with_widths(&source, &[1.0]), separate);
            source.apply_style(
                0..source.text.len(),
                &StyleRun {
                    no_break: Some(true),
                    ..Default::default()
                },
            );
            assert_eq!(line_spans_with_widths(&source, &[1.0]).len(), 1);
        }
    }
}

#[test]
fn generated_hyphens_outside_objects_cannot_shift_breaks_into_them() {
    for direction in directions() {
        for axis in axes() {
            for phrase in [
                "a café name",
                "co\u{ad}operate now",
                "שם עם שם",
                "e\u{301} é",
            ] {
                let prefix = "characteristically ";
                let text = format!("{prefix}{phrase} pseudopseudohypoparathyroidism");
                let mut source = spec(&text, direction, axis);
                let span = prefix.len()..prefix.len() + phrase.len();
                source.atomic_spans = vec![span.clone()];
                source.hyphenation_breaks = grapheme_boundaries(&text).collect();
                let original = source.clone();
                let mut selected = false;
                for widths in [&[30.0][..], &[70.0], &[110.0], &[35.0, 75.0, 90.0]] {
                    let lines = line_spans_with_widths(&source, widths);
                    assert!(!lines.is_empty());
                    selected |= lines.iter().any(|line| line.generated_hyphen);
                    assert_eq!(lines.first().unwrap().start, 0);
                    assert_eq!(lines.last().unwrap().end, text.len());
                    for line in &lines {
                        assert!(!(span.start < line.start && line.start < span.end));
                        assert!(!(span.start < line.end && line.end < span.end));
                        assert!(
                            text.is_char_boundary(line.start) && text.is_char_boundary(line.end)
                        );
                    }
                }
                assert!(selected, "{direction:?}/{axis:?}/{phrase}");
                assert_eq!(source, original);
            }
        }
    }
}

#[test]
fn invalid_source_boundaries_and_forced_breaks_are_rejected_instead_of_split() {
    for direction in directions() {
        for text in [
            "e\u{301} café",
            "before\nafter",
            "before\r\nafter",
            "before\u{2028}after",
            "before\u{2029}after",
        ] {
            let base = spec(text, direction, WritingMode::Horizontal);
            let mut spans = vec![
                usize::MAX..usize::MAX,
                std::ops::Range { start: 2, end: 1 },
                0..text.len() + 1,
            ];
            spans.extend(
                (0..=text.len())
                    .filter(|at| !grapheme_boundaries(text).any(|v| v == *at))
                    .map(|at| 0..at),
            );
            if text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                spans.push(0..text.len());
            }
            for span in spans {
                let mut invalid = base.clone();
                invalid.atomic_spans = vec![span];
                assert!(line_spans(&invalid).is_empty());
                assert!(line_spans_with_widths(&invalid, &[30.0]).is_empty());
            }
        }
    }
}

#[test]
fn composition_spans_are_transient_and_source_edits_discard_stale_coordinates() {
    let mut source = spec(
        "red blue green gold",
        ParagraphDirection::Auto,
        WritingMode::Horizontal,
    );
    source.atomic_spans = vec![0..9, 9..source.text.len()];
    let mut ordinary = source.clone();
    ordinary.atomic_spans.clear();
    let saved = serde_json::to_value(&source).unwrap();
    assert_eq!(saved, serde_json::to_value(&ordinary).unwrap());
    assert_eq!(serde_json::from_value::<TextSpec>(saved).unwrap(), ordinary);
    source.text.replace_range(0..3, "new café");
    source.splice_runs(0..3, "new café".len());
    assert!(source.atomic_spans.is_empty());
    ordinary.text.clone_from(&source.text);
    assert_eq!(
        line_spans_with_widths(&source, &[30.0]),
        line_spans_with_widths(&ordinary, &[30.0])
    );
}
