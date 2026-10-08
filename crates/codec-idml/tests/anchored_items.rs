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

/// A native package whose host story anchors a text frame between "Before "
/// and "after": an exported frame moved off the spread into the story. With
/// `own_story`, the moved frame shows the host story itself.
fn anchored_frame(setting: &str, own_story: bool) -> Vec<u8> {
    let mut doc = blank_a4();
    let host = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 400.0, 300.0),
    )
    .unwrap();
    doc.stories[host.story.0 as usize] = Story::from_text("Before after", "Body");
    let inner = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 90.0, 40.0),
    )
    .unwrap();
    doc.stories[inner.story.0 as usize] = Story::from_text("Inner words", "Body");
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let names: Vec<String> = package.names().into_iter().map(str::to_owned).collect();
    let host_part = names
        .iter()
        .find(|n| {
            n.starts_with("Stories/")
                && package
                    .text(n)
                    .unwrap()
                    .contains("<Content>Before after</Content>")
        })
        .unwrap()
        .clone();
    let host_id = host_part
        .trim_start_matches("Stories/Story_")
        .trim_end_matches(".xml")
        .to_owned();
    let spread_part = names
        .iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .clone();
    let spread = package.text(&spread_part).unwrap().to_owned();
    // The frame that does not show the host story.
    let (start, end) = spread
        .match_indices("<TextFrame ")
        .map(|(at, _)| (at, at + spread[at..].find("</TextFrame>").unwrap() + 12))
        .find(|(start, end)| !spread[*start..*end].contains(&format!("ParentStory=\"{host_id}\"")))
        .unwrap();
    let mut frame = spread[start..end].to_owned();
    if own_story {
        let from = frame.find("ParentStory=\"").unwrap() + 13;
        let to = from + frame[from..].find('"').unwrap();
        frame.replace_range(from..to, &host_id);
    }
    frame.insert_str(frame.len() - 12, setting);
    let mut spread_out = spread.clone();
    spread_out.replace_range(start..end, "");
    package.insert(&spread_part, spread_out.into_bytes());
    let story = package.text(&host_part).unwrap().replace(
        "<Content>Before after</Content>",
        &format!("<Content>Before </Content>{frame}<Content>after</Content>"),
    );
    package.insert(&host_part, story.into_bytes());
    container::write(&package.into_parts())
}

fn inner_text(doc: &LayoutDocument) -> Option<String> {
    let item = doc
        .stories
        .iter()
        .flat_map(|s| &s.structures)
        .find_map(|s| s.anchored.as_deref())?;
    let LayoutObject::TextFrame { story, .. } = item.object.object else {
        return None;
    };
    Some(doc.stories[story.0 as usize].text())
}

#[test]
fn anchored_text_frames_are_typed_with_their_story_and_survive_saves() {
    let setting = r#"<AnchoredObjectSetting AnchoredPosition="InlinePosition"/>"#;
    let imported = import::read(&anchored_frame(setting, false)).unwrap();
    let mut doc = imported.document;
    assert_eq!(inner_text(&doc).as_deref(), Some("Inner words"));
    let host = doc
        .stories
        .iter()
        .position(|s| s.text() == "Before after")
        .unwrap();
    assert!(doc.objects.iter().all(|o| !matches!(
        o.object,
        LayoutObject::TextFrame { story, .. } if doc.stories[story.0 as usize].text() == "Inner words"
    )));
    for _ in 0..3 {
        let flow = schist_layout::compose::compose_story(&doc, schist_layout::StoryId(host as u32));
        assert_eq!(flow.frames[0].unrendered_structures, 0);
        let frame = doc.object(flow.frames[0].object).unwrap();
        let lines: Vec<_> = flow.lines().cloned().collect();
        let [item] =
            &schist_layout::anchored::placements(&doc, &doc.stories[host], frame, &lines)[..]
        else {
            panic!()
        };
        let composed = schist_layout::compose::compose_object(&doc, item).unwrap();
        assert!(composed.all_lines().count() > 0);
        // A placed item's id is never a document object's, whatever was saved.
        assert_eq!(item.id, schist_layout::anchored::PLACED);
        assert!(doc.object(item.id).is_none());
        let before = doc.stories.clone();
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        assert_eq!(doc.stories, before);
        assert_eq!(inner_text(&doc).as_deref(), Some("Inner words"));
    }
}

#[test]
fn an_anchored_frame_showing_its_own_story_is_reported_and_left_untyped() {
    let imported = import::read(&anchored_frame("", true)).unwrap();
    assert!(
        imported
            .report
            .skipped
            .iter()
            .any(|s| s.contains("ParentStory")),
        "{:?}",
        imported.report
    );
    assert!(inner_text(&imported.document).is_none());
    assert!(imported
        .document
        .stories
        .iter()
        .flat_map(|s| &s.structures)
        .any(|s| s.kind == "TextFrame"));
}

#[test]
fn anchored_groups_flatten_their_members_and_survive_saves() {
    let oval = rectangle("")
        .replace("<Rectangle Self=\"art1\"", "<Oval Self=\"art2\"")
        .replace("</Rectangle>", "</Oval>");
    let group = format!(
        r#"<Group Self="g1" Name="Pair" ItemTransform="1 0 0 1 0 0">{}<Group Self="g2" ItemTransform="1 0 0 1 40 0">{oval}</Group><AnchoredObjectSetting AnchoredPosition="InlinePosition" AnchorYoffset="2"/></Group>"#,
        rectangle("")
    );
    let mut doc = import::read(&native(&group)).unwrap().document;
    for _ in 0..2 {
        let typed = item(&doc).expect("typed group");
        assert_eq!(typed.object.name, "Pair");
        assert!(matches!(typed.object.object, LayoutObject::Group { .. }));
        assert_eq!(typed.members.len(), 2);
        assert!(matches!(
            typed.members[1].object,
            LayoutObject::Shape { .. }
        ));
        assert_eq!(typed.y_offset, 2.0);
        let extent = typed.extent();
        assert!((extent.width - 70.0).abs() < 0.01, "{extent:?}");
        assert!((extent.height - 20.0).abs() < 0.01, "{extent:?}");
        let flow = schist_layout::compose::compose_story(&doc, schist_layout::StoryId(0));
        assert_eq!(flow.frames[0].unrendered_structures, 0);
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}
