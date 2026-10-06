//! An item's StrokeType names a stroke style the package declares in
//! Graphic.xml: custom dashed, dotted and striped styles with their own
//! geometry, or InDesign's built-in styles by name. Solid and the built-in
//! Dashed (the item's own StrokeDashAndGap) are drawn, as are custom styles;
//! other built-in styles are kept, saved and reported. A reference the
//! package does not declare strokes solid: InDesign's PDF of the public
//! paged-media `strokes-fills` sample (pages 11 to 14) draws
//! `StrokeStyle/$ID/Dashed`, `$ID/Dotted`, `$ID/Canned Dotted` and
//! `$ID/Japanese Dots`, none declared in its package, exactly as the solid
//! page 10: `6 w`, `4 M`, `re 197.638 370.945 200 100`, `S`, no dash array.
use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, blank_a4, decorations::DecorationStroke, History, Ink, LayoutDocument, LayoutObject,
    ObjectPaint, ObjectStyle, Paint, PlacedObject, Rect, StrokeType,
};
use schist_text_engine::{DecorationCap, DecorationDashes, DecorationFit, TextDecorationPattern};

const GAP: &str =
    r#"<Color Self="Color/Gap" Name="Gap" Model="Process" Space="CMYK" ColorValue="100 0 0 0"/>"#;

/// A package with one stroked rectangle whose element carries
/// `attributes`, and `resources` added to Graphic.xml.
fn package(attributes: &str, resources: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::stroked("Black", 6.0),
    )
    .unwrap();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let spread = spread_name(&package);
    let text =
        package
            .text(&spread)
            .unwrap()
            .replacen("<Polygon ", &format!("<Polygon {attributes} "), 1);
    package.insert(&spread, text.into_bytes());
    let graphic = package.text("Resources/Graphic.xml").unwrap().replacen(
        "</idPkg:Graphic>",
        &format!("{GAP}{resources}</idPkg:Graphic>"),
        1,
    );
    package.insert("Resources/Graphic.xml", graphic.into_bytes());
    container::write(&package.into_parts())
}

fn spread_name(package: &container::Package) -> String {
    package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned()
}

fn shape(doc: &LayoutDocument) -> &PlacedObject {
    doc.objects
        .iter()
        .find(|o| matches!(o.object, LayoutObject::Shape { .. }))
        .unwrap()
}

fn skipped(bytes: &[u8]) -> Vec<String> {
    import::read(bytes).unwrap().report.skipped
}

fn limits(bytes: &[u8]) -> bool {
    skipped(bytes)
        .iter()
        .any(|s| s.contains("unsupported categories"))
}

/// The saved spread's and Graphic.xml's text.
fn saved(doc: &LayoutDocument) -> (Vec<u8>, String, String) {
    let bytes = export::write(doc).bytes;
    let package = container::read(&bytes).unwrap();
    let spread = package.text(&spread_name(&package)).unwrap().to_owned();
    let graphic = package.text("Resources/Graphic.xml").unwrap().to_owned();
    (bytes, spread, graphic)
}

