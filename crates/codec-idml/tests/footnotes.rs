use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    blank_a4, decorations::DecorationStroke, footnotes::*, CharacterStyle, Ink, LayoutDocument,
    ParagraphStyle,
};

fn native_note(payload: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = schist_layout::authoring::text_frame(
        &mut doc,
        &mut schist_layout::History::default(),
        0,
        schist_layout::Rect::new(0.0, 0.0, 200.0, 100.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = schist_layout::Story::from_text("AéB", "Body");
    doc.styles.paragraphs.push(ParagraphStyle {
        name: "Note body".into(),
        point_size: Some(9.0),
        ..Default::default()
    });
    doc.styles.characters.push(CharacterStyle {
        name: "Note marker".into(),
        point_size: Some(7.0),
        ..Default::default()
    });
    rewrite(&export::write(&doc).bytes, |name, xml| {
        if name.starts_with("Stories/") {
            xml.replace(
                "<Content>AéB</Content>",
                &format!("<Content>Aé</Content>{payload}<Content>B</Content>"),
            )
        } else {
            xml
        }
    })
}

#[test]
fn public_academic_notes_compose_without_persisting_generated_bytes_or_styles() {
    let mut doc = import::read(include_bytes!(
        "../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"
    ))
    .unwrap()
    .document;
    let (index, original) = doc
        .stories
        .iter()
        .enumerate()
        .find(|(_, s)| s.structures.iter().any(|s| s.footnote.is_some()))
        .map(|(i, s)| (i, s.clone()))
        .unwrap();
    let styles = doc.styles.clone();
    for _ in 0..3 {
        let thread =
            schist_layout::compose::compose_story(&doc, schist_layout::StoryId(index as u32));
        let notes: Vec<_> = thread.frames.iter().flat_map(|f| &f.footnotes).collect();
        assert_eq!(notes.len(), 1);
        let text: String = notes[0]
            .lines
            .iter()
            .map(|l| schist_layout::compose::line_spec(l, &doc.stories[index], &doc).text)
            .collect();
        assert_eq!(text, "4 This is a footnote.5 This is a footnote.");
        assert!(notes[0].rule.is_some());
        assert_eq!(doc.stories[index], original);
        assert_eq!(doc.styles, styles);
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn fonts_used_only_by_generated_note_numbers_enter_native_and_package_inventories() {
    let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?></Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
    let mut doc = import::read(&native_note(payload)).unwrap().document;
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Note body")
        .unwrap()
        .family = Some("Note-only family".into());
    doc.styles
        .characters
        .iter_mut()
        .find(|c| c.name == "Note marker")
        .unwrap()
        .font_style = Some("Light".into());
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Body")
        .unwrap()
        .family = Some("Main-only family".into());
    doc.styles.characters.push(CharacterStyle {
        name: "Reference only".into(),
        font_style: Some("Semibold".into()),
        ..Default::default()
    });
    doc.footnotes.marker_style = Some(FootnoteReference::Resolved("Reference only".into()));
    let output = export::write(&doc);
    let parts = container::read(&output.bytes).unwrap();
    let fonts = parts.text("Resources/Fonts.xml").unwrap();
    assert!(fonts.contains("FontFamily=\"Note-only family\" Name=\"Note-only family Light\""));
    assert!(fonts.contains("FontFamily=\"Main-only family\" Name=\"Main-only family Semibold\""));
}

#[test]
fn note_markers_keep_their_own_utf8_coordinates_styles_and_source_bytes_through_saves() {
    let text = "é&中 4";
    for at in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
        let escape = |s: &str| s.replace('&', "&amp;");
        let payload = format!(
            r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body" PointSize="10"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content>{}<?ACE 4?>{}</Content><Br/><Content>5 literal second paragraph</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#,
            escape(&text[..at]),
            escape(&text[at..])
        );
        let mut doc = import::read(&native_note(&payload)).unwrap().document;
        let source = &doc.stories[0];
        assert_eq!(source.text(), "AéB");
        let structure = &source.structures[0];
        assert_eq!(structure.at, Some(3));
        assert_eq!(structure.payload, payload);
        let note = structure.footnote.as_ref().unwrap();
        assert!(note.valid());
        assert_eq!(
            note.story.text(),
            format!("{text}\n5 literal second paragraph")
        );
        assert_eq!(
            note.markers,
            vec![FootnoteMarker {
                at,
                character_style: "Note marker".into()
            }]
        );
        assert_eq!(note.story.ranges[0].start, 0);
        assert_eq!(note.story.ranges[0].end, text.len());
        for point in &note.story.points {
            let schist_layout::StoryPoint::Paragraph { style, .. } = point else {
                panic!()
            };
            assert_eq!(doc.styles.resolve_paragraph(style).point_size, Some(10.0));
        }
        let expected = doc.stories.clone();
        let styles = doc.styles.clone();
        for _ in 0..4 {
            let written = export::write(&doc);
            assert!(written
                .warnings
                .contains(&schist_i18n::t("design.idml_story_structure").to_string()));
            let read = import::read(&written.bytes).unwrap();
            assert!(!read
                .report
                .skipped
                .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
            doc = read.document;
            assert_eq!(doc.stories, expected);
            assert_eq!(doc.styles, styles);
        }
    }
}

#[test]
fn unsupported_note_content_stays_exact_and_cannot_be_mistaken_for_text_only_lowering() {
    for body in [
        "<Table><Content>nested</Content></Table>",
        "<Rectangle/>", "<Footnote><Content>nested</Content></Footnote>",
        "<Content>é<?ACE 999?>tail</Content>", "<?ACE 4?><Content>outside</Content>",
        "<Content>mixed<Tab/>content</Content>", "direct text<Content>tail</Content>",
        "<TextVariableInstance/>",
        "<CharacterStyleRange ParagraphBreakType=\"NextFrame\"><Content>body</Content><Br/></CharacterStyleRange>",
    ] {
        let payload = format!("<Footnote>{body}</Footnote>");
        let mut doc = import::read(&native_note(&payload)).unwrap().document;
        assert_eq!(doc.stories[0].text(), "AéB");
        let expected = doc.stories.clone();
        for _ in 0..4 {
            let structure = &doc.stories[0].structures[0];
            assert_eq!(structure.payload, payload);
            assert!(structure.footnote.is_none(), "{body}");
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.stories, expected);
        }
    }
}

#[test]
fn published_note_body_and_only_note_style_changes_keep_the_retention_guard_honest() {
    let doc = import::read(include_bytes!(
        "../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"
    ))
    .unwrap()
    .document;
    let note = doc
        .stories
        .iter()
        .flat_map(|s| &s.structures)
        .find_map(|s| s.footnote.as_ref())
        .unwrap();
    assert_eq!(
        note.story.text(),
        " This is a footnote.\n5 This is a footnote."
    );
    assert_eq!(note.markers.len(), 1);
    assert_eq!(note.markers[0].at, 0);
    for paragraph in [true, false] {
        let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?>é note</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
        let doc = import::read(&native_note(payload)).unwrap().document;
        let name = if paragraph {
            "Note body"
        } else {
            "Note marker"
        };
        let mut renamed = doc.clone();
        let mut history = schist_layout::History::default();
        assert!(schist_layout::properties::rename_style(
            &mut renamed,
            &mut history,
            paragraph,
            name,
            "Renamed in Schist"
        ));
        assert_eq!(history.undo_depth(), 1);
        let expected = renamed.stories.clone();
        for _ in 0..4 {
            renamed = import::read(&export::write(&renamed).bytes)
                .unwrap()
                .document;
            assert_eq!(renamed.stories, expected);
        }
        let bytes = rewrite(&export::write(&doc).bytes, |part, xml| {
            if part == "Resources/Styles.xml" {
                xml.replace(&format!("Name=\"{name}\""), "Name=\"External name\"")
            } else {
                xml
            }
        });
        let read = import::read(&bytes).unwrap();
        assert_eq!(read.document.stories[0].text(), "AéB");
        let structures = &read.document.stories[0].structures;
        let native = structures.iter().find(|s| s.at == Some(3)).unwrap();
        let note = native.footnote.as_ref().unwrap();
        if paragraph {
            assert!(note.story.points.iter().any(|p| matches!(p,
                schist_layout::StoryPoint::Paragraph { style, .. } if style == "External name")));
        } else {
            assert_eq!(note.markers[0].character_style, "External name");
        }
        assert!(structures
            .iter()
            .any(|s| s.at.is_none() && s.payload == payload));
        assert!(read
            .report
            .skipped
            .contains(&schist_i18n::t("design.idml_structure_location").to_string()));
    }
}

fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, String) -> String) -> Vec<u8> {
    let package = container::read(bytes).unwrap();
    container::write(
        &package
            .names()
            .into_iter()
            .map(|name| {
                let bytes = package.get(name).unwrap();
                (
                    name.to_owned(),
                    if name.ends_with(".xml") {
                        edit(name, String::from_utf8(bytes.to_vec()).unwrap()).into_bytes()
                    } else {
                        bytes.to_vec()
                    },
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn without_retention(bytes: &[u8]) -> Vec<u8> {
    rewrite(bytes, |_, mut xml| {
        while let Some(key) = xml.find("Key=\"Schist.StructuredStory.v1\"") {
            let start = xml[..key].rfind("<KeyValuePair").unwrap();
            let end = key + xml[key..].find("/>").unwrap() + 2;
            xml.replace_range(start..end, "");
        }
        xml
    })
}

#[test]
fn native_note_export_needs_no_private_record_at_any_utf8_or_paragraph_boundary() {
    use schist_layout::{Story, StyleRange};
    let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?>é &amp; 中</Content><Br/><Content>5 literal</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
    let base = import::read(&native_note(payload)).unwrap().document;
    for main in ["", "Aé中", "A\n\n中", "a\tb\nc"] {
        for at in main.char_indices().map(|(i, _)| i).chain([main.len()]) {
            let mut doc = base.clone();
            let mut story = Story::from_text(main, "Body");
            story.ranges = main
                .char_indices()
                .map(|(start, c)| StyleRange::new(start, start + c.len_utf8(), "Note marker"))
                .collect();
            let mut structure = base.stories[0].structures[0].clone();
            structure.at = Some(at);
            // Two notes at the same insertion point retain their source order.
            for suffix in ["first", "second"] {
                let mut s = structure.clone();
                s.footnote
                    .as_mut()
                    .unwrap()
                    .story
                    .push_paragraph(suffix, "Note body");
                story.structures.push(s);
            }
            doc.stories[0] = story;
            let expected = doc.stories[0].clone();
            let style_count = (doc.styles.paragraphs.len(), doc.styles.characters.len());
            for _ in 0..4 {
                let saved = export::write(&doc);
                let package = container::read(&saved.bytes).unwrap();
                let root = xml::parse(
                    package
                        .names()
                        .into_iter()
                        .find_map(|name| {
                            name.starts_with("Stories/")
                                .then(|| package.text(name).unwrap())
                        })
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(root.find_all("Footnote").len(), 2);
                let native = import::read(&without_retention(&saved.bytes))
                    .unwrap()
                    .document;
                assert_eq!(native.stories[0].text(), main);
                assert_eq!(native.stories[0].structures.len(), 2);
                for (actual, expected) in native.stories[0]
                    .structures
                    .iter()
                    .zip(&expected.structures)
                {
                    assert_eq!(actual.at, Some(at));
                    let actual = actual.footnote.as_ref().unwrap();
                    let expected = expected.footnote.as_ref().unwrap();
                    assert_eq!(actual.story, expected.story);
                    assert_eq!(actual.markers, expected.markers);
                    assert_eq!(
                        actual.reference_character_style,
                        expected.reference_character_style
                    );
                }
                doc = import::read(&saved.bytes).unwrap().document;
                assert_eq!(doc.stories[0], expected);
                assert_eq!(
                    (doc.styles.paragraphs.len(), doc.styles.characters.len()),
                    style_count
                );
            }
        }
    }
}

#[test]
fn native_note_edits_supersede_stale_retention_and_unknown_anchors_are_never_guessed() {
    let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?>original note</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
    let doc = import::read(&native_note(payload)).unwrap().document;
    let saved = export::write(&doc).bytes;
    let edited = rewrite(&saved, |name, xml| {
        if name.starts_with("Stories/") {
            xml.replace(
                "<Content>original note</Content>",
                "<Content>changed native note</Content>",
            )
        } else {
            xml
        }
    });
    let read = import::read(&edited).unwrap();
    let notes = &read.document.stories[0].structures;
    let current = notes.iter().find(|s| s.at == Some(3)).unwrap();
    assert_eq!(
        current.footnote.as_ref().unwrap().story.text(),
        "changed native note"
    );
    assert!(notes.iter().any(|s| s.at.is_none() && s.payload == payload));
    for at in [None, Some(2), Some(999)] {
        let mut doc = doc.clone();
        doc.stories[0].structures[0].at = at;
        let bytes = export::write(&doc).bytes;
        let package = container::read(&bytes).unwrap();
        for name in package
            .names()
            .into_iter()
            .filter(|n| n.starts_with("Stories/"))
        {
            assert!(!package.text(name).unwrap().contains("<Footnote>"));
        }
    }
}

#[test]
fn note_only_inherited_font_faces_join_native_resources_and_package_inventory() {
    let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?>body</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
    let mut doc = import::read(&native_note(payload)).unwrap().document;
    doc.styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Note body")
        .unwrap()
        .family = Some("Note only font".into());
    let character = doc
        .styles
        .characters
        .iter_mut()
        .find(|s| s.name == "Note marker")
        .unwrap();
    character.family = None;
    character.font_style = Some("Light".into());
    let saved = export::write(&doc);
    let package = container::read(&saved.bytes).unwrap();
    let fonts = xml::parse(package.text("Resources/Fonts.xml").unwrap()).unwrap();
    assert!(fonts
        .find_all("Font")
        .iter()
        .any(|f| f.attr("FontFamily") == Some("Note only font")
            && f.attr("FontStyleName") == Some("Light")));
}

#[test]
fn legacy_retention_loads_but_native_note_deletion_never_resurrects_an_anchor() {
    let payload = r#"<Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/Note body"><CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/Note marker"><Content><?ACE 4?>body</Content></CharacterStyleRange></ParagraphStyleRange></Footnote>"#;
    let doc = import::read(&native_note(payload)).unwrap().document;
    let mut legacy = doc.clone();
    legacy.stories[0].structures[0].footnote = None;
    let old = rewrite(&export::write(&legacy).bytes, |name, xml| {
        if !name.starts_with("Stories/") {
            return xml;
        }
        let tree = xml::parse(&xml).unwrap();
        let json = tree
            .find_all("KeyValuePair")
            .into_iter()
            .find(|e| e.attr("Key") == Some("Schist.StructuredStory.v1"))
            .unwrap()
            .attr("Value")
            .unwrap();
        let mut record: serde_json::Value = serde_json::from_str(json).unwrap();
        record.as_object_mut().unwrap().remove("native_footnotes");
        record["story"] = serde_json::to_value(&doc.stories[0]).unwrap();
        let escaped = |text: &str| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        };
        xml.replace(&escaped(json), &escaped(&record.to_string()))
    });
    let read = import::read(&old).unwrap();
    assert_eq!(read.document.stories, doc.stories);
    assert!(!read
        .report
        .skipped
        .contains(&schist_i18n::t("design.idml_structure_location").to_string()));

    let deleted = rewrite(&export::write(&doc).bytes, |name, mut xml| {
        if name.starts_with("Stories/") {
            let start = xml.find("<Footnote>").unwrap();
            let end = xml.find("</Footnote>").unwrap() + "</Footnote>".len();
            xml.replace_range(start..end, "");
        }
        xml
    });
    let mut deleted = import::read(&deleted).unwrap().document;
    for _ in 0..4 {
        assert_eq!(deleted.stories[0].text(), "AéB");
        assert_eq!(deleted.stories[0].structures.len(), 1);
        assert_eq!(deleted.stories[0].structures[0].at, None);
        assert_eq!(deleted.stories[0].structures[0].payload, payload);
        deleted = import::read(&export::write(&deleted).bytes)
            .unwrap()
            .document;
    }
}
fn preferences(fragment: &str) -> Vec<u8> {
    rewrite(&export::write(&blank_a4()).bytes, |name, xml| {
        if name == "Resources/Preferences.xml" {
            xml.replace(
                "</idPkg:Preferences>",
                &format!("{fragment}</idPkg:Preferences>"),
            )
        } else {
            xml
        }
    })
}
fn rule(on: bool) -> FootnoteRule {
    FootnoteRule {
        on: Some(on),
        stroke: Some(FootnoteReference::Resolved(DecorationStroke {
            name: "Footnotes / dashed & 空".into(),
            fitting: Default::default(),
            pattern: schist_text_engine::TextDecorationPattern::Dashes(
                schist_text_engine::DecorationDashes {
                    lengths: vec![2.0, 3.0],
                    cap: schist_text_engine::DecorationCap::Round,
                },
            ),
        })),
        paint: Some(FootnoteReference::Resolved(Ink::spot(
            "Note & rule",
            [50.0, 20.0, 5.0],
        ))),
        gap_paint: Some(FootnoteReference::Resolved(Ink::cmyk(
            "Gap ink",
            [0.2, 0.1, 0.4, 0.0],
        ))),
        weight: Some(0.5),
        tint: Some(0.375),
        gap_tint: Some(0.625),
        overprint: Some(on),
        gap_overprint: Some(!on),
        left_indent: Some(-12.0),
        width: Some(72.0),
        offset: Some(-2.0),
    }
}
fn assert_saves(mut doc: LayoutDocument) {
    let expected = doc.footnotes.clone();
    for _ in 0..4 {
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        assert!(!package
            .text("designmap.xml")
            .unwrap()
            .contains("<FootnoteOption"));
        let preferences = package.text("Resources/Preferences.xml").unwrap();
        assert_eq!(
            preferences.matches("<FootnoteOption").count(),
            usize::from(!expected.is_empty())
        );
        let imported = import::read(&written.bytes).unwrap();
        assert_eq!(imported.document.footnotes, expected);
        doc = imported.document;
    }
}

#[test]
fn all_numbering_modes_and_independent_rule_resources_survive_native_saves() {
    use FootnoteNumbering::*;
    for (i, numbering) in [
        Arabic,
        RomanUpper,
        RomanLower,
        LettersUpper,
        LettersLower,
        Symbols,
        Kanji,
        FullWidthArabic,
        SingleLeadingZeros,
        DoubleLeadingZeros,
        Asterisks,
        ArabicAlifBaTah,
        ArabicAbjad,
        HebrewBiblical,
        HebrewNonStandard,
    ]
    .into_iter()
    .enumerate()
    {
        let mut doc = blank_a4();
        let name = "Notes / 空 & \"A\"";
        doc.styles.paragraphs.push(ParagraphStyle {
            name: name.into(),
            point_size: Some(9.0),
            ..Default::default()
        });
        doc.styles.characters.push(CharacterStyle {
            name: name.into(),
            point_size: Some(7.0),
            ..Default::default()
        });
        doc.footnotes = FootnoteOptions {
            start_at: Some(100000),
            numbering: Some(numbering),
            restart: Some(
                [
                    FootnoteRestart::Continuous,
                    FootnoteRestart::Page,
                    FootnoteRestart::Spread,
                    FootnoteRestart::Section,
                ][i % 4]
                    .clone(),
            ),
            affixes: Some(
                [
                    FootnoteAffixes::None,
                    FootnoteAffixes::Reference,
                    FootnoteAffixes::Note,
                    FootnoteAffixes::Both,
                ][i % 4]
                    .clone(),
            ),
            marker_position: Some(
                [
                    FootnoteMarkerPosition::Normal,
                    FootnoteMarkerPosition::Superscript,
                    FootnoteMarkerPosition::Subscript,
                    FootnoteMarkerPosition::Ruby,
                ][i % 4]
                    .clone(),
            ),
            first_baseline: Some(
                [
                    FootnoteFirstBaseline::Ascent,
                    FootnoteFirstBaseline::CapHeight,
                    FootnoteFirstBaseline::Leading,
                    FootnoteFirstBaseline::EmBox,
                    FootnoteFirstBaseline::XHeight,
                    FootnoteFirstBaseline::Fixed,
                ][i % 6]
                    .clone(),
            ),
            prefix: Some("é".repeat(100)),
            suffix: Some(" & > < \" ' ".into()),
            separator: Some("\t\r\n".into()),
            text_style: Some(FootnoteReference::Resolved(name.into())),
            marker_style: Some(FootnoteReference::Resolved(name.into())),
            space_between: Some(2.5),
            spacer: Some(7.2),
            minimum_first_baseline: Some(8.0),
            end_of_story: Some(i % 2 == 0),
            no_splitting: Some(i % 2 != 0),
            straddle: Some(i % 2 == 0),
            rule: rule(i % 2 == 0),
            continuing_rule: rule(i % 2 != 0),
        };
        assert!(doc.footnotes.valid());
        assert_eq!(doc.all_decoration_strokes().len(), 1);
        assert!(doc.all_inks().iter().any(|ink| ink.name == "Note & rule"));
        // Referenced resources are only on the options, not in StyleSet or the swatch list.
        let saved = export::write(&doc);
        assert!(
            !saved
                .warnings
                .iter()
                .any(|w| w.contains("footnote setting")),
            "{:?}",
            saved.warnings
        );
        let package = container::read(&saved.bytes).unwrap();
        assert!(package
            .text("Resources/Preferences.xml")
            .unwrap()
            .contains("SeparatorText=\"&#9;&#13;&#10;\""));
        let read = import::read(&saved.bytes).unwrap();
        assert!(
            !read
                .report
                .skipped
                .iter()
                .any(|w| w.contains("footnote setting")),
            "{:?}",
            read.report.skipped
        );
        assert_saves(doc);
    }
}

#[test]
fn the_public_academic_fixture_retains_native_preferences_and_opaque_note_body() {
    let imported = import::read(include_bytes!(
        "../../../fixtures/indd/psu-academic-2/psu-academic-2.idml"
    ))
    .unwrap();
    let mut doc = imported.document;
    let options = &doc.footnotes;
    assert_eq!(options.start_at, Some(4));
    assert_eq!(options.spacer, Some(7.2));
    assert_eq!(options.space_between, Some(0.72));
    assert_eq!(options.separator.as_deref(), Some("\t"));
    assert_eq!(options.rule.width, Some(72.0));
    assert_eq!(options.continuing_rule.width, Some(288.0));
    assert_eq!(options.first_baseline, Some(FootnoteFirstBaseline::Leading));
    assert_eq!(
        options.marker_position,
        Some(FootnoteMarkerPosition::Superscript)
    );
    assert_eq!(options.straddle, Some(true));
    assert_eq!(options.marker_style, Some(FootnoteReference::None));
    let FootnoteReference::Resolved(name) = options.text_style.as_ref().unwrap() else {
        panic!("unresolved style")
    };
    assert!(doc.styles.paragraph(name).is_some());
    assert_eq!(name, "Academic Style 2 Template:Footnotes");
    let notes = |doc: &LayoutDocument| {
        doc.stories
            .iter()
            .flat_map(|s| &s.structures)
            .filter(|s| s.kind == "Footnote")
            .cloned()
            .collect::<Vec<_>>()
    };
    let expected = notes(&doc);
    assert!(!expected.is_empty());
    let options = options.clone();
    for _ in 0..4 {
        let saved = export::write(&doc);
        doc = import::read(&saved.bytes).unwrap().document;
        assert_eq!(doc.footnotes, options);
        assert_eq!(notes(&doc), expected);
    }
}

#[test]
fn style_ids_are_resolved_through_resources_and_later_native_changes_win() {
    let mut doc = blank_a4();
    doc.footnotes.text_style = Some(FootnoteReference::Resolved("Body".into()));
    doc.footnotes.marker_style = Some(FootnoteReference::Resolved("Default".into()));
    doc.footnotes.start_at = Some(3);
    let bytes = rewrite(&export::write(&doc).bytes, |name, xml| {
        let xml = xml
            .replace("ParagraphStyle/$ID/Body", "Opaque / P &amp; 7")
            .replace("CharacterStyle/$ID/Default", "Opaque / C &amp; 8");
        if name == "Resources/Styles.xml" {
            xml.replace("Name=\"Body\"", "Name=\"Renamed note style\"")
                .replace("Name=\"Default\"", "Name=\"Renamed marker style\"")
        } else if name == "Resources/Preferences.xml" {
            xml.replace("StartAt=\"3\"", "StartAt=\"8\"")
        } else {
            xml
        }
    });
    let doc = import::read(&bytes).unwrap().document;
    assert_eq!(doc.footnotes.start_at, Some(8));
    assert_eq!(
        doc.footnotes.text_style,
        Some(FootnoteReference::Resolved("Renamed note style".into()))
    );
    assert_eq!(
        doc.footnotes.marker_style,
        Some(FootnoteReference::Resolved("Renamed marker style".into()))
    );
    assert_saves(doc);
}

#[test]
fn omitted_settings_stay_absent_and_unknown_values_remain_explicit_across_saves() {
    assert_saves(blank_a4());
    for fragment in [
        "<FootnoteOption/>",
        "<FootnoteOption NoSplitting=\"0\" EosPlacement=\"1\" RuleOn=\"0\" FootnoteTextStyle=\"n\"/>",
        "<FootnoteOption FootnoteTextStyle=\"Opaque/Unknown\" FootnoteMarkerStyle=\"Missing/Marker\" FootnoteFirstBaselineOffset=\"FutureBaseline\"><Properties><FootnoteNumberingStyle type=\"string\">FutureSequence</FootnoteNumberingStyle><RestartNumbering type=\"string\">FutureRestart</RestartNumbering><ShowPrefixSuffix type=\"string\">FutureAffixes</ShowPrefixSuffix><MarkerPositioning type=\"string\">FutureMarker</MarkerPositioning><RuleColor type=\"object\">UnknownColor</RuleColor><RuleType type=\"object\">UnknownStroke</RuleType></Properties></FootnoteOption>",
    ] {
        let read = import::read(&preferences(fragment)).unwrap();
        if fragment.contains("FutureSequence") {
            assert_eq!(read.document.footnotes.numbering, Some(FootnoteNumbering::Other("FutureSequence".into())));
            assert_eq!(read.document.footnotes.text_style, Some(FootnoteReference::Unresolved("Opaque/Unknown".into())));
            assert_eq!(read.report.skipped.iter().filter(|w| w.contains("footnote setting")).count(), 9);
        }
        assert_saves(read.document);
    }
}

#[test]
fn invalid_preference_values_are_rejected_independently_at_both_codec_boundaries() {
    for (key, low, high) in [
        ("SpaceBetween", 0.0, 864.0),
        ("Spacer", 0.0, 864.0),
        ("FootnoteMinimumFirstBaselineOffset", 0.0, 103680.0),
        ("RuleLineWeight", 0.0, 1000.0),
        ("RuleTint", 0.0, 100.0),
        ("RuleGapTint", 0.0, 100.0),
        ("RuleLeftIndent", -103680.0, 103680.0),
        ("RuleWidth", 0.0, 103680.0),
        ("RuleOffset", -15552.0, 15552.0),
    ] {
        for key in [key.to_string(), key.replace("Rule", "ContinuingRule")] {
            for value in [
                low,
                high,
                low - 1.0,
                high + 1.0,
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ] {
                let valid = value.is_finite() && (low..=high).contains(&value);
                let raw = value.to_string();
                let read = import::read(&preferences(&format!(
                    "<FootnoteOption StartAt=\"7\" {key}=\"{raw}\"/>"
                )))
                .unwrap();
                assert_eq!(read.document.footnotes.start_at, Some(7));
                let message = schist_i18n::tf!(
                    "design.idml_text_preference_invalid",
                    property = &key,
                    value = &raw
                );
                assert_eq!(
                    read.report.skipped.contains(&message),
                    !valid,
                    "{key}: {raw}"
                );
                let written = export::write(&read.document);
                let package = container::read(&written.bytes).unwrap();
                let root = xml::parse(package.text("Resources/Preferences.xml").unwrap()).unwrap();
                assert_eq!(
                    root.find("FootnoteOption").unwrap().attr(&key).is_some(),
                    valid
                );
            }
        }
    }
    for (key, value) in [
        ("StartAt", "0"),
        ("StartAt", "1.5"),
        ("StartAt", "100001"),
        ("NoSplitting", "TRUE"),
        ("EosPlacement", "no"),
        ("RuleOn", "2"),
        ("ContinuingRuleOverprint", "NaN"),
        ("EnableStraddling", "yes"),
    ] {
        let read = import::read(&preferences(&format!(
            "<FootnoteOption {key}=\"{value}\"/>"
        )))
        .unwrap();
        assert!(read.document.footnotes.is_empty());
        assert!(read.report.skipped.contains(&schist_i18n::tf!(
            "design.idml_text_preference_invalid",
            property = key,
            value = value
        )));
    }
    let mut doc = blank_a4();
    doc.footnotes = FootnoteOptions {
        start_at: Some(0),
        spacer: Some(f32::INFINITY),
        prefix: Some("é".repeat(101)),
        rule: FootnoteRule {
            width: Some(-1.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let saved = export::write(&doc);
    assert_eq!(
        saved
            .warnings
            .iter()
            .filter(|w| w.starts_with("Invalid "))
            .count(),
        4
    );
    assert!(import::read(&saved.bytes)
        .unwrap()
        .document
        .footnotes
        .is_empty());
}

#[test]
fn native_frame_and_object_style_footnote_overrides_survive_repeated_saves() {
    use schist_layout::{authoring, History, LayoutObject, ObjectStyle, Rect};
    for enabled in [false, true] {
        let mut doc = blank_a4();
        authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(20.0, 20.0, 180.0, 200.0),
        )
        .unwrap();
        doc.frame_footnote_defaults = FrameFootnotes {
            enabled: Some(false),
            straddle: Some(true),
            spacer: Some(12.0),
            space_between: Some(6.0),
        };
        let defaults = doc.frame_footnote_defaults.clone();
        let native = export::write(&doc).bytes;
        let native = rewrite(&native, |name, xml| {
            if name.starts_with("Spreads/") {
                xml.replace("</TextFramePreference>", &format!(
                    "</TextFramePreference><TextFrameFootnoteOptionsObject EnableOverrides=\"{enabled}\" SpanFootnotesAcross=\"false\" MinimumSpacingOption=\"17.25\" SpaceBetweenFootnotes=\"8.5\"/>"
                ))
            } else {
                xml
            }
        });
        doc = import::read(&native).unwrap().document;
        let expected = FrameFootnotes {
            enabled: Some(enabled),
            straddle: Some(false),
            spacer: Some(17.25),
            space_between: Some(8.5),
        };
        doc.styles.objects.push(ObjectStyle {
            name: "Note frame / 空".into(),
            enable_footnotes: Some(true),
            footnotes: FrameFootnotes {
                enabled: Some(true),
                straddle: Some(true),
                spacer: Some(27.0),
                space_between: Some(11.0),
            },
            ..Default::default()
        });
        doc.objects[0].appearance.style = Some("Note frame / 空".into());
        let styles = doc.styles.objects.clone();
        for _ in 0..3 {
            let LayoutObject::TextFrame { footnotes, .. } = &doc.objects[0].object else {
                panic!()
            };
            assert_eq!(footnotes, &expected);
            assert_eq!(doc.styles.frame_footnotes(&doc.objects[0]), expected);
            assert_eq!(doc.styles.objects, styles);
            assert_eq!(doc.frame_footnote_defaults, defaults);
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let spread = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Spreads/"))
                .unwrap();
            let spread = xml::parse(package.text(spread).unwrap()).unwrap();
            let prefs = spread.find("TextFrameFootnoteOptionsObject").unwrap();
            assert_eq!(
                prefs.attr("EnableOverrides"),
                Some(if enabled { "true" } else { "false" })
            );
            assert_eq!(prefs.number("MinimumSpacingOption"), Some(17.25));
            assert_eq!(prefs.number("SpaceBetweenFootnotes"), Some(8.5));
            doc = import::read(&written.bytes).unwrap().document;
        }
        // New-model snapshots remain compatible with frames saved before this field existed.
        let mut value = serde_json::to_value(&doc.objects[0].object).unwrap();
        value["TextFrame"]
            .as_object_mut()
            .unwrap()
            .remove("footnotes");
        let old: LayoutObject = serde_json::from_value(value).unwrap();
        let LayoutObject::TextFrame { footnotes, .. } = old else {
            panic!()
        };
        assert!(footnotes.is_empty());
    }
}
