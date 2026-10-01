//! Automatic markers compared with independently positioned ordinary text frames.
use schist_layout::{
    authoring,
    list_numbering::CounterFormat,
    lists::{ListKind, ListStyle, ListTab, MarkerAlignment},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
};
const TEXT: [&str; 4] = ["Café", "Second", "Third", "End"];
fn marker(case: usize, row: usize) -> String {
    if case >= 20 {
        return [
            ["I.", "I.a.", "II.", "II.a."],
            ["I.", "I.a.", "II.", "II.b."],
            ["I.", "I.a.", "I.a.01.", "I.a.02."],
            ["I.", "I.a.", "I.", "I.a."],
        ][case - 20][row]
            .into();
    }
    if case >= 12 {
        return [
            ["IX.", "X.", "XI.", "XII."],
            ["ix.", "x.", "xi.", "xii."],
            ["Y.", "Z.", "AA.", "AB."],
            ["y.", "z.", "aa.", "ab."],
            ["08.", "09.", "10.", "11."],
            ["098.", "099.", "100.", "101."],
            ["0998.", "0999.", "1000.", "1001."],
            ["", "", "", ""],
        ][case - 12][row]
            .into();
    }
    if case.is_multiple_of(2) {
        "→".into()
    } else {
        format!("{}.", 9 + row)
    }
}
fn size(case: usize) -> f32 {
    if !(6..12).contains(&case) {
        12.0
    } else {
        24.0
    }
}
fn alignment(case: usize) -> MarkerAlignment {
    if case >= 12 {
        return MarkerAlignment::Right;
    }
    [
        MarkerAlignment::Left,
        MarkerAlignment::Center,
        MarkerAlignment::Right,
    ][case / 2 % 3]
}
fn spec(text: &str, size: f32) -> schist_text_engine::TextSpec {
    schist_text_engine::TextSpec {
        text: text.into(),
        family: "IBM Plex Sans".into(),
        size,
        leading: Some(28.0),
        direction: schist_text_engine::ParagraphDirection::LeftToRight,
        ..Default::default()
    }
}
pub fn document(reference: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 24]);
    for case in 0usize..24 {
        let body_name = format!("Body {case}");
        let marker_name = format!("Marker {case}");
        let fill = Ink::spot("Marker spot", [45.0, 60.0, 30.0]);
        let stroke = Ink::cmyk("Marker cyan", [1.0, 0.0, 0.0, 0.0]);
        let list = ListStyle {
            kind: Some(if case < 12 && case.is_multiple_of(2) {
                ListKind::Bullet
            } else {
                ListKind::Numbered
            }),
            bullet: Some(schist_layout::lists::BulletSymbol::unicode('→')),
            start: Some(match case {
                20..=23 => 1,
                14 | 15 => 25,
                16 => 8,
                17 => 98,
                18 => 998,
                _ => 9,
            }),
            format: match case {
                12 | 20..=23 => Some(CounterFormat::UpperRoman),
                13 => Some(CounterFormat::LowerRoman),
                14 => Some(CounterFormat::UpperLetters),
                15 => Some(CounterFormat::LowerLetters),
                16 => Some(CounterFormat::SingleLeadingZeros),
                17 => Some(CounterFormat::DoubleLeadingZeros),
                18 => Some(CounterFormat::TripleLeadingZeros),
                19 => Some(CounterFormat::None),
                _ => None,
            }
            .map(CounterFormat::native),
            expression: (case == 19).then(|| "^#^t".into()),
            continue_numbering: (case == 23).then_some(false),
            bullet_alignment: Some(alignment(case)),
            numbering_alignment: Some(alignment(case)),
            bullet_character_style: Some(marker_name.clone()),
            numbering_character_style: Some(marker_name.clone()),
            tabs: Some(vec![ListTab {
                position: 60.0,
                alignment: "LeftAlign".into(),
                alignment_character: ".".into(),
                leader: String::new(),
            }]),
            ..Default::default()
        };
        doc.styles.add_character(CharacterStyle {
            name: marker_name.clone(),
            point_size: Some(size(case)),
            fill: Some(fill.clone()),
            fill_tint: Some(0.6),
            stroke: Some(stroke.clone()),
            stroke_tint: Some(0.75),
            stroke_weight: Some(0.35),
            opacity: Some(0.7),
            overprint_fill: Some(case.is_multiple_of(2)),
            overprint_stroke: Some(!case.is_multiple_of(2)),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: body_name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(18.0),
            leading: Some(schist_layout::styles::Leading::Points(28.0)),
            direction: Some(schist_layout::ParagraphDirection::LeftToRight),
            left_indent: Some(if reference { 0.0 } else { 60.0 }),
            first_line_indent: Some(if reference { 0.0 } else { -40.0 }),
            list: if reference {
                ListStyle::default()
            } else {
                list
            },
            ..Default::default()
        });
        if !reference {
            if case >= 20 {
                for level in [2, 3] {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: format!("{body_name} level {level}"),
                        based_on: Some(body_name.clone()),
                        list: ListStyle {
                            level: Some(level),
                            continue_numbering: Some(true),
                            apply_restart_policy: Some(case != 21),
                            format: Some(
                                if level == 2 {
                                    CounterFormat::LowerLetters
                                } else {
                                    CounterFormat::SingleLeadingZeros
                                }
                                .native(),
                            ),
                            expression: Some(
                                if level == 2 {
                                    "^1.^#.^t"
                                } else {
                                    "^1.^2.^#.^t"
                                }
                                .into(),
                            ),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
            }
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                case,
                Rect::new(30.0, 30.0, 160.0, 150.0),
            )
            .unwrap();
            let mut story = Story::from_text(TEXT[0], &body_name);
            for (row, text) in TEXT.iter().enumerate().skip(1) {
                let style = if case == 22 && row >= 2 {
                    format!("{body_name} level 3")
                } else if case >= 20 && row % 2 == 1 {
                    format!("{body_name} level 2")
                } else {
                    body_name.clone()
                };
                story.push_paragraph(*text, &style);
            }
            doc.stories[frame.story.0 as usize] = story;
            continue;
        }
        // The control never reads composed list geometry or generated marker specs.
        let body_ascent = schist_text_engine::measure(&spec("Café", 18.0))
            .unwrap()
            .first_baseline;
        let marker_paragraph = format!("Explicit {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: marker_paragraph.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(size(case)),
            leading: Some(schist_layout::styles::Leading::Points(28.0)),
            direction: Some(schist_layout::ParagraphDirection::LeftToRight),
            ..Default::default()
        });
        for (row, text) in TEXT.iter().enumerate() {
            let y = 30.0 + row as f32 * 28.0;
            let value = marker(case, row);
            let metrics = schist_text_engine::measure(&spec(&value, size(case))).unwrap();
            let x = 50.0
                - match alignment(case) {
                    MarkerAlignment::Left => 0.0,
                    MarkerAlignment::Center => metrics.width / 2.0,
                    MarkerAlignment::Right => metrics.width,
                };
            let marker_frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                case,
                Rect::new(x, y + body_ascent - metrics.first_baseline, 100.0, 60.0),
            )
            .unwrap();
            let mut marker_story = Story::from_text(&value, &marker_paragraph);
            marker_story
                .ranges
                .push(schist_layout::StyleRange::new(0, value.len(), &marker_name));
            doc.stories[marker_frame.story.0 as usize] = marker_story;
            let body_frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                case,
                Rect::new(90.0, y, 100.0, 60.0),
            )
            .unwrap();
            doc.stories[body_frame.story.0 as usize] = Story::from_text(*text, &body_name);
        }
    }
    doc
}
