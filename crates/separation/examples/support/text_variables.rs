//! Whole-variable display compared with independently authored ordinary text.
//! Cases 6–11 print a parent-page last-page-number variable, and cases 12–17
//! current page numbers and section markers, from each of three destination
//! pages; their literal controls are written out by hand.
use schist_layout::{
    affine::Affine,
    authoring,
    story::{InlineControl, PageNumberKind},
    text_variables::{LastPageNumber, PageNumberFormat, TextVariable, VariableKind, VariableScope},
    CharacterStyle, History, Ink, LayoutDocument, NumberStyle, Page, ParagraphStyle, ParentObject,
    ParentPage, Rect, Section, Story, StoryStructure, StyleRange, WritingMode,
};
pub const CASES: usize = 18;
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

enum Display {
    Custom,
    LastPage(LastPageNumber),
    Marker(InlineControl),
}

/// Labels are 1, 2, then a restart at IV on the third page, whose section
/// marker is "Back"; the first section's marker is "Front". The first section
/// therefore ends at 2, and both the second section and the document at IV.
fn display(case: usize) -> (Display, &'static str) {
    let last_page = |format, scope, value| {
        (
            Display::LastPage(LastPageNumber {
                before: "of ".into(),
                format,
                after: " pp.".into(),
                scope,
            }),
            value,
        )
    };
    let marker = |control, value| (Display::Marker(control), value);
    let page_number = InlineControl::PageNumber {
        kind: PageNumberKind::Current,
        character_style: "Instance".into(),
    };
    let section = InlineControl::SectionMarker {
        character_style: "Instance".into(),
    };
    match case {
        6 | 7 => last_page(
            PageNumberFormat::Current,
            VariableScope::Section,
            "of 2 pp.",
        ),
        8 => last_page(
            PageNumberFormat::Current,
            VariableScope::Section,
            "of IV pp.",
        ),
        9 => last_page(
            PageNumberFormat::Current,
            VariableScope::Document,
            "of IV pp.",
        ),
        10 => last_page(
            PageNumberFormat::Arabic,
            VariableScope::Document,
            "of 4 pp.",
        ),
        11 => last_page(
            PageNumberFormat::LowerLetters,
            VariableScope::Document,
            "of d pp.",
        ),
        12 => marker(page_number, "1"),
        13 => marker(page_number, "2"),
        14 => marker(page_number, "IV"),
        15 | 16 => marker(section, "Front"),
        17 => marker(section, "Back"),
        _ => (Display::Custom, "Edition 7 / café"),
    }
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
    let (display, value) = display(case);
    let mut story = Story::from_text(if reference { value } else { "" }, "Source");
    if reference {
        story
            .ranges
            .push(StyleRange::new(0, value.len(), "Instance"));
    } else {
        let (kind, control) = match &display {
            Display::Marker(control) => ("ProcessingInstruction", control.clone()),
            _ => (
                "TextVariableInstance",
                InlineControl::TextVariable {
                    variable: "edition".into(),
                    character_style: "Instance".into(),
                    name: String::new(),
                },
            ),
        };
        story.structures.push(StoryStructure {
            at: Some(0),
            kind: kind.into(),
            payload: "original instance".into(),
            footnote: None,
            control: Some(control),
        });
        match &display {
            Display::LastPage(spec) => doc.text_variables.push(TextVariable::new(
                "edition",
                "Last page",
                VariableKind::LastPage(spec.clone()),
            )),
            Display::Custom => doc
                .text_variables
                .push(TextVariable::custom("edition", "Edition", value)),
            Display::Marker(_) => {}
        }
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
    if !matches!(display, Display::Custom) {
        for _ in 0..2 {
            doc.add_page(doc.pages[0].clone());
        }
        doc.pages[0].section = Some(Section {
            marker: "Front".into(),
            ..Default::default()
        });
        doc.pages[2].section = Some(Section {
            start: 4,
            continue_numbering: false,
            style: NumberStyle::RomanUpper,
            marker: "Back".into(),
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
