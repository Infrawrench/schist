use schist_codec_idml::{container, export, import};
use schist_layout::text_wrap::{ContourType, TextWrap, WrapMode, WrapPreferences, WrapSide};
use schist_layout::{
    authoring, authoring::ShapeKind, blank_a4, History, Insets, LayoutDocument, ObjectId, Rect,
    Story,
};

fn document() -> (LayoutDocument, ObjectId, Vec<ObjectId>) {
    let mut doc = blank_a4();
    let mut history = History::default();
    let frame = authoring::text_frame(
        &mut doc,
        &mut history,
        0,
        Rect::new(40.0, 40.0, 400.0, 500.0),
    )
    .unwrap();
    doc.stories[frame.story.0 as usize] = Story::from_text("Wrapped text", "Body");
    let wraps = [
        TextWrap {
            mode: WrapMode::BoundingBox,
            offsets: Insets::new(1.0, 2.0, 3.0, 4.0),
            side: WrapSide::LeftSide,
            ..Default::default()
        },
        TextWrap {
            mode: WrapMode::Contour,
            offsets: Insets::uniform(6.5),
            side: WrapSide::SideAwayFromSpine,
            inverse: true,
            contour: Some(ContourType::AlphaChannel),
            inside_edges: true,
            contour_path: "Alpha <1> & \"two\"".into(),
            ..Default::default()
        },
        TextWrap {
            mode: WrapMode::JumpObject,
            offsets: Insets::new(-2.0, 0.0, 12.0, 0.0),
            side: WrapSide::LargestArea,
            master_only: true,
            contour: Some(ContourType::SameAsClipping),
            ..Default::default()
        },
        TextWrap {
            mode: WrapMode::NextColumn,
            side: WrapSide::SideTowardsSpine,
            ..Default::default()
        },
        // An explicit record that wraps nothing survives as written.
        TextWrap {
            contour: Some(ContourType::GraphicFrame),
            ..Default::default()
        },
    ];
    let mut ids = Vec::new();
    for (index, wrap) in wraps.into_iter().enumerate() {
        let kind = if index % 2 == 0 {
            ShapeKind::Rectangle
        } else {
            ShapeKind::Ellipse
        };
        let id = authoring::shape(
            &mut doc,
            &mut history,
            0,
            Rect::new(60.0 + index as f32 * 70.0, 120.0, 50.0, 40.0),
            kind,
            authoring::Paint::none(),
        )
        .unwrap();
        doc.objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .appearance
            .text_wrap = Some(wrap);
        ids.push(id);
    }
    doc.objects
        .iter_mut()
        .find(|o| o.id == frame.object)
        .unwrap()
        .appearance
        .ignore_wrap = true;
    (doc, frame.object, ids)
}

fn wraps(doc: &LayoutDocument) -> Vec<Option<TextWrap>> {
    doc.objects
        .iter()
        .map(|o| o.appearance.text_wrap.clone())
        .collect()
}

