use schist_layout::{
    compose::{compose_thread, InsetsLike},
    list_composition::{GeneratedRole, GeneratedText},
    lists::ListKind,
    FrameOverflow, ObjectId, Rect, Story, StoryId, StoryPoint, StyleRange,
};

#[path = "../../separation/examples/support/named_initials.rs"]
mod support;

fn paragraphs(text: &str, style: &str) -> Story {
    Story {
        points: text
            .split('\n')
            .map(|text| StoryPoint::Paragraph {
                text: text.into(),
                style: style.into(),
            })
            .collect(),
        ..Default::default()
    }
}

fn markers_without_references(
    doc: &schist_layout::LayoutDocument,
    story: &Story,
) -> Vec<GeneratedText> {
    let id = doc
        .stories
        .iter()
        .position(|s| std::ptr::eq(s, story))
        .unwrap();
    let mut control = doc.clone();
    control.stories[id].structures.clear();
    let frames = [(
        ObjectId(999),
        Rect::new(0.0, 0.0, 230.0, 700.0),
        FrameOverflow::Thread,
        1,
        0.0,
        InsetsLike::default(),
    )];
    let thread = compose_thread(&control, StoryId(id as u32), &frames);
    assert!(!thread.has_overflow());
    thread
        .frames
        .iter()
        .flat_map(|f| &f.lines)
        .filter_map(|l| {
            l.generated
                .as_ref()
                .filter(|g| g.role == GeneratedRole::Marker)
                .cloned()
        })
        .collect()
}

