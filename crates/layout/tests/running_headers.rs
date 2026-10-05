//! Running headers and jump page numbers against InDesign's PDF of the public
//! paged-media `variables` sample (see docs/idml-format.md): five pages
//! labelled 1 2 1 2 3, headings on pages 1 (two), 3 and 5, keywords in a
//! character style, and a story threaded from page 3 to page 5.
use schist_layout::story::{InlineControl, PageNumberKind};
use schist_layout::text_variables::{
    ChangeCase, MatchStyle, RunningHeader, TextVariable, VariableKind,
};
use schist_layout::{
    authoring, blank_a4, compose, threading, CharacterStyle, History, LayoutDocument,
    ParagraphStyle, Rect, Section, Story, StoryPoint, StoryStructure, StyleRange,
};

const HEADINGS: [&[&str]; 5] = [
    &[
        "Introduction: the First Heading.",
        "a second heading, on page one?",
    ],
    &[],
    &["PART TWO begins (a third heading)"],
    &[],
    &["the final heading!"],
];

/// The sample's eight running headers: (name, style, last, case, delete).
fn headers() -> Vec<TextVariable> {
    let header = |id: &str, style: MatchStyle, last, case, delete| {
        TextVariable::new(
            id,
            id,
            VariableKind::RunningHeader(RunningHeader {
                before: String::new(),
                after: String::new(),
                style,
                last,
                case,
                delete_end_punctuation: delete,
            }),
        )
    };
    let heading = || MatchStyle::Paragraph("Heading".into());
    let keyword = || MatchStyle::Character("Keyword".into());
    vec![
        header("first", heading(), false, ChangeCase::None, false),
        header("last", heading(), true, ChangeCase::None, false),
        header("keyword first", keyword(), false, ChangeCase::None, false),
        header("keyword last", keyword(), true, ChangeCase::None, false),
        header("upper", heading(), false, ChangeCase::Upper, true),
        header("lower", heading(), false, ChangeCase::Lower, false),
        header("title", heading(), false, ChangeCase::Title, false),
        header("sentence", heading(), false, ChangeCase::Sentence, false),
    ]
}

fn instance(at: usize, variable: &str) -> StoryStructure {
    StoryStructure {
        at: Some(at),
        kind: "TextVariableInstance".into(),
        payload: String::new(),
        footnote: None,
        control: Some(InlineControl::TextVariable {
            variable: variable.into(),
            character_style: String::new(),
            name: variable.into(),
        }),
        anchored: None,
    }
}

fn marker(at: usize, kind: PageNumberKind) -> StoryStructure {
    StoryStructure {
        at: Some(at),
        kind: "ProcessingInstruction".into(),
        payload: String::new(),
        footnote: None,
        control: Some(InlineControl::PageNumber {
            kind,
            character_style: String::new(),
        }),
        anchored: None,
    }
}

fn values(frame: &compose::ComposedFrame) -> Vec<String> {
    let text: String = frame
        .lines
        .iter()
        .filter_map(|l| l.projected.as_ref())
        .map(|p| p.spec.text.as_str())
        .collect();
    text.split('\u{2068}')
        .skip(1)
        .map(|rest| rest.split('\u{2069}').next().unwrap().to_owned())
        .collect()
}

