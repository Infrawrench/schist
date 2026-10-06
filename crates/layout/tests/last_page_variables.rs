use schist_layout::{
    authoring, blank_a4, compose, numbering,
    story::InlineControl,
    text_variables::{
        self as variables, last_page_value, ChapterNumber, Cursor, LastPageNumber,
        PageNumberFormat, TextVariable, VariableKind, VariableScope,
    },
    threading, History, LayoutDocument, NumberStyle, ObjectId, ParentObject, ParentPage, Rect,
    Section, Story, StoryId, StoryStructure,
};

const STYLES: [NumberStyle; 5] = [
    NumberStyle::Arabic,
    NumberStyle::RomanUpper,
    NumberStyle::RomanLower,
    NumberStyle::AlphaUpper,
    NumberStyle::AlphaLower,
];

fn pages(count: usize) -> LayoutDocument {
    let mut doc = blank_a4();
    for _ in 1..count {
        doc.add_page(doc.pages[0].clone());
    }
    doc
}

/// An independent page walk: each page advances by one unless its own explicit
/// boundary restarts; the style is the nearest explicit boundary's.
fn reference_labels(doc: &LayoutDocument) -> Vec<(u32, NumberStyle, Option<String>)> {
    let mut out = Vec::new();
    let mut number = 0u32;
    let mut style = NumberStyle::Arabic;
    let mut prefix = None;
    for page in &doc.pages {
        number += 1;
        if let Some(section) = &page.section {
            style = section.style;
            prefix = (section.include_prefix && !section.prefix.is_empty())
                .then(|| section.prefix.clone());
            if !section.continue_numbering {
                number = section.start.max(1);
            }
        }
        out.push((number, style, prefix.clone()));
    }
    out
}

fn reference_value(doc: &LayoutDocument, spec: &LastPageNumber, page: usize) -> Option<String> {
    let labels = reference_labels(doc);
    let count = doc.pages.len();
    let last = match spec.scope {
        VariableScope::Document => count - 1,
        VariableScope::Section => (page + 1..count)
            .find(|i| doc.pages[*i].section.is_some())
            .map_or(count - 1, |boundary| boundary - 1),
    };
    let (number, current, prefix) = labels[last].clone();
    if prefix.is_some() {
        return None;
    }
    let style = match spec.format {
        PageNumberFormat::Current => current,
        PageNumberFormat::Arabic => NumberStyle::Arabic,
        PageNumberFormat::UpperRoman => NumberStyle::RomanUpper,
        PageNumberFormat::LowerRoman => NumberStyle::RomanLower,
        PageNumberFormat::UpperLetters => NumberStyle::AlphaUpper,
        PageNumberFormat::LowerLetters => NumberStyle::AlphaLower,
    };
    Some(format!(
        "{}{}{}",
        spec.before,
        style.format(number),
        spec.after
    ))
}

