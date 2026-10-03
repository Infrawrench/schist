use schist_layout::{
    authoring, blank_a4, compose, story::InlineControl, text_variables::TextVariable, History,
    LayoutDocument, Rect, Story, StoryId, StoryStructure,
};

fn document(text: &str, at: usize, value: &str) -> LayoutDocument {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 400.0, 300.0),
    )
    .unwrap();
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    paragraph.family = Some("IBM Plex Sans".into());
    paragraph.point_size = Some(14.0);
    paragraph.leading = Some(schist_layout::styles::Leading::Points(18.0));
    let mut story = Story::from_text(text, "Body");
    story.structures.push(StoryStructure {
        at: Some(at),
        kind: "TextVariableInstance".into(),
        payload: "inert recovery data".into(),
        footnote: None,
        control: Some(InlineControl::TextVariable {
            variable: "edition".into(),
            character_style: String::new(),
            name: "Instance".into(),
        }),
    });
    doc.stories[frame.story.0 as usize] = story;
    doc.text_variables.push(TextVariable {
        id: "edition".into(),
        name: "Definition".into(),
        contents: value.into(),
    });
    doc
}

#[test]
fn custom_values_compose_at_source_boundaries_without_replacing_editable_text() {
    let text = "Aé中🙂B";
    for at in text.char_indices().map(|(at, _)| at).chain([text.len()]) {
        let doc = document(text, at, "Edition 7");
        let source = doc.clone();
        let composed = compose::compose_story(&doc, StoryId(0));
        assert_eq!(doc, source);
        assert!(composed
            .frames
            .iter()
            .all(|f| !f.lost && f.unrendered_structures == 0));
        let projected: Vec<_> = composed
            .frames
            .iter()
            .flat_map(|f| &f.lines)
            .filter_map(|l| l.projected.as_ref())
            .collect();
        assert_eq!(
            projected
                .iter()
                .filter(|p| p.spec.text.contains("Edition 7"))
                .count(),
            1
        );
        for line in projected {
            if let Some(start) = line.spec.text.find("Edition 7") {
                let positions = line.positions.as_ref().unwrap();
                for byte in start..=start + "Edition 7".len() {
                    assert_eq!(positions.source(byte), at);
                }
            }
        }
    }
}

#[test]
fn changing_one_shared_custom_value_updates_every_instance_without_source_edits() {
    let mut doc = document("prefix suffix", 7, "first value");
    let mut second = doc.stories[0].structures[0].clone();
    second.at = Some(doc.stories[0].text_len());
    doc.stories[0].structures.push(second);
    let source = doc.stories.clone();
    for value in ["first value", "second value", "third value"] {
        doc.text_variables[0].contents = value.into();
        let composed = compose::compose_story(&doc, StoryId(0));
        assert_eq!(doc.stories, source);
        let rendered = composed
            .frames
            .iter()
            .flat_map(|f| &f.lines)
            .filter_map(|l| l.projected.as_ref())
            .map(|p| p.spec.text.as_str())
            .collect::<String>();
        assert_eq!(rendered.matches(value).count(), 2);
        assert!(composed.frames.iter().all(|f| f.unrendered_structures == 0));
    }
}

#[test]
fn overwide_variables_stay_whole_and_continue_in_a_wider_frame() {
    for value in [
        "red blue green gold",
        "café a very long edition",
        "漢 字 名",
    ] {
        let mut doc = document("", 0, value);
        doc.objects[0].bounds.width = 10.0;
        let second = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 100.0, 400.0, 100.0),
        )
        .unwrap();
        for object in &mut doc.objects {
            if let schist_layout::LayoutObject::TextFrame {
                story, overflow, ..
            } = &mut object.object
            {
                *story = StoryId(0);
                *overflow = schist_layout::FrameOverflow::Thread;
            }
        }
        let composed = compose::compose_story(&doc, StoryId(0));
        assert_eq!(composed.frames.len(), 2);
        assert!(composed.frames[0].lines.is_empty());
        assert!(composed.frames[0].passed_on);
        assert_eq!(composed.frames[1].object, second.object);
        assert!(!composed.frames[1].lost);
        assert!(composed.frames[1].lines.iter().any(|line| line
            .projected
            .as_ref()
            .unwrap()
            .spec
            .text
            .contains(value)));
        doc.objects.truncate(1);
        assert!(compose::compose_story(&doc, StoryId(0)).frames[0].lost);
    }
}

