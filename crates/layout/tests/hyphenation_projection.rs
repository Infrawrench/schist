use schist_layout::{
    hyphenation::BreakPlan,
    inline_text::{Insertion, Projection},
    language::TextLanguage,
    CharacterStyle, ParagraphStyle, Story, StoryPoint, StyleRange, StyleSet,
};

fn styles() -> StyleSet {
    let mut styles = StyleSet::default();
    styles.add_paragraph(ParagraphStyle {
        name: "P".into(),
        language: Some(TextLanguage::Tag {
            tag: "en-US".into(),
        }),
        ..Default::default()
    });
    styles.add_character(CharacterStyle {
        name: "Marker".into(),
        language: Some(TextLanguage::Tag {
            tag: "en-GB".into(),
        }),
        no_break: Some(true),
        ..Default::default()
    });
    styles
}

#[test]
fn generated_text_never_changes_source_word_eligibility_or_dictionary() {
    let styles = styles();
    for text in [
        "extensive",
        "a extensive!",
        "ex",
        "extensive probability",
        "extensive\nprobability",
    ] {
        let source = Story::from_text(text, "P");
        let saved = source.clone();
        let expected = BreakPlan::new(&source, &styles, "P", "").slice(0..source.text_len());
        for anchor in text.char_indices().map(|(at, _)| at).chain([text.len()]) {
            for marker in ["1", "99", "extensive", " β "] {
                let projection = Projection::new(
                    &source,
                    vec![Insertion {
                        at: anchor,
                        text: marker.into(),
                        style: "Marker".into(),
                    }],
                )
                .unwrap();
                let plan = BreakPlan::projected(&source, &projection, &styles, "P", "").unwrap();
                let actual = plan
                    .slice(0..projection.story.text_len())
                    .into_iter()
                    .map(|at| projection.positions.source(at))
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    expected
                        .iter()
                        .copied()
                        .filter(|at| *at != anchor)
                        .collect::<Vec<_>>(),
                    "{text:?} {anchor} {marker}"
                );
            }
        }
        assert_eq!(source, saved);
    }
}

#[test]
fn projected_paragraph_aliases_apply_policy_to_original_words_and_character_ranges() {
    let mut styles = styles();
    let mut alias = ParagraphStyle {
        name: "Alias".into(),
        based_on: Some("P".into()),
        ..Default::default()
    };
    alias.hyphenation.last_word = Some(false);
    styles.add_paragraph(alias);
    let mut source = Story::from_text("extensive extensive extensive", "P");
    source.ranges.push(StyleRange::new(0, 9, "Marker"));
    let mut projected = Projection::new(
        &source,
        vec![Insertion {
            at: source.text_len(),
            text: " extensive".into(),
            style: "".into(),
        }],
    )
    .unwrap();
    if let StoryPoint::Paragraph { style, .. } = &mut projected.story.points[0] {
        *style = "Alias".into();
    }
    let plan = BreakPlan::projected(&source, &projected, &styles, "P", "").unwrap();
    assert_eq!(plan.slice(0..projected.story.text_len()), vec![12, 15]);
    assert_eq!(source.ranges[0], StyleRange::new(0, 9, "Marker"));
}

#[test]
fn projected_slices_keep_original_candidates_and_exclude_both_edges() {
    let styles = styles();
    let source = Story::from_text("extensive probability", "P");
    let projection = Projection::new(
        &source,
        vec![Insertion {
            at: 3,
            text: "①".into(),
            style: "Marker".into(),
        }],
    )
    .unwrap();
    let plan = BreakPlan::projected(&source, &projection, &styles, "P", "").unwrap();
    let text = projection.story.text();
    let all = plan.slice(0..text.len());
    assert!(!all.is_empty());
    for start in 0..=text.len() {
        for end in 0..=text.len() {
            assert_eq!(
                plan.slice(start..end),
                all.iter()
                    .copied()
                    .filter(|at| start < *at && *at < end)
                    .map(|at| at - start)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn mismatched_source_or_point_topology_cannot_supply_a_projected_plan() {
    let styles = styles();
    let source = Story::from_text("extensive", "P");
    let mut projection = Projection::new(&source, vec![]).unwrap();
    assert!(BreakPlan::projected(
        &Story::from_text("probability", "P"),
        &projection,
        &styles,
        "P",
        ""
    )
    .is_none());
    projection.story.points.push(StoryPoint::FrameBreak);
    assert!(BreakPlan::projected(&source, &projection, &styles, "P", "").is_none());
}
