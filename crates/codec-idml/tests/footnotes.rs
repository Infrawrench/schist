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
        assert_eq!(read.document.stories[0].structures[0].at, None);
        assert_eq!(read.document.stories[0].structures[0].payload, payload);
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