#[test]
fn undeclared_types_stroke_solid_as_indesign_draws_them() {
    for name in ["Dashed", "Dotted", "Canned Dotted", "Japanese Dots"] {
        let bytes = package(
            &format!(r#"StrokeType="StrokeStyle/$ID/{name}" StrokeDashAndGap="12 4""#),
            "",
        );
        assert!(!limits(&bytes), "{name}");
        let doc = import::read(&bytes).unwrap().document;
        let paint = doc.styles.object_paint(shape(&doc));
        assert_eq!(paint.stroke_type, None, "{name}");
        assert_eq!(paint.stroke_pattern(), None, "{name}");
    }
}

#[test]
fn the_built_in_dashed_type_dashes_with_the_items_lengths() {
    let bytes = package(
        r#"StrokeType="StrokeStyle/$ID/Dashed" StrokeDashAndGap="12 4 2 4" StrokeCornerAdjustment="DashesAndGaps" EndCap="RoundEndCap" GapColor="Color/Gap" GapTint="40" OverprintGap="true""#,
        r#"<StrokeStyle Self="StrokeStyle/$ID/Dashed" Name="$ID/Dashed"/>"#,
    );
    assert!(!limits(&bytes));
    let mut doc = import::read(&bytes).unwrap().document;
    for _ in 0..2 {
        let paint = doc.styles.object_paint(shape(&doc));
        assert_eq!(paint.stroke_type, Some(StrokeType::Dashed));
        let pattern = paint.stroke_pattern().unwrap();
        assert_eq!(pattern.fitting, DecorationFit::DashesAndGaps);
        assert_eq!(
            pattern.pattern,
            TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![12.0, 4.0, 2.0, 4.0],
                cap: DecorationCap::Round,
            })
        );
        assert_eq!(paint.gap_ink().map(|i| i.name.as_str()), Some("Gap"));
        assert_eq!(paint.gap_tint, Some(0.4));
        assert_eq!(paint.overprint_gap, Some(true));
        let (bytes, spread, graphic) = saved(&doc);
        for expected in [
            r#"StrokeType="StrokeStyle/$ID/Dashed""#,
            r#"StrokeDashAndGap="12 4 2 4""#,
            r#"StrokeCornerAdjustment="DashesAndGaps""#,
            r#"GapTint="40""#,
            r#"OverprintGap="true""#,
        ] {
            assert!(spread.contains(expected), "{expected}: {spread}");
        }
        // Declared, so InDesign draws it dashed.
        assert!(
            graphic.contains(r#"<StrokeStyle Self="StrokeStyle/$ID/Dashed" Name="$ID/Dashed"/>"#),
            "{graphic}"
        );
        doc = import::read(&bytes).unwrap().document;
    }
}

