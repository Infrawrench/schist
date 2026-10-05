use schist_layout::{
    authoring, blank_a4, compose,
    story::{InlineControl, PageNumberKind},
    text_variables::{
        chapter_value, current_page_value, section_marker_value, ChapterNumber, ChapterNumbering,
        ChapterSource, PageNumberFormat, TextVariable, VariableKind, ARABIC_CHAPTER_FORMAT,
    },
    threading, CharacterStyle, History, LayoutDocument, NumberStyle, ObjectId, ParentObject,
    ParentPage, Rect, Section, Story, StoryId, StoryStructure,
};

fn pages(count: usize) -> LayoutDocument {
    let mut doc = blank_a4();
    for _ in 1..count {
        doc.add_page(doc.pages[0].clone());
    }
    doc
}

fn fonts(doc: &mut LayoutDocument) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let body = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    body.family = Some("IBM Plex Sans".into());
    body.point_size = Some(10.0);
}

fn marker(at: usize, control: InlineControl) -> StoryStructure {
    StoryStructure {
        at: Some(at),
        kind: "ProcessingInstruction".into(),
        payload: "original instruction".into(),
        footnote: None,
        control: Some(control),
        anchored: None,
    }
}

fn page_number(at: usize, kind: PageNumberKind) -> StoryStructure {
    marker(
        at,
        InlineControl::PageNumber {
            kind,
            character_style: String::new(),
        },
    )
}

