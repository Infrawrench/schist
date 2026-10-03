#[path = "../../separation/examples/support/named_initials.rs"]
mod proof;
use schist_codec_idml::{container, export, import};
use schist_layout::{compose, StoryId};
#[test]
fn native_saves_preserve_named_initial_rules_without_projected_ranges_or_aliases() {
    proof::register_font();
    for case in 0..proof::CASES {
        let mut doc = proof::document(false, case);
        let source = doc.stories[0].clone();
        let rule = doc
            .styles
            .paragraph("Source")
            .unwrap()
            .nested_styles
            .clone();
        for _ in 0..4 {
            let before = doc.clone();
            let thread = compose::compose_story(&doc, StoryId(0));
            assert!(!thread.has_overflow(), "case {case}");
            let rendered: Vec<_> = thread
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            assert_eq!(doc, before);
            let written = export::write(&doc);
            assert!(!written
                .warnings
                .iter()
                .any(|s| s.contains("AllNestedStyles")));
            let package = container::read(&written.bytes).unwrap();
            assert!(!package
                .text("Resources/Styles.xml")
                .unwrap()
                .contains("Schist generated initial"));
            let fonts = package.text("Resources/Fonts.xml").unwrap();
            assert!(fonts.contains("IBM Plex Sans") && fonts.contains("Light"));
            let imported = import::read(&written.bytes).unwrap();
            assert!(!imported
                .report
                .skipped
                .iter()
                .any(|s| s.contains("AllNestedStyles")));
            doc = imported.document;
            assert_eq!(doc.styles.paragraph("Source").unwrap().nested_styles, rule);
            assert_eq!(doc.stories[0].text(), source.text());
            assert_eq!(doc.stories[0].ranges, source.ranges);
            for (a, b) in doc.stories[0].structures.iter().zip(&source.structures) {
                let (Some(a), Some(b)) = (&a.footnote, &b.footnote) else {
                    continue;
                };
                assert_eq!(a.story.text(), b.story.text());
                assert_eq!(a.story.ranges, b.story.ranges);
            }
            let restored = compose::compose_story(&doc, StoryId(0));
            let restored: Vec<_> = restored
                .frames
                .iter()
                .flat_map(|f| f.all_lines())
                .map(|line| {
                    (
                        compose::line_spec(line, &doc.stories[0], &doc),
                        compose::line_paint_styles(line, &doc.stories[0], &doc),
                    )
                })
                .collect();
            assert_eq!(restored, rendered, "case {case}");
        }
    }
}

#[test]
fn native_counts_enable_one_line_named_initials_without_changing_explicit_or_dormant_counts() {
    use schist_codec_idml::xml;
    proof::register_font();
    for named in [false, true] {
        for lines in [0, 1, 3] {
            for count in [None, Some(0), Some(2)] {
                let mut doc = proof::document(false, 7);
                let style = doc
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|s| s.name == "Source")
                    .unwrap();
                style.drop_caps_lines = Some(lines);
                style.drop_caps_characters = count;
                if !named {
                    style.nested_styles = None;
                }
                let before = doc.clone();
                let written = export::write(&doc);
                assert_eq!(doc, before);
                let package = container::read(&written.bytes).unwrap();
                let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                let styles = root.find_all("ParagraphStyle");
                let native = styles
                    .iter()
                    .find(|s| s.attr("Name") == Some("Source"))
                    .unwrap();
                let expected = count
                    .or_else(|| (lines > 1 || (lines == 1 && named)).then_some(1))
                    .map(|v| v.to_string());
                assert_eq!(
                    native.attr("DropCapCharacters"),
                    expected.as_deref(),
                    "{named}/{lines}/{count:?}"
                );
            }
        }
    }
}

