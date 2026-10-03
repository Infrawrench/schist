use schist_text_engine::{
    line_spans, line_spans_with_widths, ParagraphDirection, StyleRun, TextSpec, WritingMode,
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

#[test]
fn protected_phrases_remain_whole_across_widths_axes_and_shapers() {
    for direction in [
        ParagraphDirection::Auto,
        ParagraphDirection::LeftToRight,
        ParagraphDirection::RightToLeft,
    ] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            for phrase in ["a café name", "漢 字 名", "שם עם שם", "ffi office"] {
                let text = format!("Prefix {phrase} suffix tail");
                let mut plain = spec(&text, direction, axis);
                let start = "Prefix ".len();
                let end = start + phrase.len();
                let mut protected = plain.clone();
                protected.apply_style(
                    start..end,
                    &StyleRun {
                        no_break: Some(true),
                        ..Default::default()
                    },
                );
                let original = protected.clone();
                // No wrapping means exactly the same shaping, ink geometry and carets.
                assert_eq!(line_spans(&plain), line_spans(&protected));
                assert_eq!(
                    schist_text_engine::carets(&plain),
                    schist_text_engine::carets(&protected)
                );
                for widths in [&[15.0][..], &[60.0], &[100.0], &[30.0, 70.0, 90.0]] {
                    let lines = line_spans_with_widths(&protected, widths);
                    assert!(!lines.is_empty());
                    assert!(
                        lines.iter().all(|l| !(start < l.start && l.start < end)
                            && !(start < l.end && l.end < end)),
                        "{direction:?}/{axis:?}/{phrase}/{widths:?}"
                    );
                    assert_eq!(lines.first().unwrap().start, 0);
                    assert_eq!(lines.last().unwrap().end, text.len());
                }
                assert_eq!(protected, original);
                protected.apply_style(
                    start..end,
                    &StyleRun {
                        no_break: Some(false),
                        ..Default::default()
                    },
                );
                plain.wrap_width = Some(35.0);
                protected.wrap_width = plain.wrap_width;
                assert_eq!(line_spans(&plain), line_spans(&protected));
            }
        }
    }
}

#[test]
fn forced_lines_edits_and_legacy_snapshots_keep_no_break_semantics() {
    for direction in [ParagraphDirection::Auto, ParagraphDirection::LeftToRight] {
        let mut source = spec("a café\nnew line", direction, WritingMode::Horizontal);
        source.wrap_width = Some(10.0);
        source.apply_style(
            0..source.text.len(),
            &StyleRun {
                no_break: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(line_spans(&source).len(), 2);
        assert!(source.style_at(2).as_run().no_break == Some(true));
        let serialized = serde_json::to_value(&source).unwrap();
        assert_eq!(
            serde_json::from_value::<TextSpec>(serialized.clone()).unwrap(),
            source
        );
        let mut old = serialized;
        for run in old["runs"].as_array_mut().unwrap() {
            run.as_object_mut().unwrap().remove("no_break");
        }
        let old: TextSpec = serde_json::from_value(old).unwrap();
        assert!(!old.no_break_at(2));
        source.apply_style(
            2..7,
            &StyleRun {
                bold: Some(true),
                ..Default::default()
            },
        );
        assert!((0..source.text.len()).all(|at| source.no_break_at(at)));
        assert_eq!(line_spans(&source).len(), 2);
    }
}