fn section_marker(at: usize) -> StoryStructure {
    marker(
        at,
        InlineControl::SectionMarker {
            character_style: String::new(),
        },
    )
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

/// An independent label walk, including visible prefixes.
fn label(doc: &LayoutDocument, page: usize) -> String {
    let mut number = 0u32;
    let mut current = Section::default();
    for (index, p) in doc.pages.iter().enumerate().take(page + 1) {
        number += 1;
        if let Some(section) = &p.section {
            current = section.clone();
            if !section.continue_numbering {
                number = section.start.max(1);
            }
        }
        if index == page {
            let prefix = if current.include_prefix {
                current.prefix.as_str()
            } else {
                ""
            };
            return format!("{prefix}{}", current.style.format(number));
        }
    }
    unreachable!()
}

#[test]
fn current_page_and_section_markers_follow_labels_without_guessing_between_pages() {
    let mut doc = pages(6);
    doc.pages[2].section = Some(Section {
        start: 4,
        continue_numbering: false,
        style: NumberStyle::RomanUpper,
        prefix: "A-".into(),
        include_prefix: true,
        marker: "Part Two".into(),
        ..Default::default()
    });
    doc.pages[4].section = Some(Section {
        style: NumberStyle::AlphaLower,
        prefix: "hidden".into(),
        marker: "Coda é".into(),
        ..Default::default()
    });
    for page in 0..6 {
        assert_eq!(
            current_page_value(&doc, &[page]).as_deref(),
            Some(label(&doc, page).as_str()),
            "page {page}"
        );
        assert_eq!(
            current_page_value(&doc, &[page, page, page]),
            current_page_value(&doc, &[page])
        );
        let marker = match page {
            0 | 1 => "",
            2 | 3 => "Part Two",
            _ => "Coda é",
        };
        assert_eq!(section_marker_value(&doc, &[page]).as_deref(), Some(marker));
    }
    assert_eq!(current_page_value(&doc, &[2]).as_deref(), Some("A-IV"));
    assert_eq!(current_page_value(&doc, &[4]).as_deref(), Some("f"));
    for ambiguous in [vec![], vec![0, 1], vec![6], vec![1, 9]] {
        assert_eq!(current_page_value(&doc, &ambiguous), None, "{ambiguous:?}");
    }
    assert_eq!(
        section_marker_value(&doc, &[2, 3]).as_deref(),
        Some("Part Two")
    );
    assert_eq!(section_marker_value(&doc, &[1, 2]), None);
    // Marker text that cannot be displayed on one line stays unrendered.
    doc.pages[4].section.as_mut().unwrap().marker = "a\tb".into();
    assert_eq!(section_marker_value(&doc, &[4]), None);
}

#[test]
fn chapter_values_use_only_established_numbering_semantics() {
    let mut doc = blank_a4();
    let spec = |format| ChapterNumber {
        before: "ch ".into(),
        format,
        after: ".".into(),
    };
    // Absent preferences are the application default.
    assert_eq!(
        chapter_value(&doc, &spec(PageNumberFormat::Current)).as_deref(),
        Some("ch 1.")
    );
    for (number, source, format, current, roman) in [
        (
            7,
            ChapterSource::UserDefined,
            ARABIC_CHAPTER_FORMAT,
            Some("ch 7."),
            Some("ch VII."),
        ),
        (
            1,
            ChapterSource::ContinueFromPreviousDocument,
            ARABIC_CHAPTER_FORMAT,
            Some("ch 1."),
            Some("ch I."),
        ),
        (
            1,
            ChapterSource::SameAsPreviousDocument,
            ARABIC_CHAPTER_FORMAT,
            Some("ch 1."),
            Some("ch I."),
        ),
        // A book would decide these; a standalone document does not guess.
        (
            5,
            ChapterSource::ContinueFromPreviousDocument,
            ARABIC_CHAPTER_FORMAT,
            None,
            None,
        ),
        (
            5,
            ChapterSource::SameAsPreviousDocument,
            ARABIC_CHAPTER_FORMAT,
            None,
            None,
        ),
        // An unrecognized document format only blocks the Current format.
        (
            3,
            ChapterSource::UserDefined,
            "I, II, III, IV...",
            None,
            Some("ch III."),
        ),
        (3, ChapterSource::UserDefined, "", None, Some("ch III.")),
    ] {
        doc.chapter_numbering = Some(ChapterNumbering {
            number,
            source,
            format: format.into(),
        });
        assert_eq!(
            chapter_value(&doc, &spec(PageNumberFormat::Current)).as_deref(),
            current,
            "{number} {source:?} {format}"
        );
        assert_eq!(
            chapter_value(&doc, &spec(PageNumberFormat::UpperRoman)).as_deref(),
            roman
        );
    }
    doc.chapter_numbering = Some(ChapterNumbering {
        number: 28,
        source: ChapterSource::UserDefined,
        format: ARABIC_CHAPTER_FORMAT.into(),
    });
    for (format, expected) in [
        (PageNumberFormat::Arabic, "28"),
        (PageNumberFormat::LowerRoman, "xxviii"),
        (PageNumberFormat::UpperLetters, "BB"),
        (PageNumberFormat::LowerLetters, "bb"),
    ] {
        let value = chapter_value(
            &doc,
            &ChapterNumber {
                format,
                ..Default::default()
            },
        );
        assert_eq!(value.as_deref(), Some(expected));
    }
}

/// A parent footer: page number, section marker and chapter on every page.
fn footer() -> (LayoutDocument, ObjectId) {
    let mut doc = pages(5);
    fonts(&mut doc);
    doc.pages[2].section = Some(Section {
        start: 1,
        continue_numbering: false,
        marker: "Part Two".into(),
        ..Default::default()
    });
    doc.pages[0].section = Some(Section {
        marker: "Part One".into(),
        ..Default::default()
    });
    doc.chapter_numbering = Some(ChapterNumbering {
        number: 3,
        source: ChapterSource::UserDefined,
        format: ARABIC_CHAPTER_FORMAT.into(),
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 760.0, 520.0, 40.0),
    )
    .unwrap();
    let text = "Page  · Section:  · ch ";
    let mut story = Story::from_text(text, "Body");
    story.structures = vec![
        page_number("Page ".len(), PageNumberKind::Current),
        section_marker(text.find(" · ch").unwrap()),
        StoryStructure {
            at: Some(text.len()),
            kind: "TextVariableInstance".into(),
            payload: String::new(),
            footnote: None,
            control: Some(InlineControl::TextVariable {
                variable: "chapter".into(),
                character_style: String::new(),
                name: String::new(),
            }),
            anchored: None,
        },
        // Adjacent-frame page numbers need frame placement and stay diagnosed.
        page_number(text.len(), PageNumberKind::Next),
        page_number(text.len(), PageNumberKind::Previous),
    ];
    doc.stories[frame.story.0 as usize] = story;
    doc.text_variables = vec![TextVariable::new(
        "chapter",
        "Chapter",
        VariableKind::Chapter(ChapterNumber::default()),
    )];
    let object = doc.objects.remove(0);
    let id = object.id;
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: (0..5).collect(),
        based_on: None,
        hidden: false,
        sheets: Vec::new(),
        placements: Vec::new(),
        objects: vec![ParentObject {
            object,
            overridden_on: Vec::new(),
        }],
    });
    (doc, id)
}

#[test]
fn parent_footers_show_each_destination_pages_number_marker_and_chapter() {
    let (doc, id) = footer();
    let source = doc.clone();
    for page in 0..5 {
        let object = doc
            .page_objects(page)
            .into_iter()
            .find(|o| o.id == id)
            .unwrap();
        let frame = compose::compose_object(&doc, &object).unwrap();
        let (number, section) = match page {
            0 => ("1", "Part One"),
            1 => ("2", "Part One"),
            2 => ("1", "Part Two"),
            3 => ("2", "Part Two"),
            _ => ("3", "Part Two"),
        };
        assert_eq!(values(&frame), [number, section, "3"], "page {page}");
        assert_eq!(frame.unrendered_structures, 2, "next and previous");
    }
    assert_eq!(doc, source);
}

