use schist_layout::{
    compose,
    nested_styles::{self, CharacterStyle as Applied, Delimiter, NestedStyle},
    CharacterStyle, ParagraphStyle, Story, StyleRange, StyleSet,
};
fn rule(name: Option<&str>, delimiter: Delimiter, repetition: i32, inclusive: bool) -> NestedStyle {
    NestedStyle {
        character_style: name.map_or(Applied::None, |name| Applied::Named(name.into())),
        delimiter,
        repetition,
        inclusive,
    }
}
fn character(name: Option<&str>) -> NestedStyle {
    rule(name, Delimiter::Enumeration("AnyCharacter".into()), 1, true)
}
fn repeat(count: i32) -> NestedStyle {
    rule(None, Delimiter::Enumeration("Repeat".into()), count, true)
}
fn styles(rules: Vec<NestedStyle>) -> StyleSet {
    let mut styles = StyleSet::default();
    for (index, name) in ["A", "B", "C", "D", "E"].into_iter().enumerate() {
        styles.add_character(CharacterStyle {
            name: name.into(),
            point_size: Some(15.0 + 3.0 * index as f32),
            tracking: Some(index as f32 * 17.0),
            ..Default::default()
        });
    }
    styles.add_paragraph(ParagraphStyle {
        name: "Source".into(),
        nested_styles: Some(rules),
        ..Default::default()
    });
    styles
}
fn matches_explicit(text: &str, styles: &StyleSet, ranges: Vec<StyleRange>) {
    let source = Story::from_text(text, "Source");
    let mut control = source.clone();
    control.ranges = ranges;
    let mut plain = styles.clone();
    plain
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
            &plain,
            "Source",
            "Default",
            120.0,
        );
        for (at, _) in actual.text.char_indices() {
            assert_eq!(
                actual.style_at(at),
                expected.style_at(at),
                "{text:?}/{start}/{at}"
            );
        }
    }
}
#[test]
fn repeat_cycles_only_the_requested_suffix_across_every_source_grapheme_and_slice() {
    let text = "É👩‍🔬אֵ漢abcde É👩‍🔬אֵ漢abcde";
    let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
    for length in 1..=5 {
        for count in 1..=length {
            for no_style in [false, true] {
                let names = [
                    Some("A"),
                    if no_style { None } else { Some("B") },
                    Some("C"),
                    Some("D"),
                    Some("E"),
                ];
                let mut rules: Vec<_> = names[..length].iter().copied().map(character).collect();
                rules.push(repeat(count as i32));
                let styles = styles(rules);
                assert_eq!(
                    nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    None
                );
                let ranges = boundaries
                    .windows(2)
                    .enumerate()
                    .filter_map(|(at, pair)| {
                        let index = if at < length {
                            at
                        } else {
                            length - count + (at - length) % count
                        };
                        names[index].map(|name| StyleRange::new(pair[0], pair[1], name))
                    })
                    .collect();
                matches_explicit(text, &styles, ranges);
            }
        }
    }
}
#[test]
fn a_zero_width_member_does_not_stop_a_cycle_whose_later_member_advances() {
    let text = "one|two|three|tail";
    let styles = styles(vec![
        rule(Some("A"), Delimiter::Text("|".into()), 1, false),
        rule(Some("B"), Delimiter::Text("|".into()), 1, true),
        repeat(2),
    ]);
    matches_explicit(
        text,
        &styles,
        vec![
            StyleRange::new(0, 3, "A"),
            StyleRange::new(3, 4, "B"),
            StyleRange::new(4, 7, "A"),
            StyleRange::new(7, 8, "B"),
            StyleRange::new(8, 13, "A"),
            StyleRange::new(13, 14, "B"),
            StyleRange::new(14, text.len(), "A"),
        ],
    );
    // At an immediate terminator, the A member itself consumes no source.
    matches_explicit(
        "||tail",
        &styles,
        vec![
            StyleRange::new(0, 1, "B"),
            StyleRange::new(1, 2, "B"),
            StyleRange::new(2, 6, "A"),
        ],
    );
}
#[test]
fn repeat_terminates_when_a_whole_cycle_cannot_advance_without_inventing_formatting() {
    for text in ["head|tail", "|tail", "", "no delimiter"] {
        let styles = styles(vec![
            rule(Some("A"), Delimiter::Text("|".into()), 1, false),
            repeat(1),
        ]);
        let end = text.find('|').unwrap_or(text.len());
        matches_explicit(text, &styles, vec![StyleRange::new(0, end, "A")]);
    }
    let styles = styles(vec![
        rule(
            Some("A"),
            Delimiter::Enumeration("AnyCharacter".into()),
            1,
            false,
        ),
        repeat(1),
    ]);
    matches_explicit("É👩‍🔬אֵ漢", &styles, Vec::new());
}
#[test]
fn valid_repeat_ignores_later_records_but_invalid_controls_leave_a_diagnosed_prefix() {
    let ignored = rule(
        Some("E"),
        Delimiter::Enumeration("FutureDelimiter".into()),
        1,
        true,
    );
    let styles = styles(vec![character(Some("A")), repeat(1), ignored.clone()]);
    assert_eq!(
        nested_styles::unsupported(&styles.resolve_paragraph("Source")),
        None
    );
    matches_explicit("tail", &styles, vec![StyleRange::new(0, 4, "A")]);
    for count in [i32::MIN, -1, 0, 2, i32::MAX] {
        let styles = self::styles(vec![
            character(Some("A")),
            repeat(count),
            character(Some("B")),
        ]);
        assert_eq!(
            nested_styles::unsupported(&styles.resolve_paragraph("Source")),
            Some("AllNestedStyles")
        );
        matches_explicit("tail", &styles, vec![StyleRange::new(0, 1, "A")]);
    }
    let styles = self::styles(vec![ignored, repeat(1), character(Some("A"))]);
    assert_eq!(
        nested_styles::unsupported(&styles.resolve_paragraph("Source")),
        Some("AllNestedStyles")
    );
    matches_explicit("tail", &styles, Vec::new());
}

