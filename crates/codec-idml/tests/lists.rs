use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, compose,
    lists::{
        BulletSymbol, ListKind, ListStyle, ListTab, MarkerAlignment, NumberingFormat,
        NumberingList, RestartPolicy,
    },
    History, ParagraphStyle, Rect, Story, StoryPoint,
};

fn markers(
    doc: &schist_layout::LayoutDocument,
    story: schist_layout::StoryId,
) -> Vec<(usize, String)> {
    compose::compose_story(doc, story)
        .lines()
        .filter_map(|l| l.generated.as_ref().map(|g| (l.start, g.spec.text.clone())))
        .collect()
}

#[test]
fn native_list_properties_resources_and_local_overrides_survive_repeated_saves() {
    let fixture = include_bytes!("../../../fixtures/idml/multipage.idml");
    let mut doc = import::read(fixture).unwrap().document;
    assert!(doc
        .styles
        .numbering_lists
        .iter()
        .any(|l| l.id == "NumberingList/$ID/[Default]" && !l.across_stories));
    let expected = doc.styles.clone();
    let stories = doc.stories.clone();
    let lists = doc
        .stories
        .iter()
        .flat_map(|story| &story.points)
        .filter_map(|p| match p {
            StoryPoint::Paragraph { style, .. } => Some(doc.styles.resolve_paragraph(style)),
            _ => None,
        })
        .filter(|p| matches!(p.list.kind, Some(ListKind::Bullet | ListKind::Numbered)))
        .collect::<Vec<_>>();
    assert!(lists.iter().any(
        |p| p.list.kind == Some(ListKind::Numbered) && p.list.continue_numbering == Some(false)
    ));
    assert!(lists.iter().any(|p| p.list.kind == Some(ListKind::Bullet)
        && p.list.bullet == Some(BulletSymbol::unicode('•'))));
    assert!(lists
        .iter()
        .all(|p| p.left_indent == Some(18.0) && p.first_line_indent == Some(-18.0)));
    for _ in 0..4 {
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        assert!(package
            .text("designmap.xml")
            .unwrap()
            .contains("<NumberingList"));
        let xml = package.text("Resources/Styles.xml").unwrap();
        assert!(xml.contains("BulletsAndNumberingListType=\"NumberedList\""));
        assert!(xml.contains("<BulletsFontStyle type=\"enumeration\">Nothing</BulletsFontStyle>"));
        let imported = import::read(&written.bytes).unwrap();
        assert!(imported.report.skipped.iter().any(|w| w.contains("36 pt")));
        doc = imported.document;
        assert_eq!(doc.stories, stories);
        assert_eq!(doc.styles.numbering_lists, expected.numbering_lists);
        for style in &expected.paragraphs {
            assert_eq!(doc.styles.paragraph(&style.name).unwrap().list, style.list);
        }
    }
}

