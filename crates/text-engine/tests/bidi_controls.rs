use schist_text_engine::{
    carets, line_spans, rasterize, ParagraphDirection, StyleRun, TextSpec, WritingMode,
};

fn spec(text: &str, direction: ParagraphDirection, axis: WritingMode, tracking: f32) -> TextSpec {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size: 16.0,
        tracking,
        direction,
        writing_mode: axis,
        ..Default::default()
    }
}

#[test]
fn bidi_formatting_controls_add_no_advance_ink_or_tracking() {
    let pairs = [
        ("\u{2066}", "\u{2069}"),
        ("\u{2067}", "\u{2069}"),
        ("\u{2068}", "\u{2069}"),
        ("\u{202a}", "\u{202c}"),
        ("\u{202b}", "\u{202c}"),
        ("\u{202d}", "\u{202c}"),
        ("\u{202e}", "\u{202c}"),
        ("\u{200e}", "\u{200e}"),
        ("\u{200f}", "\u{200f}"),
        ("\u{061c}", "\u{061c}"),
    ];
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
            for tracking in [0.0, 1.0, 3.0] {
                let plain = spec("aaa", direction, axis, tracking);
                let width = line_spans(&plain)[0].width;
                for (open, close) in pairs {
                    let text = format!("{open}aaa{close}");
                    let mut controlled = spec(&text, direction, axis, tracking);
                    for range in [0..open.len(), text.len() - close.len()..text.len()] {
                        controlled.apply_style(
                            range,
                            &StyleRun {
                                tracking: Some(17.0),
                                ..Default::default()
                            },
                        );
                    }
                    let source = controlled.clone();
                    assert!(
                        (line_spans(&controlled)[0].width - width).abs() < 0.001,
                        "{axis:?}/{direction:?}/{tracking}/{open:?}"
                    );
                    let painted = rasterize(&controlled).unwrap();
                    let expected = rasterize(&plain).unwrap();
                    assert_eq!(painted.bounds, expected.bounds);
                    assert_eq!(painted.coverage, expected.coverage);
                    assert!(carets(&controlled)
                        .iter()
                        .all(|(at, _)| text.is_char_boundary(*at)));
                    assert_eq!(controlled, source);
                }
            }
        }
    }
}

#[test]
fn empty_isolates_have_zero_inline_width_even_with_tracking() {
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
                "\u{2066}\u{2069}",
                "\u{2067}\u{2069}",
                "\u{2068}\u{2069}",
                "\u{2068}\u{2067}\u{2069}\u{2069}",
            ] {
                let source = spec(text, direction, axis, 11.0);
                let lines = line_spans(&source);
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].width, 0.0);
                assert!(rasterize(&source).unwrap().coverage.is_empty());
            }
        }
    }
}