#[test]
fn coincident_notes_and_variables_keep_structure_order_and_their_own_styles() {
    use schist_layout::footnotes::{FootnoteBody, FootnoteMarker};
    for reverse in [false, true] {
        let mut doc = document("before after", 7, "Edition");
        doc.footnotes.no_splitting = Some(true);
        doc.footnotes.rule.on = Some(false);
        let note = StoryStructure {
            at: Some(7),
            kind: "Footnote".into(),
            payload: "original note".into(),
            control: None,
            footnote: Some(FootnoteBody {
                story: Story::from_text(" note body", "Body"),
                markers: vec![FootnoteMarker {
                    at: 0,
                    character_style: String::new(),
                }],
                reference_paragraph_style: "Body".into(),
                reference_character_style: String::new(),
            }),
        };
        doc.stories[0].structures.push(note);
        if reverse {
            doc.stories[0].structures.reverse();
        }
        let source = doc.clone();
        let composed = compose::compose_story(&doc, StoryId(0));
        assert_eq!(doc, source);
        let frame = &composed.frames[0];
        assert_eq!(frame.unrendered_structures, 0);
        assert!(!frame.lost);
        assert_eq!(frame.footnotes.len(), 1);
        assert_eq!(frame.footnotes[0].structure, usize::from(!reverse));
        let main = frame
            .lines
            .iter()
            .map(|l| l.projected.as_ref().unwrap().spec.text.as_str())
            .collect::<String>();
        assert!(main.contains(if reverse {
            "1\u{2068}Edition\u{2069}"
        } else {
            "\u{2068}Edition\u{2069}1"
        }));
        let spec = &frame.lines[0].projected.as_ref().unwrap().spec;
        assert_eq!(spec.style_at(spec.text.find("Edition").unwrap()).size, 14.0);
        assert!(spec.style_at(spec.text.find('1').unwrap()).size < 14.0);
    }
}

#[test]
fn invalid_notes_do_not_disable_independent_variables_and_invalid_variables_stay_diagnosed() {
    let mut doc = document("source", 0, "display");
    doc.footnotes.start_at = Some(0);
    let frame = &compose::compose_story(&doc, StoryId(0)).frames[0];
    assert_eq!(frame.unrendered_structures, 0);
    assert!(frame.lines[0]
        .projected
        .as_ref()
        .unwrap()
        .spec
        .text
        .contains("display"));
    for value in ["bad\nvalue", "bad\tvalue", "bad\u{2069}value"] {
        doc.text_variables[0].contents = value.into();
        assert_eq!(
            compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
            1
        );
    }
    doc.text_variables[0].contents = "valid".into();
    doc.text_variables.push(doc.text_variables[0].clone());
    assert_eq!(
        compose::compose_story(&doc, StoryId(0)).frames[0].unrendered_structures,
        1
    );
    let mut interior = document("e\u{301}é", 1, "valid");
    assert_eq!(
        compose::compose_story(&interior, StoryId(0)).frames[0].unrendered_structures,
        1
    );
    interior.stories[0].structures[0].at = Some(usize::MAX);
    assert_eq!(
        compose::compose_story(&interior, StoryId(0)).frames[0].unrendered_structures,
        1
    );
}

#[test]
fn empty_values_remain_resolved_and_justification_never_stretches_a_variable() {
    for text in ["", "a b"] {
        let doc = document(text, 0, "");
        let composed = compose::compose_story(&doc, StoryId(0));
        assert!(!composed.frames[0].lost);
        assert_eq!(composed.frames[0].unrendered_structures, 0);
    }
    let mut doc = document("a  b", 2, "red blue gold");
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .align = Some(schist_layout::styles::Align::JustifyAll);
    let composed = compose::compose_story(&doc, StoryId(0));
    let line = &composed.frames[0].lines[0];
    assert!(
        (line.word_space.unwrap() * 2.0 + line.natural_width - line.bounds.width).abs() < 0.001
    );
    let spec = &line.projected.as_ref().unwrap().spec;
    assert!((schist_text_engine::line_spans(spec)[0].width - line.bounds.width).abs() < 0.001);
}
