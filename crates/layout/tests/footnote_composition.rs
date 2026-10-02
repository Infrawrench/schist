use schist_layout::{
    footnote_composition,
    footnotes::*,
    inline_text::Projection,
    styles::{BaselineShift, TextPosition},
    CharacterStyle, LayoutDocument, ParagraphStyle, Story, StoryId, StoryStructure, StyleRange,
};

fn document(text: &str, anchors: &[usize]) -> LayoutDocument {
    let mut doc = LayoutDocument::default();
    doc.styles.paragraphs.push(ParagraphStyle {
        name: "Note".into(),
        based_on: Some("Body".into()),
        point_size: Some(9.0),
        space_before: Some(50.0),
        space_after: Some(70.0),
        ..Default::default()
    });
    doc.styles.characters.push(CharacterStyle {
        name: "Reference".into(),
        bold: Some(true),
        baseline_shift: Some(BaselineShift::Offset(2.0)),
        ..Default::default()
    });
    let mut story = Story::from_text(text, "Body");
    for at in anchors {
        let mut note_story = Story::from_text(" é note", "Note");
        note_story.push_paragraph("5 literal", "Note");
        story.structures.push(StoryStructure {
            at: Some(*at),
            kind: "Footnote".into(),
            payload: "original native XML".into(),
            footnote: Some(FootnoteBody {
                story: note_story,
                markers: vec![FootnoteMarker {
                    at: 0,
                    character_style: "Reference".into(),
                }],
                reference_paragraph_style: "Body".into(),
                reference_character_style: "Reference".into(),
            }),
        });
    }
    doc.stories.push(story);
    doc
}

#[test]
fn numbered_notes_own_no_main_source_bytes_and_keep_independent_style_contexts() {
    for text in ["", "Aé界B", "A\n\nB"] {
        let anchors: Vec<_> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len(), text.len()])
            .collect();
        let mut doc = document(text, &anchors);
        doc.footnotes.start_at = Some(9);
        doc.footnotes.affixes = Some(FootnoteAffixes::Both);
        doc.footnotes.prefix = Some("[".into());
        doc.footnotes.suffix = Some("]".into());
        doc.styles.characters.push(CharacterStyle {
            name: "Explicit".into(),
            italic: Some(true),
            ..Default::default()
        });
        doc.footnotes.marker_style = Some(FootnoteReference::Resolved("Explicit".into()));
        let original = doc.clone();
        let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
        assert_eq!(doc, original);
        assert_eq!(prepared.notes.len(), anchors.len());
        let projected = prepared.main.story.text();
        for (index, note) in prepared.notes.iter().enumerate() {
            let expected = format!("[{}]", index + 9);
            assert_eq!(&projected[note.reference.clone()], expected);
            assert_eq!(
                prepared.main.positions.source(note.reference.start),
                note.anchor
            );
            assert_eq!(
                prepared.main.positions.source(note.reference.end),
                note.anchor
            );
            assert_eq!(
                note.body.story.text(),
                format!("{expected} é note\n5 literal")
            );
            let reference = prepared
                .main
                .story
                .ranges
                .iter()
                .find(|r| r.start == note.reference.start)
                .unwrap();
            let style = prepared.styles.resolve_character(&reference.style);
            assert_eq!(style.position, Some(TextPosition::Superscript));
            assert_eq!(style.bold, Some(true));
            assert_eq!(style.italic, Some(true));
            for (index, point) in note.body.story.points.iter().enumerate() {
                let schist_layout::StoryPoint::Paragraph { style, .. } = point else {
                    panic!()
                };
                let style = prepared.styles.resolve_paragraph(style);
                assert_eq!(
                    style.space_before,
                    Some(if index == 0 { 0.0 } else { 50.0 })
                );
                assert_eq!(style.space_after, Some(if index == 0 { 70.0 } else { 0.0 }));
                assert_eq!(style.point_size, Some(9.0));
            }
        }
        assert_eq!(
            prepared.styles.resolve_paragraph("Note").space_before,
            Some(50.0)
        );
    }
}

#[test]
fn temporary_styles_preserve_every_resolved_property_without_colliding_with_saved_names() {
    let mut doc = document("aéb", &[1]);
    doc.styles.characters.push(CharacterStyle {
        name: "Schist generated footnote reference 0".into(),
        point_size: Some(100.0),
        ..Default::default()
    });
    for source in &doc.styles.characters {
        let resolved = doc.styles.resolve_character(&source.name);
        let mut styles = doc.styles.clone();
        styles
            .characters
            .push(resolved.clone().into_style("Temporary"));
        assert_eq!(styles.resolve_character("Temporary"), resolved);
    }
    let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
    assert_ne!(
        prepared.main.story.ranges[0].style,
        "Schist generated footnote reference 0"
    );
    assert_eq!(
        prepared
            .styles
            .resolve_character("Schist generated footnote reference 0")
            .point_size,
        Some(100.0)
    );
}

