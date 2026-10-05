use schist_codec_idml::{container, export, import};
use schist_layout::anchored::{
    AnchorPoint, AnchoredPosition, HorizontalAlignment, HorizontalReference, Placement,
    VerticalAlignment, VerticalReference,
};
use schist_layout::{authoring, blank_a4, History, LayoutDocument, LayoutObject, Rect, Story};

fn rectangle(setting: &str) -> String {
    format!(
        r#"<Rectangle Self="art1" Name="Art" ItemTransform="1 0 0 1 0 0" FillColor="Swatch/None" StrokeColor="Swatch/None" StrokeWeight="0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray><PathPointType Anchor="0 -20" LeftDirection="0 -20" RightDirection="0 -20"/><PathPointType Anchor="0 0" LeftDirection="0 0" RightDirection="0 0"/><PathPointType Anchor="30 0" LeftDirection="30 0" RightDirection="30 0"/><PathPointType Anchor="30 -20" LeftDirection="30 -20" RightDirection="30 -20"/></PathPointArray></GeometryPathType></PathGeometry></Properties>{setting}</Rectangle>"#
    )
}

/// A native package whose story holds `item` between "Before " and "after".
fn native(item: &str) -> Vec<u8> {
    let mut doc = blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 400.0, 300.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Before after", "Body");
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let name = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    let text = package.text(&name).unwrap().to_owned();
    assert!(text.contains("<Content>Before after</Content>"));
    let edited = text.replace(
        "<Content>Before after</Content>",
        &format!("<Content>Before </Content>{item}<Content>after</Content>"),
    );
    package.insert(&name, edited.into_bytes());
    container::write(&package.into_parts())
}

fn item(doc: &LayoutDocument) -> Option<&schist_layout::anchored::AnchoredItem> {
    doc.stories[0]
        .structures
        .iter()
        .find_map(|s| s.anchored.as_deref())
}

#[test]
fn inline_rectangles_are_typed_from_their_retained_xml() {
    for (setting, position, offset) in [
        ("", AnchoredPosition::Inline, 0.0),
        (
            r#"<AnchoredObjectSetting AnchoredPosition="InlinePosition" AnchorYoffset="2.5"/>"#,
            AnchoredPosition::Inline,
            2.5,
        ),
        (
            r#"<AnchoredObjectSetting AnchoredPosition="AboveLine"/>"#,
            AnchoredPosition::AboveLine,
            0.0,
        ),
        (
            r#"<AnchoredObjectSetting AnchoredPosition="Anchored" AnchorYoffset="-4"/>"#,
            AnchoredPosition::Anchored,
            -4.0,
        ),
    ] {
        let imported = import::read(&native(&rectangle(setting))).unwrap();
        let doc = imported.document;
        assert_eq!(doc.stories[0].text(), "Before after");
        let structure = doc.stories[0]
            .structures
            .iter()
            .find(|s| s.kind == "Rectangle")
            .expect("retained structure");
        assert_eq!(structure.at, Some("Before ".len()));
        let item = structure.anchored.as_deref().expect("typed item");
        assert_eq!(item.position, position);
        assert_eq!(item.y_offset, offset);
        assert!(matches!(item.object.object, LayoutObject::Shape { .. }));
        let extent = item.extent();
        assert!((extent.width - 30.0).abs() < 0.01 && (extent.height - 20.0).abs() < 0.01);
    }
}

