//! Visible hyphenated and unhyphenated source frames independently control ink.
use schist_layout::{
    authoring, CharacterStyle, FrameOverflow, History, Ink, Insets, LayoutDocument, LayoutObject,
    Page, ParagraphDirection, ParagraphStyle, Rect, Story, StyleRange, WritingMode,
};

pub fn document(reference: bool, spot: bool, axis: WritingMode, reverse: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 240.0, 240.0)]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Body".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(14.0),
        tracking: Some(30.0),
        writing_mode: Some(axis),
        direction: Some(if reverse {
            ParagraphDirection::RightToLeft
        } else {
            ParagraphDirection::LeftToRight
        }),
        fill: Some(Ink::cmyk("Cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Violet".into(),
        fill: Some(Ink::spot("Violet", [40.0, 50.0, -50.0])),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Latin fragment".into(),
        based_on: Some("Body".into()),
        direction: Some(ParagraphDirection::LeftToRight),
        ..Default::default()
    });
    let first = if axis == WritingMode::Horizontal {
        Rect::new(15.0, 35.0, 26.0, 20.0)
    } else {
        Rect::new(15.0, 35.0, 20.0, 26.0)
    };
    let mut frames = Vec::new();
    for bounds in [
        first,
        Rect::new(70.0, 35.0, 135.0, 135.0),
        Rect::new(15.0, 175.0, 200.0, 50.0),
    ] {
        let made = authoring::text_frame(&mut doc, &mut History::default(), 0, bounds).unwrap();
        if let LayoutObject::TextFrame {
            insets, overflow, ..
        } = &mut doc.objects.last_mut().unwrap().object
        {
            *insets = Insets::ZERO;
            *overflow = FrameOverflow::Thread;
        }
        frames.push(made);
    }
    let text = |value: &str| {
        let mut s = Story::from_text(value, "Body");
        if spot {
            s.ranges.push(StyleRange::new(0, s.text_len(), "Violet"));
        }
        s
    };
    if reference {
        let mut fragment = text("hy-");
        if let schist_layout::story::Point::Paragraph { style, .. } = &mut fragment.points[0] {
            *style = "Latin fragment".into();
        }
        doc.stories[frames[0].story.0 as usize] = fragment;
        doc.stories[frames[1].story.0 as usize] = text("phenation");
    } else {
        doc.stories[frames[0].story.0 as usize] = text("hy\u{ad}phenation");
        if let LayoutObject::TextFrame { story, .. } = &mut doc.objects[1].object {
            *story = frames[0].story;
        }
        doc.thread_order = vec![(frames[0].story, vec![frames[0].object, frames[1].object])];
    }
    // Wide text checks invisible glyphs/tracking. Its explicit newline must
    // leave a terminal discretionary character hidden during isolated paint.
    if axis != WritingMode::Horizontal {
        doc.objects[2].bounds = Rect::new(15.0, 85.0, 45.0, 145.0);
    }
    doc.stories[frames[2].story.0 as usize] = text(if reference {
        "AVATAR\nhy"
    } else {
        "AV\u{ad}ATAR\u{ad}\nhy"
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(7.0),
        ..Default::default()
    });
    let heading = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(10.0, 8.0, 220.0, 15.0),
    )
    .unwrap();
    doc.stories[heading.story.0 as usize] = Story::from_text(
        format!(
            "Discretionary hyphen / {axis:?} / {} / {}",
            if reverse { "RTL" } else { "LTR" },
            if spot { "spot" } else { "process" }
        ),
        "Heading",
    );
    doc
}
