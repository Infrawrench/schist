//! Text frame auto-size settings are read and saved, and an auto-sized frame
//! fits its text when it is opened, as InDesign's PDF of the public
//! paged-media `text-autosize` sample grows an undersized HeightOnly frame.
use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring,
    auto_size::{AutoSize, AutoSizing, ReferencePoint},
    blank_a4,
    compose::compose_story,
    History, LayoutDocument, LayoutObject, PlacedObject, Rect, Story,
};

/// A package with one text frame of `lines` paragraphs, `bounds` tall, its
/// TextFramePreference carrying `attributes`.
fn package(lines: usize, bounds: Rect, attributes: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let made = authoring::text_frame(&mut doc, &mut History::default(), 0, bounds).unwrap();
    let mut story = Story::new();
    for line in 0..lines {
        story.push_paragraph(format!("Headline line {line}"), "Body");
    }
    doc.stories[made.story.0 as usize] = story;
    doc.objects
        .iter_mut()
        .find(|o| o.id == made.object)
        .unwrap()
        .bounds = bounds;
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let spread = spread_name(&package);
    let text = package.text(&spread).unwrap().replacen(
        "<TextFramePreference ",
        &format!("<TextFramePreference {attributes} "),
        1,
    );
    package.insert(&spread, text.into_bytes());
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

fn frame(doc: &LayoutDocument) -> &PlacedObject {
    doc.objects
        .iter()
        .find(|o| matches!(o.object, LayoutObject::TextFrame { .. }))
        .unwrap()
}

fn reported(bytes: &[u8]) -> bool {
    import::read(bytes)
        .unwrap()
        .report
        .skipped
        .iter()
        .any(|s| s.contains("Auto-size not applied"))
}

#[test]
fn an_undersized_frame_fits_its_text_when_opened() {
    let bounds = Rect::new(36.0, 60.0, 240.0, 40.0);
    let bytes = package(
        12,
        bounds,
        r#"AutoSizingType="HeightOnly" AutoSizingReferencePoint="TopLeftPoint""#,
    );
    assert!(!reported(&bytes));
    let doc = import::read(&bytes).unwrap().document;
    let object = frame(&doc);
    assert!(object.bounds.height > 100.0, "{:?}", object.bounds);
    assert_eq!(object.bounds.y, 60.0);
    let LayoutObject::TextFrame { story, .. } = object.object else {
        unreachable!()
    };
    assert!(!compose_story(&doc, story).has_overflow());
}

#[test]
fn settings_are_read_and_saved() {
    let bytes = package(
        2,
        Rect::new(36.0, 60.0, 240.0, 200.0),
        r#"AutoSizingType="HeightAndWidth" AutoSizingReferencePoint="BottomRightPoint" UseMinimumHeightForAutoSizing="true" MinimumHeightForAutoSizing="108" UseMinimumWidthForAutoSizing="false" MinimumWidthForAutoSizing="50" UseNoLineBreaksForAutoSizing="true""#,
    );
    let mut doc = import::read(&bytes).unwrap().document;
    let expected = AutoSize {
        sizing: AutoSizing::HeightAndWidth,
        reference: ReferencePoint::BottomRight,
        minimum_height: Some(108.0),
        minimum_width: None,
        no_line_breaks: true,
    };
    for _ in 0..2 {
        assert_eq!(frame(&doc).appearance.auto_size, Some(expected));
        // Fitted about its bottom right, at least 108 pt tall.
        assert_eq!(frame(&doc).bounds.bottom(), 260.0);
        assert!(frame(&doc).bounds.height >= 108.0);
        let saved = export::write(&doc).bytes;
        let package = container::read(&saved).unwrap();
        let spread = package.text(&spread_name(&package)).unwrap().to_owned();
        assert!(
            spread.contains(r#"AutoSizingType="HeightAndWidth""#),
            "{spread}"
        );
        assert!(spread.contains(r#"AutoSizingReferencePoint="BottomRightPoint""#));
        assert!(spread.contains(r#"MinimumHeightForAutoSizing="108""#));
        doc = import::read(&saved).unwrap().document;
    }
    // Off is no setting at all.
    let off = package(
        2,
        Rect::new(36.0, 60.0, 240.0, 200.0),
        r#"AutoSizingType="Off""#,
    );
    assert_eq!(
        frame(&import::read(&off).unwrap().document)
            .appearance
            .auto_size,
        None
    );
}

#[test]
fn fits_schist_does_not_apply_are_reported() {
    let bounds = Rect::new(36.0, 60.0, 240.0, 40.0);
    let proportional = package(
        12,
        bounds,
        r#"AutoSizingType="HeightAndWidthProportionally""#,
    );
    assert!(reported(&proportional));
    assert_eq!(
        frame(&import::read(&proportional).unwrap().document).bounds,
        bounds
    );
    // A value the specification does not name is reported and read as Off.
    let odd = package(12, bounds, r#"AutoSizingType="Taller""#);
    let imported = import::read(&odd).unwrap();
    assert!(imported
        .report
        .skipped
        .iter()
        .any(|s| s.contains("AutoSizingType")));
    assert_eq!(frame(&imported.document).appearance.auto_size, None);
}