#[test]
fn an_initial_may_precede_the_cycle_but_cannot_be_repeated_as_an_ordinary_rule() {
    let text = "E\u{301}👩‍🔬abcde";
    let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
    for lines in [0, 1, 3] {
        for count in [0, 1, 2, 5] {
            for inclusive in [false, true] {
                let mut control = repeat(1);
                control.inclusive = inclusive;
                let mut styles = styles(vec![
                    rule(Some("A"), Delimiter::Enumeration("Dropcap".into()), 1, true),
                    character(Some("B")),
                    control,
                ]);
                let paragraph = styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Source")
                    .unwrap();
                paragraph.drop_caps_lines = Some(lines);
                paragraph.drop_caps_characters = Some(count);
                let end = if lines == 0 { 0 } else { boundaries[count] };
                assert_eq!(
                    nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    None
                );
                matches_explicit(
                    text,
                    &styles,
                    vec![
                        StyleRange::new(0, end, "A"),
                        StyleRange::new(end, text.len(), "B"),
                    ],
                );
                styles
                    .paragraphs
                    .iter_mut()
                    .find(|p| p.name == "Source")
                    .unwrap()
                    .nested_styles
                    .as_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .repetition = 2;
                assert_eq!(
                    nested_styles::unsupported(&styles.resolve_paragraph("Source")),
                    Some("AllNestedStyles")
                );
                let after = boundaries
                    .iter()
                    .copied()
                    .find(|at| *at > end)
                    .unwrap_or(text.len());
                matches_explicit(
                    text,
                    &styles,
                    vec![
                        StyleRange::new(0, end, "A"),
                        StyleRange::new(end, after, "B"),
                    ],
                );
            }
        }
    }
}

#[test]
fn inherited_repeat_restarts_per_paragraph_and_recomputes_after_one_undoable_edit() {
    let mut doc = schist_layout::blank_a4();
    doc.styles = styles(vec![character(Some("A")), character(Some("B")), repeat(2)]);
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Child".into(),
        based_on: Some("Source".into()),
        ..Default::default()
    });
    let mut story = Story::from_text("", "Child");
    story.points = ["Éab", "👩‍🔬cd", "", "E\u{301}longer"]
        .into_iter()
        .map(|text| schist_layout::StoryPoint::Paragraph {
            text: text.into(),
            style: "Child".into(),
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
        "Q"
    ));
    assert_eq!(history.undo_depth(), 1);
    let edited = doc.clone();
    for state in [&before, &edited] {
        let story = state.story(id).unwrap();
        assert!(story.ranges.is_empty());
        assert_eq!(state.styles.paragraph("Child").unwrap().nested_styles, None);
        for (point, at) in story.points.iter().zip(story.point_offsets()) {
            let schist_layout::StoryPoint::Paragraph { text, .. } = point else {
                panic!()
            };
            let boundaries: Vec<_> = schist_text_engine::grapheme_boundaries(text).collect();
            for start in &boundaries {
                let spec = compose::spec_for(
                    story,
                    at + start,
                    at + text.len(),
                    &state.styles,
                    "Child",
                    "Default",
                    120.0,
                );
                for (byte, _) in spec.text.char_indices() {
                    let grapheme = boundaries.partition_point(|b| *b <= start + byte) - 1;
                    assert_eq!(
                        spec.style_at(byte).size,
                        if grapheme % 2 == 0 { 15.0 } else { 18.0 },
                        "{text:?}/{start}/{byte}"
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
