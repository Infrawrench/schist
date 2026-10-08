use schist_codec_idml::{container, export, import};
use schist_layout::story::{InlineControl, PageNumberKind};
use schist_layout::{authoring, blank_a4, CharacterStyle, History, LayoutDocument, Rect, Story};

/// Every style a story or style names must be defined in the package.
fn assert_references_resolve(bytes: &[u8]) {
    let package = container::read(bytes).unwrap();
    let styles = package.text("Resources/Styles.xml").unwrap();
    let defined: Vec<&str> = styles
        .split("Self=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let mut checked = 0;
    for name in package.names() {
        if !(name.starts_with("Stories/") || name == "Resources/Styles.xml") {
            continue;
        }
        let text = package.text(name).unwrap();
        for key in ["AppliedCharacterStyle=\"", "AppliedParagraphStyle=\""] {
            for rest in text.split(key).skip(1) {
                let reference = rest.split('"').next().unwrap();
                assert!(
                    reference == "n" || defined.contains(&reference),
                    "{name}: {reference} is not defined"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

fn document() -> LayoutDocument {
    let mut doc = blank_a4();
    doc.styles.add_character(CharacterStyle {
        name: "Emphasis".into(),
        italic: Some(true),
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 400.0, 300.0),
    )
    .unwrap();
    let mut story = Story::from_text("Plain words, emphasised words and a page number.", "Body");
    story.ranges.push(schist_layout::StyleRange {
        start: 13,
        end: 23,
        style: "Emphasis".into(),
    });
    doc.stories[frame.story.0 as usize] = story;
    let cursor = schist_layout::text_variables::Cursor::capture(&doc, frame.story, 48).unwrap();
    assert!(cursor.insert_marker(&mut doc, &mut History::default(), false));
    assert!(doc.stories[frame.story.0 as usize]
        .structures
        .iter()
        .any(|s| matches!(
            &s.control,
            Some(InlineControl::PageNumber {
                kind: PageNumberKind::Current,
                character_style,
            }) if character_style.is_empty()
        )));
    doc
}

#[test]
fn unstyled_text_names_the_root_style_and_round_trips_without_new_styles() {
    let doc = document();
    let mut current = doc.clone();
    let mut previous = None;
    for _ in 0..3 {
        let written = export::write(&current);
        let package = container::read(&written.bytes).unwrap();
        for name in package.names() {
            let text = package.text(name).unwrap_or_default();
            assert!(
                !text.contains("CharacterStyle/$ID/\"") && !text.contains("ParagraphStyle/$ID/\""),
                "{name} names an empty style"
            );
        }
        assert_references_resolve(&written.bytes);
        let imported = import::read(&written.bytes).unwrap().document;
        assert_eq!(imported.styles.characters, doc.styles.characters);
        assert_eq!(imported.styles.paragraphs, doc.styles.paragraphs);
        assert_eq!(imported.stories, doc.stories);
        let styles = package.text("Resources/Styles.xml").unwrap().to_owned();
        if let Some(previous) = &previous {
            assert_eq!(previous, &styles);
        }
        previous = Some(styles);
        current = imported;
    }
}

#[test]
fn indesign_root_character_style_is_not_a_document_style() {
    for fixture in ["text.idml", "themes.idml", "bounded-text.idml"] {
        let path = format!(
            "{}/../../fixtures/idml/{fixture}",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap();
        let doc = import::read(&bytes).unwrap().document;
        assert!(
            doc.styles
                .characters
                .iter()
                .all(|s| s.name != "[No character style]"
                    && s.based_on.as_deref() != Some("[No character style]")
                    && s.based_on.as_deref() != Some("")),
            "{fixture}"
        );
        let written = export::write(&doc);
        assert_references_resolve(&written.bytes);
        let again = import::read(&written.bytes).unwrap().document;
        assert_eq!(again.styles.characters, doc.styles.characters, "{fixture}");
        assert_eq!(again.styles.paragraphs, doc.styles.paragraphs, "{fixture}");
    }
}
