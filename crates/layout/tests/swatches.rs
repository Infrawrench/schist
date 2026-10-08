use schist_layout::{
    authoring, swatches, CharacterStyle, History, Ink, LayoutObject, ParentObject, ParentPage, Rect,
};

#[test]
fn tint_and_base_edits_update_every_use_and_undo_exactly_once() {
    for count in [1, 3, 17] {
        let mut doc = schist_layout::blank_a4();
        let base = Ink::cmyk("Brand", [0.6, 0.4, 0.2, 0.8]);
        doc.inks.push(base.clone());
        let mut history = History::default();
        let tint_index = swatches::add_tint(&mut doc, &mut history, &base, 0.25).unwrap();
        let tint = doc.inks[tint_index].clone();
        assert_eq!(history.undo_depth(), 1);
        for _ in 0..count {
            authoring::rectangle(
                &mut doc,
                &mut history,
                0,
                Rect::new(5.0, 5.0, 30.0, 30.0),
                authoring::Paint::filled(&tint.name),
            )
            .unwrap();
        }
        doc.parents.push(ParentPage {
            name: "Parent".into(),
            applied_to: vec![0],
            based_on: None,
            sheets: vec![],
            placements: vec![],
            objects: vec![ParentObject {
                object: doc.objects[0].clone(),
                overridden_on: vec![],
            }],
            hidden: false,
        });
        doc.styles.paragraphs[0].fill = Some(tint.clone());
        for rule in [&mut doc.footnotes.rule, &mut doc.footnotes.continuing_rule] {
            rule.paint = Some(schist_layout::footnotes::FootnoteReference::Resolved(
                tint.clone(),
            ));
        }
        doc.footnotes.rule.gap_paint = Some(schist_layout::footnotes::FootnoteReference::Resolved(
            base.clone(),
        ));
        doc.footnotes.continuing_rule.gap_paint = Some(
            schist_layout::footnotes::FootnoteReference::Unresolved(base.name.clone()),
        );
        doc.styles.add_character(CharacterStyle {
            name: "Tint".into(),
            fill: Some(tint.clone()),
            stroke: Some(base.clone()),
            ..Default::default()
        });
        history.clear();
        let original = doc.clone();
        for value in [0.0, 0.125, 1.0] {
            let mut changed = tint.clone();
            changed.tint.as_mut().unwrap().value = value;
            assert!(swatches::replace(
                &mut doc,
                &mut history,
                &tint,
                changed.clone()
            ));
            assert_eq!(history.undo_depth(), 1);
            for object in doc.objects.iter().chain(
                doc.parents
                    .iter()
                    .flat_map(|p| p.objects.iter().map(|o| &o.object)),
            ) {
                assert!(
                    matches!(&object.object, LayoutObject::Shape {fill:Some(ink), ..} if *ink == changed)
                );
            }
            assert_eq!(doc.styles.paragraphs[0].fill.as_ref(), Some(&changed));
            for rule in [&doc.footnotes.rule, &doc.footnotes.continuing_rule] {
                assert_eq!(
                    rule.paint
                        .as_ref()
                        .and_then(schist_layout::footnotes::FootnoteReference::resolved),
                    Some(&changed)
                );
            }
            assert_eq!(
                doc.styles.character("Tint").unwrap().fill.as_ref(),
                Some(&changed)
            );
            let edited = doc.clone();
            assert!(history.undo(&mut doc));
            assert_eq!(doc, original);
            assert!(history.redo(&mut doc));
            assert_eq!(doc, edited);
            assert!(history.undo(&mut doc));
        }
        let after = Ink::cmyk("Brand", [0.1, 0.2, 0.3, 0.4]);
        assert!(swatches::replace(
            &mut doc,
            &mut history,
            &base,
            after.clone()
        ));
        assert_eq!(history.undo_depth(), 1);
        let expected = after.named_tint(&tint.name, 0.25).unwrap();
        assert_eq!(doc.inks[tint_index], expected);
        assert_eq!(doc.styles.paragraphs[0].fill.as_ref(), Some(&expected));
        for rule in [&doc.footnotes.rule, &doc.footnotes.continuing_rule] {
            assert_eq!(
                rule.paint
                    .as_ref()
                    .and_then(schist_layout::footnotes::FootnoteReference::resolved),
                Some(&expected)
            );
        }
        assert_eq!(
            doc.footnotes.rule.gap_paint,
            Some(schist_layout::footnotes::FootnoteReference::Resolved(after))
        );
        assert_eq!(
            doc.footnotes.continuing_rule.gap_paint,
            original.footnotes.continuing_rule.gap_paint
        );
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
    }
}