#[test]
fn footnote_references_cannot_replace_list_marker_source_context() {
    support::register_font();
    for kind in [ListKind::Bullet, ListKind::Numbered] {
        for explicit in [false, true] {
            for initial in [false, true] {
                for split in [false, true] {
                    for text in ["", "E\u{301}abc first\nSecond"] {
                        for anchor in [0, text.len().min(3), text.find('\n').map_or(0, |at| at + 1)]
                        {
                            let mut doc = support::document(false, 7);
                            let paragraph = doc
                                .styles
                                .paragraphs
                                .iter_mut()
                                .find(|p| p.name == "Source")
                                .unwrap();
                            paragraph.list.kind = Some(kind);
                            paragraph.list.bullet_character_style =
                                explicit.then(|| "Direct".into());
                            paragraph.list.numbering_character_style =
                                explicit.then(|| "Direct".into());
                            paragraph.list.continue_numbering = Some(true);
                            paragraph.left_indent = Some(30.0);
                            paragraph.first_line_indent = Some(-30.0);
                            if !initial {
                                paragraph.nested_styles = None;
                            }
                            doc.footnotes.no_splitting = Some(!split);
                            let mut structure = doc.stories[0].structures[0].clone();
                            structure.at = Some(anchor);
                            let note = structure.footnote.as_mut().unwrap();
                            let note_text = if split && !text.is_empty() {
                                format!("{text}\nThird\nFourth\nFifth\nSixth\nSeventh\nEighth")
                            } else {
                                text.into()
                            };
                            note.story = paragraphs(&note_text, "Source");
                            note.markers[0].at = anchor;
                            doc.stories[0] = paragraphs(text, "Source");
                            if !text.is_empty() {
                                doc.stories[0].ranges.push(StyleRange::new(0, 3, "Direct"));
                                note.story.ranges.push(StyleRange::new(0, 3, "Direct"));
                            }
                            let mut scratch = schist_layout::LayoutDocument::new(Vec::new());
                            scratch.styles = doc.styles.clone();
                            scratch.default_paragraph_style = doc.default_paragraph_style.clone();
                            scratch.default_character_style = doc.default_character_style.clone();
                            scratch.stories.push(note.story.clone());
                            let expected_note =
                                markers_without_references(&scratch, &scratch.stories[0]);
                            doc.stories[0].structures.push(structure);
                            let expected_main = markers_without_references(&doc, &doc.stories[0]);
                            let before = doc.clone();
                            let frames: Vec<_> = (0..8)
                                .map(|i| {
                                    (
                                        ObjectId(1000 + i),
                                        Rect::new(
                                            i as f32 * 250.0,
                                            0.0,
                                            230.0,
                                            if split { 105.0 } else { 700.0 },
                                        ),
                                        FrameOverflow::Thread,
                                        1,
                                        0.0,
                                        InsetsLike::default(),
                                    )
                                })
                                .collect();
                            let thread = compose_thread(&doc, StoryId(0), &frames);
                            assert!(!thread.has_overflow());
                            let generated = |line: &schist_layout::ComposedLine| {
                                line.generated
                                    .as_ref()
                                    .filter(|g| g.role == GeneratedRole::Marker)
                                    .cloned()
                            };
                            let main: Vec<_> = thread
                                .frames
                                .iter()
                                .flat_map(|f| &f.lines)
                                .filter_map(generated)
                                .collect();
                            let note: Vec<_> = thread
                                .frames
                                .iter()
                                .flat_map(|f| &f.footnotes)
                                .flat_map(|a| &a.lines)
                                .filter_map(generated)
                                .collect();
                            assert_eq!(
                                main, expected_main,
                                "main {kind:?}/{explicit}/{initial}/{split}/{text:?}/{anchor}"
                            );
                            assert_eq!(
                                note, expected_note,
                                "note {kind:?}/{explicit}/{initial}/{split}/{text:?}/{anchor}"
                            );
                            if split && !text.is_empty() {
                                assert!(
                                    thread
                                        .frames
                                        .iter()
                                        .filter(|f| !f.footnotes.is_empty())
                                        .count()
                                        > 1
                                );
                            }
                            assert_eq!(doc, before);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn projected_markers_keep_document_counter_chronology_and_explicit_marker_styles() {
    use schist_layout::{authoring, compose::compose_story, lists::NumberingList, History};
    support::register_font();
    for explicit in [false, true] {
        let mut doc = support::document(false, 7);
        let paragraph = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|p| p.name == "Source")
            .unwrap();
        paragraph.list.kind = Some(ListKind::Numbered);
        paragraph.list.list = Some("sequence".into());
        paragraph.list.continue_numbering = Some(true);
        paragraph.list.expression = Some("^#)^t".into());
        paragraph.list.numbering_character_style = explicit.then(|| "Direct".into());
        let mut note_style = paragraph.clone();
        note_style.name = "Note".into();
        note_style.list.kind = Some(ListKind::None);
        doc.styles.add_paragraph(note_style);
        doc.styles.numbering_lists.push(NumberingList {
            id: "sequence".into(),
            name: "Sequence".into(),
            across_stories: true,
            ..Default::default()
        });
        let mut structure = doc.stories[0].structures[0].clone();
        structure.footnote.as_mut().unwrap().story = Story::from_text("Note", "Note");
        doc.stories[0] = paragraphs("First\nSecond", "Source");
        doc.stories[0].structures.push(structure);
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 350.0, 280.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = doc.stories[0].clone();
        let before = doc.clone();
        for (index, id) in [StoryId(0), frame.story].into_iter().enumerate() {
            let expected = markers_without_references(&doc, &doc.stories[id.0 as usize]);
            assert_eq!(
                expected
                    .iter()
                    .map(|g| g.spec.text.clone())
                    .collect::<Vec<_>>(),
                vec![format!("{})", index * 2 + 1), format!("{})", index * 2 + 2)]
            );
            let thread = compose_story(&doc, id);
            assert!(!thread.has_overflow());
            let actual: Vec<_> = thread
                .frames
                .iter()
                .flat_map(|f| &f.lines)
                .filter_map(|l| {
                    l.generated
                        .as_ref()
                        .filter(|g| g.role == GeneratedRole::Marker)
                        .cloned()
                })
                .collect();
            assert_eq!(actual, expected);
        }
        assert_eq!(doc, before);
    }
}