#[test]
fn custom_dashed_dotted_and_striped_styles_stroke_items_and_save() {
    let resources = r#"<DashedStrokeStyle Self="DashedStrokeStyle/u1" Name="Long" DashArray="6 3" EndCap="ProjectingEndCap" StrokeCornerAdjustment="Gaps"/><DottedStrokeStyle Self="DottedStrokeStyle/u2" Name="Spaced" DotArray="5.554054054054054 6.445945945945946" StrokeCornerAdjustment="Gaps"/><StripedStrokeStyle Self="StripedStrokeStyle/u3" Name="Thick Thin" StripeArray="0 60 80 100"/>"#;
    for (reference, expected) in [
        (
            "DashedStrokeStyle/u1",
            TextDecorationPattern::Dashes(DecorationDashes {
                lengths: vec![6.0, 3.0],
                cap: DecorationCap::Projecting,
            }),
        ),
        (
            "DottedStrokeStyle/u2",
            TextDecorationPattern::Dots(
                ["5.554054054054054", "6.445945945945946"]
                    .map(|v| v.parse().unwrap())
                    .to_vec(),
            ),
        ),
        (
            "StripedStrokeStyle/u3",
            TextDecorationPattern::Stripes(vec![0.0, 60.0, 80.0, 100.0]),
        ),
    ] {
        let bytes = package(
            &format!(r#"StrokeType="{reference}" GapColor="Color/Gap""#),
            resources,
        );
        assert!(!limits(&bytes), "{reference}");
        let mut doc = import::read(&bytes).unwrap().document;
        for _ in 0..2 {
            let pattern = doc
                .styles
                .object_paint(shape(&doc))
                .stroke_pattern()
                .unwrap();
            assert_eq!(pattern.pattern, expected, "{reference}");
            let (bytes, spread, graphic) = saved(&doc);
            let Some(StrokeType::Style(stroke)) = doc.styles.object_paint(shape(&doc)).stroke_type
            else {
                panic!("a style");
            };
            // The item names the resource Graphic.xml saves.
            let written = graphic
                .split('<')
                .find(|e| e.contains(&format!(r#"Name="{}""#, stroke.name)))
                .unwrap_or_else(|| panic!("{graphic}"));
            let id = written
                .split(r#"Self=""#)
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            assert!(
                spread.contains(&format!(r#"StrokeType="{id}""#)),
                "{spread}"
            );
            doc = import::read(&bytes).unwrap().document;
        }
    }
}

#[test]
fn other_built_in_types_are_kept_saved_and_reported() {
    let bytes = package(
        r#"StrokeType="StrokeStyle/$ID/Japanese Dots""#,
        r#"<StrokeStyle Self="StrokeStyle/$ID/Japanese Dots" Name="$ID/Japanese Dots"/><StrokeStyle Self="StrokeStyle/$ID/ThickThin" Name="$ID/ThickThin"/>"#,
    );
    assert!(limits(&bytes));
    let doc = import::read(&bytes).unwrap().document;
    let paint = doc.styles.object_paint(shape(&doc));
    assert_eq!(
        paint.stroke_type,
        Some(StrokeType::Builtin("$ID/Japanese Dots".into()))
    );
    assert_eq!(paint.stroke_pattern(), None, "drawn solid");
    let (bytes, spread, graphic) = saved(&doc);
    assert!(spread.contains(r#"StrokeType="StrokeStyle/$ID/Japanese Dots""#));
    assert!(graphic.contains(
        r#"<StrokeStyle Self="StrokeStyle/$ID/Japanese Dots" Name="$ID/Japanese Dots"/>"#
    ));
    assert!(limits(&bytes));
}

#[test]
fn arrowheads_and_invalid_dashes_are_still_reported() {
    assert!(limits(&package(r#"RightLineEnd="TriangleArrowHead""#, "")));
    let skipped = skipped(&package(
        r#"StrokeType="StrokeStyle/$ID/Dashed" StrokeDashAndGap="12" StrokeCornerAdjustment="Corners""#,
        r#"<StrokeStyle Self="StrokeStyle/$ID/Dashed" Name="$ID/Dashed"/>"#,
    ));
    assert!(skipped.iter().any(|s| s.contains("StrokeDashAndGap")));
    assert!(skipped.iter().any(|s| s.contains("StrokeCornerAdjustment")));
}

#[test]
fn object_styles_keep_their_stroke_type_and_gap() {
    let mut doc = blank_a4();
    let stroke = DecorationStroke {
        name: "Long".into(),
        fitting: DecorationFit::Dashes,
        pattern: TextDecorationPattern::Dashes(DecorationDashes {
            lengths: vec![8.0, 4.0],
            cap: DecorationCap::Butt,
        }),
    };
    doc.styles.objects.push(ObjectStyle {
        name: "Dashed".into(),
        enable_stroke: Some(true),
        enable_stroke_options: Some(true),
        paint: ObjectPaint {
            stroke: Some(Paint::Ink(Ink::black())),
            stroke_width: Some(2.0),
            stroke_type: Some(StrokeType::Style(stroke.clone())),
            gap: Some(Paint::Ink(Ink::cmyk("Gap", [0.0, 1.0, 0.0, 0.0]))),
            gap_tint: Some(0.25),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut history = History::default();
    let id = authoring::shape(
        &mut doc,
        &mut history,
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::none(),
    )
    .unwrap();
    schist_layout::object_styles::apply_style(&mut doc, &mut history, &[id], Some("Dashed"));
    for _ in 0..2 {
        let (bytes, _, _) = saved(&doc);
        assert!(!limits(&bytes));
        doc = import::read(&bytes).unwrap().document;
        let style = doc.styles.object_style("Dashed").unwrap();
        assert_eq!(
            style.paint.stroke_type,
            Some(StrokeType::Style(stroke.clone()))
        );
        assert_eq!(style.paint.gap_tint, Some(0.25));
        let paint = doc.styles.object_paint(shape(&doc));
        assert_eq!(paint.stroke_pattern(), Some(stroke.clone()));
        assert_eq!(paint.gap_ink().map(|i| i.name.as_str()), Some("Gap"));
    }
}