#[test]
fn authored_lists_preserve_native_fields_and_generated_ink_without_changing_story_bytes() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for kind in [ListKind::Bullet, ListKind::Numbered] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.numbering_lists.push(NumberingList {
            id: "opaque sequence".into(),
            name: "Display & name".into(),
            labels: vec![("test".into(), "<keep>".into())],
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            family: Some("IBM Plex Sans".into()),
            point_size: Some(12.0),
            left_indent: Some(36.0),
            first_line_indent: Some(-18.0),
            list: ListStyle {
                kind: Some(kind),
                bullet: Some(BulletSymbol::unicode('→')),
                start: Some(9),
                level: Some(1),
                continue_numbering: Some(true),
                list: Some("opaque sequence".into()),
                format: Some("1, 2, 3, 4...".into()),
                expression: Some("Item ^#)^t".into()),
                text_after: Some(" ^t".into()),
                bullet_alignment: Some(MarkerAlignment::Right),
                numbering_alignment: Some(MarkerAlignment::Right),
                tabs: Some(vec![ListTab {
                    position: 36.0,
                    alignment: "LeftAlign".into(),
                    alignment_character: ".".into(),
                    leader: String::new(),
                }]),
                apply_restart_policy: Some(true),
                restart_policy: Some(RestartPolicy {
                    policy: "AnyPreviousLevel".into(),
                    lower: 0,
                    upper: 0,
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(70.0, 20.0, 300.0, 500.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("é first", "List");
        doc.stories[frame.story.0 as usize].push_paragraph("second", "List");
        let before = doc.stories.clone();
        let list = doc.styles.paragraph("List").unwrap().list.clone();
        let expected = markers(&doc, frame.story);
        assert_eq!(expected.len(), 2);
        let pixels = |doc: &schist_layout::LayoutDocument| {
            compose::compose_story(doc, frame.story)
                .lines()
                .filter(|l| l.generated.is_some())
                .map(|l| {
                    schist_text_engine::rasterize(&compose::line_spec(
                        l,
                        &doc.stories[frame.story.0 as usize],
                        doc,
                    ))
                    .unwrap()
                    .coverage
                })
                .collect::<Vec<_>>()
        };
        let expected_pixels = pixels(&doc);
        for _ in 0..4 {
            let written = export::write(&doc);
            assert!(
                !written
                    .warnings
                    .iter()
                    .any(|w| w.contains("list setting") || w.contains("36 pt")),
                "{:?}",
                written.warnings
            );
            let imported = import::read(&written.bytes).unwrap();
            assert!(imported.report.is_complete(), "{:?}", imported.report);
            doc = imported.document;
            assert_eq!(doc.stories, before);
            assert_eq!(doc.styles.paragraph("List").unwrap().list, list);
            assert_eq!(markers(&doc, frame.story), expected);
            assert_eq!(pixels(&doc), expected_pixels);
        }
    }
}

#[test]
fn unsupported_list_semantics_are_retained_and_diagnosed_in_both_directions() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.numbering_lists.push(NumberingList {
        id: "book".into(),
        across_stories: true,
        across_documents: true,
        ..Default::default()
    });
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Unsupported".into(),
        list: ListStyle {
            kind: Some(ListKind::Numbered),
            level: Some(10),
            list: Some("book".into()),
            expression: Some("^x.^#^t".into()),
            format: Some("custom counter".into()),
            tabs: Some(vec![ListTab {
                position: 40.0,
                alignment: "CharacterAlign".into(),
                alignment_character: ",".into(),
                leader: ".".into(),
            }]),
            ..Default::default()
        },
        ..Default::default()
    });
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 100.0, 100.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("text", "Unsupported");
    let expected = doc.styles.paragraph("Unsupported").unwrap().list.clone();
    let written = export::write(&doc);
    let imported = import::read(&written.bytes).unwrap();
    for property in [
        "NumberingLevel",
        "NumberingFormat",
        "NumberingExpression",
        "TabList",
        "ContinueNumbersAcrossStories",
        "ContinueNumbersAcrossDocuments",
    ] {
        assert!(
            written.warnings.iter().any(|w| w.contains(property)),
            "{property}"
        );
        assert!(
            imported.report.skipped.iter().any(|w| w.contains(property)),
            "{property}"
        );
    }
    assert_eq!(
        imported
            .document
            .styles
            .paragraph("Unsupported")
            .unwrap()
            .list,
        expected
    );
    assert!(markers(&imported.document, frame.story).is_empty());
}

#[test]
fn legacy_list_labels_are_guarded_by_the_native_definition() {
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let bullets = [
        schist_layout::styles::Bullet::None,
        schist_layout::styles::Bullet::Character {
            char: '•',
            indent: 6.25,
        },
        schist_layout::styles::Bullet::Numbered {
            start: 4,
            suffix: ')',
        },
    ]
    .into_iter()
    .chain(
        (33u8..=126)
            .map(char::from)
            .filter(char::is_ascii_punctuation)
            .chain(['é', '。', '—', '→'])
            .map(|suffix| schist_layout::styles::Bullet::Numbered { start: 4, suffix }),
    );
    for bullet in bullets {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Legacy".into(),
            family: Some("IBM Plex Sans".into()),
            bullet: Some(bullet),
            ..Default::default()
        });
        let before = doc.styles.paragraph("Legacy").unwrap().clone();
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(0.0, 0.0, 400.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("word", "Legacy");
        let expected = match bullet {
            schist_layout::styles::Bullet::None => vec![],
            schist_layout::styles::Bullet::Character { char, .. } => vec![(0, char.to_string())],
            schist_layout::styles::Bullet::Numbered { start, suffix } => {
                vec![(0, format!("{start}{suffix}"))]
            }
        };
        assert_eq!(markers(&doc, frame.story), expected, "{bullet:?}");
        for _ in 0..3 {
            doc = import::read(&export::write(&doc).bytes).unwrap().document;
            assert_eq!(doc.styles.paragraph("Legacy"), Some(&before));
            assert_eq!(markers(&doc, frame.story), expected, "{bullet:?}");
        }
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let xml = package
            .text("Resources/Styles.xml")
            .unwrap()
            .replace("Name=\"Legacy\"", "Name=\"Legacy\" NumberingLevel=\"2\"");
        package.insert("Resources/Styles.xml", xml.into_bytes());
        let doc = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        assert_eq!(doc.styles.paragraph("Legacy").unwrap().bullet, None);
        assert_eq!(doc.styles.paragraph("Legacy").unwrap().list.level, Some(2));
    }
}

