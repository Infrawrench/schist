use schist_layout::{
    compose,
    nested_styles::{CharacterStyle as NestedCharacter, Delimiter, NestedStyle},
    CharacterStyle, ParagraphStyle, Story, StyleRange, StyleSet,
};

fn rule(delimiter: Delimiter, repetition: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: NestedCharacter::Named("Nested".into()),
        delimiter,
        repetition,
        inclusive,
    }
}

fn styles(rules: Vec<NestedStyle>) -> StyleSet {
    let mut styles = StyleSet::default();
    styles.add_character(CharacterStyle {
        name: "Nested".into(),
        point_size: Some(19.0),
        tracking: Some(70.0),
        no_break: Some(true),
        ..Default::default()
    });
    styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        nested_styles: Some(rules),
        ..Default::default()
    });
    styles
}

fn matches_explicit_slices(text: &str, styles: &StyleSet, expected: Vec<StyleRange>) {
    let source = Story::from_text(text, "Source");
    let mut control = source.clone();
    control.ranges = expected;
    let mut control_styles = styles.clone();
    control_styles
        .paragraphs
        .iter_mut()
        .find(|p| p.name == "Source")
        .unwrap()
        .nested_styles = Some(Vec::new());
    for start in schist_text_engine::grapheme_boundaries(text) {
        let actual = compose::spec_for(
            &source,
            start,
            text.len(),
            styles,
            "Source",
            "Default",
            120.0,
        );
        let expected = compose::spec_for(
            &control,
            start,
            text.len(),
            &control_styles,
            "Source",
            "Default",
            120.0,
        );
        for (byte, _) in actual.text.char_indices() {
            assert_eq!(
                actual.style_at(byte),
                expected.style_at(byte),
                "{text:?}/{start}/{byte}"
            );
        }
    }
}

#[test]
fn nested_delimiters_match_authored_ranges_on_every_continuation_slice() {
    for (native, separator) in [
        (None, '|'),
        (Some("Tabs"), '\t'),
        (Some("ForcedLineBreak"), '\u{2028}'),
        (Some("EmSpace"), '\u{2003}'),
        (Some("EnSpace"), '\u{2002}'),
        (Some("NonbreakingSpace"), '\u{a0}'),
    ] {
        let text = format!("E\u{301}α{separator}body{separator}tail");
        for inclusive in [false, true] {
            for repetition in [1, 2, 9, i32::MAX] {
                let delimiter = native.map_or_else(
                    || Delimiter::Text(separator.to_string()),
                    |name| Delimiter::Enumeration(name.into()),
                );
                let styles = styles(vec![rule(delimiter, repetition, inclusive)]);
                let end = text
                    .match_indices(separator)
                    .nth(repetition as usize - 1)
                    .map_or(text.len(), |(at, value)| {
                        at + usize::from(inclusive) * value.len()
                    });
                matches_explicit_slices(&text, &styles, vec![StyleRange::new(0, end, "Nested")]);
                assert_eq!(
                    schist_layout::nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    None
                );
            }
        }
    }
}

#[test]
fn nested_character_counts_follow_source_graphemes_on_every_continuation_slice() {
    for text in ["E\u{301}abcdef", "👩‍🔬alpha", "שָלוֹם abc"] {
        let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
        for repetition in [1, 2, 4, i32::MAX] {
            for inclusive in [false, true] {
                let index = repetition as usize - usize::from(!inclusive);
                let end = boundaries.get(index).copied().unwrap_or(text.len());
                let styles = styles(vec![rule(
                    Delimiter::Enumeration("AnyCharacter".into()),
                    repetition,
                    inclusive,
                )]);
                let expected = if end == 0 {
                    Vec::new()
                } else {
                    vec![StyleRange::new(0, end, "Nested")]
                };
                matches_explicit_slices(text, &styles, expected);
            }
        }
    }
}

