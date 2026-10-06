use schist_codec_idml::{container, export, import, xml};
use schist_layout::{authoring, object_styles, *};

fn document() -> LayoutDocument {
    let mut doc = blank_a4();
    let mut history = History::default();
    let ink = Ink::cmyk("Frame / 青 & cyan", [0.75, 0.1, 0.0, 0.0]);
    doc.styles.objects.push(ObjectStyle {
        name: "Base & 青".into(),
        enable_fill: Some(true),
        enable_stroke: Some(true),
        paint: ObjectPaint {
            fill: Some(Paint::Ink(ink)),
            stroke: Some(Paint::Ink(Ink::black())),
            stroke_width: Some(3.0),
            fill_tint: Some(0.4),
            overprint_stroke: Some(true),
            ..Default::default()
        },
        ..Default::default()
    });
    doc.styles.objects.push(ObjectStyle {
        name: "Child".into(),
        based_on: Some("Base & 青".into()),
        ..Default::default()
    });
    let text = authoring::text_frame(&mut doc, &mut history, 0, Rect::new(10.0, 10.0, 80.0, 50.0))
        .unwrap();
    authoring::set_text(&mut doc, &mut history, text.story, "Frame paint");
    let shape = authoring::rectangle(
        &mut doc,
        &mut history,
        0,
        Rect::new(100.0, 10.0, 80.0, 50.0),
        authoring::Paint::none(),
    )
    .unwrap();
    let graphic = authoring::graphic_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(10.0, 80.0, 80.0, 50.0),
        "art.png",
        false,
    )
    .unwrap();
    object_styles::apply_style(
        &mut doc,
        &mut history,
        &[text.object, shape, graphic],
        Some("Child"),
    );
    doc.objects[0].appearance.outline = Some(ShapePath::ellipse(1.0, 1.0));
    object_styles::edit_paint(
        &mut doc,
        &mut history,
        &[shape],
        &ObjectPaint {
            fill: Some(Paint::None),
            ..Default::default()
        },
    );
    doc
}

