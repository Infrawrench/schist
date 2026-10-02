//! Footnotes compared with ordinary, independently positioned text and shapes.
use schist_layout::{
    authoring,
    footnotes::*,
    styles::{Leading, TextPosition},
    CharacterStyle, History, Ink, LayoutDocument, Page, ParagraphStyle, Rect, Story,
    StoryStructure, StyleRange,
};

pub fn document(reference: bool) -> LayoutDocument {
    let mut doc = LayoutDocument::new(vec![Page::new("proof", 200.0, 200.0); 4]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Main".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(12.0),
        leading: Some(Leading::Points(16.0)),
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Note".into(),
        family: Some("IBM Plex Sans".into()),
        point_size: Some(9.0),
        leading: Some(Leading::Points(12.0)),
        fill: Some(Ink::cmyk("Note cyan", [1.0, 0.0, 0.0, 0.0])),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Reference".into(),
        position: Some(TextPosition::Superscript),
        fill: Some(Ink::spot("Reference spot", [50.0, 60.0, 30.0])),
        fill_tint: Some(0.6),
        opacity: Some(0.8),
        ..Default::default()
    });
    doc.styles.add_character(CharacterStyle {
        name: "Note marker".into(),
        fill: Some(Ink::spot("Note marker spot", [65.0, -50.0, 20.0])),
        ..Default::default()
    });
    let rule_ink = Ink::spot("Rule spot", [45.0, 20.0, -50.0]);
    doc.footnotes = FootnoteOptions {
        start_at: Some(7),
        no_splitting: Some(true),
        first_baseline: Some(FootnoteFirstBaseline::Ascent),
        spacer: Some(10.0),
        marker_style: Some(FootnoteReference::Resolved("Reference".into())),
        rule: FootnoteRule {
            on: Some(true),
            width: Some(40.0),
            weight: Some(1.0),
            paint: Some(FootnoteReference::Resolved(rule_ink.clone())),
            tint: Some(0.75),
            overprint: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    // EosPlacement is document-wide. Separate stories use a shorter frame to
    // exercise another bottom position without changing that common preference.
    for page in 0..4 {
        let bounds = Rect::new(25.0, 20.0, 130.0, if page % 2 == 0 { 150.0 } else { 75.0 });
        let frame = authoring::text_frame(&mut doc, &mut History::default(), page, bounds).unwrap();
        if reference {
            let mut main = Story::from_text("Body7", "Main");
            main.ranges.push(StyleRange::new(4, 5, "Reference"));
            doc.stories[frame.story.0 as usize] = main;
            // A single explicit note line uses the independent text engine's
            // cell height, with no footnote plan or reserved-area code involved.
            let mut note = Story::from_text("7 Note", "Note");
            note.ranges.push(StyleRange::new(0, 1, "Note marker"));
            let spec = schist_layout::compose::spec_for(
                &note,
                0,
                note.text_len(),
                &doc.styles,
                "Note",
                &doc.default_character_style,
                0.0,
            );
            let height = schist_text_engine::measure(&spec).unwrap().height;
            let top = bounds.bottom() - height;
            let note_frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                page,
                Rect::new(bounds.x, top, bounds.width, height + 0.01),
            )
            .unwrap();
            doc.stories[note_frame.story.0 as usize] = note;
            let rule = authoring::rectangle(
                &mut doc,
                &mut History::default(),
                page,
                Rect::new(bounds.x, top - 0.5, 40.0, 1.0),
                authoring::Paint::default(),
            )
            .unwrap();
            let object = doc.objects.iter_mut().find(|o| o.id == rule).unwrap();
            // Authoring gives click-sized shapes an 8pt minimum. The explicit
            // reference is a one-point divider, not that gesture default.
            let scale = 1.0 / object.bounds.height;
            object.bounds.height = 1.0;
            if let schist_layout::LayoutObject::Shape {
                path,
                fill,
                stroke,
                tints,
                ..
            } = &mut object.object
            {
                path.map_points(|p| schist_layout::Point::new(p.x, p.y * scale));
                *fill = Some(rule_ink.clone());
                *stroke = None;
                tints.fill = 0.75;
            }
            object.overprint = true;
        } else {
            let mut main = Story::from_text("Body", "Main");
            main.structures.push(StoryStructure {
                at: Some(4),
                kind: "Footnote".into(),
                payload: "source".into(),
                footnote: Some(FootnoteBody {
                    story: Story::from_text(" Note", "Note"),
                    markers: vec![FootnoteMarker {
                        at: 0,
                        character_style: "Note marker".into(),
                    }],
                    reference_paragraph_style: "Main".into(),
                    reference_character_style: String::new(),
                }),
            });
            doc.stories[frame.story.0 as usize] = main;
        }
    }
    for object in &mut doc.objects {
        object.transparency = 0.75;
        if object.page >= 2 {
            object.transform = schist_core::Affine::rotate(0.09)
                .around(90.0, 90.0)
                .around(-object.bounds.x, -object.bounds.y);
        }
    }
    doc
}
