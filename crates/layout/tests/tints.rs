use schist_layout::{
    authoring::{self, Paint},
    compose::spec_for,
    properties::{set_object_property, ObjectProperty},
    CharacterStyle, History, Ink, LayoutDocument, LayoutObject, Page, PaintTints, ParagraphStyle,
    Rect, Story,
};

#[test]
fn tint_commits_are_atomic_for_any_selection_size_and_reject_invalid_values() {
    for count in 1..8 {
        for property in [ObjectProperty::FillTint, ObjectProperty::StrokeTint] {
            let mut doc = LayoutDocument::new(vec![Page::a4()]);
            let ids: Vec<_> = (0..count)
                .map(|_| {
                    authoring::rectangle(
                        &mut doc,
                        &mut History::default(),
                        0,
                        Rect::new(10.0, 10.0, 20.0, 20.0),
                        Paint::filled("Black"),
                    )
                    .unwrap()
                })
                .collect();
            let original = doc.clone();
            let mut history = History::default();
            for invalid in [-1.0, 100.01, f32::NAN, f32::INFINITY] {
                assert!(!set_object_property(
                    &mut doc,
                    &mut history,
                    &ids,
                    property,
                    invalid
                ));
                assert_eq!(history.undo_depth(), 0);
                assert_eq!(doc, original);
            }
            for percent in [0.0, 12.5, 75.0] {
                assert!(set_object_property(
                    &mut doc,
                    &mut history,
                    &ids,
                    property,
                    percent
                ));
                assert_eq!(history.undo_depth(), 1);
                assert!(doc
                    .objects
                    .iter()
                    .all(|o| property.value(o) == Some(percent)));
                let edited = doc.clone();
                assert!(history.undo(&mut doc));
                assert_eq!(doc, original);
                assert!(history.redo(&mut doc));
                assert_eq!(doc, edited);
                assert!(history.undo(&mut doc));
            }
            doc.objects.last_mut().unwrap().locked = true;
            let locked = doc.clone();
            assert!(!set_object_property(
                &mut doc,
                &mut history,
                &ids,
                property,
                25.0
            ));
            assert_eq!(doc, locked);
        }
    }
}

#[test]
fn tint_inheritance_is_independent_of_ink_and_opacity_in_every_text_run() {
    for tint in [0.0, 0.125, 0.5, 1.0] {
        let mut doc = schist_layout::blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Base".into(),
            fill: Some(Ink::black()),
            fill_tint: Some(tint),
            stroke_tint: Some(0.75),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Body".into(),
            based_on: Some("Base".into()),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Base character".into(),
            fill_tint: Some(0.25),
            stroke_tint: Some(0.0),
            opacity: Some(0.5),
            ..Default::default()
        });
        doc.styles.add_character(CharacterStyle {
            name: "Child character".into(),
            based_on: Some("Base character".into()),
            fill: Some(Ink::process("Red", [1.0, 0.0, 0.0])),
            ..Default::default()
        });
        let mut story = Story::from_text("AB", "Body");
        story.apply_style(1, 2, "Child character");
        let spec = spec_for(&story, 0, 2, &doc.styles, "Body", "Default", 200.0);
        let gray = ((1.0 - tint) * 255.0).round() as u8;
        assert_eq!(spec.style_at(0).color, Some([gray, gray, gray, 255]));
        assert_eq!(spec.style_at(1).color, Some([255, 191, 191, 128]));
        assert_eq!(doc.styles.resolve_paragraph("Body").stroke_tint, Some(0.75));
        assert_eq!(
            doc.styles.resolve_character("Child character").stroke_tint,
            Some(0.0)
        );
    }
}

#[test]
fn tint_preview_retains_device_channels_and_legacy_shapes_default_to_full_ink() {
    let ink = Ink::cmyk("Rich", [0.6, 0.4, 0.3, 0.8]);
    for tint in [0.0, 0.25, 0.5, 1.0] {
        let rgb = ink.preview_at_tint(tint);
        for (actual, channel) in rgb.into_iter().zip([0.6, 0.4, 0.3]) {
            assert!((actual - (1.0 - channel * tint) * (1.0 - 0.8 * tint)).abs() < 1e-6);
        }
    }
    let mut doc = schist_layout::blank_a4();
    authoring::rectangle(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(1.0, 2.0, 30.0, 40.0),
        Paint::filled("Black"),
    )
    .unwrap();
    let mut old = serde_json::to_value(&doc.objects[0].object).unwrap();
    old["Shape"].as_object_mut().unwrap().remove("tints");
    let LayoutObject::Shape { tints, .. } = serde_json::from_value(old).unwrap() else {
        panic!()
    };
    assert_eq!(tints, PaintTints::default());
}

#[test]
fn pasteboard_keeps_fill_stroke_tints_and_opacity_independent() {
    for tint in [0.0, 0.25, 0.5, 1.0] {
        let mut doc = schist_layout::blank_a4();
        authoring::rectangle(
            &mut doc,
            &mut History::default(),
            0,
            Rect::new(10.0, 10.0, 30.0, 30.0),
            Paint::filled("Black"),
        )
        .unwrap();
        let object = &mut doc.objects[0];
        object.transparency = 0.375;
        let LayoutObject::Shape {
            stroke,
            stroke_width,
            tints,
            ..
        } = &mut object.object
        else {
            panic!()
        };
        *stroke = Some(Ink::black());
        *stroke_width = 3.0;
        *tints = PaintTints {
            fill: tint,
            stroke: 1.0 - tint,
        };
        let board = schist_layout::pasteboard(&doc, &Default::default()).unwrap();
        let shape = board
            .objects()
            .find(|d| matches!(d, schist_layout::Display::Shape { .. }))
            .unwrap();
        let schist_layout::Display::Shape { fill, stroke, .. } = shape else {
            panic!()
        };
        assert_eq!(*fill, Some([1.0 - tint, 1.0 - tint, 1.0 - tint, 0.375]));
        assert_eq!(*stroke, Some(([tint, tint, tint, 0.375], 3.0)));
    }
}