#[test]
fn native_object_styles_retain_live_inheritance_local_none_and_curved_frames_on_every_save() {
    let mut doc = document();
    let original_styles = doc.styles.objects.clone();
    for _ in 0..4 {
        let before: Vec<_> = doc
            .objects
            .iter()
            .map(|o| doc.styles.object_paint(o))
            .collect();
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        let spread = xml::parse(
            package
                .text(
                    package
                        .names()
                        .into_iter()
                        .find(|n| n.starts_with("Spreads/"))
                        .unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        let frames = spread.find_all("TextFrame");
        assert_eq!(
            frames[0].attr("AppliedObjectStyle"),
            Some("ObjectStyle/$ID/Child")
        );
        assert!(
            frames[0].attr("FillColor").is_none(),
            "inherited paint must not be flattened"
        );
        assert_eq!(
            spread.find_all("Polygon")[0].attr("FillColor"),
            Some("Swatch/None")
        );
        let back = import::read(&written.bytes).unwrap();
        assert!(
            !back
                .report
                .skipped
                .iter()
                .any(|s| s.contains("Missing object style")),
            "{:?}",
            back.report
        );
        doc = back.document;
        assert_eq!(doc.styles.objects, original_styles);
        assert_eq!(
            doc.objects
                .iter()
                .map(|o| doc.styles.object_paint(o))
                .collect::<Vec<_>>(),
            before
        );
        assert!(doc.objects[0]
            .appearance
            .outline
            .as_ref()
            .is_some_and(|p| p.subpaths[0].handles.iter().any(|h| h.incoming.is_some())));
    }
    doc.styles.objects[0].paint.fill = Some(Paint::Ink(Ink::white()));
    assert_eq!(
        doc.styles.object_paint(&doc.objects[0]).fill_ink(),
        Some(&Ink::white())
    );
    assert!(doc
        .styles
        .object_paint(&doc.objects[1])
        .fill_ink()
        .is_none());
    assert_eq!(
        doc.styles.object_paint(&doc.objects[2]).fill_ink(),
        Some(&Ink::white())
    );
}

#[test]
fn opaque_object_ids_and_disabled_categories_keep_native_local_overrides() {
    let doc = document();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let styles = package
        .text("Resources/Styles.xml")
        .unwrap()
        .replace("ObjectStyle/$ID/Base &amp; 青", "uBase")
        .replace("ObjectStyle/$ID/Child", "uChild")
        .replace("EnableFill=\"true\"", "EnableFill=\"false\"");
    package.insert("Resources/Styles.xml", styles.into_bytes());
    let spreads: Vec<_> = package
        .names()
        .into_iter()
        .filter(|n| n.starts_with("Spreads/"))
        .map(str::to_owned)
        .collect();
    for path in spreads {
        let text = package
            .text(&path)
            .unwrap()
            .replace("ObjectStyle/$ID/Child", "uChild");
        package.insert(path, text.into_bytes());
    }
    let mut doc = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    for _ in 0..4 {
        for object in &doc.objects {
            assert_eq!(object.appearance.style.as_deref(), Some("Child"));
            let paint = doc.styles.object_paint(object);
            assert!(
                paint.fill_ink().is_none(),
                "disabled fill cannot leak stored cyan"
            );
            assert_eq!(paint.stroke_width, Some(3.0));
            assert_eq!(paint.overprint_stroke, Some(true));
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}

#[test]
fn missing_cyclic_and_unsupported_object_styles_are_reported_without_hanging() {
    let doc = document();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let styles = package
        .text("Resources/Styles.xml")
        .unwrap()
        .replace(
            "StrokeWeight=\"3\"",
            "StrokeWeight=\"NaN\" EnableFrameFittingOptions=\"true\"",
        )
        .replace(
            "<ObjectStyle Self=\"ObjectStyle/$ID/Base &amp; 青\"",
            "<ObjectStyle BasedOn=\"ObjectStyle/$ID/Child\" Self=\"ObjectStyle/$ID/Base &amp; 青\"",
        );
    package.insert("Resources/Styles.xml", styles.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    for message in [
        "Invalid frame paint",
        "unsupported categories",
        "Object style cycle",
    ] {
        assert!(
            imported.report.skipped.iter().any(|s| s.contains(message)),
            "missing {message}: {:?}",
            imported.report
        );
    }
    assert!(imported.document.objects.iter().all(|o| imported
        .document
        .styles
        .object_paint(o)
        .stroke_width
        .unwrap()
        .is_finite()));
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let spread = package.text(&path).unwrap().replace(
        "AppliedObjectStyle=\"ObjectStyle/$ID/Child\"",
        "AppliedObjectStyle=\"missing\"",
    );
    package.insert(path, spread.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|s| s.contains("Missing object style")));
}

#[test]
fn every_frame_reports_each_invalid_inline_paint_once_with_or_without_a_style() {
    for kind in ["TextFrame", "Polygon", "Rectangle"] {
        for styled in [false, true] {
            let mut package = container::read(&export::write(&document()).bytes).unwrap();
            let path = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Spreads/"))
                .unwrap()
                .to_owned();
            let mut spread = package.text(&path).unwrap().replacen(
                &format!("<{kind} "),
                &format!(
                    "<{kind} StrokeWeight=\"NaN\" FillTint=\"NaN\" StrokeColor=\"MissingColor\" "
                ),
                1,
            );
            if !styled {
                spread = spread.replace(" AppliedObjectStyle=\"ObjectStyle/$ID/Child\"", "");
            }
            package.insert(path, spread.into_bytes());
            let imported = import::read(&container::write(&package.into_parts())).unwrap();
            for marker in ["StrokeWeight", "FillTint", "MissingColor"] {
                assert_eq!(
                    imported
                        .report
                        .skipped
                        .iter()
                        .filter(|m| m.contains(marker))
                        .count(),
                    1,
                    "{kind} styled={styled}: {:?}",
                    imported.report
                );
            }
            assert!(imported.document.objects.iter().all(|o| imported
                .document
                .styles
                .object_paint(o)
                .stroke_width
                .unwrap()
                .is_finite()));
        }
    }
}

/// The Text Wrap & Other category: its switch and TextWrapPreference survive
/// saves and give unwrapped items their wrap. Its other member, Nonprinting,
/// is still reported.
#[test]
fn object_style_text_wrap_round_trips_and_reaches_items() {
    let mut doc = document();
    let wrap = text_wrap::TextWrap {
        mode: text_wrap::WrapMode::BoundingBox,
        offsets: Insets::uniform(4.5),
        side: text_wrap::WrapSide::RightSide,
        ..Default::default()
    };
    doc.styles.objects[0].enable_text_wrap = Some(true);
    doc.styles.objects[0].text_wrap = Some(wrap.clone());
    let bytes = export::write(&doc).bytes;
    let package = container::read(&bytes).unwrap();
    let styles = package.text("Resources/Styles.xml").unwrap();
    assert!(styles.contains("EnableTextWrapAndOthers=\"true\""));
    let imported = import::read(&bytes).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("unsupported categories")),
        "{:?}",
        imported.report
    );
    let back = imported.document;
    let base = back
        .styles
        .objects
        .iter()
        .find(|s| s.name == "Base & 青")
        .unwrap();
    assert_eq!(base.enable_text_wrap, Some(true));
    assert_eq!(base.text_wrap.as_ref(), Some(&wrap));
    let object = back
        .objects
        .iter()
        .find(|o| o.appearance.style.as_deref() == Some("Child"))
        .unwrap();
    assert!(object.appearance.text_wrap.is_none());
    assert_eq!(back.styles.object_wrap(object), Some(wrap));
    let native = styles.replace(
        "EnableTextWrapAndOthers=\"true\"",
        "EnableTextWrapAndOthers=\"true\" Nonprinting=\"true\"",
    );
    let mut package = container::read(&bytes).unwrap();
    package.insert("Resources/Styles.xml", native.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|s| s.contains("unsupported categories")));
}
