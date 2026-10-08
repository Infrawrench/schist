//! Reading what the public paged-media corpus samples write: Lab colours in
//! the specification's LAB spelling, guides hanging off a spread by
//! PageIndex, bullets remembered with their font, and stroke caps, joins,
//! mitre limits and alignment.
use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, blank_a4, History, LayoutObject, Rect, StrokeAlignment, StrokeCap, StrokeJoin,
};

/// A package holding one stroked rectangle, with `edit` applied to its
/// spread and `colors` added to its Graphic part.
fn package(edit: impl Fn(&str) -> String, colors: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    authoring::shape(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(100.0, 100.0, 200.0, 100.0),
        authoring::ShapeKind::Rectangle,
        authoring::Paint::filled("Black"),
    )
    .unwrap();
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let spread = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let text = edit(package.text(&spread).unwrap());
    package.insert(&spread, text.into_bytes());
    let graphic = package
        .text("Resources/Graphic.xml")
        .unwrap()
        .replace("</idPkg:Graphic>", &format!("{colors}</idPkg:Graphic>"));
    package.insert("Resources/Graphic.xml", graphic.into_bytes());
    container::write(&package.into_parts())
}

#[test]
fn lab_colours_in_the_specifications_spelling_are_read() {
    let colors = r#"<Color Self="Color/InkFull" Model="Spot" Space="LAB" ColorValue="30 40 -55" Name="Brand Ink"/>"#;
    let bytes = package(
        |xml| {
            let at = xml.find("<Polygon ").unwrap();
            let end = at + xml[at..].find('>').unwrap();
            let tag = &xml[at..end];
            let start = tag.find(r#"FillColor=""#).unwrap() + r#"FillColor=""#.len();
            let stop = start + tag[start..].find('"').unwrap();
            format!(
                "{}{}{}",
                &xml[..at + start],
                "Color/InkFull",
                &xml[at + stop..]
            )
        },
        colors,
    );
    let imported = import::read(&bytes).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Brand Ink")),
        "{:?}",
        imported.report.skipped
    );
    let brand = imported
        .document
        .inks
        .iter()
        .find(|i| i.name == "Brand Ink")
        .expect("the spot colour");
    assert!(brand.spot);
    assert_eq!(brand.lab, [30.0, 40.0, -55.0]);
    // Saving writes the specification's spelling, and it reads back.
    let saved = export::write(&imported.document).bytes;
    let graphic = container::read(&saved)
        .unwrap()
        .text("Resources/Graphic.xml")
        .unwrap()
        .to_owned();
    assert!(graphic.contains(r#"Space="LAB""#));
    assert!(import::read(&saved)
        .unwrap()
        .document
        .inks
        .iter()
        .any(|i| i.name == "Brand Ink" && i.spot));
}

#[test]
fn guides_on_a_spread_land_on_their_page() {
    let bytes = package(
        |xml| {
            xml.replace(
                "</Spread>",
                r#"<Guide Self="g1" Orientation="Vertical" Location="219.4253" PageIndex="0"/></Spread>"#,
            )
        },
        "",
    );
    let imported = import::read(&bytes).unwrap();
    assert!(
        !imported.report.skipped.iter().any(|s| s.contains("Guide")),
        "{:?}",
        imported.report.skipped
    );
    let guides = &imported.document.pages[0].guides;
    assert!(
        guides
            .iter()
            .any(|g| !g.horizontal && (g.position - 219.4253).abs() < 1e-3),
        "{guides:?}"
    );
}

#[test]
fn a_bullet_remembered_with_its_font_is_still_its_character() {
    let bullet = schist_layout::lists::BulletSymbol {
        kind: "UnicodeWithFont".into(),
        value: 187,
    };
    assert_eq!(bullet.character(), Some('»'));
    let glyph = schist_layout::lists::BulletSymbol {
        kind: "GlyphWithFont".into(),
        value: 187,
    };
    assert_eq!(glyph.character(), None);
}

#[test]
fn stroke_caps_joins_limits_and_alignment_are_read_and_saved() {
    let bytes = package(
        |xml| {
            xml.replacen(
                "<Polygon ",
                r#"<Polygon EndCap="RoundEndCap" EndJoin="BevelEndJoin" MiterLimit="7" StrokeAlignment="InsideAlignment" "#,
                1,
            )
        },
        "",
    );
    let imported = import::read(&bytes).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("unsupported categories")),
        "{:?}",
        imported.report.skipped
    );
    let mut doc = imported.document;
    for _ in 0..2 {
        let shape = doc
            .objects
            .iter()
            .find(|o| matches!(o.object, LayoutObject::Shape { .. }))
            .unwrap();
        let paint = doc.styles.object_paint(shape);
        assert_eq!(paint.stroke_cap, Some(StrokeCap::Round));
        assert_eq!(paint.stroke_join, Some(StrokeJoin::Bevel));
        assert_eq!(paint.miter_limit, Some(7.0));
        assert_eq!(paint.stroke_alignment, Some(StrokeAlignment::Inside));
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
    // A value the specification does not name is reported.
    let odd = package(
        |xml| xml.replacen("<Polygon ", r#"<Polygon EndJoin="Wobbly" "#, 1),
        "",
    );
    assert!(import::read(&odd)
        .unwrap()
        .report
        .skipped
        .iter()
        .any(|s| s.contains("EndJoin")));
}