#[test]
fn ordered_no_style_segments_and_excluded_delimiters_control_the_next_start() {
    for inclusive in [false, true] {
        let mut skip = rule(Delimiter::Text("|".into()), 1, inclusive);
        skip.character_style = NestedCharacter::None;
        let styles = styles(vec![skip, rule(Delimiter::Text("|".into()), 1, true)]);
        // Up-to leaves the first bar to the next rule; through starts after it.
        let (start, end) = if inclusive { (5, 9) } else { (4, 5) };
        matches_explicit_slices(
            "head|mid|tail",
            &styles,
            vec![StyleRange::new(start, end, "Nested")],
        );
    }
    for inclusive in [false, true] {
        for (text, delimiter, count, through) in [
            ("aE\u{301}bc", "E", 1, 4),
            ("aE\u{301}bc", "\u{301}", 1, 4),
            ("aE\u{301}\u{301}bc", "\u{301}", 2, 6),
        ] {
            let styles = styles(vec![rule(
                Delimiter::Text(delimiter.into()),
                count,
                inclusive,
            )]);
            let end = if inclusive { through } else { 1 };
            matches_explicit_slices(text, &styles, vec![StyleRange::new(0, end, "Nested")]);
        }
    }
}

#[test]
fn unknown_or_invalid_bounds_cannot_silently_start_later_named_rules() {
    for delimiter in [
        Delimiter::Text(String::new()),
        Delimiter::Enumeration("Repeat".into()),
        Delimiter::Enumeration("Future".into()),
    ] {
        for repetition in [i32::MIN, 0, 1, i32::MAX] {
            let mut skip = rule(delimiter.clone(), repetition, true);
            skip.character_style = NestedCharacter::None;
            let styles = styles(vec![
                skip,
                rule(Delimiter::Enumeration("AnyCharacter".into()), 2, true),
            ]);
            assert!(
                schist_layout::nested_styles::unsupported(&styles.resolve_paragraph("Source"))
                    .is_some()
            );
            matches_explicit_slices("abcdef", &styles, Vec::new());
        }
    }
    for repetition in [i32::MIN, 0] {
        let styles = styles(vec![rule(Delimiter::Text("|".into()), repetition, true)]);
        assert!(
            schist_layout::nested_styles::unsupported(&styles.resolve_paragraph("Source"))
                .is_some()
        );
        matches_explicit_slices("abc|def", &styles, Vec::new());
    }
}

#[test]
fn nested_boundaries_restart_per_source_paragraph_and_recompute_after_one_undoable_edit() {
    let mut doc = schist_layout::blank_a4();
    doc.styles = styles(vec![rule(Delimiter::Text("|".into()), 1, false)]);
    let mut story = Story::from_text("", "Source");
    story.points = ["abc|def", "Éx|tail", "z|w"]
        .into_iter()
        .map(|text| schist_layout::StoryPoint::Paragraph {
            text: text.into(),
            style: "Source".into(),
        })
        .collect();
    let id = doc.add_story(story);
    let before = doc.clone();
    let mut history = schist_layout::History::default();
    assert!(schist_layout::authoring::replace_text(
        &mut doc,
        &mut history,
        id,
        0..0,
        "Q|"
    ));
    assert_eq!(history.undo_depth(), 1);
    let edited = doc.clone();
    for state in [&before, &edited] {
        let story = state.story(id).unwrap();
        assert!(story.ranges.is_empty());
        for (point, at) in story.points.iter().zip(story.point_offsets()) {
            let schist_layout::StoryPoint::Paragraph { text, .. } = point else {
                continue;
            };
            let end = text.find('|').unwrap();
            for start in schist_text_engine::grapheme_boundaries(text) {
                let spec = compose::spec_for(
                    story,
                    at + start,
                    at + text.len(),
                    &state.styles,
                    "Source",
                    "Default",
                    120.0,
                );
                for (byte, _) in spec.text.char_indices() {
                    assert_eq!(
                        spec.style_at(byte).size,
                        if start + byte < end { 19.0 } else { 11.0 }
                    );
                }
            }
        }
    }
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, edited);
}