#[test]
fn ordinary_threads_render_page_numbers_only_on_a_single_page() {
    for (second_page, expected) in [(0, Some("1")), (1, None)] {
        let mut doc = pages(2);
        fonts(&mut doc);
        let mut history = History::default();
        let first = authoring::text_frame(
            &mut doc,
            &mut history,
            0,
            Rect::new(30.0, 30.0, 300.0, 100.0),
        )
        .unwrap();
        let second = authoring::text_frame(
            &mut doc,
            &mut history,
            second_page,
            Rect::new(30.0, 300.0, 300.0, 100.0),
        )
        .unwrap();
        assert!(threading::link(
            &mut doc,
            &mut history,
            first.object,
            second.object
        ));
        let story = threading::story_of(&doc, first.object).unwrap();
        let mut text = Story::from_text("p", "Body");
        text.structures = vec![page_number(1, PageNumberKind::Current)];
        doc.stories[story.0 as usize] = text;
        let composed = compose::compose_story(&doc, story);
        match expected {
            Some(value) => {
                assert_eq!(values(&composed.frames[0]), [value]);
                assert_eq!(composed.frames[0].unrendered_structures, 0);
            }
            None => {
                assert!(values(&composed.frames[0]).is_empty());
                assert_eq!(composed.frames[0].unrendered_structures, 1);
            }
        }
    }
}

#[test]
fn renaming_a_marker_style_and_editing_sections_each_undo_once() {
    let (mut doc, id) = footer();
    doc.styles.add_character(CharacterStyle {
        name: "Folio".into(),
        ..Default::default()
    });
    let story = StoryId(0);
    for structure in &mut doc.stories[0].structures {
        if let Some(control) = &mut structure.control {
            *control.character_style_mut() = "Folio".into();
        }
    }
    let mut history = History::default();
    let before = doc.clone();
    assert!(schist_layout::properties::rename_style(
        &mut doc,
        &mut history,
        false,
        "Folio",
        "Running foot"
    ));
    assert_eq!(history.undo_depth(), 1);
    assert!(doc.stories[story.0 as usize].structures.iter().all(|s| s
        .control
        .as_ref()
        .unwrap()
        .character_style()
        == "Running foot"));
    let renamed = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, renamed);

    // A numbering edit refreshes every footer and undoes once.
    assert!(schist_layout::numbering::set_section(
        &mut doc,
        &mut history,
        2,
        Some(Section {
            start: 10,
            continue_numbering: false,
            style: NumberStyle::RomanLower,
            marker: "Back".into(),
            ..Default::default()
        })
    ));
    let object = doc
        .page_objects(4)
        .into_iter()
        .find(|o| o.id == id)
        .unwrap();
    let frame = compose::compose_object(&doc, &object).unwrap();
    assert_eq!(values(&frame), ["xii", "Back", "3"]);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, renamed);
}

#[test]
fn marker_controls_and_chapter_numbering_round_trip_through_snapshots() {
    for control in [
        InlineControl::PageNumber {
            kind: PageNumberKind::Current,
            character_style: "a".into(),
        },
        InlineControl::PageNumber {
            kind: PageNumberKind::Next,
            character_style: String::new(),
        },
        InlineControl::PageNumber {
            kind: PageNumberKind::Previous,
            character_style: "é".into(),
        },
        InlineControl::SectionMarker {
            character_style: "b".into(),
        },
    ] {
        let json = serde_json::to_string(&control).unwrap();
        assert_eq!(
            serde_json::from_str::<InlineControl>(&json).unwrap(),
            control
        );
    }
    let mut doc = blank_a4();
    let json = serde_json::to_value(&doc).unwrap();
    assert!(json.get("chapter_numbering").is_none());
    doc.chapter_numbering = Some(ChapterNumbering {
        number: 9,
        source: ChapterSource::SameAsPreviousDocument,
        format: "custom".into(),
    });
    let text = serde_json::to_string(&doc).unwrap();
    assert_eq!(serde_json::from_str::<LayoutDocument>(&text).unwrap(), doc);
    let old: TextVariable = serde_json::from_str(
        r#"{"id":"a","name":"Last","contents":"","last_page":{"before":"","format":"Current","after":"","scope":"Section"}}"#,
    )
    .unwrap();
    assert!(matches!(old.kind(), Some(VariableKind::LastPage(_))));
}

