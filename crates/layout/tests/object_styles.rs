use schist_layout::{authoring, object_styles, properties, *};

fn ink() -> Ink {
    Ink::cmyk("Frame cyan", [0.8, 0.0, 0.0, 0.0])
}
fn style(name: &str) -> ObjectStyle {
    ObjectStyle {
        name: name.into(),
        enable_fill: Some(true),
        enable_stroke: Some(true),
        paint: ObjectPaint {
            fill: Some(Paint::Ink(ink())),
            stroke: Some(Paint::Ink(Ink::black())),
            stroke_width: Some(6.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn object_paints_distinguish_inheritance_disabled_categories_and_explicit_none() {
    for depth in 1..7 {
        let mut doc = blank_a4();
        let mut history = History::default();
        let id =
            authoring::text_frame(&mut doc, &mut history, 0, Rect::new(10.0, 10.0, 80.0, 80.0))
                .unwrap()
                .object;
        doc.styles.objects.push(style("base"));
        for n in 0..depth {
            doc.styles.objects.push(ObjectStyle {
                name: format!("s{n}"),
                based_on: Some(if n == 0 {
                    "base".into()
                } else {
                    format!("s{}", n - 1)
                }),
                ..Default::default()
            });
        }
        let name = format!("s{}", depth - 1);
        doc.objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .appearance
            .style = Some(name.clone());
        assert_eq!(
            doc.styles.object_paint(doc.object(id).unwrap()).fill_ink(),
            Some(&ink())
        );
        doc.styles.objects.last_mut().unwrap().paint.fill = Some(Paint::None);
        assert!(doc
            .styles
            .object_paint(doc.object(id).unwrap())
            .fill_ink()
            .is_none());
        doc.styles.objects.last_mut().unwrap().paint.fill = None;
        doc.styles.objects.last_mut().unwrap().enable_fill = Some(false);
        assert!(doc
            .styles
            .object_paint(doc.object(id).unwrap())
            .fill_ink()
            .is_none());
        assert_eq!(
            doc.styles
                .object_paint(doc.object(id).unwrap())
                .stroke_width,
            Some(6.0)
        );
        doc.objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .appearance
            .paint
            .fill = Some(Paint::Ink(Ink::white()));
        assert_eq!(
            doc.styles.object_paint(doc.object(id).unwrap()).fill_ink(),
            Some(&Ink::white())
        );
        doc.styles.objects[0].based_on = Some(name);
        assert_eq!(
            doc.styles.object_paint(doc.object(id).unwrap()).fill_ink(),
            Some(&Ink::white()),
            "cycles terminate and nearest values win"
        );
    }
}

#[test]
fn style_apply_clear_detach_and_rename_each_undo_once_for_every_selection_size() {
    for count in 1..9 {
        let mut doc = blank_a4();
        doc.styles.objects.push(style("base"));
        let mut history = History::default();
        let ids: Vec<_> = (0..count)
            .map(|n| {
                if n % 2 == 0 {
                    authoring::text_frame(
                        &mut doc,
                        &mut history,
                        0,
                        Rect::new(10.0, 10.0, 80.0, 80.0),
                    )
                    .unwrap()
                    .object
                } else {
                    authoring::rectangle(
                        &mut doc,
                        &mut history,
                        0,
                        Rect::new(10.0, 10.0, 80.0, 80.0),
                        authoring::Paint::none(),
                    )
                    .unwrap()
                }
            })
            .collect();
        for action in 0..4 {
            let before = doc.clone();
            let depth = history.undo_depth();
            assert!(match action {
                0 => object_styles::apply_style(&mut doc, &mut history, &ids, Some("base")),
                1 => object_styles::edit_paint(
                    &mut doc,
                    &mut history,
                    &ids,
                    &ObjectPaint {
                        fill: Some(Paint::None),
                        ..Default::default()
                    }
                ),
                2 => object_styles::rename_style(&mut doc, &mut history, "base", "Renamed & 青"),
                _ => object_styles::apply_style(&mut doc, &mut history, &ids, None),
            });
            assert_eq!(history.undo_depth(), depth + 1);
            let after = doc.clone();
            history.undo(&mut doc);
            assert_eq!(doc, before);
            history.redo(&mut doc);
            assert_eq!(doc, after);
            if action == 1 {
                object_styles::apply_style(&mut doc, &mut history, &ids, Some("base"));
                for id in &ids {
                    assert_eq!(
                        doc.styles.object_paint(doc.object(*id).unwrap()).fill_ink(),
                        Some(&ink())
                    );
                }
            }
        }
        for id in &ids {
            assert_eq!(
                doc.styles.object_paint(doc.object(*id).unwrap()).fill_ink(),
                Some(&ink())
            );
        }
    }
}

#[test]
fn frame_paints_surround_content_and_strokes_contribute_across_the_gutter() {
    let mut doc = LayoutDocument::new(vec![
        Page::new("1", 100.0, 100.0),
        Page::new("2", 100.0, 100.0),
    ]);
    doc.spreads = vec![Spread {
        pages: vec![0, 1],
        binding_location: None,
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    let mut history = History::default();
    let frame = authoring::text_frame(&mut doc, &mut history, 0, Rect::new(80.0, 15.0, 18.0, 70.0))
        .unwrap();
    doc.styles.objects.push(style("base"));
    object_styles::apply_style(&mut doc, &mut history, &[frame.object], Some("base"));
    let plan = pasteboard(&doc, &PasteboardView::default()).unwrap();
    let displays: Vec<_> = plan
        .objects()
        .filter(|d| d.object() == frame.object)
        .collect();
    let fill = displays
        .iter()
        .position(|d| {
            matches!(
                d,
                Display::Shape {
                    fill: Some(_),
                    path_editable: false,
                    ..
                }
            )
        })
        .unwrap();
    let text = displays
        .iter()
        .position(|d| matches!(d, Display::Text { .. }))
        .unwrap();
    let stroke = displays
        .iter()
        .position(|d| {
            matches!(
                d,
                Display::Shape {
                    stroke: Some(_),
                    path_editable: false,
                    ..
                }
            )
        })
        .unwrap();
    assert!(fill < text && text < stroke);
    let contributors = doc.page_artwork(1, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert!(
        contributors.iter().any(|o| o.id == frame.object),
        "stroke crosses even though frame does not"
    );
    let before = doc.clone();
    let depth = history.undo_depth();
    assert!(properties::set_object_property(
        &mut doc,
        &mut history,
        &[frame.object],
        properties::ObjectProperty::StrokeWidth,
        0.0
    ));
    assert_eq!(history.undo_depth(), depth + 1);
    assert!(!doc
        .page_artwork(1, Rect::new(0.0, 0.0, 100.0, 100.0))
        .iter()
        .any(|o| o.id == frame.object));
    history.undo(&mut doc);
    assert_eq!(doc, before);
}

#[test]
fn named_tints_and_swatch_edits_reach_object_styles_and_local_frame_paints() {
    let mut doc = blank_a4();
    let mut history = History::default();
    let base = ink();
    let tint = base.named_tint("Pale cyan", 0.25).unwrap();
    doc.inks.extend([base.clone(), tint.clone()]);
    let frame = authoring::text_frame(&mut doc, &mut history, 0, Rect::new(10.0, 10.0, 80.0, 80.0))
        .unwrap()
        .object;
    let mut s = style("base");
    s.paint.fill = Some(Paint::Ink(tint));
    doc.styles.objects.push(s);
    object_styles::apply_style(&mut doc, &mut history, &[frame], Some("base"));
    let before = doc.clone();
    let depth = history.undo_depth();
    let mut after = base.clone();
    after.source_cmyk = Some([0.4, 0.1, 0.0, 0.0]);
    assert!(swatches::replace(
        &mut doc,
        &mut history,
        &base,
        after.clone()
    ));
    assert_eq!(history.undo_depth(), depth + 1);
    assert_eq!(
        doc.styles
            .object_paint(doc.object(frame).unwrap())
            .fill_ink()
            .unwrap()
            .base_color()
            .as_ref(),
        &after
    );
    history.undo(&mut doc);
    assert_eq!(doc, before);
    properties::set_object_property(
        &mut doc,
        &mut history,
        &[frame],
        properties::ObjectProperty::FillTint,
        70.0,
    );
    let paint = doc.styles.object_paint(doc.object(frame).unwrap());
    assert_eq!(paint.fill_ink(), Some(&base));
    assert_eq!(paint.fill_tint, Some(0.7));
}