#[test]
fn explicit_marker_styles_override_document_position_and_invalid_empty_bodies_stay_opaque() {
    for position in [
        TextPosition::Normal,
        TextPosition::Superscript,
        TextPosition::Subscript,
    ] {
        for requested in [
            FootnoteMarkerPosition::Normal,
            FootnoteMarkerPosition::Superscript,
            FootnoteMarkerPosition::Subscript,
        ] {
            let mut doc = document("body", &[2]);
            doc.styles.characters.push(CharacterStyle {
                name: "Override".into(),
                position: Some(position),
                ..Default::default()
            });
            doc.footnotes.marker_style = Some(FootnoteReference::Resolved("Override".into()));
            doc.footnotes.marker_position = Some(requested);
            let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
            assert_eq!(
                prepared
                    .styles
                    .resolve_character(&prepared.main.story.ranges[0].style)
                    .position,
                Some(position)
            );
        }
    }
    let mut doc = document("body", &[2]);
    doc.stories[0].structures[0]
        .footnote
        .as_mut()
        .unwrap()
        .story = Story::new();
    assert!(footnote_composition::prepare(&doc, StoryId(0)).is_none());
}

#[test]
fn unresolved_numbering_and_structure_coordinates_never_receive_guessed_markers() {
    let original = document("aéb", &[1]);
    for restart in [
        FootnoteRestart::Page,
        FootnoteRestart::Spread,
        FootnoteRestart::Section,
        FootnoteRestart::Other("Unknown".into()),
    ] {
        let mut doc = original.clone();
        doc.footnotes.restart = Some(restart);
        assert!(footnote_composition::prepare(&doc, StoryId(0)).is_none());
    }
    for at in [None, Some(2), Some(usize::MAX)] {
        let mut doc = original.clone();
        doc.stories[0].structures[0].at = at;
        assert!(footnote_composition::prepare(&doc, StoryId(0)).is_none());
    }
    let mut doc = original.clone();
    doc.footnotes.marker_style = Some(FootnoteReference::Unresolved("Native Missing".into()));
    assert!(footnote_composition::prepare(&doc, StoryId(0)).is_none());
    doc = original;
    doc.stories[0].structures[0].footnote = None;
    assert!(footnote_composition::prepare(&doc, StoryId(0)).is_none());
}

#[test]
fn projected_runs_shape_real_inline_advances_and_map_engine_hits_to_source_boundaries() {
    let source = Story::from_text("éword", "Body");
    let doc = document(&source.text(), &[2]);
    let prepared = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
    let text = prepared.main.story.text();
    let spec = schist_layout::compose::spec_for(
        &prepared.main.story,
        0,
        text.len(),
        &prepared.styles,
        "Body",
        &doc.default_character_style,
        0.0,
    );
    let original = schist_layout::compose::spec_for(
        &source,
        0,
        source.text_len(),
        &doc.styles,
        "Body",
        &doc.default_character_style,
        0.0,
    );
    assert!(
        schist_text_engine::measure(&spec).unwrap().width
            > schist_text_engine::measure(&original).unwrap().width
    );
    let positions = prepared.main.positions.line(&text, 0, text.len()).unwrap();
    for (position, _) in schist_text_engine::insertion_points(&spec) {
        assert!(source
            .text()
            .is_char_boundary(positions.source(position.byte)));
    }
    // Character styles that cross generated text are split, including overlaps;
    // source precedence is unchanged and cannot leak into the generated marker.
    let mut overlapping = source;
    overlapping.ranges = vec![
        StyleRange::new(0, 4, "First"),
        StyleRange::new(2, 6, "Second"),
    ];
    let projection = Projection::new(
        &overlapping,
        vec![schist_layout::inline_text::Insertion {
            at: 3,
            text: "99".into(),
            style: "Marker".into(),
        }],
    )
    .unwrap();
    for at in [2, 3, 4, 5] {
        let visual = projection.positions.after(at);
        let expected = overlapping
            .ranges
            .iter()
            .find(|r| r.start <= at && at < r.end)
            .unwrap();
        let actual = projection
            .story
            .ranges
            .iter()
            .find(|r| r.start <= visual && visual < r.end)
            .unwrap();
        assert_eq!(actual.style, expected.style);
    }
}

type TestFrame = (
    schist_layout::ObjectId,
    schist_layout::Rect,
    schist_layout::FrameOverflow,
    u16,
    f32,
    schist_layout::compose::InsetsLike,
);

fn frames(heights: &[f32]) -> Vec<TestFrame> {
    heights
        .iter()
        .enumerate()
        .map(|(index, height)| {
            (
                schist_layout::ObjectId(index as u32 + 1),
                schist_layout::Rect::new(10.0, 20.0, 95.0, *height),
                schist_layout::FrameOverflow::Thread,
                1,
                0.0,
                schist_layout::Insets::ZERO.into(),
            )
        })
        .collect()
}

