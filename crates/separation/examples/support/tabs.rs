//! Tab fields compared with ordinary frames at independently fixed positions.
use schist_layout::{
    authoring,
    lists::{ListStyle, ListTab},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, StyleRange,
    WritingMode,
};

const FIELDS: [&str; 3] = ["A", "H", "é"];
pub fn document(reference: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 12]);
    for case in 0..12 {
        let mode = [
            WritingMode::Horizontal,
            WritingMode::VerticalLeftToRight,
            WritingMode::VerticalRightToLeft,
        ][case / 4];
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
                    [48.0, 96.0]
                        .map(|position| ListTab {
                            position,
                            alignment: "LeftAlign".into(),
                            alignment_character: ".".into(),
                            leader: String::new(),
                        })
                        .into()
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
            for (field, text) in FIELDS.iter().enumerate() {
                let offset = [indent, 48.0, 96.0][field];
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
            let mut story = Story::from_text(FIELDS.join("\t"), &name);
            let mut from = 0;
            for (field, text) in FIELDS.iter().enumerate() {
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