#[test]
fn native_numbering_format_types_survive_inheritance_and_repeated_saves() {
    for value in [
        "Arabic",
        "UpperRoman",
        "LowerRoman",
        "UpperLetters",
        "LowerLetters",
        "Kanji",
        "KatakanaModern",
        "KatakanaTraditional",
        "FormatNone",
        "SingleLeadingZeros",
        "DoubleLeadingZeros",
        "TripleLeadingZeros",
        "ArabicAlifBaTah",
        "ArabicAbjad",
        "HebrewBiblical",
        "HebrewNonStandard",
        "1, 2, 3, 4...",
        "Custom & name",
    ] {
        let mut doc = schist_layout::blank_a4();
        for (name, format) in [
            ("Named", NumberingFormat::Named(value.into())),
            (
                "Enumeration",
                NumberingFormat::Enumeration {
                    enumeration: value.into(),
                },
            ),
        ] {
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.into(),
                list: ListStyle {
                    format: Some(format),
                    ..Default::default()
                },
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: format!("Child of {name}"),
                based_on: Some(name.into()),
                ..Default::default()
            });
        }
        let before = doc.styles.clone();
        for _ in 0..4 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let xml = package.text("Resources/Styles.xml").unwrap();
            for kind in ["string", "enumeration"] {
                let escaped = value.replace('&', "&amp;");
                assert!(xml.contains(&format!(
                    "<NumberingFormat type=\"{kind}\">{escaped}</NumberingFormat>"
                )));
            }
            doc = import::read(&written.bytes).unwrap().document;
            for style in &before.paragraphs {
                assert_eq!(doc.styles.paragraph(&style.name).unwrap().list, style.list);
                assert_eq!(
                    doc.styles.resolve_paragraph(&style.name).list,
                    before.resolve_paragraph(&style.name).list
                );
            }
        }
    }
}

