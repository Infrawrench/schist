//! Named source initials compared with independently authored character ranges.
//! Controls use ordinary named character styles and no nested paragraph rules.
use schist_layout::{
    affine::Affine,
    authoring,
    footnotes::*,
    nested_styles::{CharacterStyle as NestedCharacter, Delimiter, NestedStyle},
    styles::{Leading, TextPosition},
    CharacterStyle, FrameOverflow, History, Ink, Insets, LayoutDocument, LayoutObject, Page,
    ParagraphStyle, Rect, Story, StoryStructure, StyleRange, WritingMode,
};
pub const CASES: usize = 8;
pub fn register_font() {
    for data in [
        include_bytes!("../../../../web/fonts/IBMPlexSans-Regular.ttf").as_slice(),
        include_bytes!("../../../text-engine/tests/fixtures/IBMPlexSans-Light.ttf").as_slice(),
    ] {
        schist_text_engine::add_font_data(data.to_vec());
    }
}
fn story(text: &str, reference: bool, direct: bool) -> Story {
    let mut story = Story::from_text(text, "Source");
    if reference {
        if direct {
            story.ranges = vec![
                StyleRange::new(0, 3, "Initial"),
                StyleRange::new(3, 4, "Combined"),
                StyleRange::new(4, 6, "Direct"),
            ];
        } else {
            story.ranges.push(StyleRange::new(0, 4, "Initial"));
        }
    } else if direct {
        story.ranges.push(StyleRange::new(3, 6, "Direct"));
    }
    story
}
pub fn document(reference: bool, case: usize) -> LayoutDocument {
    assert!(case < CASES);
    let mode = match case {
        2 => WritingMode::VerticalRightToLeft,
        3 => WritingMode::VerticalLeftToRight,
        _ => WritingMode::Horizontal,
    };
    let direct = case % 2 == 1 || case == 2 || case == 6;
    let notes = case >= 4;
    let split = case == 6;
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 420.0, 320.0)]);
    doc.styles.add_character(CharacterStyle {
        name: "Initial".into(),
        font_style: Some("Light".into()),
        point_size: Some(18.0),
        tracking: Some(20.0),
        fill: Some(Ink::spot("Initial violet", [45.0, 55.0, -35.0])),
        fill_tint: Some(0.85),
        stroke: Some(Ink::cmyk("Initial cyan", [1.0, 0.0, 0.0, 0.0])),
        stroke_weight: Some(0.3),
        ..Default::default()
    });
    for (name, base) in [("Direct", None), ("Combined", Some("Initial".into()))] {
        doc.styles.add_character(CharacterStyle {
            name: name.into(),
            based_on: base,
            font_style: Some("Regular".into()),
            tracking: Some(60.0),
            fill: Some(Ink::cmyk("Direct magenta", [0.0, 1.0, 0.0, 0.0])),
            fill_tint: Some(1.0),
            ..Default::default()
        });
    }
    doc.styles.add_character(CharacterStyle {
        name: "Reference".into(),
        family: Some("IBM Plex Sans".into()),
        font_style: Some("Regular".into()),
        position: Some(TextPosition::Superscript),
        fill: Some(Ink::cmyk("Reference cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(12.0),
        leading: Some(Leading::Points(18.0)),
        writing_mode: Some(mode),
        drop_caps_lines: Some(if case == 7 { 1 } else { 3 }),
        drop_caps_characters: Some(2),
        keep_lines: Some(1),
        nested_styles: Some(if reference {
            Vec::new()
        } else {
            vec![NestedStyle {
                character_style: NestedCharacter::Named("Initial".into()),
                delimiter: Delimiter::Enumeration("Dropcap".into()),
                repetition: 1,
                inclusive: true,
            }]
        }),
        ..Default::default()
    });
    let text = "E\u{301}abcdef first row\u{2028}second row\u{2028}third row\u{2028}last row";
    let mut main = story(text, reference, direct);
    if notes {
        doc.footnotes = FootnoteOptions {
            start_at: Some(7),
            prefix: Some("[".into()),
            suffix: Some("]".into()),
            affixes: Some(FootnoteAffixes::Both),
            no_splitting: Some(!split),
            first_baseline: Some(FootnoteFirstBaseline::Ascent),
            spacer: Some(10.0),
            rule: FootnoteRule {
                on: Some(false),
                ..Default::default()
            },
            continuing_rule: FootnoteRule {
                on: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let body = if split {
            format!("{text}\u{2028}fifth row\u{2028}sixth row\u{2028}seventh row\u{2028}eighth row")
        } else {
            text.into()
        };
        main.structures.push(StoryStructure {
            at: Some(if case == 5 { 3 } else { 0 }),
            kind: "Footnote".into(),
            payload: "source".into(),
            footnote: Some(FootnoteBody {
                story: story(&body, reference, direct),
                markers: vec![FootnoteMarker {
                    at: 0,
                    character_style: "Reference".into(),
                }],
                reference_paragraph_style: "Source".into(),
                reference_character_style: "Reference".into(),
            }),
        });
    }
    let bounds = if split {
        vec![
            Rect::new(20.0, 45.0, 180.0, 185.0),
            Rect::new(220.0, 45.0, 180.0, 185.0),
        ]
    } else {
        vec![Rect::new(30.0, 45.0, 350.0, 255.0)]
    };
    let mut story_id = None;
    for (index, bounds) in bounds.into_iter().enumerate() {
        let made = authoring::text_frame(&mut doc, &mut History::default(), 0, bounds).unwrap();
        let id = *story_id.get_or_insert(made.story);
        let object = doc.objects.last_mut().unwrap();
        if let LayoutObject::TextFrame {
            story,
            insets,
            overflow,
            ..
        } = &mut object.object
        {
            *story = id;
            *insets = Insets::ZERO;
            *overflow = FrameOverflow::Thread;
        }
        if case == 1 {
            object.transform = Affine::skew(0.04, 0.0).around(-bounds.x, -bounds.y);
        }
        if index == 0 {
            doc.stories[id.0 as usize] = main.clone();
        }
    }
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
        Rect::new(12.0, 10.0, 390.0, 15.0),
    )
    .unwrap();
    doc.stories[title.story.0 as usize] = Story::from_text(
        format!("Named initials / case {} / {mode:?}", case + 1),
        "Heading",
    );
    doc
}
