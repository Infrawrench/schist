use schist_text_engine::InlineBox;
use schist_text_engine::{
    inline_box_positions, line_spans, line_spans_with_widths, measure, rasterize,
    ParagraphDirection, TextSpec,
};

const OBJECT: &str = "\u{2068}\u{fffc}\u{2069}";

fn spec(before: &str, after: &str, width: f32, ascent: f32, descent: f32) -> TextSpec {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let text = format!("{before}{OBJECT}{after}");
    let start = before.len();
    TextSpec {
        inline_objects: std::iter::once(start..start + OBJECT.len()).collect(),
        inline_boxes: vec![InlineBox {
            at: start + '\u{2068}'.len_utf8(),
            width,
            ascent,
            descent,
        }],
        text,
        family: "IBM Plex Sans".into(),
        size: 14.0,
        ..Default::default()
    }
}

fn plain(text: &str) -> TextSpec {
    TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size: 14.0,
        ..Default::default()
    }
}

#[test]
fn a_box_advances_by_its_width_and_raises_its_line() {
    let reference = measure(&spec("ab ", " cd", 0.0, 0.0, 0.0)).unwrap();
    for width in [0.0, 12.5, 80.0] {
        let boxed = measure(&spec("ab ", " cd", width, 2.0, 1.0)).unwrap();
        assert!(
            (boxed.width - reference.width - width).abs() < 0.01,
            "{width}"
        );
        // A short box keeps the font's metrics.
        assert!((boxed.first_baseline - reference.first_baseline).abs() < 0.01);
    }
    let tall = measure(&spec("ab ", " cd", 20.0, 60.0, 8.0)).unwrap();
    assert!((tall.first_baseline - 60.0).abs() < 0.01);
    assert!(tall.height > reference.height + 40.0);
}

#[test]
fn box_positions_follow_the_text_before_them_in_either_direction() {
    let s = spec("ab ", " cd", 30.0, 10.0, 0.0);
    let before = measure(&plain("ab ")).unwrap().width;
    let [position] = inline_box_positions(&s)[..] else {
        panic!()
    };
    assert_eq!(position.at, s.inline_boxes[0].at);
    assert!((position.x - before).abs() < 0.5, "{} {before}", position.x);
    let first = measure(&s).unwrap().first_baseline;
    assert!((position.baseline - first).abs() < 0.01);
    // Right to left, the box still spans its own advance inside the line.
    let mut rtl = spec("אב ", " גד", 30.0, 10.0, 0.0);
    rtl.direction = ParagraphDirection::RightToLeft;
    let width = measure(&rtl).unwrap().width;
    let [position] = inline_box_positions(&rtl)[..] else {
        panic!()
    };
    assert!(position.x >= -0.01 && position.x + 30.0 <= width + 0.01);
}

#[test]
fn a_box_wraps_whole_like_a_word() {
    let s = spec("one two ", " three", 60.0, 10.0, 0.0);
    let at = s.inline_boxes[0].at;
    for width in [40.0, 70.0, 120.0, 400.0] {
        let lines = line_spans_with_widths(&s, &[width]);
        assert!(!lines.is_empty());
        let owner: Vec<_> = lines
            .iter()
            .filter(|l| l.start <= at && at < l.end)
            .collect();
        assert_eq!(owner.len(), 1, "{width}");
    }
    assert_eq!(line_spans(&s).len(), 1);
}

#[test]
fn nothing_is_drawn_for_a_box() {
    let boxed = spec("", "", 40.0, 30.0, 0.0);
    let raster = rasterize(&boxed);
    assert!(raster.is_none_or(|r| r.coverage.iter().all(|v| *v == 0)));
}

#[test]
fn invalid_boxes_reject_layout() {
    let valid = spec("ab ", " cd", 10.0, 5.0, 0.0);
    assert!(!line_spans(&valid).is_empty());
    let mut cases = Vec::new();
    // Not on a U+FFFC.
    let mut s = valid.clone();
    s.inline_boxes[0].at -= '\u{2068}'.len_utf8();
    cases.push(s);
    // Outside any inline object.
    let mut s = valid.clone();
    s.inline_objects.clear();
    cases.push(s);
    // Not finite or negative.
    for bad in [f32::NAN, f32::INFINITY, -1.0] {
        let mut s = valid.clone();
        s.inline_boxes[0].width = bad;
        cases.push(s);
    }
    // Duplicated.
    let mut s = valid.clone();
    s.inline_boxes.push(s.inline_boxes[0]);
    cases.push(s);
    for case in cases {
        assert!(line_spans(&case).is_empty(), "{:?}", case.inline_boxes);
    }
}