#[test]
fn empty_source_nested_spans_stay_empty_after_reference_and_note_marker_insertion() {
    use schist_layout::{
        footnote_composition, footnotes::*, LayoutDocument, StoryId, StoryStructure,
    };
    for (text, delimiter, inclusive) in [
        ("|tail", Delimiter::Text("|".into()), false),
        ("", Delimiter::Enumeration("AnyCharacter".into()), true),
        ("", Delimiter::Text("|".into()), true),
    ] {
        for number in [1, 123] {
            for direct in [false, true] {
                let mut doc = LayoutDocument {
                    styles: styles(vec![rule(delimiter.clone(), 1, inclusive)]),
                    ..Default::default()
                };
                doc.footnotes.start_at = Some(number);
                doc.styles.add_character(CharacterStyle {
                    name: "Reference".into(),
                    ..Default::default()
                });
                let mut story = Story::from_text(text, "Source");
                story.structures.push(StoryStructure {
                    at: Some(0),
                    kind: "Footnote".into(),
                    payload: "test source".into(),
                    footnote: Some(FootnoteBody {
                        story: Story::from_text(text, "Source"),
                        markers: vec![FootnoteMarker {
                            at: 0,
                            character_style: "Reference".into(),
                        }],
                        reference_paragraph_style: "Source".into(),
                        reference_character_style: "Reference".into(),
                    }),
                });
                if direct && !text.is_empty() {
                    doc.styles.add_character(CharacterStyle {
                        name: "Authored".into(),
                        point_size: Some(23.0),
                        bold: Some(true),
                        ..Default::default()
                    });
                    story
                        .ranges
                        .push(StyleRange::new(0, text.len(), "Authored"));
                    story.structures[0].footnote.as_mut().unwrap().story.ranges =
                        story.ranges.clone();
                }
                doc.stories.push(story);
                let source = doc.clone();
                let mut control = doc.clone();
                control
                    .styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Source")
                    .unwrap()
                    .nested_styles = Some(Vec::new());
                let actual = footnote_composition::prepare(&doc, StoryId(0)).unwrap();
                let expected = footnote_composition::prepare(&control, StoryId(0)).unwrap();
                for (a, b) in [
                    (&actual.main, &expected.main),
                    (&actual.notes[0].body, &expected.notes[0].body),
                ] {
                    assert_eq!(a.story.text(), b.story.text());
                    let para = |story: &Story| match &story.points[0] {
                        schist_layout::StoryPoint::Paragraph { style, .. } => style.clone(),
                        _ => unreachable!(),
                    };
                    let a = compose::spec_for(
                        &a.story,
                        0,
                        a.story.text_len(),
                        &actual.styles,
                        &para(&a.story),
                        "Default",
                        120.0,
                    );
                    let b = compose::spec_for(
                        &b.story,
                        0,
                        b.story.text_len(),
                        &expected.styles,
                        &para(&b.story),
                        "Default",
                        120.0,
                    );
                    for (at, _) in a.text.char_indices() {
                        assert_eq!(a.style_at(at), b.style_at(at), "{text:?}/{number}/{at}");
                    }
                }
                assert_eq!(doc, source);
            }
        }
    }
}

#[test]
fn literal_delimiter_sets_match_any_member_independent_of_order_and_duplicates() {
    for (text, sets, matches) in [
        (
            "Éhead-foo:bar?tail",
            vec!["-:?", "?:-", "??::--"],
            vec![(6, 7), (10, 11), (14, 15)],
        ),
        ("xbyaz", vec!["ab", "ba", "aabba"], vec![(1, 2), (3, 4)]),
        (
            "E\u{301}:x👩‍🔬?tail",
            vec!["\u{301}?", "?\u{301}", "?\u{301}\u{301}?"],
            vec![(0, 3), (16, 17)],
        ),
        // A string which spells an enumeration remains a set of literals.
        ("ABcend", vec!["Dropcap", "parcDo"], vec![(2, 3)]),
    ] {
        for set in sets {
            for repetition in [1, 2, 3, 9, i32::MAX] {
                for inclusive in [false, true] {
                    let end =
                        matches
                            .get(repetition as usize - 1)
                            .map_or(
                                text.len(),
                                |(up, through)| if inclusive { *through } else { *up },
                            );
                    let styles = styles(vec![rule(
                        Delimiter::Text(set.into()),
                        repetition,
                        inclusive,
                    )]);
                    matches_explicit_slices(
                        text,
                        &styles,
                        if end == 0 {
                            Vec::new()
                        } else {
                            vec![StyleRange::new(0, end, "Nested")]
                        },
                    );
                    assert_eq!(
                        schist_layout::nested_styles::unsupported(
                            &styles.resolve_paragraph("Source")
                        ),
                        None
                    );
                }
            }
        }
    }
    for inclusive in [false, true] {
        let mut skip = rule(Delimiter::Text("?:-".into()), 2, inclusive);
        skip.character_style = NestedCharacter::None;
        let styles = styles(vec![skip, rule(Delimiter::Text("?:-".into()), 1, true)]);
        let (start, end) = if inclusive { (11, 15) } else { (10, 11) };
        matches_explicit_slices(
            "Éhead-foo:bar?tail",
            &styles,
            vec![StyleRange::new(start, end, "Nested")],
        );
    }
}