fn sample() -> (
    LayoutDocument,
    Vec<schist_layout::StoryId>,
    schist_layout::StoryId,
) {
    let mut doc = blank_a4();
    for _ in 1..5 {
        doc.add_page(doc.pages[0].clone());
    }
    doc.pages[2].section = Some(Section {
        start: 1,
        continue_numbering: false,
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        ..Default::default()
    });
    doc.styles.characters.push(CharacterStyle {
        name: "Keyword".into(),
        ..Default::default()
    });
    doc.text_variables = headers();
    let mut history = History::default();
    // The body: one frame per page, a frame break after each page's text.
    let body: Vec<_> = (0..5)
        .map(|page| {
            authoring::text_frame(
                &mut doc,
                &mut history,
                page,
                Rect::new(60.0, 200.0, 400.0, 400.0),
            )
            .unwrap()
        })
        .collect();
    for pair in body.windows(2) {
        assert!(threading::link(
            &mut doc,
            &mut history,
            pair[0].object,
            pair[1].object
        ));
    }
    let bodies = [
        vec![
            (HEADINGS[0][0], "Heading", None),
            (
                "Body text with an alpha keyword in it.",
                "Body",
                Some("alpha keyword"),
            ),
            (HEADINGS[0][1], "Heading", None),
            ("More text and a beta keyword.", "Body", Some("beta")),
        ],
        vec![(
            "No heading and no keyword on this page: both carry page one's forward.",
            "Body",
            None,
        )],
        vec![
            (HEADINGS[2][0], "Heading", None),
            ("Text with the gamma keyword.", "Body", Some("gamma")),
        ],
        vec![("No heading here either.", "Body", None)],
        vec![
            (HEADINGS[4][0], "Heading", None),
            ("And the delta keyword, last.", "Body", Some("delta")),
        ],
    ];
    let mut story = Story::new();
    for (page, paragraphs) in bodies.iter().enumerate() {
        if page > 0 {
            story.points.push(StoryPoint::FrameBreak);
        }
        for (text, style, keyword) in paragraphs {
            let (start, _) = story.push_paragraph(*text, *style);
            if let Some(keyword) = keyword {
                let at = start + text.find(keyword).unwrap();
                story.ranges.push(StyleRange {
                    start: at,
                    end: at + keyword.len(),
                    style: "Keyword".into(),
                });
            }
        }
    }
    let story_id = threading::story_of(&doc, body[0].object).unwrap();
    doc.stories[story_id.0 as usize] = story;
    // A header frame on every page showing all eight variables.
    let mut header_stories = Vec::new();
    for page in 0..5 {
        let frame = authoring::text_frame(
            &mut doc,
            &mut history,
            page,
            Rect::new(60.0, 20.0, 470.0, 160.0),
        )
        .unwrap();
        let text = "| | | | | | | | ";
        let mut header = Story::from_text(text, "Body");
        header.structures = headers()
            .iter()
            .zip(text.match_indices('|').map(|(at, _)| at))
            .map(|(variable, at)| instance(at, &variable.id))
            .collect();
        doc.stories[frame.story.0 as usize] = header;
        header_stories.push(frame.story);
    }
    // A jump story threaded from page 3 to page 5.
    let first = authoring::text_frame(
        &mut doc,
        &mut history,
        2,
        Rect::new(60.0, 650.0, 400.0, 60.0),
    )
    .unwrap();
    let second = authoring::text_frame(
        &mut doc,
        &mut history,
        4,
        Rect::new(60.0, 650.0, 400.0, 60.0),
    )
    .unwrap();
    assert!(threading::link(
        &mut doc,
        &mut history,
        first.object,
        second.object
    ));
    let jump_id = threading::story_of(&doc, first.object).unwrap();
    let on = "Jump: continued on page , previous .";
    let from = "Jump: continued from page , next .";
    let mut jump = Story::new();
    let (start, _) = jump.push_paragraph(on, "Body");
    jump.structures
        .push(marker(start + on.find(',').unwrap(), PageNumberKind::Next));
    jump.structures.push(marker(
        start + on.rfind('.').unwrap(),
        PageNumberKind::Previous,
    ));
    jump.points.push(StoryPoint::FrameBreak);
    let (start, _) = jump.push_paragraph(from, "Body");
    jump.structures.push(marker(
        start + from.find(',').unwrap(),
        PageNumberKind::Previous,
    ));
    jump.structures.push(marker(
        start + from.rfind('.').unwrap(),
        PageNumberKind::Next,
    ));
    doc.stories[jump_id.0 as usize] = jump;
    (doc, header_stories, jump_id)
}

#[test]
fn running_headers_match_the_native_sample_page_by_page() {
    let (doc, headers, _) = sample();
    let first = [
        "Introduction: the First Heading.",
        "a second heading, on page one?",
        "alpha keyword",
        "beta",
        "INTRODUCTION: THE FIRST HEADING",
        "introduction: the first heading.",
        "Introduction: The First Heading.",
        "Introduction: the first heading.",
    ];
    let third = [
        "PART TWO begins (a third heading)",
        "PART TWO begins (a third heading)",
        "gamma",
        "gamma",
        "PART TWO BEGINS (A THIRD HEADING)",
        "part two begins (a third heading)",
        "Part Two Begins (a Third Heading)",
        "Part two begins (a third heading)",
    ];
    let fifth = [
        "the final heading!",
        "the final heading!",
        "delta",
        "delta",
        "THE FINAL HEADING",
        "the final heading!",
        "The Final Heading!",
        "The final heading!",
    ];
    // Pages without a heading carry the previous page's values forward.
    for (page, expected) in [first, first, third, third, fifth].iter().enumerate() {
        let thread = compose::compose_story(&doc, headers[page]);
        assert_eq!(
            values(&thread.frames[0]),
            expected.to_vec(),
            "page {}",
            page + 1
        );
        assert_eq!(thread.frames[0].unrendered_structures, 0);
    }
}

#[test]
fn jump_numbers_in_a_thread_follow_the_frame_they_land_in() {
    let (doc, _, jump) = sample();
    let thread = compose::compose_story(&doc, jump);
    assert_eq!(values(&thread.frames[0]), ["3", "1"]);
    assert_eq!(values(&thread.frames[1]), ["1", "3"]);
    assert!(thread.frames.iter().all(|f| f.unrendered_structures == 0));
}
