use schist_codec_idml::{container, export, import};
use schist_layout::{
    authoring, compose, text_path, FrameOverflow, History, LayoutObject, Point, Rect, ShapePath,
    Story,
};

#[test]
fn a_box_with_an_additional_path_retains_its_primary_story_with_a_diagnostic() {
    let mut doc = schist_layout::blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 20.0, 160.0, 80.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Inside the box", "Default");
    let id = authoring::path_shape(
        &mut doc,
        &mut History::default(),
        0,
        ShapePath::ellipse(160.0, 80.0),
        authoring::Paint::none(),
    )
    .unwrap();
    let path = text_path::attach(&mut doc, &mut History::default(), id).unwrap();
    doc.stories[path.story.0 as usize] = Story::from_text("Along the outside", "Default");
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let name = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let xml = package.text(&name).unwrap();
    let start = xml.find("<TextPath ").unwrap();
    let end = xml[start..].find("</TextPath>").unwrap() + start + "</TextPath>".len();
    let child = &xml[start..end];
    let parent_start = xml[..start].rfind("<Polygon ").unwrap();
    let parent_end = xml[end..].find("</Polygon>").unwrap() + end + "</Polygon>".len();
    let changed = format!("{}{}", &xml[..parent_start], &xml[parent_end..]).replacen(
        "</TextFrame>",
        &format!("{child}</TextFrame>"),
        1,
    );
    package.insert(name, changed.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    assert!(format!("{:?}", imported.report).contains("TextFrame/TextPath"));
    assert_eq!(imported.document.objects.len(), 1);
    let LayoutObject::TextFrame {
        story,
        text_path: None,
        ..
    } = imported.document.objects[0].object
    else {
        panic!("primary box was replaced")
    };
    assert_eq!(
        imported.document.stories[story.0 as usize].text(),
        "Inside the box"
    );
}

#[test]
fn unsupported_native_path_options_and_invalid_brackets_are_diagnosed() {
    let original = include_bytes!("../../../fixtures/idml/text.idml");
    for (from, to, expected) in [
        (
            "PathEffect=\"RainbowPathEffect\"",
            "PathEffect=\"GravityPathEffect\"",
            "Unsupported text path setting: PathEffect",
        ),
        (
            "PathSpacing=\"0\"",
            "PathSpacing=\"4\"",
            "Unsupported text path setting: PathSpacing",
        ),
        (
            "StartBracket=\"0\"",
            "StartBracket=\"NaN\"",
            "Invalid text path geometry or brackets",
        ),
        (
            "StartBracket=\"0\"",
            "StartBracket=\"-10\"",
            "Invalid text path geometry or brackets",
        ),
    ] {
        let mut package = container::read(original).unwrap();
        let name = "Spreads/Spread_ueb.xml";
        let xml = package.text(name).unwrap();
        assert!(xml.contains(from));
        let xml = xml.replace(from, to);
        package.insert(name, xml.into_bytes());
        let imported = import::read(&container::write(&package.into_parts())).unwrap();
        assert!(
            format!("{:?}", imported.report).contains(expected),
            "{:?}",
            imported.report
        );
        // Even malformed input leaves a serializable, editable document.
        let json = serde_json::to_string(&imported.document).unwrap();
        assert!(serde_json::from_str::<schist_layout::LayoutDocument>(&json).is_ok());
    }
}

#[test]
fn native_public_text_paths_keep_their_stories_geometry_and_brackets_after_repeated_saves() {
    let mut doc = import::read(include_bytes!("../../../fixtures/idml/text.idml"))
        .unwrap()
        .document;
    let describe = |doc: &schist_layout::LayoutDocument| {
        doc.objects
            .iter()
            .filter_map(|o| {
                let LayoutObject::TextFrame {
                    story,
                    text_path: Some(path),
                    ..
                } = &o.object
                else {
                    return None;
                };
                Some((
                    o.name.clone(),
                    doc.stories[story.0 as usize].text(),
                    path.clone(),
                    o.bounds,
                ))
            })
            .collect::<Vec<_>>()
    };
    let expected = describe(&doc);
    assert_eq!(expected.len(), 2);
    assert!(expected
        .iter()
        .any(|(_, text, _, _)| text == "Rich Text on path"));
    assert!(expected
        .iter()
        .any(|(_, text, _, _)| text == "Text on path"));
    for (_, _, path, bounds) in &expected {
        assert_eq!(path.start, 0.0);
        assert!((path.end.unwrap() - 348.03287).abs() < 0.001);
        assert_eq!(bounds.height, 0.0);
        assert!(path.engine_path().is_some());
    }
    for _ in 0..4 {
        for object in &doc.objects {
            if matches!(
                object.object,
                LayoutObject::TextFrame {
                    text_path: Some(_),
                    ..
                }
            ) {
                let flow = compose::compose_object(&doc, object).unwrap();
                assert_eq!(flow.lines.len(), 1);
                assert!(!flow.lost);
                let LayoutObject::TextFrame { story, .. } = object.object else {
                    unreachable!()
                };
                let spec = compose::line_spec(&flow.lines[0], &doc.stories[story.0 as usize], &doc);
                assert!(schist_text_engine::rasterize(&spec)
                    .unwrap()
                    .coverage
                    .iter()
                    .any(|v| *v != 0));
            }
        }
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
        let actual = describe(&doc);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_eq!(
                (&actual.0, &actual.1, &actual.2),
                (&expected.0, &expected.1, &expected.2)
            );
            assert!((actual.3.x - expected.3.x).abs() < 0.001);
            assert!((actual.3.y - expected.3.y).abs() < 0.001);
        }
    }
}

