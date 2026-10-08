use schist_text_engine::{
    line_spans, rasterize, ParagraphDirection, StyleRun, TextSpec, WritingMode,
};

fn spec(text: &str, direction: ParagraphDirection, writing_mode: WritingMode) -> TextSpec {
    static FONT: std::sync::Once = std::sync::Once::new();
    FONT.call_once(|| {
        schist_text_engine::add_font_data(
            include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
        );
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
fn chosen_hyphens_paint_and_measure_like_visible_source_without_rewriting_bytes() {
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
            // The discretionary glyph belongs to the Latin word, including
            // when that word is embedded in a right-to-left paragraph.
            let word_direction = if direction == ParagraphDirection::Auto {
                direction
            } else {
                ParagraphDirection::LeftToRight
            };
            let visible = spec("hy-", word_direction, axis);
            let hyphen_width = line_spans(&visible)[0].width;
            let mut actual = spec("hy\u{ad}phenation", direction, axis);
            actual.wrap_width = Some(hyphen_width + 0.5);
            let before = actual.clone();
            let lines = line_spans(&actual);
            assert_eq!(lines.len(), 2, "{direction:?}/{axis:?}");
            assert_eq!(lines[0].end, "hy\u{ad}".len());
            assert_eq!(lines[1].start, lines[0].end);
            assert_eq!(lines[1].end, actual.text.len());
            assert!((lines[0].width - hyphen_width).abs() < 0.001);
            let reference = spec("hy-\nphenation", word_direction, axis);
            let a = rasterize(&actual).unwrap();
            let b = rasterize(&reference).unwrap();
            assert_eq!(a.bounds, b.bounds, "{direction:?}/{axis:?}");
            assert_eq!(a.coverage, b.coverage, "{direction:?}/{axis:?}");
            assert_eq!(actual, before);
            let mut projected = spec("hy\u{ad}", direction, axis);
            projected.show_final_soft_hyphen = true;
            assert!((line_spans(&projected)[0].width - hyphen_width).abs() < 0.001);
            assert_eq!(
                rasterize(&projected).unwrap().coverage,
                rasterize(&visible).unwrap().coverage
            );
            assert!(
                !serde_json::from_str::<TextSpec>(&serde_json::to_string(&projected).unwrap())
                    .unwrap()
                    .show_final_soft_hyphen
            );
        }
    }
}

#[test]
fn hyphens_must_fit_and_no_break_or_forced_end_prevents_a_discretionary_split() {
    for direction in [ParagraphDirection::Auto, ParagraphDirection::LeftToRight] {
        let mut actual = spec("hy\u{ad}phenation", direction, WritingMode::Horizontal);
        let plain = line_spans(&spec("hy", direction, WritingMode::Horizontal))[0].width;
        let visible = line_spans(&spec("hy-", direction, WritingMode::Horizontal))[0].width;
        actual.wrap_width = Some((plain + visible) / 2.0);
        assert_eq!(
            line_spans(&actual).len(),
            1,
            "An overwide hyphen must not be selected"
        );
        actual.wrap_width = Some(visible + 0.5);
        actual.apply_style(
            0..actual.text.len(),
            &StyleRun {
                no_break: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(line_spans(&actual).len(), 1);
        let mut forced = spec("hy\u{ad}\nend\u{ad}", direction, WritingMode::Horizontal);
        forced.wrap_width = Some(visible + 0.5);
        let reference = spec("hy\nend", direction, WritingMode::Horizontal);
        assert_eq!(
            rasterize(&forced).unwrap().coverage,
            rasterize(&reference).unwrap().coverage
        );
        let mut leading = spec("\u{ad}longword", direction, WritingMode::Horizontal);
        leading.wrap_width = Some(plain);
        assert_eq!(
            line_spans(&leading).len(),
            1,
            "No standalone leading hyphen line"
        );
    }
}

#[test]
fn unused_discretionary_hyphens_have_no_ink_or_advance() {
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
            for tracking in [0.0, 0.75] {
                for text in [
                    "hy\u{ad}phenation",
                    "\u{ad}word",
                    "word\u{ad}",
                    "AV\u{ad}ATAR",
                ] {
                    let mut actual = spec(text, direction, axis);
                    actual.tracking = tracking;
                    let before = actual.clone();
                    let mut expected = actual.clone();
                    expected.text = expected.text.replace('\u{ad}', "");
                    let a = rasterize(&actual).unwrap();
                    let b = rasterize(&expected).unwrap();
                    assert!(
                        (a.layout_width - b.layout_width).abs() < 0.001,
                        "{direction:?}/{axis:?}/{tracking}/{text:?}: {} != {}",
                        a.layout_width,
                        b.layout_width
                    );
                    assert_eq!(a.bounds, b.bounds);
                    assert_eq!(a.coverage, b.coverage);
                    assert_eq!(line_spans(&actual).len(), 1);
                    assert_eq!(actual, before);
                }
            }
        }
    }
}

#[test]
fn displayed_hyphens_follow_word_direction_and_keep_source_carets() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansHebrew-Regular.ttf").to_vec(),
    );
    for (prefix, family, word_direction, paragraph_direction) in [
        (
            "café",
            "IBM Plex Sans",
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ),
        (
            "אבג",
            "Noto Sans Hebrew",
            ParagraphDirection::RightToLeft,
            ParagraphDirection::LeftToRight,
        ),
    ] {
        for axis in [
            WritingMode::Horizontal,
            WritingMode::VerticalLr,
            WritingMode::VerticalRl,
        ] {
            let mut actual = spec(&format!("{prefix}\u{ad}"), paragraph_direction, axis);
            actual.family = family.into();
            actual.show_final_soft_hyphen = true;
            let mut expected = actual.clone();
            expected.text = format!("{prefix}-");
            expected.direction = word_direction;
            expected.show_final_soft_hyphen = false;
            let a = rasterize(&actual).unwrap();
            let b = rasterize(&expected).unwrap();
            assert_eq!(a.bounds, b.bounds, "{prefix}/{axis:?}");
            assert_eq!(a.coverage, b.coverage, "{prefix}/{axis:?}");
            for (byte, _) in schist_text_engine::carets(&actual) {
                assert!(actual.text.is_char_boundary(byte));
            }
        }
    }
}