#[test]
fn whole_notes_and_references_move_together_without_losing_source_or_overlapping_body() {
    let text = "é words that cross several lines and keep their source positions.";
    for at in text.char_indices().map(|(at, _)| at).chain([text.len()]) {
        for height in [12.0, 35.0, 55.0, 80.0, 120.0] {
            let mut doc = document(text, &[at]);
            doc.footnotes.no_splitting = Some(true);
            doc.footnotes.spacer = Some(7.0);
            let original = doc.clone();
            let composed =
                schist_layout::compose_thread(&doc, StoryId(0), &frames(&[height, 400.0]));
            assert_eq!(doc, original);
            assert!(
                !composed.frames.last().unwrap().lost,
                "at={at} height={height}"
            );
            assert_eq!(composed.frames.last().unwrap().consumed_to, text.len());
            assert_eq!(
                composed
                    .frames
                    .iter()
                    .map(|f| f.footnotes.len())
                    .sum::<usize>(),
                1
            );
            let mut source_end = 0;
            for frame in &composed.frames {
                assert_eq!(frame.unrendered_structures, 0);
                let mut references = 0;
                for line in &frame.lines {
                    assert!(text[source_end..line.start].trim().is_empty());
                    source_end = line.end;
                    let spec = schist_layout::compose::line_spec(line, &doc.stories[0], &doc);
                    references += spec.text.matches('1').count();
                    for (position, _) in schist_text_engine::insertion_points(&spec) {
                        assert!(text.is_char_boundary(line.source_byte(position.byte)));
                    }
                }
                assert_eq!(references, frame.footnotes.len());
                let body_bottom = frame
                    .lines
                    .iter()
                    .map(|l| l.bounds.bottom())
                    .fold(20.0f32, f32::max);
                for note in &frame.footnotes {
                    assert!(note.bounds.y + 0.001 >= body_bottom + 7.0);
                    assert!(
                        note.bounds.bottom()
                            <= 20.0 + if frame.object.0 == 1 { height } else { 400.0 } + 0.001
                    );
                    assert_eq!(note.anchor, at);
                    assert!(note
                        .lines
                        .iter()
                        .all(|l| l.is_generated() && l.start == at && l.end == at));
                    let body: String = note
                        .lines
                        .iter()
                        .map(|l| schist_layout::compose::line_spec(l, &doc.stories[0], &doc).text)
                        .collect();
                    assert!(body.contains("1 é note"));
                    assert!(body.contains("5 literal"));
                }
            }
            assert!(text[source_end..].trim().is_empty());
        }
    }
}

#[test]
fn note_gaps_end_placement_and_impossible_fits_have_explicit_outcomes() {
    let mut doc = document("body", &[0, 4]);
    doc.footnotes.no_splitting = Some(true);
    doc.footnotes.space_between = Some(8.0);
    doc.footnotes.spacer = Some(10.0);
    doc.footnotes.rule.on = Some(true);
    doc.footnotes.rule.width = Some(40.0);
    doc.footnotes.rule.weight = Some(1.0);
    for below_text in [false, true] {
        doc.footnotes.end_of_story = Some(below_text);
        let result = schist_layout::compose_thread(&doc, StoryId(0), &frames(&[400.0]));
        let frame = &result.frames[0];
        assert!(!frame.lost);
        assert_eq!(frame.footnotes.len(), 2);
        let notes = &frame.footnotes;
        assert!((notes[1].bounds.y - notes[0].bounds.bottom() - 8.0).abs() < 0.001);
        assert_eq!(notes[0].rule.as_ref().unwrap().bounds.width, 40.0);
        assert!(notes[1].rule.is_none());
        if below_text {
            let bottom = frame
                .lines
                .iter()
                .map(|l| l.bounds.bottom())
                .fold(0.0f32, f32::max);
            assert!((notes[0].bounds.y - bottom - 10.0).abs() < 0.001);
        } else {
            assert!((notes[1].bounds.bottom() - 420.0).abs() < 0.001);
        }
    }
    let result = schist_layout::compose_thread(&doc, StoryId(0), &frames(&[15.0, 15.0]));
    assert!(result.frames.last().unwrap().lost);
    assert!(result
        .frames
        .iter()
        .all(|f| f.footnotes.is_empty() && f.lines.is_empty()));
    let mut multi = frames(&[400.0]);
    multi[0].3 = 2;
    let unsupported = schist_layout::compose_thread(&doc, StoryId(0), &multi);
    assert_eq!(unsupported.frames[0].unrendered_structures, 2);
    assert!(unsupported.frames[0].footnotes.is_empty());
    assert!(unsupported.frames[0]
        .lines
        .iter()
        .all(|l| l.projected.is_none()));
}