#[test]
fn invalid_settings_are_reported_and_left_untyped() {
    for (setting, value) in [
        (
            r#"<AnchoredObjectSetting AnchoredPosition="Sideways"/>"#,
            "Sideways",
        ),
        (r#"<AnchoredObjectSetting AnchorYoffset="NaN"/>"#, "NaN"),
    ] {
        let imported = import::read(&native(&rectangle(setting))).unwrap();
        assert!(
            imported.report.skipped.iter().any(|s| s.contains(value)),
            "{:?}",
            imported.report
        );
        assert!(item(&imported.document).is_none());
        assert!(imported.document.stories[0]
            .structures
            .iter()
            .any(|s| s.kind == "Rectangle"));
    }
}

#[test]
fn typed_items_survive_repeated_saves_and_compose_in_their_line() {
    let setting = r#"<AnchoredObjectSetting AnchoredPosition="InlinePosition" AnchorYoffset="1"/>"#;
    let mut doc = import::read(&native(&rectangle(setting))).unwrap().document;
    let expected = item(&doc).unwrap().clone();
    for _ in 0..3 {
        let flow = schist_layout::compose::compose_story(&doc, schist_layout::StoryId(0));
        assert!(flow
            .lines()
            .any(|l| l.projected.as_ref().is_some_and(|p| p.anchored.len() == 1)));
        assert_eq!(flow.frames[0].unrendered_structures, 0);
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        let again = item(&doc).unwrap();
        assert_eq!(again.position, expected.position);
        assert_eq!(again.y_offset, expected.y_offset);
        assert_eq!(again.object.object, expected.object.object);
        assert_eq!(again.extent(), expected.extent());
        assert_eq!(doc.stories[0].text(), "Before after");
    }
}

/// The public paged-media `anchored` sample's settings, as its generator
/// writes them (see docs/idml-format.md).
#[test]
fn custom_and_above_line_settings_are_typed_with_native_spellings() {
    let setting = r#"<AnchoredObjectSetting AnchoredPosition="Anchored" SpineRelative="true" LockPosition="true" PinPosition="false" AnchorPoint="TopRightAnchor" HorizontalReferencePoint="PageMargins" VerticalReferencePoint="Capheight" HorizontalAlignment="RightAlign" VerticalAlignment="BottomAlign" AnchorXoffset="24" AnchorYoffset="12" AnchorSpaceAbove="3"/>"#;
    let imported = import::read(&native(&rectangle(setting))).unwrap();
    assert!(
        !imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("Anchored item")),
        "{:?}",
        imported.report
    );
    let typed = item(&imported.document).unwrap();
    assert_eq!(typed.position, AnchoredPosition::Anchored);
    assert_eq!(typed.y_offset, 12.0);
    assert_eq!(
        typed.placement,
        Placement {
            anchor_point: AnchorPoint::TopRight,
            horizontal_alignment: HorizontalAlignment::Right,
            horizontal_reference: HorizontalReference::PageMargins,
            vertical_alignment: VerticalAlignment::Bottom,
            vertical_reference: VerticalReference::CapHeight,
            x_offset: 24.0,
            space_above: 3.0,
            spine_relative: true,
            pin_position: false,
            lock_position: true,
        }
    );
    // Absent attributes take IDML's defaults.
    let imported = import::read(&native(&rectangle(
        r#"<AnchoredObjectSetting AnchoredPosition="AboveLine"/>"#,
    )))
    .unwrap();
    assert_eq!(
        item(&imported.document).unwrap().placement,
        Placement::default()
    );
    assert!(Placement::default().pin_position);
}

/// The sample writes VerticalReferencePoint values the specification does
/// not define (AnchorLocation, LineCapHeight, LineXHeight); InDesign's PDF
/// places those pages exactly as LineBaseline. Schist reports them and reads
/// the default; an invalid offset still leaves the item untyped.
#[test]
fn unknown_references_fall_back_to_their_defaults() {
    for value in ["AnchorLocation", "LineCapHeight", "LineXHeight"] {
        let setting = format!(
            r#"<AnchoredObjectSetting AnchoredPosition="Anchored" AnchorPoint="TopLeftAnchor" HorizontalReferencePoint="AnchorLocation" VerticalReferencePoint="{value}" AnchorXoffset="24" AnchorYoffset="12"/>"#
        );
        let imported = import::read(&native(&rectangle(&setting))).unwrap();
        assert!(
            imported.report.skipped.iter().any(|s| s.contains(value)),
            "{:?}",
            imported.report
        );
        let typed = item(&imported.document).expect("typed");
        assert_eq!(
            typed.placement.vertical_reference,
            VerticalReference::LineBaseline
        );
        assert_eq!(
            typed.placement.horizontal_reference,
            HorizontalReference::AnchorLocation
        );
        assert_eq!(typed.placement.x_offset, 24.0);
    }
    for (setting, value) in [
        (
            r#"<AnchoredObjectSetting HorizontalAlignment="Inward"/>"#,
            "Inward",
        ),
        (r#"<AnchoredObjectSetting PinPosition="yes"/>"#, "yes"),
        (r#"<AnchoredObjectSetting AnchorPoint="Middle"/>"#, "Middle"),
    ] {
        let imported = import::read(&native(&rectangle(setting))).unwrap();
        assert!(imported.report.skipped.iter().any(|s| s.contains(value)));
        assert_eq!(
            item(&imported.document).unwrap().placement,
            Placement::default()
        );
    }
    for (setting, value) in [
        (r#"<AnchoredObjectSetting AnchorXoffset="NaN"/>"#, "NaN"),
        (r#"<AnchoredObjectSetting AnchorSpaceAbove="1e9"/>"#, "1e9"),
    ] {
        let imported = import::read(&native(&rectangle(setting))).unwrap();
        assert!(imported.report.skipped.iter().any(|s| s.contains(value)));
        assert!(item(&imported.document).is_none());
    }
}