#[test]
fn native_implicit_initial_counts_preserve_descendant_inheritance_after_parent_edits() {
    use schist_codec_idml::xml;
    use schist_layout::nested_styles::{CharacterStyle, Delimiter, NestedStyle};
    use schist_layout::ParagraphStyle;
    for named in [false, true] {
        for depth in [1, 2, 5] {
            for reverse in [false, true] {
                let mut doc = schist_layout::blank_a4();
                doc.styles.add_character(schist_layout::CharacterStyle {
                    name: "Initial face".into(),
                    ..Default::default()
                });
                for index in 0..=depth {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: format!("Initial {index}"),
                        based_on: (index > 0).then(|| format!("Initial {}", index - 1)),
                        drop_caps_lines: (index == 0).then_some(if named { 1 } else { 3 }),
                        nested_styles: (index == 0 && named).then(|| {
                            vec![NestedStyle {
                                character_style: CharacterStyle::Named("Initial face".into()),
                                delimiter: Delimiter::Enumeration("Dropcap".into()),
                                repetition: 1,
                                inclusive: true,
                            }]
                        }),
                        ..Default::default()
                    });
                }
                if reverse {
                    doc.styles.paragraphs.reverse();
                }
                for count in [1, 0, 2, 4] {
                    let before = doc.clone();
                    let written = export::write(&doc);
                    assert_eq!(doc, before);
                    let package = container::read(&written.bytes).unwrap();
                    let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                    for index in 0..=depth {
                        let name = format!("Initial {index}");
                        let style = root
                            .find_all("ParagraphStyle")
                            .into_iter()
                            .find(|s| s.attr("Name") == Some(name.as_str()))
                            .unwrap();
                        let expected = (index == 0).then(|| count.to_string());
                        assert_eq!(
                            style.attr("DropCapCharacters"),
                            expected.as_deref(),
                            "{named}/{depth}/{reverse}/{index}/{count}"
                        );
                    }
                    doc = import::read(&written.bytes).unwrap().document;
                    // A parent edit after reopening must still reach every child.
                    let next = match count {
                        1 => 0,
                        0 => 2,
                        2 => 4,
                        _ => 3,
                    };
                    doc.styles
                        .paragraphs
                        .iter_mut()
                        .find(|style| style.name == "Initial 0")
                        .unwrap()
                        .drop_caps_characters = Some(next);
                    for index in 1..=depth {
                        let name = format!("Initial {index}");
                        assert_eq!(
                            doc.styles.paragraph(&name).unwrap().drop_caps_characters,
                            None
                        );
                        assert_eq!(
                            doc.styles.resolve_paragraph(&name).drop_caps_characters,
                            Some(next)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn native_initial_defaults_start_at_activation_and_preserve_dormant_and_explicit_counts() {
    use schist_codec_idml::xml;
    use schist_layout::ParagraphStyle;
    for ancestor in [None, Some(0), Some(2)] {
        for local in [None, Some(0), Some(3)] {
            for dormant in [None, Some(0), Some(4)] {
                let mut doc = schist_layout::blank_a4();
                for (name, parent, lines, count) in [
                    ("Dormant root", None, Some(0), ancestor),
                    ("Active", Some("Dormant root"), Some(3), local),
                    ("Dormant child", Some("Active"), Some(0), dormant),
                    ("Active child", Some("Dormant child"), Some(3), None),
                    ("Dormant sibling", Some("Dormant root"), None, None),
                ] {
                    doc.styles.add_paragraph(ParagraphStyle {
                        name: name.into(),
                        based_on: parent.map(str::to_owned),
                        drop_caps_lines: lines,
                        drop_caps_characters: count,
                        ..Default::default()
                    });
                }
                let lowered = local.or_else(|| ancestor.is_none().then_some(1));
                for _ in 0..3 {
                    let before = doc.clone();
                    let written = export::write(&doc);
                    assert_eq!(doc, before);
                    let package = container::read(&written.bytes).unwrap();
                    let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
                    for (name, expected) in [
                        ("Dormant root", ancestor),
                        ("Active", lowered),
                        ("Dormant child", dormant),
                        ("Active child", None),
                        ("Dormant sibling", None),
                    ] {
                        let native = root
                            .find_all("ParagraphStyle")
                            .into_iter()
                            .find(|s| s.attr("Name") == Some(name))
                            .unwrap();
                        assert_eq!(
                            native.attr("DropCapCharacters"),
                            expected.map(|v| v.to_string()).as_deref(),
                            "{ancestor:?}/{local:?}/{dormant:?}/{name}"
                        );
                    }
                    doc = import::read(&written.bytes).unwrap().document;
                    assert_eq!(
                        doc.styles
                            .resolve_paragraph("Active child")
                            .drop_caps_characters,
                        dormant.or(local).or(ancestor).or(Some(1))
                    );
                    assert_eq!(
                        doc.styles
                            .resolve_paragraph("Dormant sibling")
                            .drop_caps_characters,
                        ancestor
                    );
                }
            }
        }
    }
}

#[test]
fn native_initial_defaults_do_not_depend_on_another_member_of_a_broken_chain() {
    use schist_codec_idml::xml;
    use schist_layout::ParagraphStyle;
    for cycle in [false, true] {
        for reverse in [false, true] {
            let mut doc = schist_layout::blank_a4();
            for (name, parent, lines) in [
                ("A", "B", Some(3)),
                ("B", if cycle { "A" } else { "Missing" }, Some(3)),
                ("Descendant", "A", None),
            ] {
                doc.styles.add_paragraph(ParagraphStyle {
                    name: name.into(),
                    based_on: Some(parent.into()),
                    drop_caps_lines: lines,
                    ..Default::default()
                });
            }
            if reverse {
                doc.styles.paragraphs.reverse();
            }
            let before = doc.clone();
            let written = export::write(&doc);
            assert_eq!(doc, before);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
            for name in ["A", "B", "Descendant"] {
                let native = root
                    .find_all("ParagraphStyle")
                    .into_iter()
                    .find(|s| s.attr("Name") == Some(name))
                    .unwrap();
                assert_eq!(native.attr("DropCapCharacters"), Some("1"));
            }
        }
    }
}