#[test]
fn digit_rules_count_only_ascii_digits_and_keep_whole_graphemes() {
    for (text, matches) in [
        ("٢x１y7\u{fe0f}\u{20e3}z9q", vec![(7, 14), (15, 16)]),
        ("٢１७᠘tail", Vec::new()),
    ] {
        for repetition in [1, 2, 3, i32::MAX] {
            for inclusive in [false, true] {
                let end =
                    matches
                        .get(repetition as usize - 1)
                        .map_or(
                            text.len(),
                            |(up, through)| if inclusive { *through } else { *up },
                        );
                let styles = styles(vec![rule(
                    Delimiter::Enumeration("Digits".into()),
                    repetition,
                    inclusive,
                )]);
                matches_explicit_slices(text, &styles, vec![StyleRange::new(0, end, "Nested")]);
                assert_eq!(
                    schist_layout::nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    None
                );
            }
        }
    }
}

#[test]
fn word_rules_count_nonempty_whitespace_delimited_units_and_preserve_graphemes() {
    // Independently enumerated end delimiters. Leading and repeated whitespace
    // belongs to the span but does not create empty words.
    for (text, ends) in [
        ("  Éab  second tail", vec![(6, 7), (14, 15)]),
        ("E\u{301}x\tword\u{2028}last", vec![(4, 5), (9, 12)]),
        ("alpha,beta gamma! tail", vec![(10, 11), (17, 18)]),
        ("漢字\u{3000}次語 tail", vec![(6, 9), (15, 16)]),
        ("a \u{301}b tail", vec![(1, 4), (5, 6)]),
        ("   ", Vec::new()),
    ] {
        for count in [1, 2, 3, i32::MAX] {
            for inclusive in [false, true] {
                let end = ends
                    .get(count as usize - 1)
                    .map_or(
                        text.len(),
                        |(up, through)| {
                            if inclusive {
                                *through
                            } else {
                                *up
                            }
                        },
                    );
                let styles = styles(vec![rule(
                    Delimiter::Enumeration("AnyWord".into()),
                    count,
                    inclusive,
                )]);
                matches_explicit_slices(text, &styles, vec![StyleRange::new(0, end, "Nested")]);
                assert_eq!(
                    schist_layout::nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    None
                );
            }
        }
    }
}

#[test]
fn nonbreaking_spaces_keep_multiple_terms_inside_one_nested_word() {
    for separator in ['\u{a0}', '\u{2007}', '\u{202f}'] {
        let prefix = format!("red{separator}green{separator}blue");
        let text = format!("{prefix} next tail");
        for inclusive in [false, true] {
            let styles = styles(vec![rule(
                Delimiter::Enumeration("AnyWord".into()),
                1,
                inclusive,
            )]);
            matches_explicit_slices(
                &text,
                &styles,
                vec![StyleRange::new(
                    0,
                    prefix.len() + usize::from(inclusive),
                    "Nested",
                )],
            );
        }
    }
}

#[test]
fn ordered_word_rules_advance_past_excluded_and_repeated_leading_separators() {
    for inclusive in [false, true] {
        let mut skip = rule(Delimiter::Enumeration("AnyWord".into()), 1, inclusive);
        skip.character_style = NestedCharacter::None;
        let styles = styles(vec![
            skip,
            rule(Delimiter::Enumeration("AnyWord".into()), 1, true),
        ]);
        let start = if inclusive { 7 } else { 6 };
        matches_explicit_slices(
            "  Éab  second tail",
            &styles,
            vec![StyleRange::new(start, 15, "Nested")],
        );
    }
}
