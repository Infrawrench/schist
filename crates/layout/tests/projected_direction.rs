use schist_layout::{
    compose::spec_for,
    footnote_composition,
    footnotes::{FootnoteAffixes, FootnoteBody, FootnoteMarker},
    ParagraphDirection, ParagraphStyle, Story, StoryPoint, StoryStructure,
};
use schist_text_engine::ParagraphDirection as EngineDirection;

#[test]
fn generated_reference_text_cannot_choose_the_base_direction_of_an_authored_paragraph() {
    for (text, automatic) in [
        ("אבג source", EngineDirection::RightToLeft),
        ("source אבג", EngineDirection::LeftToRight),
        ("123 —", EngineDirection::LeftToRight),
        ("", EngineDirection::LeftToRight),
    ] {
        for direction in [
            None,
            Some(ParagraphDirection::Auto),
            Some(ParagraphDirection::LeftToRight),
            Some(ParagraphDirection::RightToLeft),
        ] {
            for prefix in ["A", "א", "ا", ""] {
                for anchor in [0, text.len()] {
                    let mut doc = schist_layout::blank_a4();
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: "Source".into(),
                        direction,
                        ..Default::default()
                    });
                    doc.footnotes.prefix = Some(prefix.into());
                    doc.footnotes.affixes = Some(FootnoteAffixes::Both);
                    let mut story = Story::from_text(text, "Source");
                    story.structures.push(StoryStructure {
                        control: None,
                        at: Some(anchor),
                        kind: "Footnote".into(),
                        payload: "source".into(),
                        footnote: Some(FootnoteBody {
                            story: Story::from_text(text, "Source"),
                            markers: vec![FootnoteMarker {
                                at: 0,
                                character_style: "Default".into(),
                            }],
                            reference_paragraph_style: "Source".into(),
                            reference_character_style: "Default".into(),
                        }),
                    });
                    let id = doc.add_story(story);
                    let before = doc.clone();
                    let prepared = footnote_composition::prepare(&doc, id).unwrap();
                    let expected = match direction {
                        Some(ParagraphDirection::LeftToRight) => EngineDirection::LeftToRight,
                        Some(ParagraphDirection::RightToLeft) => EngineDirection::RightToLeft,
                        _ => automatic,
                    };
                    for projection in [&prepared.main, &prepared.notes[0].body] {
                        let StoryPoint::Paragraph { style, .. } = &projection.story.points[0]
                        else {
                            panic!()
                        };
                        let spec = spec_for(
                            &projection.story,
                            0,
                            projection.story.text_len(),
                            &prepared.styles,
                            style,
                            &doc.default_character_style,
                            0.0,
                        );
                        assert_eq!(spec.direction, expected, "source {text:?}, reference prefix {prefix:?}, explicit {direction:?}, anchor {anchor}");
                    }
                    assert_eq!(doc, before);
                }
            }
        }
    }
}
