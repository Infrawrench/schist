use schist_text_engine::{
    line_spans, rasterize, ParagraphDirection, StyleRun, TextSpec, WritingMode,
};
use schist_text_engine::{CaretAffinity, CaretPosition};

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

#[test]
fn unused_generated_opportunities_do_not_change_geometry_ink_or_carets() {
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
            for text in [
                "AVATAR",
                "café",
                "cafe\u{301}",
                "a café tab\tvalue",
                "abc\nDEF",
            ] {
                for caps in [
                    schist_text_engine::Capitalization::Normal,
                    schist_text_engine::Capitalization::SmallCaps,
                    schist_text_engine::Capitalization::AllCaps,
                ] {
                    let mut reference = spec(text, direction, axis);
                    reference.wrap_width = Some(1000.0);
                    if text.contains('\t') {
                        reference.tabs = Some(schist_text_engine::TabStops {
                            positions: vec![140.0],
                            leaders: vec![".".into()],
                            ..Default::default()
                        });
                    }
                    reference.apply_style(
                        0..text.len(),
                        &StyleRun {
                            tracking: Some(0.75),
                            capitalization: Some(caps),
                            ..Default::default()
                        },
                    );
                    let mut actual = reference.clone();
                    actual.hyphenation_breaks = (0..=text.len() + 1).rev().collect();
                    actual.hyphenation_breaks.extend([usize::MAX, 1, 1]);
                    let a = rasterize(&actual).unwrap();
                    let b = rasterize(&reference).unwrap();
                    assert_eq!(a.bounds, b.bounds, "{text}/{caps:?}/{direction:?}/{axis:?}");
                    assert_eq!(
                        a.coverage, b.coverage,
                        "{text}/{caps:?}/{direction:?}/{axis:?}"
                    );
                    let a_lines = line_spans(&actual);
                    let b_lines = line_spans(&reference);
                    assert_eq!(a_lines.len(), b_lines.len());
                    for (a, b) in a_lines.iter().zip(b_lines) {
                        assert_eq!(
                            (a.start, a.end, a.discretionary_hyphen, a.generated_hyphen),
                            (b.start, b.end, b.discretionary_hyphen, b.generated_hyphen)
                        );
                        for (a, b) in [
                            (a.x, b.x),
                            (a.width, b.width),
                            (a.top, b.top),
                            (a.baseline, b.baseline),
                            (a.height, b.height),
                            (a.advance, b.advance),
                        ] {
                            // Legacy word-width accumulation can differ by an
                            // f32 rounding step from the discretionary pass.
                            assert!(
                                (a - b).abs() < 0.0001,
                                "{text}/{caps:?}/{direction:?}/{axis:?}: {a} != {b}"
                            );
                        }
                    }
                    let a = schist_text_engine::insertion_points(&actual);
                    let b = schist_text_engine::insertion_points(&reference);
                    assert_eq!(a, b, "{text}/{caps:?}/{direction:?}/{axis:?}");
                }
            }
        }
    }
}

