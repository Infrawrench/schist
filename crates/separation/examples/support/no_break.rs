//! Ordinary frames independently control the protected thread's destination.
use schist_layout::{
    authoring, CharacterStyle, FrameOverflow, History, Ink, Insets, LayoutDocument, LayoutObject,
    Page, ParagraphDirection, ParagraphStyle, Rect, Story, StyleRange, WritingMode,
};

pub fn document(
    reference: bool,
    character: bool,
    axis: WritingMode,
    reverse: bool,
) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0)]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Proof".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(10.0),
        no_break: (!reference && !character).then_some(true),
        writing_mode: Some(axis),
        direction: Some(if reverse {
            ParagraphDirection::RightToLeft
        } else {
            ParagraphDirection::LeftToRight
        }),
        fill: Some(Ink::cmyk("Cyan text", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Protected".into(),
        no_break: (!reference).then_some(true),
        fill: Some(Ink::spot("Violet", [40.0, 50.0, -50.0])),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(7.0),
        ..Default::default()
    });
    let mut ids = Vec::new();
    let mut story = None;
    for bounds in [
        Rect::new(10.0, 30.0, 20.0, 20.0),
        Rect::new(50.0, 30.0, 135.0, 135.0),
    ]
    .into_iter()
    .skip(usize::from(reference))
    {
        let made = authoring::text_frame(&mut doc, &mut History::default(), 0, bounds).unwrap();
        let id = *story.get_or_insert(made.story);
        let o = doc.objects.last_mut().unwrap();
        o.name = "Protected destination".into();
        if let LayoutObject::TextFrame {
            story,
            overflow,
            insets,
            ..
        } = &mut o.object
        {
            *story = id;
            *overflow = FrameOverflow::Thread;
            *insets = Insets::ZERO;
        }
        ids.push(made.object);
    }
    let id = story.unwrap();
    doc.thread_order = vec![(id, ids)];
    let mut text = Story::from_text("a café name", "Proof");
    if character {
        text.ranges
            .push(StyleRange::new(0, text.text_len(), "Protected"));
    }
    doc.stories[id.0 as usize] = text;
    let header = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(10.0, 8.0, 180.0, 15.0),
    )
    .unwrap();
    doc.stories[header.story.0 as usize] = Story::from_text(
        format!(
            "No Break / {} / {axis:?} / {}",
            if character { "character" } else { "paragraph" },
            if reverse { "RTL" } else { "LTR" }
        ),
        "Heading",
    );
    doc
}
