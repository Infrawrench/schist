use schist_layout::{
    nested_styles::{self, CharacterStyle, Delimiter, NestedStyle},
    ParagraphStyle,
};

fn rule() -> NestedStyle {
    NestedStyle {
        character_style: CharacterStyle::Named("Initial".into()),
        delimiter: Delimiter::Enumeration("Dropcap".into()),
        repetition: 1,
        inclusive: true,
    }
}

#[test]
fn nested_lists_inherit_as_whole_ordered_values_and_empty_resets_undo_once() {
    for local in [None, Some(Vec::new()), Some(vec![rule(), rule()])] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            nested_styles: Some(vec![rule()]),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Base".into()),
            nested_styles: local.clone(),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Leaf".into(),
            based_on: Some("Child".into()),
            ..Default::default()
        });
        let expected = local.or_else(|| Some(vec![rule()]));
        assert_eq!(doc.styles.resolve_paragraph("Leaf").nested_styles, expected);
        let before = doc.clone();
        let mut history = schist_layout::History::default();
        assert!(schist_layout::properties::edit_styles(
            &mut doc,
            &mut history,
            |styles| {
                styles
                    .paragraphs
                    .iter_mut()
                    .find(|style| style.name == "Base")
                    .unwrap()
                    .nested_styles = Some(Vec::new());
            }
        ));
        assert_eq!(history.undo_depth(), 1);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert_eq!(
            nested_styles::unsupported(&doc.styles.resolve_paragraph("Leaf")).is_some(),
            !expected.unwrap().is_empty()
        );
    }
}

#[test]
fn snapshots_preserve_nested_rules_and_legacy_snapshots_do_not_invent_them() {
    let mut doc = schist_layout::blank_a4();
    let mut legacy = serde_json::to_value(&doc).unwrap();
    for style in legacy["styles"]["paragraphs"].as_array_mut().unwrap() {
        style.as_object_mut().unwrap().remove("nested_styles");
    }
    assert_eq!(
        serde_json::from_value::<schist_layout::LayoutDocument>(legacy).unwrap(),
        doc
    );
    doc.styles.paragraphs[0].nested_styles = Some(vec![rule()]);
    assert_eq!(
        serde_json::from_value::<schist_layout::LayoutDocument>(
            serde_json::to_value(&doc).unwrap()
        )
        .unwrap(),
        doc
    );
}

#[test]
fn character_style_rename_updates_every_typed_nested_reference_in_one_undo_step() {
    let mut doc = schist_layout::blank_a4();
    doc.styles.add_character(schist_layout::CharacterStyle {
        name: "Initial".into(),
        ..Default::default()
    });
    for name in ["First", "Second", "Third"] {
        let mut unresolved = rule();
        unresolved.character_style = CharacterStyle::Unresolved("Initial".into());
        doc.styles.add_paragraph(ParagraphStyle {
            name: name.into(),
            nested_styles: Some(vec![rule(), unresolved, rule()]),
            ..Default::default()
        });
    }
    let before = doc.clone();
    let mut history = schist_layout::History::default();
    assert!(schist_layout::properties::rename_style(
        &mut doc,
        &mut history,
        false,
        "Initial",
        "Renamed"
    ));
    assert_eq!(history.undo_depth(), 1);
    for name in ["First", "Second", "Third"] {
        let rules = doc
            .styles
            .paragraph(name)
            .unwrap()
            .nested_styles
            .as_ref()
            .unwrap();
        assert_eq!(
            rules[0].character_style,
            CharacterStyle::Named("Renamed".into())
        );
        assert_eq!(
            rules[1].character_style,
            CharacterStyle::Unresolved("Initial".into())
        );
        assert_eq!(
            rules[2].character_style,
            CharacterStyle::Named("Renamed".into())
        );
    }
    let after = doc.clone();
    assert!(history.undo(&mut doc));
    assert_eq!(doc, before);
    assert!(history.redo(&mut doc));
    assert_eq!(doc, after);
}

#[test]
fn no_style_rules_reset_inherited_paint_without_reporting_uncomposed_formatting() {
    for delimiter in [
        Delimiter::Text(": ".into()),
        Delimiter::Enumeration("Dropcap".into()),
        Delimiter::Enumeration("Repeat".into()),
        Delimiter::Enumeration("FutureDelimiter".into()),
    ] {
        for repetition in [i32::MIN, 0, 1, i32::MAX] {
            let mut doc = schist_layout::blank_a4();
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Base".into(),
                nested_styles: Some(vec![rule()]),
                ..Default::default()
            });
            doc.styles.add_paragraph(ParagraphStyle {
                name: "Child".into(),
                based_on: Some("Base".into()),
                nested_styles: Some(vec![NestedStyle {
                    character_style: CharacterStyle::None,
                    delimiter: delimiter.clone(),
                    repetition,
                    inclusive: true,
                }]),
                ..Default::default()
            });
            assert!(nested_styles::unsupported(&doc.styles.resolve_paragraph("Base")).is_some());
            assert_eq!(
                nested_styles::unsupported(&doc.styles.resolve_paragraph("Child")),
                None
            );
        }
    }
}
