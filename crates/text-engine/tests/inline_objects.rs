use schist_text_engine::{
    line_spans, line_spans_with_widths, ParagraphDirection, StyleRun, TextSpec, WritingMode,
};

fn spec(text: &str) -> TextSpec {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size: 14.0,
        ..Default::default()
    }
}

#[test]
fn objects_wrap_as_independent_replacement_characters_without_splitting_their_values() {
    for axis in [
        WritingMode::Horizontal,
        WritingMode::VerticalLr,
        WritingMode::VerticalRl,
    ] {
        for direction in [
            ParagraphDirection::Auto,
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for values in [
                ["red blue", "green gold"],
                ["漢 字", "שם עם שם"],
                ["co\u{ad}operate", "e\u{301} é"],
            ] {
                let first = format!("\u{2068}{}\u{2069}", values[0]);
                let second = format!("\u{2068}{}\u{2069}", values[1]);
                let mut s = spec(&format!("{first}{second}"));
                s.writing_mode = axis;
                s.direction = direction;
                s.inline_objects = vec![0..first.len(), first.len()..s.text.len()];
                let original = s.clone();
                for width in [1.0, 20.0, 70.0, 120.0] {
                    for line in line_spans_with_widths(&s, &[width]) {
                        assert!([0, first.len()].contains(&line.start));
                        assert!([first.len(), s.text.len()].contains(&line.end));
                    }
                }
                assert_eq!(line_spans_with_widths(&s, &[1.0]).len(), 2);
                assert_eq!(s, original);
                s.apply_style(
                    0..s.text.len(),
                    &StyleRun {
                        no_break: Some(true),
                        ..Default::default()
                    },
                );
                assert_eq!(line_spans_with_widths(&s, &[1.0]).len(), 1);
            }
        }
    }
}

#[test]
fn paragraph_justification_expands_only_spaces_outside_objects() {
    for axis in [
        WritingMode::Horizontal,
        WritingMode::VerticalLr,
        WritingMode::VerticalRl,
    ] {
        for direction in [
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            let value = "\u{2068}red blue gold\u{2069}";
            let mut s = spec(&format!("a {value} b"));
            s.inline_objects = std::iter::once(2..2 + value.len()).collect();
            s.writing_mode = axis;
            s.direction = direction;
            let natural = line_spans(&s)[0].width;
            for spacing in [1.0, 3.0, 9.0] {
                s.word_spacing = spacing;
                assert!((line_spans(&s)[0].width - natural - 2.0 * spacing).abs() < 0.001);
            }
        }
    }
}

#[test]
fn object_edges_keep_unicode_glue_and_word_joiner_rules() {
    for glue in ["\u{2060}", "\u{a0}"] {
        let value = "\u{2068}a value\u{2069}";
        for text in [format!("a{glue}{value}"), format!("{value}{glue}a")] {
            let start = text.find('\u{2068}').unwrap();
            let mut s = spec(&text);
            s.inline_objects = std::iter::once(start..start + value.len()).collect();
            assert_eq!(line_spans_with_widths(&s, &[1.0]).len(), 1, "{text:?}");
        }
    }
}

#[test]
fn objects_keep_their_coordinates_through_generated_hyphens_and_discard_them_on_edit() {
    let value = "\u{2068}red blue\u{2069}";
    let prefix = "characteristically ";
    let mut s = spec(&format!("{prefix}{value} uncharacteristically"));
    let object = prefix.len()..prefix.len() + value.len();
    s.inline_objects = std::iter::once(object.clone()).collect();
    s.hyphenation_breaks = schist_text_engine::grapheme_boundaries(&s.text).collect();
    let mut hyphens = 0;
    for width in [30.0, 70.0, 110.0] {
        for line in line_spans_with_widths(&s, &[width]) {
            assert!(!(object.start < line.start && line.start < object.end));
            assert!(!(object.start < line.end && line.end < object.end));
            hyphens += usize::from(line.generated_hyphen);
        }
    }
    assert!(hyphens > 0);
    let saved = serde_json::to_value(&s).unwrap();
    assert!(serde_json::from_value::<TextSpec>(saved)
        .unwrap()
        .inline_objects
        .is_empty());
    s.text.insert(0, 'x');
    s.splice_runs(0..0, 1);
    assert!(s.inline_objects.is_empty());
}

#[test]
fn malformed_or_nested_object_ranges_reject_layout() {
    for text in [
        "ordinary",
        "\u{2068}tab\tvalue\u{2069}",
        "\u{2068}a\u{2028}b\u{2069}",
        "\u{2068}\u{2069}escape\u{2069}",
    ] {
        let mut s = spec(text);
        s.inline_objects = std::iter::once(0..text.len()).collect();
        assert!(line_spans(&s).is_empty());
    }
    let text = "\u{2068}value\u{2069}";
    for ranges in [
        vec![0..text.len(), 0..text.len()],
        std::iter::once(0..usize::MAX).collect(),
        std::iter::once(1..text.len()).collect(),
    ] {
        let mut s = spec(text);
        s.inline_objects = ranges;
        assert!(line_spans(&s).is_empty());
    }
}
