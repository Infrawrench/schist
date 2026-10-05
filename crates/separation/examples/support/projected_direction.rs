//! Automatic source direction compared with an explicit authored direction.
//! Opposite-script reference prefixes must not choose the paragraph's base.
use schist_layout::{
    authoring,
    footnotes::*,
    styles::{Leading, TextPosition},
    CharacterStyle, History, Ink, Insets, LayoutDocument, LayoutObject, Page, ParagraphDirection,
    ParagraphStyle, Rect, Story, StoryStructure,
};

pub fn register_font() {
    for data in [
        include_bytes!("../../../../web/fonts/IBMPlexSans-Regular.ttf").as_slice(),
        include_bytes!("../../../../web/fonts/NotoSansHebrew-Regular.ttf").as_slice(),
        include_bytes!("../../../../web/fonts/NotoSansArabic-Regular.ttf").as_slice(),
    ] {
        schist_text_engine::add_font_data(data.to_vec());
    }
}

pub fn document(reference: bool, rtl: bool, initial: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 360.0, 280.0)]);
    let family = if rtl {
        "Noto Sans Hebrew"
    } else {
        "IBM Plex Sans"
    };
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        family: Some(family.into()),
        point_size: Some(12.0),
        leading: Some(Leading::Points(18.0)),
        direction: reference.then_some(if rtl {
            ParagraphDirection::RightToLeft
        } else {
            ParagraphDirection::LeftToRight
        }),
        drop_caps_lines: initial.then_some(3),
        drop_caps_characters: initial.then_some(1),
        left_indent: Some(17.0),
        first_line_indent: Some(5.0),
        fill: Some(Ink::spot("Source violet", [40.0, 50.0, -50.0])),
        keep_lines: Some(1),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Reference".into(),
        family: Some(
            if rtl {
                "IBM Plex Sans"
            } else {
                "Noto Sans Arabic"
            }
            .into(),
        ),
        position: Some(TextPosition::Superscript),
        fill: Some(Ink::cmyk("Reference cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.footnotes = FootnoteOptions {
        start_at: Some(7),
        // Arabic covers both the RTL prefix and decimal number; the bundled
        // Hebrew font only contains Hebrew glyphs.
        prefix: Some(if rtl { "A" } else { "ا" }.into()),
        affixes: Some(FootnoteAffixes::Both),
        no_splitting: Some(true),
        first_baseline: Some(FootnoteFirstBaseline::Ascent),
        spacer: Some(8.0),
        rule: FootnoteRule {
            on: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = if rtl {
        "אבג מילים\u{2028}שורה שנייה\u{2028}שלישית\u{2028}אחרונה"
    } else {
        "Alpha beta\u{2028}second row\u{2028}third row\u{2028}last row"
    };
    let mut main = Story::from_text(text, "Source");
    main.structures.push(StoryStructure {
        control: None,
        at: Some(0),
        kind: "Footnote".into(),
        payload: "source".into(),
        footnote: Some(FootnoteBody {
            story: Story::from_text(text, "Source"),
            markers: vec![FootnoteMarker {
                at: 0,
                character_style: "Reference".into(),
            }],
            reference_paragraph_style: "Source".into(),
            reference_character_style: "Reference".into(),
        }),
        anchored: None,
    });
    let made = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(25.0, 40.0, 310.0, 215.0),
    )
    .unwrap();
    if let LayoutObject::TextFrame { insets, .. } = &mut doc.objects.last_mut().unwrap().object {
        *insets = Insets::ZERO;
    }
    doc.stories[made.story.0 as usize] = main;
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(8.0),
        ..Default::default()
    });
    let title = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(12.0, 10.0, 336.0, 15.0),
    )
    .unwrap();
    doc.stories[title.story.0 as usize] = Story::from_text(
        format!(
            "Source paragraph direction / {} / {}",
            if rtl { "RTL" } else { "LTR" },
            if initial { "initial" } else { "ordinary" }
        ),
        "Heading",
    );
    doc
}
