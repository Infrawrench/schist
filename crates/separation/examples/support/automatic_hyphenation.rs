//! Dictionary-selected ink is checked against literal, independently split text.
#[path = "soft_hyphen.rs"]
mod source_hyphen;
use schist_layout::{
    hyphenation::HyphenationOptions, language::TextLanguage, LayoutDocument, Story, StyleRange,
    WritingMode,
};

pub fn document(reference: bool, spot: bool, axis: WritingMode, reverse: bool) -> LayoutDocument {
    let mut doc = source_hyphen::document(reference, spot, axis, reverse);
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|style| style.name == "Body")
        .unwrap();
    body.language = Some(TextLanguage::Tag {
        tag: "en-US".into(),
    });
    body.hyphenation = HyphenationOptions {
        zone: Some(0.0),
        weight: Some(0),
        ..Default::default()
    };
    if axis == WritingMode::Horizontal {
        doc.objects[0].bounds.width = 45.0;
    } else {
        doc.objects[0].bounds.height = 45.0;
    }
    let text = |value: &str, style| {
        let mut story = Story::from_text(value, style);
        if spot {
            story
                .ranges
                .push(StyleRange::new(0, story.text_len(), "Violet"));
        }
        story
    };
    doc.stories[0] = if reference {
        text("exten-", "Latin fragment")
    } else {
        text("extensive", "Body")
    };
    if reference {
        doc.stories[1] = text("sive", "Body");
    }
    doc.stories[2] = text("extensive\nprobability", "Body");
    doc.stories[3] = Story::from_text(
        format!(
            "Automatic hyphen / {axis:?} / {} / {}",
            if reverse { "RTL" } else { "LTR" },
            if spot { "spot" } else { "process" }
        ),
        "Heading",
    );
    doc
}