#[test]
fn generated_hyphens_require_room_valid_boundaries_and_permission_to_break() {
    for direction in [ParagraphDirection::Auto, ParagraphDirection::LeftToRight] {
        let plain = line_spans(&spec("hy", direction, WritingMode::Horizontal))[0].width;
        let visible = line_spans(&spec("hy-", direction, WritingMode::Horizontal))[0].width;
        let mut actual = spec("hyphenation", direction, WritingMode::Horizontal);
        actual.hyphenation_breaks = vec![2];
        actual.wrap_width = Some((plain + visible) / 2.0);
        assert_eq!(line_spans(&actual).len(), 1);
        actual.wrap_width = Some(visible + 0.5);
        actual.apply_style(
            0..actual.text.len(),
            &StyleRun {
                no_break: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(line_spans(&actual).len(), 1);
        for (text, at) in [
            ("éclair", 1),
            ("e\u{301}clair", 1),
            ("hy word", 2),
            ("hy word", 3),
            ("hy\nword", 2),
            ("hy\nword", 3),
            ("hy", 0),
            ("hy", 2),
            ("hy\u{ad}word", 2),
            ("hy\u{ad}word", 4),
        ] {
            let mut a = spec(text, direction, WritingMode::Horizontal);
            a.hyphenation_breaks = vec![at];
            a.wrap_width = Some(1.0);
            let mut b = a.clone();
            b.hyphenation_breaks.clear();
            assert_eq!(line_spans(&a), line_spans(&b), "{text}/{at}");
        }
    }
}

#[test]
fn both_caret_affinities_at_a_generated_break_match_real_line_edges() {
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
            for prefix in ["hy", "café", "cafe\u{301}"] {
                let word_direction = if direction == ParagraphDirection::Auto {
                    direction
                } else {
                    ParagraphDirection::LeftToRight
                };
                let reference = spec(&format!("{prefix}-\ncontinuation"), word_direction, axis);
                let width = line_spans(&spec(&format!("{prefix}-"), word_direction, axis))[0].width;
                let mut actual = spec(&format!("{prefix}continuation"), direction, axis);
                actual.hyphenation_breaks = vec![prefix.len()];
                actual.wrap_width = Some(width + 0.5);
                for (affinity, at) in [
                    (CaretAffinity::Upstream, prefix.len() + 1),
                    (CaretAffinity::Downstream, prefix.len() + 2),
                ] {
                    let a = schist_text_engine::caret_at_position(
                        &actual,
                        CaretPosition {
                            byte: prefix.len(),
                            affinity,
                        },
                    )
                    .unwrap();
                    let b = schist_text_engine::caret_at_position(
                        &reference,
                        CaretPosition { byte: at, affinity },
                    )
                    .unwrap();
                    assert!(
                        (a.x - b.x).abs() < 0.001 && (a.top - b.top).abs() < 0.001,
                        "{direction:?}/{axis:?}/{prefix}/{affinity:?}: {a:?} != {b:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn repeated_generated_breaks_partition_original_text_once_in_every_writing_mode() {
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
            let word_direction = if direction == ParagraphDirection::Auto {
                direction
            } else {
                ParagraphDirection::LeftToRight
            };
            let mut actual = spec("ababababab", direction, axis);
            let reference = spec("ab-\nab-\nab-\nab-\nab", word_direction, axis);
            actual.wrap_width = Some(line_spans(&reference)[0].width + 0.1);
            actual.hyphenation_breaks = vec![8, 2, 4, 4, 6, usize::MAX];
            let lines = line_spans(&actual);
            assert_eq!(lines.len(), 5);
            for (i, line) in lines.iter().enumerate() {
                assert_eq!((line.start, line.end), (i * 2, i * 2 + 2));
                assert_eq!(line.generated_hyphen, i < 4);
                assert!(!line.discretionary_hyphen);
            }
            let a = rasterize(&actual).unwrap();
            let b = rasterize(&reference).unwrap();
            assert_eq!(a.bounds, b.bounds);
            assert_eq!(a.coverage, b.coverage);
        }
    }
}

#[test]
fn generated_rtl_hyphens_follow_the_word_and_keep_the_upstream_caret_at_its_end() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansHebrew-Regular.ttf").to_vec(),
    );
    for axis in [
        WritingMode::Horizontal,
        WritingMode::VerticalLr,
        WritingMode::VerticalRl,
    ] {
        let mut actual = spec("אבגדהוז", ParagraphDirection::LeftToRight, axis);
        actual.family = "Noto Sans Hebrew".into();
        actual.hyphenation_breaks = vec!["אבג".len()];
        let mut reference = actual.clone();
        reference.hyphenation_breaks.clear();
        reference.text = "אבג-\nדהוז".into();
        reference.direction = ParagraphDirection::RightToLeft;
        actual.wrap_width = Some(line_spans(&reference)[0].width + 0.1);
        let a = rasterize(&actual).unwrap();
        let b = rasterize(&reference).unwrap();
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(a.coverage, b.coverage);
        let a = schist_text_engine::caret_at_position(
            &actual,
            CaretPosition {
                byte: "אבג".len(),
                affinity: CaretAffinity::Upstream,
            },
        )
        .unwrap();
        let b = schist_text_engine::caret_at_position(
            &reference,
            CaretPosition {
                byte: "אבג-".len(),
                affinity: CaretAffinity::Upstream,
            },
        )
        .unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn generated_hyphens_keep_source_ranges_and_match_independent_visible_glyphs() {
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
            for prefix in ["hy", "café", "cafe\u{301}"] {
                for styled in 0..4 {
                    let word_direction = if direction == ParagraphDirection::Auto {
                        direction
                    } else {
                        ParagraphDirection::LeftToRight
                    };
                    let mut visible = spec(&format!("{prefix}-"), word_direction, axis);
                    let mut actual = spec(&format!("{prefix}continuation"), direction, axis);
                    if styled > 0 {
                        let style = StyleRun {
                            size: Some(23.0),
                            tracking: Some(0.75),
                            capitalization: match styled {
                                2 => Some(schist_text_engine::Capitalization::SmallCaps),
                                3 => Some(schist_text_engine::Capitalization::AllCaps),
                                _ => None,
                            },
                            ..Default::default()
                        };
                        actual.apply_style(0..prefix.len(), &style);
                        visible.apply_style(0..visible.text.len(), &style);
                    }
                    let width = line_spans(&visible)[0].width;
                    actual.hyphenation_breaks = vec![prefix.len()];
                    actual.wrap_width = Some(width + 0.5);
                    let original = actual.clone();
                    let lines = line_spans(&actual);
                    assert_eq!(lines.len(), 2, "{direction:?}/{axis:?}/{prefix}/{styled}");
                    assert_eq!(lines[0].end, prefix.len());
                    assert_eq!(lines[1].start, prefix.len());
                    assert_eq!(lines[1].end, actual.text.len());
                    assert!(lines[0].generated_hyphen && !lines[0].discretionary_hyphen);
                    assert!(!lines[1].generated_hyphen);
                    assert!((lines[0].width - width).abs() < 0.001);
                    let mut isolated = actual.clone();
                    isolated.text.truncate(prefix.len());
                    isolated.wrap_width = None;
                    isolated.hyphenation_breaks.clear();
                    isolated.show_final_generated_hyphen = true;
                    let a = rasterize(&isolated).unwrap();
                    let b = rasterize(&visible).unwrap();
                    assert_eq!(a.bounds, b.bounds);
                    assert_eq!(
                        a.coverage, b.coverage,
                        "{direction:?}/{axis:?}/{prefix}/{styled}"
                    );
                    for (byte, _) in schist_text_engine::carets(&actual) {
                        assert!(actual.text.is_char_boundary(byte));
                    }
                    assert_eq!(actual, original);
                    let saved: TextSpec =
                        serde_json::from_str(&serde_json::to_string(&isolated).unwrap()).unwrap();
                    assert!(
                        !saved.show_final_generated_hyphen && saved.hyphenation_breaks.is_empty()
                    );
                }
            }
        }
    }
}