/// Imported objects get fresh session ids; the rest of a save is stable.
fn without_object_ids(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find("SchistObject") {
        out.push_str(&rest[..at + "SchistObject".len()]);
        rest = rest[at + "SchistObject".len()..].trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

#[test]
fn wrap_settings_round_trip_through_repeated_saves() {
    let (mut doc, _, _) = document();
    let layer = doc.layers[0];
    schist_layout::structure::change_layer(&mut doc, &mut History::default(), layer, |l| {
        l.ignore_wrap = true
    });
    doc.text_wrap_preferences = WrapPreferences {
        only_beneath: true,
        abut: false,
        justify: true,
    };
    let expected = wraps(&doc);
    let mut current = doc.clone();
    let mut previous = None;
    for _ in 0..3 {
        let written = export::write(&current);
        let imported = import::read(&written.bytes).unwrap();
        assert!(
            !imported
                .report
                .skipped
                .iter()
                .any(|s| s.contains("wrap") || s.contains("Wrap")),
            "{:?}",
            imported.report
        );
        current = imported.document;
        assert_eq!(wraps(&current), expected);
        assert!(current.objects[0].appearance.ignore_wrap);
        assert!(current.objects[1..]
            .iter()
            .all(|o| !o.appearance.ignore_wrap));
        assert!(current.layer_ignores_wrap(current.layers[0]));
        assert_eq!(current.text_wrap_preferences, doc.text_wrap_preferences);
        let package = container::read(&written.bytes).unwrap();
        let spreads: Vec<_> = package
            .names()
            .into_iter()
            .filter(|n| n.starts_with("Spreads/"))
            .map(|n| without_object_ids(package.text(n).unwrap()))
            .collect();
        if let Some(previous) = &previous {
            assert_eq!(previous, &spreads);
        }
        previous = Some(spreads);
    }
}

#[test]
fn native_spellings_and_offset_order_are_read() {
    let (doc, _, _) = document();
    let mut plain = doc.clone();
    for object in &mut plain.objects {
        object.appearance.text_wrap = None;
    }
    let mut package = container::read(&export::write(&plain).bytes).unwrap();
    let spread = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Spreads/"))
        .unwrap()
        .to_owned();
    let text = package.text(&spread).unwrap().to_owned();
    let native = r#"<TextWrapPreference Inverse="false" ApplyToMasterPageOnly="false" TextWrapSide="RightSide" TextWrapMode="BoundingBoxTextWrap"><Properties><TextWrapOffset Top="1" Left="2" Bottom="3" Right="4"/></Properties><ContourOption ContourType="SameAsClipping" IncludeInsideEdges="false" ContourPathName="$ID/"/></TextWrapPreference>"#;
    let at = text.find("<Polygon").unwrap();
    let close = at + text[at..].find("</Properties>").unwrap() + "</Properties>".len();
    let mut edited = text.clone();
    edited.insert_str(close, native);
    package.insert(&spread, edited.into_bytes());
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    let wrap = imported
        .document
        .objects
        .iter()
        .find_map(|o| o.appearance.text_wrap.clone())
        .unwrap();
    assert_eq!(
        wrap,
        TextWrap {
            mode: WrapMode::BoundingBox,
            side: WrapSide::RightSide,
            offsets: Insets::new(1.0, 4.0, 3.0, 2.0),
            contour: Some(ContourType::SameAsClipping),
            ..Default::default()
        }
    );
}

#[test]
fn invalid_wrap_values_are_reported_and_read_as_defaults() {
    let (doc, _, _) = document();
    let written = export::write(&doc);
    let mut package = container::read(&written.bytes).unwrap();
    for name in package
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let Some(text) = package.text(&name).map(str::to_owned) else {
            continue;
        };
        let edited = text
            .replace(
                "TextWrapMode=\"BoundingBoxTextWrap\"",
                "TextWrapMode=\"Sideways\"",
            )
            .replace("TextWrapSide=\"LeftSide\"", "TextWrapSide=\"Upward\"")
            .replace("Inverse=\"true\"", "Inverse=\"maybe\"")
            .replace("Top=\"6.5\"", "Top=\"NaN\"")
            .replace("ContourType=\"AlphaChannel\"", "ContourType=\"Magic\"")
            .replace(" IgnoreWrap=\"true\"", " IgnoreWrap=\"perhaps\"")
            .replace(
                "AbutTextToTextWrap=\"true\"",
                "AbutTextToTextWrap=\"often\"",
            );
        if edited != text {
            package.insert(&name, edited.into_bytes());
        }
    }
    let imported = import::read(&container::write(&package.into_parts())).unwrap();
    for value in [
        "Sideways", "Upward", "maybe", "NaN", "Magic", "perhaps", "often",
    ] {
        assert!(
            imported.report.skipped.iter().any(|s| s.contains(value)),
            "missing {value}: {:?}",
            imported.report
        );
    }
    let wraps = wraps(&imported.document);
    let first = wraps[1].clone().unwrap();
    assert_eq!(first.mode, WrapMode::None);
    assert_eq!(first.side, WrapSide::BothSides);
    let second = wraps[2].clone().unwrap();
    assert!(!second.inverse);
    assert_eq!(second.offsets.top, 0.0);
    assert_eq!(second.contour, None);
    assert!(!imported.document.objects[0].appearance.ignore_wrap);
    assert!(imported.document.text_wrap_preferences.abut);
}

#[test]
fn parent_items_keep_their_wrap_and_master_only_flag() {
    let (mut doc, _, ids) = document();
    let index = doc.objects.iter().position(|o| o.id == ids[2]).unwrap();
    let object = doc.objects.remove(index);
    let expected = object.appearance.text_wrap.clone();
    doc.parents.push(schist_layout::ParentPage {
        name: "A".into(),
        applied_to: vec![0],
        based_on: None,
        hidden: false,
        sheets: vec![schist_layout::parents::ParentSheet {
            source: None,
            page: doc.pages[0].clone(),
            origin: schist_layout::Point::ZERO,
        }],
        placements: Vec::new(),
        objects: vec![schist_layout::ParentObject {
            object,
            overridden_on: Vec::new(),
        }],
    });
    let imported = import::read(&export::write(&doc).bytes).unwrap().document;
    let wrap = imported
        .parents
        .iter()
        .flat_map(|p| &p.objects)
        .find_map(|o| o.object.appearance.text_wrap.clone());
    assert_eq!(wrap, expected);
    assert!(wrap.unwrap().master_only);
}