#[test]
fn last_page_values_match_an_independent_page_walk_for_every_section_layout() {
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut checked = 0;
    for count in 1..=6usize {
        for mask in 0u32..(1 << count) {
            let mut doc = pages(count);
            for (index, page) in doc.pages.iter_mut().enumerate() {
                if mask & (1 << index) == 0 {
                    continue;
                }
                let random = next();
                page.section = Some(Section {
                    start: 1 + random % 40,
                    continue_numbering: random & 0x100 != 0,
                    style: STYLES[(random as usize >> 9) % STYLES.len()],
                    prefix: if random & 0x1000 != 0 {
                        "A-".into()
                    } else {
                        String::new()
                    },
                    include_prefix: random & 0x2000 != 0 && random & 0x4000 != 0,
                    ..Default::default()
                });
            }
            for format in PageNumberFormat::ALL {
                for scope in [VariableScope::Document, VariableScope::Section] {
                    let spec = LastPageNumber {
                        before: "of é ".into(),
                        format,
                        after: " ⟩".into(),
                        scope,
                    };
                    for page in 0..count {
                        assert_eq!(
                            last_page_value(&doc, &spec, &[page]),
                            reference_value(&doc, &spec, page),
                            "{count} pages, mask {mask:b}, page {page}, {format:?} {scope:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    // 12 format/scope pairs for every page of every boundary mask.
    assert_eq!(checked, 7_704);
}

#[test]
fn the_public_native_reference_labels_are_reproduced() {
    // Five pages, labels 1 2 1 2 3: the native footers read 3, then 2 or 3,
    // then III. The document value is the final label, not the page count.
    let mut doc = pages(5);
    doc.pages[2].section = Some(Section {
        start: 1,
        continue_numbering: false,
        ..Default::default()
    });
    let spec = |format, scope| LastPageNumber {
        format,
        scope,
        ..Default::default()
    };
    for page in 0..5 {
        let at = [page];
        let value = |format, scope| last_page_value(&doc, &spec(format, scope), &at);
        assert_eq!(
            value(PageNumberFormat::Current, VariableScope::Document).as_deref(),
            Some("3")
        );
        assert_eq!(
            value(PageNumberFormat::Current, VariableScope::Section).as_deref(),
            Some(if page < 2 { "2" } else { "3" })
        );
        assert_eq!(
            value(PageNumberFormat::UpperRoman, VariableScope::Document).as_deref(),
            Some("III")
        );
    }
}

#[test]
fn section_scope_is_known_only_when_every_candidate_page_shares_one_section() {
    let mut doc = pages(6);
    doc.pages[3].section = Some(Section {
        style: NumberStyle::RomanLower,
        ..Default::default()
    });
    let section = LastPageNumber::default();
    let document = LastPageNumber {
        scope: VariableScope::Document,
        ..Default::default()
    };
    for (candidates, expected) in [
        (vec![0], Some("3")),
        (vec![0, 1, 2], Some("3")),
        (vec![2, 0, 2], Some("3")),
        (vec![3, 5], Some("vi")),
        (vec![2, 3], None),
        (vec![0, 5], None),
        (vec![], None),
        (vec![6], None),
        (vec![1, 9], None),
    ] {
        assert_eq!(
            last_page_value(&doc, &section, &candidates).as_deref(),
            expected,
            "{candidates:?}"
        );
        // The document value never depends on placement.
        assert_eq!(
            last_page_value(&doc, &document, &candidates).as_deref(),
            Some("vi")
        );
    }
    // A visible prefix has no established native meaning in this value.
    doc.pages[3].section.as_mut().unwrap().prefix = "B-".into();
    assert_eq!(last_page_value(&doc, &document, &[]).as_deref(), Some("vi"));
    doc.pages[3].section.as_mut().unwrap().include_prefix = true;
    assert_eq!(last_page_value(&doc, &document, &[]), None);
    assert_eq!(last_page_value(&doc, &section, &[0]).as_deref(), Some("3"));
    assert_eq!(last_page_value(&doc, &section, &[4]), None);
    doc.pages.clear();
    assert_eq!(last_page_value(&doc, &document, &[]), None);
}

fn fonts() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
}

fn styled(doc: &mut LayoutDocument) {
    let paragraph = doc
        .styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap();
    paragraph.family = Some("IBM Plex Sans".into());
    paragraph.point_size = Some(10.0);
}

fn reference(at: usize, variable: &str) -> StoryStructure {
    StoryStructure {
        at: Some(at),
        kind: "TextVariableInstance".into(),
        payload: String::new(),
        footnote: None,
        control: Some(InlineControl::TextVariable {
            variable: variable.into(),
            character_style: String::new(),
            name: String::new(),
        }),
        table: None,
        anchored: None,
    }
}

fn definition(id: &str, format: PageNumberFormat, scope: VariableScope) -> TextVariable {
    TextVariable::new(
        id,
        id,
        VariableKind::LastPage(LastPageNumber {
            before: String::new(),
            format,
            after: String::new(),
            scope,
        }),
    )
}

fn rendered(frame: &compose::ComposedFrame) -> String {
    frame
        .lines
        .iter()
        .filter_map(|l| l.projected.as_ref())
        .map(|p| p.spec.text.as_str())
        .collect()
}

/// Every isolated display value, in source order.
fn values(text: &str) -> Vec<String> {
    text.split('\u{2068}')
        .skip(1)
        .map(|rest| rest.split('\u{2069}').next().unwrap().to_owned())
        .collect()
}

/// The native reference footer: three last-page values on a parent page.
fn footer_document() -> (LayoutDocument, ObjectId) {
    fonts();
    let mut doc = pages(5);
    styled(&mut doc);
    doc.pages[0].section = Some(Section {
        marker: "Part One".into(),
        ..Default::default()
    });
    doc.pages[2].section = Some(Section {
        start: 1,
        continue_numbering: false,
        marker: "Part Two".into(),
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(30.0, 760.0, 520.0, 40.0),
    )
    .unwrap();
    let text = "Page of , section ends at , roman ";
    let mut story = Story::from_text(text, "Body");
    story.structures = vec![
        reference("Page of ".len(), "document"),
        reference(text.find(", roman").unwrap(), "section"),
        reference(text.len(), "roman"),
    ];
    doc.stories[frame.story.0 as usize] = story;
    doc.text_variables = vec![
        definition(
            "document",
            PageNumberFormat::Current,
            VariableScope::Document,
        ),
        definition("section", PageNumberFormat::Current, VariableScope::Section),
        definition(
            "roman",
            PageNumberFormat::UpperRoman,
            VariableScope::Document,
        ),
    ];
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
fn parent_footers_evaluate_last_page_values_for_each_destination_page() {
    let (doc, id) = footer_document();
    let source = doc.clone();
    for _ in 0..2 {
        for page in 0..5 {
            let object = doc
                .page_objects(page)
                .into_iter()
                .find(|o| o.id == id)
                .unwrap();
            let frame = compose::compose_object(&doc, &object).unwrap();
            assert_eq!(frame.unrendered_structures, 0, "page {page}");
            let section = if page < 2 { "2" } else { "3" };
            let text = rendered(&frame);
            assert!(text.starts_with("Page of "), "{text}");
            assert_eq!(values(&text), ["3", section, "III"], "page {page}");
        }
    }
    assert_eq!(doc, source);
}

fn threaded(second_page: usize) -> (LayoutDocument, StoryId) {
    fonts();
    let mut doc = pages(4);
    styled(&mut doc);
    doc.pages[2].section = Some(Section {
        style: NumberStyle::RomanUpper,
        ..Default::default()
    });
    let mut history = History::default();
    let first = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(30.0, 30.0, 300.0, 200.0),
    )
    .unwrap();
    let second = authoring::text_frame(
        &mut doc,
        &mut history,
        second_page,
        Rect::new(30.0, 30.0, 300.0, 200.0),
    )
    .unwrap();
    assert!(threading::link(
        &mut doc,
        &mut history,
        first.object,
        second.object
    ));
    let story = threading::story_of(&doc, first.object).unwrap();
    let mut text = Story::from_text("a b", "Body");
    text.structures = vec![reference(1, "section"), reference(3, "document")];
    doc.stories[story.0 as usize] = text;
    doc.text_variables = vec![
        definition("section", PageNumberFormat::Current, VariableScope::Section),
        definition(
            "document",
            PageNumberFormat::Arabic,
            VariableScope::Document,
        ),
    ];
    (doc, story)
}

#[test]
fn ordinary_threads_crossing_sections_leave_section_values_unrendered() {
    // Frames on pages 0 and 1 share a section; page 2 starts another one.
    for (second_page, section) in [(1, Some("2")), (2, None), (3, None)] {
        let (doc, story) = threaded(second_page);
        let composed = compose::compose_story(&doc, story);
        let first = &composed.frames[0];
        let text = rendered(first);
        match section {
            Some(value) => {
                assert_eq!(first.unrendered_structures, 0);
                assert_eq!(values(&text), [value, "4"], "{text}");
            }
            None => {
                assert_eq!(first.unrendered_structures, 1);
                assert_eq!(values(&text), ["4"], "{text}");
            }
        }
        assert_eq!(doc.stories[story.0 as usize].text(), "a b");
    }
}

#[test]
fn definition_kinds_and_section_changes_each_undo_once_and_refresh_instances() {
    fonts();
    let mut doc = blank_a4();
    styled(&mut doc);
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(30.0, 30.0, 300.0, 200.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Page ", "Body");
    let story = frame.story;
    let spec = LastPageNumber {
        before: "of ".into(),
        ..Default::default()
    };
    // Literal contents and computed kinds cannot be combined in one record.
    let mut combined = TextVariable::new("x", "x", VariableKind::LastPage(spec.clone()));
    combined.contents = "x".into();
    assert!(combined.kind().is_none() && !combined.valid());
    combined.contents.clear();
    combined.chapter = Some(ChapterNumber::default());
    assert!(combined.kind().is_none() && !combined.valid());
    let invalid = LastPageNumber {
        after: "\t".into(),
        ..spec.clone()
    };
    let depth = history.undo_depth();
    assert!(variables::create_definition(
        &mut doc,
        &mut history,
        "Bad",
        VariableKind::LastPage(invalid.clone())
    )
    .is_none());
    assert_eq!(history.undo_depth(), depth);
    let id = variables::create_definition(
        &mut doc,
        &mut history,
        "Last",
        VariableKind::LastPage(spec.clone()),
    )
    .unwrap();
    assert_eq!(history.undo_depth(), depth + 1);
    let defined = doc.text_variables[0].clone();
    assert!(Cursor::capture(&doc, story, 5)
        .unwrap()
        .insert(&mut doc, &mut history, &defined));
    let text =
        |doc: &LayoutDocument| values(&rendered(&compose::compose_story(doc, story).frames[0]));
    assert_eq!(text(&doc), ["of 1"]);

    let before = doc.clone();
    assert!(numbering::set_section(
        &mut doc,
        &mut history,
        0,
        Some(Section {
            start: 7,
            continue_numbering: false,
            style: NumberStyle::RomanUpper,
            ..Default::default()
        })
    ));
    assert_eq!(text(&doc), ["of VII"]);
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert_eq!(text(&doc), ["of 1"]);

    let stories = doc.stories.clone();
    for (kind, expected) in [
        (VariableKind::Custom("literal".into()), "literal"),
        (
            VariableKind::LastPage(LastPageNumber {
                format: PageNumberFormat::LowerLetters,
                after: " ✓".into(),
                ..spec.clone()
            }),
            "of a ✓",
        ),
        (
            VariableKind::Chapter(ChapterNumber {
                before: "ch ".into(),
                format: PageNumberFormat::UpperRoman,
                after: String::new(),
            }),
            "ch I",
        ),
    ] {
        let captured = doc.text_variables[0].clone();
        let previous = doc.clone();
        let depth = history.undo_depth();
        assert!(!variables::update_definition(
            &mut doc,
            &mut history,
            &captured,
            "Last",
            VariableKind::LastPage(invalid.clone())
        ));
        assert!(variables::update_definition(
            &mut doc,
            &mut history,
            &captured,
            "Renamed",
            kind.clone()
        ));
        assert_eq!(history.undo_depth(), depth + 1);
        assert_eq!(doc.text_variables[0].id, id);
        assert_eq!(doc.stories, stories);
        assert_eq!(text(&doc), [expected]);
        // The captured definition is stale after its own edit.
        assert!(!variables::update_definition(
            &mut doc,
            &mut history,
            &captured,
            "Again",
            kind.clone()
        ));
        let after = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, previous);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, after);
    }
}

#[test]
fn older_snapshots_load_as_custom_text_and_custom_definitions_keep_their_shape() {
    let old: TextVariable =
        serde_json::from_str(r#"{"id":"a","name":"Edition","contents":"7"}"#).unwrap();
    assert_eq!(old, TextVariable::custom("a", "Edition", "7"));
    assert_eq!(
        serde_json::to_string(&old).unwrap(),
        r#"{"id":"a","name":"Edition","contents":"7"}"#
    );
    for format in PageNumberFormat::ALL {
        for scope in [VariableScope::Document, VariableScope::Section] {
            let value = definition("b", format, scope);
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<TextVariable>(&json).unwrap(), value);
        }
    }
}
