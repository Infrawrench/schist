//! Whole-variable display compared with independently authored ordinary text.
use schist_layout::{
    affine::Affine, authoring, story::InlineControl, text_variables::TextVariable, CharacterStyle,
    History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story, StoryStructure, StyleRange,
    WritingMode,
};
pub const CASES: usize = 6;
pub fn register_font() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 420.0, 320.0)]);
    let mode = match case % 3 {
        0 => WritingMode::Horizontal,
        1 => WritingMode::VerticalLeftToRight,
        _ => WritingMode::VerticalRightToLeft,
    };
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(18.0),
        writing_mode: Some(mode),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Instance".into(),
        point_size: Some(24.0),
        tracking: Some(40.0),
        fill: Some(Ink::spot("Variable violet", [45.0, 55.0, -35.0])),
        fill_tint: Some(0.85),
        stroke: Some(Ink::cmyk("Variable cyan", [1.0, 0.0, 0.0, 0.0])),
        stroke_weight: Some(0.3),
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(60.0, 50.0, 250.0, 220.0),
    )
    .unwrap();
    let value = "Edition 7 / café";
    let mut story = Story::from_text(if reference { value } else { "" }, "Source");
    if reference {
        story
            .ranges
            .push(StyleRange::new(0, value.len(), "Instance"));
    } else {
        story.structures.push(StoryStructure {
            at: Some(0),
            kind: "TextVariableInstance".into(),
            payload: "original instance".into(),
            footnote: None,
            control: Some(InlineControl::TextVariable {
                variable: "edition".into(),
                character_style: "Instance".into(),
                name: String::new(),
            }),
        });
        doc.text_variables.push(TextVariable {
            id: "edition".into(),
            name: "Edition".into(),
            contents: value.into(),
        });
    }
    doc.stories[frame.story.0 as usize] = story;
    if case >= 3 {
        doc.objects[0].transform = Affine {
            a: 0.9,
            b: 0.1,
            c: -0.1,
            d: 0.9,
            tx: 0.0,
            ty: 0.0,
        };
    }
    doc
}