#[test]
fn hidden_hyphens_preserve_complex_script_shaping() {
    for font in [
        include_bytes!("../../../web/fonts/NotoSansArabic-Regular.ttf").as_slice(),
        include_bytes!("../../../web/fonts/NotoSansDevanagari-Regular.ttf").as_slice(),
    ] {
        schist_text_engine::add_font_data(font.to_vec());
    }
    for (text, family) in [
        ("الع\u{ad}ربية", "Noto Sans Arabic"),
        ("ला\u{ad}ल", "Noto Sans Devanagari"),
    ] {
        for direction in [
            ParagraphDirection::Auto,
            ParagraphDirection::LeftToRight,
            ParagraphDirection::RightToLeft,
        ] {
            for tracking in [0.0, 0.75] {
                let mut actual = spec(text, direction, WritingMode::Horizontal);
                actual.family = family.into();
                actual.tracking = tracking;
                let mut reference = actual.clone();
                reference.text = text.replace('\u{ad}', "");
                let a = rasterize(&actual).unwrap();
                let b = rasterize(&reference).unwrap();
                assert_eq!(a.bounds, b.bounds, "{text:?}/{direction:?}/{tracking}");
                assert_eq!(a.coverage, b.coverage, "{text:?}/{direction:?}/{tracking}");
                assert!((a.layout_width - b.layout_width).abs() < 0.001);
            }
        }
    }
}

#[test]
fn hidden_hyphens_do_not_interrupt_synthetic_small_caps_or_their_kerning() {
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
                "\u{ad}ava\u{ad}tar",
                "a ca\u{ad}fé tab\tvalue",
                "a\u{ad}b\u{ad}c\u{ad}",
            ] {
                let mut actual = spec(text, direction, axis);
                actual.apply_style(
                    0..text.len(),
                    &StyleRun {
                        capitalization: Some(schist_text_engine::Capitalization::SmallCaps),
                        tracking: Some(0.75),
                        ..Default::default()
                    },
                );
                let mut expected = actual.clone();
                expected.text = text.replace('\u{ad}', "");
                expected.runs[0].end = expected.text.len();
                let a = rasterize(&actual).unwrap();
                let b = rasterize(&expected).unwrap();
                assert_eq!(a.bounds, b.bounds, "{text}/{direction:?}/{axis:?}");
                assert_eq!(a.coverage, b.coverage, "{text}/{direction:?}/{axis:?}");
                assert!((a.layout_width - b.layout_width).abs() < 0.001);
            }
        }
    }
}

#[test]
fn hebrew_discretionary_breaks_retain_no_break_and_joiner_constraints() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/NotoSansHebrew-Regular.ttf").to_vec(),
    );
    for prefix in ["אבג", "א\u{5b7}בג\u{5b8}"] {
        for suffix in ["דהוז", "\u{2060}דהוז", "\u{5b7}דהוז"] {
            for protected in [false, true] {
                let mut actual = spec(
                    &format!("{prefix}\u{ad}{suffix}"),
                    ParagraphDirection::LeftToRight,
                    WritingMode::Horizontal,
                );
                actual.family = "Noto Sans Hebrew".into();
                actual.apply_style(
                    0..actual.text.len(),
                    &StyleRun {
                        no_break: Some(protected),
                        ..Default::default()
                    },
                );
                let mut reference = actual.clone();
                reference.text = format!("{prefix}-");
                reference.direction = ParagraphDirection::RightToLeft;
                actual.wrap_width = Some(line_spans(&reference)[0].width + 0.1);
                let lines = line_spans(&actual);
                assert_eq!(
                    lines.len(),
                    if suffix == "דהוז" && !protected {
                        2
                    } else {
                        1
                    },
                    "{prefix}/{suffix}/{protected}"
                );
                if lines.len() == 2 {
                    assert!(lines[0].discretionary_hyphen && !lines[0].generated_hyphen);
                    assert_eq!(lines[0].end, prefix.len() + 2);
                }
            }
        }
    }
}