#[test]
fn formatted_counter_text_and_native_types_survive_saves_without_touching_source_text() {
    use schist_layout::list_numbering::CounterFormat;
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for (kind, named, start, numbers) in [
        (
            CounterFormat::Decimal,
            "1, 2, 3, 4...",
            9,
            ["9", "10", "11"],
        ),
        (
            CounterFormat::UpperRoman,
            "I, II, III, IV...",
            9,
            ["IX", "X", "XI"],
        ),
        (
            CounterFormat::LowerRoman,
            "i, ii, iii, iv...",
            9,
            ["ix", "x", "xi"],
        ),
        (
            CounterFormat::UpperLetters,
            "A, B, C, D...",
            25,
            ["Y", "Z", "AA"],
        ),
        (
            CounterFormat::LowerLetters,
            "a, b, c, d...",
            25,
            ["y", "z", "aa"],
        ),
        (
            CounterFormat::SingleLeadingZeros,
            "01, 02, 03...",
            9,
            ["09", "10", "11"],
        ),
        (
            CounterFormat::DoubleLeadingZeros,
            "001, 002, 003...",
            99,
            ["099", "100", "101"],
        ),
        (
            CounterFormat::TripleLeadingZeros,
            "0001, 0002, 0003...",
            999,
            ["0999", "1000", "1001"],
        ),
        (CounterFormat::None, "FormatNone", 9, ["", "", ""]),
    ] {
        for format in [kind.native(), NumberingFormat::Named(format!(" {named} "))] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "List".into(),
                family: Some("IBM Plex Sans".into()),
                point_size: Some(12.0),
                left_indent: Some(90.0),
                first_line_indent: Some(-90.0),
                list: ListStyle {
                    kind: Some(ListKind::Numbered),
                    format: Some(format.clone()),
                    start: Some(start),
                    expression: Some("[^#]^t".into()),
                    ..Default::default()
                },
                ..Default::default()
            });
            let frame = authoring::text_frame(
                &mut doc,
                &mut History::default(),
                0,
                Rect::new(20.0, 20.0, 400.0, 500.0),
            )
            .unwrap();
            let mut story = Story::from_text("é", "List");
            story.push_paragraph("", "List");
            story.push_paragraph("last", "List");
            let expected = story
                .point_offsets()
                .into_iter()
                .zip(numbers.map(|n| format!("[{n}]")))
                .collect::<Vec<_>>();
            doc.stories[frame.story.0 as usize] = story.clone();
            for _ in 0..4 {
                assert_eq!(markers(&doc, frame.story), expected, "{format:?}");
                let written = export::write(&doc);
                assert!(!written
                    .warnings
                    .iter()
                    .any(|w| w.contains("NumberingFormat")));
                doc = import::read(&written.bytes).unwrap().document;
                assert_eq!(
                    doc.styles.paragraph("List").unwrap().list.format.as_ref(),
                    Some(&format)
                );
                assert_eq!(doc.stories[frame.story.0 as usize], story);
            }
        }
    }
}

#[test]
fn continued_roman_overflow_is_retained_and_diagnosed_on_import_and_export() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Roman".into(),
        list: ListStyle {
            kind: Some(ListKind::Numbered),
            format: Some(schist_layout::list_numbering::CounterFormat::UpperRoman.native()),
            start: Some(3999),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut story = Story::from_text("Before", "Roman");
    story.push_paragraph("After", "Roman");
    doc.add_story(story);
    let written = export::write(&doc);
    assert!(written
        .warnings
        .iter()
        .any(|w| w.contains("NumberingFormat.RomanRange")));
    let imported = import::read(&written.bytes).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|w| w.contains("NumberingFormat.RomanRange")));
    assert_eq!(
        imported.document.styles.paragraph("Roman"),
        doc.styles.paragraph("Roman")
    );
    assert_eq!(imported.document.stories, doc.stories);
}

