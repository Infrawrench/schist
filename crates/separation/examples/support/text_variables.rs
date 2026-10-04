//! Whole-variable display compared with independently authored ordinary text.
//! Cases 6–11 print a parent-page last-page-number variable from each of its
//! three destination pages; their literal controls are written out by hand.
use schist_layout::{
    affine::Affine,
    authoring,
    story::InlineControl,
    text_variables::{LastPageNumber, PageNumberFormat, TextVariable, VariableScope},
    CharacterStyle, History, Ink, LayoutDocument, NumberStyle, Page, ParagraphStyle, ParentObject,
    ParentPage, Rect, Section, Story, StoryStructure, StyleRange, WritingMode,
};
pub const CASES: usize = 12;
pub fn register_font() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
}

/// The page each case is printed from.
pub fn page(case: usize) -> usize {
    if case < 6 {
        0
    } else {
        case % 3
    }
}

/// Labels are 1, 2, then a restart at IV on the third page. The first section
/// therefore ends at 2, and both the second section and the document at IV.
fn last_page(case: usize) -> Option<(LastPageNumber, &'static str)> {
    let (format, scope, value) = match case {
        6 | 7 => (
            PageNumberFormat::Current,
            VariableScope::Section,
            "of 2 pp.",
        ),
        8 => (
            PageNumberFormat::Current,
            VariableScope::Section,
            "of IV pp.",
        ),
        9 => (
            PageNumberFormat::Current,
            VariableScope::Document,
            "of IV pp.",
        ),
        10 => (
            PageNumberFormat::Arabic,
            VariableScope::Document,
            "of 4 pp.",
        ),
        11 => (
            PageNumberFormat::LowerLetters,
            VariableScope::Document,
            "of d pp.",
        ),
        _ => return None,
    };
    Some((
        LastPageNumber {
            before: "of ".into(),
            format,
            after: " pp.".into(),
            scope,
        },
        value,
    ))
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
    let computed = last_page(case);
    let value = computed.as_ref().map_or("Edition 7 / café", |(_, v)| *v);
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
        doc.text_variables.push(match &computed {
            Some((spec, _)) => TextVariable {
                id: "edition".into(),
                name: "Last page".into(),
                contents: String::new(),
                last_page: Some(spec.clone()),
            },
            None => TextVariable::custom("edition", "Edition", value),
        });
    }
    doc.stories[frame.story.0 as usize] = story;
    if case % 6 >= 3 {
        doc.objects[0].transform = Affine {
            a: 0.9,
            b: 0.1,
            c: -0.1,
            d: 0.9,
            tx: 0.0,
            ty: 0.0,
        };
    }
    if computed.is_some() {
        for _ in 0..2 {
            doc.add_page(doc.pages[0].clone());
        }
        doc.pages[2].section = Some(Section {
            start: 4,
            continue_numbering: false,
            style: NumberStyle::RomanUpper,
            ..Default::default()
        });
        let object = doc.objects.remove(0);
        doc.parents.push(ParentPage {
            name: "A".into(),
            applied_to: vec![0, 1, 2],
            based_on: None,
            hidden: false,
            sheets: Vec::new(),
            placements: Vec::new(),
            objects: vec![ParentObject {
                object,
                overridden_on: Vec::new(),
            }],
        });
    }
    doc
}