#[test]
fn tint_creation_preserves_base_identity_serialization_and_validates_fractions() {
    let mut doc = schist_layout::blank_a4();
    let base = doc.inks[0].clone();
    let original = doc.clone();
    let mut history = History::default();
    for invalid in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(swatches::add_tint(&mut doc, &mut history, &base, invalid).is_none());
        assert_eq!(doc, original);
        assert_eq!(history.undo_depth(), 0);
    }
    for value in [0.0, 0.5, 1.0] {
        let index = swatches::add_tint(&mut doc, &mut history, &base, value).unwrap();
        let tint = doc.inks[index].clone();
        assert_eq!(tint.base_color().as_ref(), &base);
        assert!(swatches::set_tint(&mut doc, &mut history, &tint, 0.75));
        assert!(doc.inks[index].name.contains("75%"));
        assert!(history.undo(&mut doc));
        assert_eq!(doc.inks[index], tint);
        let another = swatches::add_tint(&mut doc, &mut history, &tint, value).unwrap();
        assert_ne!(doc.inks[another].name, tint.name);
        assert_eq!(doc.inks[another].base_color().as_ref(), &base);
        assert_eq!(tint.preview_at_tint(0.5), base.preview_at_tint(value));
        assert_eq!(tint.to_cmyk(), base.to_cmyk().map(|v| v * value));
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            serde_json::from_value::<schist_layout::LayoutDocument>(json).unwrap(),
            doc
        );
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
    }
    let old = serde_json::to_value(base).unwrap();
    assert!(old.get("tint").is_none());
    assert!(serde_json::from_value::<Ink>(old).unwrap().tint.is_none());
}

#[test]
fn per_object_percentages_detach_named_tints_and_undo_the_whole_selection() {
    use schist_layout::properties::{self, ObjectProperty};
    for count in [1, 4, 19] {
        let mut doc = schist_layout::blank_a4();
        let base = doc.inks[0].clone();
        let tint = base.named_tint("Quarter", 0.25).unwrap();
        doc.inks.push(tint.clone());
        let mut history = History::default();
        let ids: Vec<_> = (0..count)
            .map(|_| {
                authoring::rectangle(
                    &mut doc,
                    &mut history,
                    0,
                    Rect::new(0.0, 0.0, 30.0, 30.0),
                    authoring::Paint::filled("Quarter"),
                )
                .unwrap()
            })
            .collect();
        history.clear();
        let original = doc.clone();
        for value in [0.0, 50.0, 100.0] {
            assert_eq!(ObjectProperty::FillTint.value(&doc.objects[0]), Some(25.0));
            assert!(properties::set_object_property(
                &mut doc,
                &mut history,
                &ids,
                ObjectProperty::FillTint,
                value
            ));
            assert_eq!(history.undo_depth(), 1);
            for object in &doc.objects {
                assert!(
                    matches!(&object.object, LayoutObject::Shape { fill:Some(ink), tints, ..} if ink == &base && tints.fill == value / 100.0)
                );
            }
            assert!(history.undo(&mut doc));
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn direct_tint_overrides_detach_only_inherited_named_paints() {
    use schist_layout::{ParagraphStyle, Story};
    let base = Ink::cmyk("Brand", [0.0, 0.0, 0.0, 1.0]);
    let named = base.named_tint("Quarter", 0.25).unwrap();
    for value in [0.0, 0.5, 1.0] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Named".into(),
            fill: Some(named.clone()),
            stroke: Some(named.clone()),
            fill_tint: Some(1.0), // An explicit named paint owns the percentage.
            stroke_tint: Some(1.0),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Direct".into(),
            based_on: Some("Named".into()),
            fill_tint: Some(value),
            stroke_tint: Some(value),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Named".into(),
            fill: Some(named.clone()),
            stroke: Some(named.clone()),
            fill_tint: Some(1.0),
            stroke_tint: Some(1.0),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Direct".into(),
            based_on: Some("Named".into()),
            fill_tint: Some(value),
            stroke_tint: Some(value),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Local".into(),
            fill_tint: Some(value),
            ..Default::default()
        });
        assert_eq!(
            doc.styles.resolve_paragraph("Named").fill,
            Some(named.clone())
        );
        assert_eq!(
            doc.styles.resolve_character("Named").fill,
            Some(named.clone())
        );
        for ink in [
            doc.styles.resolve_paragraph("Direct").fill,
            doc.styles.resolve_paragraph("Direct").stroke,
            doc.styles.resolve_character("Direct").fill,
            doc.styles.resolve_character("Direct").stroke,
        ] {
            assert_eq!(ink, Some(base.clone()));
        }
        let mut story = Story::from_text("plain local", "Named");
        story.ranges.push(schist_layout::story::StyleRange {
            start: 6,
            end: 11,
            style: "Local".into(),
        });
        let spec =
            schist_layout::compose::spec_for(&story, 0, 11, &doc.styles, "Named", "Default", 400.0);
        assert_eq!(spec.style_at(0).color, Some([191, 191, 191, 255]));
        let grey = ((1.0 - value) * 255.0).round() as u8;
        assert_eq!(spec.style_at(7).color, Some([grey, grey, grey, 255]));
    }
}