#[test]
fn hierarchical_counters_preserve_references_formats_and_restart_policies_on_every_save() {
    use schist_layout::list_numbering::CounterFormat;
    schist_text_engine::add_font_data(
        include_bytes!("../../../web/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    for restart in [true, false] {
        let mut doc = schist_layout::blank_a4();
        for (name, level, format, expression) in [
            ("Parent", 1, CounterFormat::UpperRoman, "^#^t"),
            ("Child", 2, CounterFormat::LowerLetters, "^1.^#^t"),
            (
                "Grandchild",
                3,
                CounterFormat::SingleLeadingZeros,
                "^1.^2.^#^t",
            ),
        ] {
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.into(),
                family: Some("IBM Plex Sans".into()),
                point_size: Some(12.0),
                list: ListStyle {
                    kind: Some(ListKind::Numbered),
                    level: Some(level),
                    format: Some(format.native()),
                    expression: Some(expression.into()),
                    apply_restart_policy: Some(restart),
                    restart_policy: Some(RestartPolicy {
                        policy: "AnyPreviousLevel".into(),
                        lower: 0,
                        upper: 0,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 300.0, 500.0),
        )
        .unwrap();
        let story = Story {
            points: [
                "Parent",
                "Child",
                "Grandchild",
                "Parent",
                "Child",
                "Grandchild",
            ]
            .into_iter()
            .map(|style| StoryPoint::Paragraph {
                text: "é".into(),
                style: style.into(),
            })
            .collect(),
            ..Default::default()
        };
        doc.stories[frame.story.0 as usize] = story.clone();
        let expected = [
            "I",
            "I.a",
            "I.a.01",
            "II",
            if restart { "II.a" } else { "II.b" },
            if restart { "II.a.01" } else { "II.b.02" },
        ];
        let lists = ["Parent", "Child", "Grandchild"]
            .map(|n| doc.styles.paragraph(n).unwrap().list.clone());
        for _ in 0..4 {
            assert_eq!(
                markers(&doc, frame.story)
                    .into_iter()
                    .map(|(_, s)| s)
                    .collect::<Vec<_>>(),
                expected
            );
            let written = export::write(&doc);
            assert!(
                !written.warnings.iter().any(|w| w.contains("NumberingLevel")
                    || w.contains("NumberingExpression")
                    || w.contains("NumberingRestartPolicies"))
            );
            let read = import::read(&written.bytes).unwrap();
            assert!(!read
                .report
                .skipped
                .iter()
                .any(|w| w.contains("NumberingLevel")
                    || w.contains("NumberingExpression")
                    || w.contains("NumberingRestartPolicies")));
            doc = read.document;
            assert_eq!(doc.stories[frame.story.0 as usize], story);
            for (name, list) in ["Parent", "Child", "Grandchild"].into_iter().zip(&lists) {
                assert_eq!(&doc.styles.paragraph(name).unwrap().list, list);
            }
        }
    }
}

#[test]
fn unsupported_restart_policies_and_missing_ancestors_remain_visible_on_native_saves() {
    for policy in [
        None,
        Some("AfterSpecificLevel"),
        Some("RangeOfLevels"),
        Some("Unknown"),
    ] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "List".into(),
            list: ListStyle {
                kind: Some(ListKind::Numbered),
                level: Some(2),
                expression: Some("^1.^#^t".into()),
                restart_policy: policy.map(|policy| RestartPolicy {
                    policy: policy.into(),
                    lower: 1,
                    upper: 2,
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        let frame = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 300.0, 100.0),
        )
        .unwrap();
        doc.stories[frame.story.0 as usize] = Story::from_text("body still fits", "List");
        let expected = doc.styles.paragraph("List").unwrap().list.clone();
        let code = if policy.is_some() {
            "NumberingRestartPolicies"
        } else {
            "NumberingExpression.MissingLevel"
        };
        for _ in 0..3 {
            let written = export::write(&doc);
            assert!(written.warnings.iter().any(|w| w.contains(code)), "{code}");
            let read = import::read(&written.bytes).unwrap();
            assert!(
                read.report.skipped.iter().any(|w| w.contains(code)),
                "{code}"
            );
            doc = read.document;
            assert_eq!(doc.styles.paragraph("List").unwrap().list, expected);
        }
    }
}