#[test]
fn markers_insert_and_remove_at_a_captured_cursor_in_one_step_each() {
    use schist_layout::text_variables::{removable, Cursor};
    let mut doc = pages(3);
    fonts(&mut doc);
    doc.pages[1].section = Some(Section {
        marker: "Two".into(),
        ..Default::default()
    });
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        1,
        Rect::new(30.0, 30.0, 300.0, 100.0),
    )
    .unwrap();
    let text = "Aé中";
    let mut story = Story::from_text(text, "Body");
    story
        .ranges
        .push(schist_layout::StyleRange::new(0, text.len(), "Folio"));
    doc.stories[frame.story.0 as usize] = story;
    doc.styles.add_character(CharacterStyle {
        name: "Folio".into(),
        ..Default::default()
    });
    let id = frame.story;
    for at in [0, 1, 3] {
        let base = doc.clone();
        let depth = history.undo_depth();
        let cursor = Cursor::capture(&doc, id, at).unwrap();
        assert!(cursor.insert_marker(&mut doc, &mut history, false));
        assert!(!cursor.insert_marker(&mut doc, &mut history, true), "stale");
        let cursor = Cursor::capture(&doc, id, at).unwrap();
        assert!(cursor.insert_marker(&mut doc, &mut history, true));
        assert_eq!(history.undo_depth(), depth + 2);
        let story = &doc.stories[id.0 as usize];
        assert_eq!(story.text(), text);
        let placed: Vec<_> = story.structures.iter().filter(|s| removable(s)).collect();
        assert_eq!(placed.len(), 2);
        assert!(placed.iter().all(|s| s.at == Some(at)
            && s.kind == "ProcessingInstruction"
            && s.control.as_ref().unwrap().character_style() == "Folio"));
        assert!(matches!(
            placed[0].control,
            Some(InlineControl::PageNumber {
                kind: PageNumberKind::Current,
                ..
            })
        ));
        assert!(matches!(
            placed[1].control,
            Some(InlineControl::SectionMarker { .. })
        ));
        let composed = compose::compose_story(&doc, id);
        assert_eq!(values(&composed.frames[0]), ["2", "Two"]);
        // Remove exactly the page number, leaving the coincident marker.
        let cursor = Cursor::capture(&doc, id, at).unwrap();
        let index = story
            .structures
            .iter()
            .position(|s| matches!(s.control, Some(InlineControl::PageNumber { .. })))
            .unwrap();
        let inserted = doc.clone();
        assert!(cursor.remove_instance(&mut doc, &mut history, index));
        assert_eq!(values(&compose::compose_story(&doc, id).frames[0]), ["Two"]);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, inserted);
        for _ in 0..2 {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc, base);
    }
}

#[test]
fn insertions_take_the_formatting_typed_text_would_take() {
    use schist_layout::text_variables::{self as variables, Cursor};
    let mut doc = blank_a4();
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 30.0, 300.0, 100.0),
    )
    .unwrap();
    // "Bold" covers the whole first paragraph and its separator; the second
    // paragraph is unstyled.
    let mut story = Story::from_text("Aé", "Body");
    story.push_paragraph("B", "Body");
    story
        .ranges
        .push(schist_layout::StyleRange::new(0, 4, "Bold"));
    doc.stories[frame.story.0 as usize] = story;
    let id = variables::create(&mut doc, &mut history, "Edition", "7").unwrap();
    let definition = doc
        .text_variables
        .iter()
        .find(|d| d.id == id)
        .unwrap()
        .clone();
    for (at, expected) in [(0, "Bold"), (1, "Bold"), (3, "Bold"), (4, ""), (5, "")] {
        for marker in [false, true] {
            let mut doc = doc.clone();
            let cursor = Cursor::capture(&doc, frame.story, at).unwrap();
            if marker {
                assert!(cursor.insert_marker(&mut doc, &mut history, false));
            } else {
                assert!(cursor.insert(&mut doc, &mut history, &definition));
            }
            let style = doc.stories[frame.story.0 as usize].structures[0]
                .control
                .as_ref()
                .unwrap()
                .character_style()
                .to_owned();
            assert_eq!(style, expected, "at {at}, marker {marker}");
        }
    }
}

#[test]
fn a_soft_line_break_does_not_end_the_run_that_insertions_inherit() {
    use schist_layout::text_variables::Cursor;
    let mut doc = blank_a4();
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 30.0, 300.0, 100.0),
    )
    .unwrap();
    // One paragraph containing a forced line break; "Bold" ends at the break.
    let mut story = Story::from_text("Aé\nB", "Body");
    story
        .ranges
        .push(schist_layout::StyleRange::new(0, 4, "Bold"));
    doc.stories[frame.story.0 as usize] = story;
    let cursor = Cursor::capture(&doc, frame.story, 4).unwrap();
    assert!(cursor.insert_marker(&mut doc, &mut history, false));
    assert_eq!(
        doc.stories[frame.story.0 as usize].structures[0]
            .control
            .as_ref()
            .unwrap()
            .character_style(),
        "Bold"
    );
}
