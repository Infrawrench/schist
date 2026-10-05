use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, blank_a4, compose,
    story::{InlineControl, PageNumberKind},
    text_variables::{ChapterNumbering, ChapterSource, ARABIC_CHAPTER_FORMAT},
    History, LayoutDocument, Rect, Story, StoryId,
};

/// A native one-frame package whose story body is `ranges` inside one paragraph.
fn native(ranges: &str, definitions: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 300.0, 200.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("seed", "Body");
    let package = container::read(&export::write(&doc).bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.starts_with("Stories/") {
                let root = xml::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
                let id = root.find("Story").unwrap().attr("Self").unwrap();
                format!(r#"<idPkg:Story><Story Self="{id}"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Body">{ranges}</ParagraphStyleRange></Story></idPkg:Story>"#).into_bytes()
            } else if name == "designmap.xml" {
                std::str::from_utf8(bytes).unwrap().replace("</Document>", &format!("{definitions}</Document>")).into_bytes()
            } else {
                bytes.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    container::write(&parts)
}

fn range(content: &str, mode: Option<&str>) -> String {
    let mode = mode
        .map(|value| format!(r#" PageNumberType="{value}""#))
        .unwrap_or_default();
    format!(
        r#"<CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"{mode}>{content}</CharacterStyleRange>"#
    )
}

fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, String) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = package.get(name).unwrap();
            let bytes = if name.ends_with(".xml") {
                edit(name, std::str::from_utf8(bytes).unwrap().to_owned()).into_bytes()
            } else {
                bytes.to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    container::write(&parts)
}

/// Every native story instruction with its range's PageNumberType.
fn native_instructions(bytes: &[u8]) -> Vec<(String, Option<String>)> {
    let package = container::read(bytes).unwrap();
    let mut out = Vec::new();
    for name in package
        .names()
        .into_iter()
        .filter(|n| n.starts_with("Stories/"))
    {
        let root = xml::parse(package.text(name).unwrap()).unwrap();
        for range in root.find_all("CharacterStyleRange") {
            for content in range.children_named("Content") {
                for (_, pi) in &content.instructions {
                    out.push((
                        pi.split_whitespace().collect::<Vec<_>>().join(" "),
                        range.attr("PageNumberType").map(str::to_owned),
                    ));
                }
            }
        }
    }
    out
}

fn controls(doc: &LayoutDocument) -> Vec<(Option<usize>, Option<InlineControl>)> {
    doc.stories[0]
        .structures
        .iter()
        .map(|s| (s.at, s.control.clone()))
        .collect()
}

fn values(doc: &LayoutDocument) -> (Vec<String>, usize) {
    let composed = compose::compose_story(doc, StoryId(0));
    let text: String = composed.frames[0]
        .lines
        .iter()
        .filter_map(|l| l.projected.as_ref())
        .map(|p| p.spec.text.as_str())
        .collect();
    (
        text.split('\u{2068}')
            .skip(1)
            .map(|rest| rest.split('\u{2069}').next().unwrap().to_owned())
            .collect(),
        composed.frames[0].unrendered_structures,
    )
}

fn fonts(doc: &mut LayoutDocument) {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .family = Some("IBM Plex Sans".into());
}

#[test]
fn page_number_and_section_markers_type_render_and_save_natively_at_every_boundary() {
    let text = "Aé中🙂B";
    for at in text.char_indices().map(|(at, _)| at).chain([text.len()]) {
        for (mode, instruction, expected, value) in [
            (None, "ACE 18", Some(PageNumberKind::Current), Some("1")),
            (
                Some("AutoPageNumber"),
                "ACE 18",
                Some(PageNumberKind::Current),
                Some("1"),
            ),
            (
                Some("NextPageNumber"),
                "ACE 18",
                Some(PageNumberKind::Next),
                // A standalone frame is its own next and previous frame.
                Some("1"),
            ),
            (
                Some("PreviousPageNumber"),
                "ACE 18",
                Some(PageNumberKind::Previous),
                // A standalone frame is its own next and previous frame.
                Some("1"),
            ),
            (None, "ACE 19", None, Some("")),
            (Some("AutoPageNumber"), "ACE 19", None, Some("")),
        ] {
            let ranges = range(&format!("<Content>{}</Content>", &text[..at]), None)
                + &range(&format!("<Content><?{instruction}?></Content>"), mode)
                + &range(&format!("<Content>{}</Content>", &text[at..]), None);
            let mut doc = import::read(&native(&ranges, "")).unwrap().document;
            fonts(&mut doc);
            let control = match expected {
                // The root no-style is Schist's empty character style.
                Some(kind) => InlineControl::PageNumber {
                    kind,
                    character_style: String::new(),
                },
                None => InlineControl::SectionMarker {
                    character_style: String::new(),
                },
            };
            assert_eq!(doc.stories[0].text(), text);
            assert_eq!(controls(&doc), [(Some(at), Some(control.clone()))]);
            let native_mode = match expected {
                Some(PageNumberKind::Current) => Some("AutoPageNumber"),
                Some(PageNumberKind::Next) => Some("NextPageNumber"),
                Some(PageNumberKind::Previous) => Some("PreviousPageNumber"),
                None => None,
            };
            for _ in 0..3 {
                let (shown, unrendered) = values(&doc);
                match value {
                    Some(value) => {
                        assert_eq!(shown, [value]);
                        assert_eq!(unrendered, 0);
                    }
                    None => {
                        assert!(shown.is_empty());
                        assert_eq!(unrendered, 1);
                    }
                }
                let saved = export::write(&doc).bytes;
                assert_eq!(
                    native_instructions(&saved),
                    [(instruction.to_owned(), native_mode.map(str::to_owned))]
                );
                let story = doc.stories[0].clone();
                doc = import::read(&saved).unwrap().document;
                fonts(&mut doc);
                assert_eq!(doc.stories[0].text(), story.text());
                assert_eq!(controls(&doc), [(Some(at), Some(control.clone()))]);
            }
        }
    }
}

#[test]
fn unknown_page_number_modes_and_instructions_remain_recovery_only() {
    for (mode, instruction) in [
        (Some("FuturePageNumber"), "ACE 18"),
        (Some("TextVariable"), "ACE 18"),
        (Some("NextPageNumber"), "ACE 19"),
        (Some("FuturePageNumber"), "ACE 19"),
        (None, "ACE 7"),
        (None, "future data='é'"),
    ] {
        let ranges = range("<Content>A</Content>", None)
            + &range(&format!("<Content><?{instruction}?></Content>"), mode);
        let mut doc = import::read(&native(&ranges, "")).unwrap().document;
        let expected = doc.stories[0].structures.clone();
        assert_eq!(expected.len(), 1);
        assert!(expected[0].control.is_none(), "{instruction} {mode:?}");
        for _ in 0..3 {
            assert_eq!(values(&doc).1, 1);
            let saved = export::write(&doc).bytes;
            assert!(native_instructions(&saved).is_empty());
            doc = import::read(&saved).unwrap().document;
            assert_eq!(doc.stories[0].structures, expected);
        }
    }
}

#[test]
fn legacy_recovery_only_markers_upgrade_after_the_story_guard_agrees() {
    let ranges = range("<Content>Pg </Content>", None)
        + &range("<Content><?ACE 18?></Content>", None)
        + &range("<Content> of </Content>", None)
        + &range("<Content><?ACE 19?></Content>", None)
        + &range("<Content><?ACE 18?></Content>", Some("NextPageNumber"));
    let typed = import::read(&native(&ranges, "")).unwrap().document;
    let expected = controls(&typed);
    assert!(expected.iter().all(|(_, control)| control.is_some()));
    // An older Schist kept these as untyped recovery data with no native output.
    let mut legacy = typed.clone();
    for structure in &mut legacy.stories[0].structures {
        structure.control = None;
    }
    let saved = export::write(&legacy).bytes;
    assert!(native_instructions(&saved).is_empty());
    let mut doc = import::read(&saved).unwrap().document;
    assert_eq!(controls(&doc), expected);
    for _ in 0..2 {
        let saved = export::write(&doc).bytes;
        assert_eq!(native_instructions(&saved).len(), 3);
        doc = import::read(&saved).unwrap().document;
        assert_eq!(controls(&doc), expected);
    }
}

#[test]
fn external_native_edits_to_markers_supersede_saved_story_data() {
    let ranges =
        range("<Content>Pg </Content>", None) + &range("<Content><?ACE 18?></Content>", None);
    let doc = import::read(&native(&ranges, "")).unwrap().document;
    let saved = export::write(&doc).bytes;
    let changed = rewrite(&saved, |name, xml| {
        if name.starts_with("Stories/") {
            xml.replace(
                r#"PageNumberType="AutoPageNumber""#,
                r#"PageNumberType="PreviousPageNumber""#,
            )
        } else {
            xml
        }
    });
    let edited = import::read(&changed).unwrap().document;
    // The native edit wins; stale archived data can only remain unplaced.
    let placed: Vec<_> = controls(&edited)
        .into_iter()
        .filter(|(at, _)| at.is_some())
        .collect();
    assert!(matches!(
        placed.as_slice(),
        [(
            Some(3),
            Some(InlineControl::PageNumber {
                kind: PageNumberKind::Previous,
                ..
            })
        )]
    ));
    let removed = rewrite(&saved, |name, xml| {
        if name.starts_with("Stories/") {
            xml.replace("<?ACE 18?>", "")
        } else {
            xml
        }
    });
    let removed = import::read(&removed).unwrap().document;
    assert!(removed.stories[0]
        .structures
        .iter()
        .all(|s| s.at.is_none() || s.control.is_none()));
    assert_eq!(removed.stories[0].text(), "Pg ");
}

#[test]
fn public_template_page_numbers_become_typed_native_markers() {
    for bytes in [
        include_bytes!("../../../fixtures/indd/psu-academic-2/psu-academic-2.idml").as_slice(),
        include_bytes!("../../../fixtures/indd/psu-literary/psu-literary.idml").as_slice(),
    ] {
        let original = native_instructions(bytes)
            .into_iter()
            .filter(|(pi, _)| pi == "ACE 18")
            .count();
        assert_eq!(original, 2);
        let mut doc = import::read(bytes).unwrap().document;
        let markers = |doc: &LayoutDocument| {
            doc.stories
                .iter()
                .flat_map(|s| s.structures.iter())
                .filter(|s| s.kind == "ProcessingInstruction")
                .map(|s| (s.at, s.control.clone()))
                .collect::<Vec<_>>()
        };
        let expected = markers(&doc);
        assert_eq!(expected.len(), 2);
        assert!(expected.iter().all(|(_, control)| matches!(
            control,
            Some(InlineControl::PageNumber {
                kind: PageNumberKind::Current,
                ..
            })
        )));
        for _ in 0..3 {
            let saved = export::write(&doc).bytes;
            assert_eq!(
                native_instructions(&saved)
                    .iter()
                    .filter(|(pi, mode)| pi == "ACE 18" && mode.as_deref() == Some("AutoPageNumber"))
                    .count(),
                2
            );
            doc = import::read(&saved).unwrap().document;
            assert_eq!(markers(&doc), expected);
        }
    }
}

fn chapter_preference(bytes: &[u8]) -> Option<String> {
    let package = container::read(bytes).unwrap();
    let text = package.text("Resources/Preferences.xml").unwrap();
    let root = xml::parse(text).unwrap();
    root.find("ChapterNumberPreference").map(|element| {
        format!(
            "{}|{}|{}",
            element.attr("ChapterNumber").unwrap_or_default(),
            element.attr("ChapterNumberSource").unwrap_or_default(),
            element
                .find("ChapterNumberFormat")
                .map(|f| f.text.clone())
                .unwrap_or_default()
        )
    })
}

#[test]
fn chapter_numbering_preferences_round_trip_and_invalid_values_are_reported() {
    let bytes = include_bytes!("../../../fixtures/idml/text.idml");
    let mut doc = import::read(bytes).unwrap().document;
    let expected = Some(ChapterNumbering {
        number: 1,
        source: ChapterSource::ContinueFromPreviousDocument,
        format: ARABIC_CHAPTER_FORMAT.into(),
    });
    assert_eq!(doc.chapter_numbering, expected);
    for _ in 0..3 {
        let saved = export::write(&doc).bytes;
        assert_eq!(
            chapter_preference(&saved).as_deref(),
            Some("1|ContinueFromPreviousDocument|1, 2, 3, 4...")
        );
        doc = import::read(&saved).unwrap().document;
        assert_eq!(doc.chapter_numbering, expected);
    }
    for (from, to, property) in [
        (
            r#"ChapterNumber="1""#,
            r#"ChapterNumber="0""#,
            "ChapterNumber",
        ),
        (
            r#"ChapterNumber="1""#,
            r#"ChapterNumber="x""#,
            "ChapterNumber",
        ),
        (
            r#"ChapterNumberSource="ContinueFromPreviousDocument""#,
            r#"ChapterNumberSource="FromBookFuture""#,
            "ChapterNumberSource",
        ),
    ] {
        let edited = rewrite(bytes, |name, xml| {
            if name == "Resources/Preferences.xml" {
                xml.replace(from, to)
            } else {
                xml
            }
        });
        let imported = import::read(&edited).unwrap();
        assert_eq!(imported.document.chapter_numbering, None, "{to}");
        assert!(
            imported.report.skipped.iter().any(|s| s.contains(property)),
            "{:?}",
            imported.report.skipped
        );
        assert_eq!(
            chapter_preference(&export::write(&imported.document).bytes),
            None
        );
    }
    let mut doc = blank_a4();
    assert_eq!(chapter_preference(&export::write(&doc).bytes), None);
    doc.chapter_numbering = Some(ChapterNumbering {
        number: 12,
        source: ChapterSource::UserDefined,
        format: "custom & <format>".into(),
    });
    let saved = export::write(&doc).bytes;
    assert_eq!(
        import::read(&saved).unwrap().document.chapter_numbering,
        doc.chapter_numbering
    );
}

fn chapter_definition(id: &str, attributes: &str) -> String {
    format!(
        r#"<TextVariable Self="{id}" Name="Chapter" VariableType="ChapterNumberType"><ChapterNumberVariablePreference {attributes}/></TextVariable>"#
    )
}

fn instance(definition: &str) -> String {
    range(
        &format!(
            r#"<TextVariableInstance Self="i{definition}" Name="Chapter" AssociatedTextVariable="{definition}" ResultText="9"/>"#
        ),
        Some("TextVariable"),
    )
}

#[test]
fn native_chapter_variables_lower_render_and_save_with_document_numbering() {
    for (format, default_value, user_value) in [
        ("Current", "1", "14"),
        ("Arabic", "1", "14"),
        ("UpperRoman", "I", "XIV"),
        ("LowerLetters", "a", "n"),
    ] {
        let ranges = range("<Content>Ch </Content>", None) + &instance("chapter");
        let bytes = native(
            &ranges,
            &chapter_definition(
                "chapter",
                &format!(r#"TextBefore="" Format="{format}" TextAfter="""#),
            ),
        );
        let mut doc = import::read(&bytes).unwrap().document;
        fonts(&mut doc);
        assert_eq!(doc.text_variables.len(), 1);
        assert_eq!(values(&doc), (vec![default_value.to_owned()], 0));
        doc.chapter_numbering = Some(ChapterNumbering {
            number: 14,
            source: ChapterSource::UserDefined,
            format: ARABIC_CHAPTER_FORMAT.into(),
        });
        let definitions = doc.text_variables.clone();
        for _ in 0..3 {
            assert_eq!(values(&doc), (vec![user_value.to_owned()], 0));
            let saved = export::write(&doc).bytes;
            let package = container::read(&saved).unwrap();
            let root = xml::parse(package.text("designmap.xml").unwrap()).unwrap();
            let native = root
                .children_named("TextVariable")
                .find(|e| e.attr("VariableType") == Some("ChapterNumberType"))
                .unwrap();
            assert_eq!(
                native
                    .child("ChapterNumberVariablePreference")
                    .unwrap()
                    .attr("Format"),
                Some(format)
            );
            doc = import::read(&saved).unwrap().document;
            fonts(&mut doc);
            assert_eq!(doc.text_variables, definitions);
        }
    }
    for attributes in [
        r#"TextBefore="" Format="Kanji" TextAfter="""#,
        r#"TextBefore="" TextAfter="""#,
        r#"Format="Current" Future="1""#,
    ] {
        let ranges = range("<Content>Ch </Content>", None) + &instance("chapter");
        let doc = import::read(&native(&ranges, &chapter_definition("chapter", attributes)))
            .unwrap()
            .document;
        assert!(doc.text_variables.is_empty(), "{attributes}");
        assert_eq!(values(&doc).1, 1);
    }
}

#[test]
fn every_public_fixture_keeps_its_chapter_definition_typed_through_saves() {
    for bytes in [
        include_bytes!("../../../fixtures/idml/bounded-text.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/images.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/multipage.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/placeholders.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/shapes.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/text.idml").as_slice(),
        include_bytes!("../../../fixtures/idml/themes.idml").as_slice(),
    ] {
        let mut doc = import::read(bytes).unwrap().document;
        let typed: Vec<_> = doc.text_variables.iter().map(|d| d.name.clone()).collect();
        assert_eq!(typed, ["Chapter Number", "Last Page Number"]);
        let expected = (
            doc.text_variables.clone(),
            doc.retained_text_variables.clone(),
            doc.chapter_numbering.clone(),
        );
        for _ in 0..3 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(
                (
                    doc.text_variables.clone(),
                    doc.retained_text_variables.clone(),
                    doc.chapter_numbering.clone()
                ),
                expected
            );
        }
    }
}

#[test]
fn authored_markers_save_as_native_instructions_and_reopen_unchanged() {
    use schist_layout::text_variables::Cursor;
    let mut doc = import::read(&native(&range("<Content>Folio </Content>", None), ""))
        .unwrap()
        .document;
    let mut history = History::default();
    for section in [false, true, false] {
        let cursor = Cursor::capture(&doc, StoryId(0), "Folio ".len()).unwrap();
        assert!(cursor.insert_marker(&mut doc, &mut history, section));
    }
    let expected = controls(&doc);
    assert_eq!(expected.len(), 3);
    for _ in 0..3 {
        let saved = export::write(&doc).bytes;
        assert_eq!(
            native_instructions(&saved),
            [
                ("ACE 18".to_owned(), Some("AutoPageNumber".to_owned())),
                ("ACE 19".to_owned(), None),
                ("ACE 18".to_owned(), Some("AutoPageNumber".to_owned())),
            ]
        );
        doc = import::read(&saved).unwrap().document;
        assert_eq!(controls(&doc), expected);
        assert_eq!(doc.stories[0].text(), "Folio ");
    }
}