#[test]
fn mixed_native_threads_reference_text_path_children_and_preserve_follow_end_intent() {
    let mut doc = schist_layout::blank_a4();
    let shape = authoring::path_shape(
        &mut doc,
        &mut History::default(),
        0,
        {
            let mut p = ShapePath::ellipse(180.0, 60.0);
            p.map_points(|p| p + Point::new(60.0, 80.0));
            p
        },
        authoring::Paint::none(),
    )
    .unwrap();
    let path = text_path::attach(&mut doc, &mut History::default(), shape).unwrap();
    doc.stories[path.story.0 as usize] =
        Story::from_text("écho words on a curve then a box ".repeat(30), "Default");
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(20.0, 170.0, 420.0, 600.0),
    )
    .unwrap();
    for id in [path.object, frame.object] {
        let object = doc.objects.iter_mut().find(|o| o.id == id).unwrap();
        let LayoutObject::TextFrame {
            story, overflow, ..
        } = &mut object.object
        else {
            unreachable!()
        };
        *story = path.story;
        *overflow = FrameOverflow::Thread;
    }
    doc.thread_order = vec![(path.story, vec![path.object, frame.object])];
    for _ in 0..4 {
        let written = export::write(&doc);
        let package = container::read(&written.bytes).unwrap();
        let xml = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Spreads/"))
            .and_then(|n| package.text(n))
            .unwrap();
        let child = xml
            .split("<TextPath Self=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(xml.contains(&format!("PreviousTextFrame=\"{child}\"")));
        assert!(xml.contains("Schist.TextPath.FollowEnd.v1"));
        let read = import::read(&written.bytes).unwrap();
        assert!(read.report.is_complete(), "{:?}", read.report);
        doc = read.document;
        let first = doc.story_frames(path.story)[0];
        let LayoutObject::TextFrame {
            text_path: Some(baseline),
            ..
        } = &first.object
        else {
            panic!("wrong thread head")
        };
        assert_eq!(baseline.end, None);
        let flow = compose::compose_story(&doc, path.story);
        assert!(flow.frames[0].passed_on);
        assert_eq!(flow.frames[0].lines.len(), 1);
        assert!(!flow.has_overflow());
        assert!(flow.frames[1].lines[0].start >= flow.frames[0].consumed_to);
    }
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let name = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let xml = package.text(&name).unwrap();
    let old = xml
        .split("EndBracket=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let changed = xml.replace(&format!("EndBracket=\"{old}\""), "EndBracket=\"30\"");
    package.insert(name, changed.into_bytes());
    let edited = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert!(
        matches!(&edited.story_frames(path.story)[0].object, LayoutObject::TextFrame { text_path: Some(p), .. } if p.end == Some(30.0))
    );
}
