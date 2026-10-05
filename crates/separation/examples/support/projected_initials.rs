//! Compare generated references with ordinary authored text. The control owns
//! literal numbers and explicit initial counts, and uses no footnote projection.
use schist_layout::{
    authoring,
    compose::compose_object,
    footnotes::*,
    styles::{Leading, TextPosition},
    CharacterStyle, History, Ink, Insets, LayoutDocument, LayoutObject, Page, ParagraphStyle, Rect,
    Story, StoryStructure, StyleRange,
};

pub fn register_font() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
}

fn frame(doc: &mut LayoutDocument, bounds: Rect, story: Story) {
    let made = authoring::text_frame(doc, &mut History::default(), 0, bounds).unwrap();
    if let LayoutObject::TextFrame { insets, .. } = &mut doc.objects.last_mut().unwrap().object {
        *insets = Insets::ZERO;
    }
    doc.stories[made.story.0 as usize] = story;
}

pub fn document(reference: bool, spot: bool, middle: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 360.0, 280.0)]);
    for (name, characters) in [("Main", 2), ("Note", 1)] {
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(11.0),
            leading: Some(Leading::Points(16.0)),
            drop_caps_lines: Some(3),
            drop_caps_characters: Some(characters),
            fill: Some(if spot {
                Ink::spot("Initial violet", [40.0, 50.0, -50.0])
            } else {
                Ink::cmyk("Initial cyan", [1.0, 0.0, 0.0, 0.0])
            }),
            keep_lines: Some(1),
            ..Default::default()
        });
    }
    doc.styles.add_character(CharacterStyle {
        name: "Reference".into(),
        position: Some(TextPosition::Superscript),
        fill: Some(Ink::spot("Reference gold", [60.0, 30.0, 60.0])),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Note marker".into(),
        fill: Some(Ink::cmyk("Marker magenta", [0.0, 1.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.footnotes = FootnoteOptions {
        start_at: Some(105),
        no_splitting: Some(true),
        first_baseline: Some(FootnoteFirstBaseline::Ascent),
        spacer: Some(8.0),
        rule: FootnoteRule {
            on: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = "E\u{301}xalpha\u{2028}bravo\u{2028}cello\u{2028}delta";
    let note = "Énote\u{2028}second\u{2028}third\u{2028}fourth";
    let anchor = if middle { "E\u{301}".len() } else { 0 };
    let bounds = Rect::new(25.0, 40.0, 310.0, 215.0);
    if reference {
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Main")
            .unwrap()
            .drop_caps_characters = Some(5);
        doc.styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Note")
            .unwrap()
            .drop_caps_characters = Some(4);
        let mut main =
            Story::from_text(format!("{}105{}", &text[..anchor], &text[anchor..]), "Main");
        main.ranges
            .push(StyleRange::new(anchor, anchor + 3, "Reference"));
        frame(&mut doc, bounds, main);
        let mut note = Story::from_text(format!("105{note}"), "Note");
        note.ranges.push(StyleRange::new(0, 3, "Note marker"));
        // Measure the independently authored note in an ordinary frame. Initial
        // geometry is tested separately; this reference isolates projection and
        // footnote placement, without consulting generated lines or counters.
        let mut control = doc.clone();
        frame(
            &mut control,
            Rect::new(0.0, 0.0, bounds.width, 1000.0),
            note.clone(),
        );
        let composed = compose_object(&control, control.objects.last().unwrap()).unwrap();
        assert!(!composed.lost);
        let height = composed
            .lines
            .iter()
            .map(|line| line.bounds.bottom())
            .fold(0.0, f32::max);
        frame(
            &mut doc,
            Rect::new(
                bounds.x,
                bounds.bottom() - height,
                bounds.width,
                height + 0.01,
            ),
            note,
        );
    } else {
        let mut main = Story::from_text(text, "Main");
        main.structures.push(StoryStructure {
            control: None,
            at: Some(anchor),
            kind: "Footnote".into(),
            payload: "source".into(),
            footnote: Some(FootnoteBody {
                story: Story::from_text(note, "Note"),
                markers: vec![FootnoteMarker {
                    at: 0,
                    character_style: "Note marker".into(),
                }],
                reference_paragraph_style: "Main".into(),
                reference_character_style: "Reference".into(),
            }),
            anchored: None,
        });
        frame(&mut doc, bounds, main);
    }
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Heading".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(8.0),
        ..Default::default()
    });
    frame(
        &mut doc,
        Rect::new(12.0, 10.0, 336.0, 15.0),
        Story::from_text(
            format!(
                "Source initials / reference {} / {}",
                if middle { "inside" } else { "before" },
                if spot { "spot" } else { "process" }
            ),
            "Heading",
        ),
    );
    doc
}
