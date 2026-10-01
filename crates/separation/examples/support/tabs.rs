//! Tab fields compared with ordinary frames at independently fixed positions.
use schist_layout::{
    authoring,
    lists::{ListStyle, ListTab},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, StyleRange,
    WritingMode,
};

pub fn document(reference: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 48]);
    for case in 0..48 {
        let alignment = ["LeftAlign", "RightAlign", "CenterAlign", "CharacterAlign"][case / 12];
        let fields = if case < 12 {
            ["A", "H", "é"]
        } else {
            ["A", "12.34", "é,15"]
        };
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ][case % 12 / 4];
        let stops = if case < 12 {
            [48.0, 96.0]
        } else {
            [48.0, 108.0]
        };
        let indent = if case.is_multiple_of(2) { 0.0 } else { 12.0 };
        let size = if case % 4 < 2 { 12.0 } else { 18.0 };
        let name = format!("Tabs {case}");
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.clone(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(size),
            writing_mode: Some(mode),
            leading: Some(schist_layout::styles::Leading::Points(24.0)),
            first_line_indent: Some(if reference { 0.0 } else { indent }),
            list: ListStyle {
                tabs: (!reference).then(|| {
                    stops
                        .into_iter()
                        .enumerate()
                        .map(|(index, offset)| {
                            // Independently shape ordinary text, then choose
                            // a ruler stop whose anchor lands at a fixed frame.
                            // No tab geometry is used to construct the control.
                            let character = ['.', ','][index];
                            let text = fields[index + 1];
                            let spec = schist_text_engine::TextSpec {
                                text: text.into(),
                                family: "IBM Plex Sans".into(),
                                size,
                                direction: schist_text_engine::ParagraphDirection::LeftToRight,
                                writing_mode: match mode {
                                    WritingMode::Horizontal => {
                                        schist_text_engine::WritingMode::Horizontal
                                    }
                                    WritingMode::VerticalLeftToRight => {
                                        schist_text_engine::WritingMode::VerticalLr
                                    }
                                    WritingMode::VerticalRightToLeft => {
                                        schist_text_engine::WritingMode::VerticalRl
                                    }
                                },
                                ..Default::default()
                            };
                            let width = schist_text_engine::measure(&spec).unwrap().width;
                            let anchor = match alignment {
                                "RightAlign" => width,
                                "CenterAlign" => width / 2.0,
                                "CharacterAlign" => {
                                    let caret = schist_text_engine::caret_at(
                                        &spec,
                                        text.find(character).unwrap(),
                                    )
                                    .unwrap();
                                    if mode == WritingMode::Horizontal {
                                        caret.x
                                    } else {
                                        caret.top
                                    }
                                }
                                _ => 0.0,
                            };
                            ListTab {
                                position: offset + anchor,
                                alignment: alignment.into(),
                                alignment_character: character.into(),
                                leader: String::new(),
                            }
                        })
                        .collect()
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        for field in 0..3 {
            doc.styles.add_character(CharacterStyle {
                name: format!("Ink {case}/{field}"),
                fill: Some(if field == 1 {
                    Ink::spot("Tab spot", [45.0, 60.0, 30.0])
                } else {
                    Ink::cmyk("Tab cyan", [1.0, 0.0, 0.0, 0.0])
                }),
                fill_tint: Some(0.65),
                opacity: Some(0.7),
                stroke: Some(Ink::cmyk("Tab magenta", [0.0, 1.0, 0.0, 0.0])),
                stroke_weight: Some(0.35),
                stroke_tint: Some(0.75),
                overprint_fill: Some(case.is_multiple_of(2)),
                overprint_stroke: Some(!case.is_multiple_of(2)),
                ..Default::default()
            });
        }
        if reference {
            for (field, text) in fields.iter().enumerate() {
                let offset = [indent, stops[0], stops[1]][field];
                let bounds = if mode == WritingMode::Horizontal {
                    Rect::new(20.0 + offset, 20.0, 160.0 - offset, 160.0)
                } else {
                    Rect::new(20.0, 20.0 + offset, 160.0, 160.0 - offset)
                };
                let frame =
                    authoring::text_frame(&mut doc, &mut History::default(), case, bounds).unwrap();
                let mut story = Story::from_text(*text, &name);
                story.ranges.push(StyleRange::new(
                    0,
                    text.len(),
                    format!("Ink {case}/{field}"),
                ));
                doc.stories[frame.story.0 as usize] = story;
            }
        } else {
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                case,
                Rect::new(20.0, 20.0, 160.0, 160.0),
            )
            .unwrap();
            let mut story = Story::from_text(fields.join("\t"), &name);
            let mut from = 0;
            for (field, text) in fields.iter().enumerate() {
                story.ranges.push(StyleRange::new(
                    from,
                    from + text.len(),
                    format!("Ink {case}/{field}"),
                ));
                from += text.len() + 1;
            }
            doc.stories[frame.story.0 as usize] = story;
        }
    }
    doc
}
