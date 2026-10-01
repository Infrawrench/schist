use schist_codec_idml::{container, export, import, xml};
use schist_layout::{blank_a4, ParagraphDirection, ParagraphStyle, Story, StoryPoint};

fn directions(doc: &schist_layout::LayoutDocument) -> Vec<schist_text_engine::ParagraphDirection> {
    let story = &doc.stories[0];
    story
        .points
        .iter()
        .zip(story.point_offsets())
        .filter_map(|(point, offset)| {
            let StoryPoint::Paragraph { text, style } = point else {
                return None;
            };
            Some(
                schist_layout::compose::spec_for(
                    story,
                    offset,
                    offset + text.len(),
                    &doc.styles,
                    style,
                    "Default",
                    400.0,
                )
                .direction,
            )
        })
        .collect()
}

fn without_labels(bytes: &[u8]) -> Vec<u8> {
    let mut parts = container::read(bytes).unwrap().into_parts();
    for (name, bytes) in &mut parts {
        if !name.ends_with(".xml") {
            continue;
        }
        let mut text = String::from_utf8(bytes.clone()).unwrap();
        while let Some(start) = text.find("<Label>") {
            let end = start + text[start..].find("</Label>").unwrap() + "</Label>".len();
            text.replace_range(start..end, "");
        }
        *bytes = text.into_bytes();
    }
    container::write(&parts)
}

#[test]
fn automatic_direction_is_native_per_paragraph_and_stays_editable_after_saving() {
    for parent in [
        None,
        Some(ParagraphDirection::LeftToRight),
        Some(ParagraphDirection::RightToLeft),
    ] {
        let mut doc = blank_a4();
        for (name, based_on, direction) in [
            ("Parent", None, parent),
            ("Auto", Some("Parent"), Some(ParagraphDirection::Auto)),
            ("Inherited", Some("Auto"), None),
            ("Implicit", None, None),
            ("Fixed", None, Some(ParagraphDirection::RightToLeft)),
        ] {
            doc.styles.add_paragraph(ParagraphStyle {
                name: name.into(),
                based_on: based_on.map(str::to_owned),
                direction,
                ..Default::default()
            });
        }
        let mut story = Story::new();
        for style in ["Auto", "Inherited", "Implicit", "Fixed"] {
            for text in ["abc אבג", "123 אבג abc", "مرحبا abc", "123", ""] {
                story.push_paragraph(text, style);
            }
        }
        doc.stories.push(story);
        let expected = directions(&doc);
        let original = doc.stories[0].clone();
        let count = doc.styles.paragraphs.len();
        for _ in 0..5 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let name = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Stories/"))
                .unwrap();
            let tree = xml::parse(package.text(name).unwrap()).unwrap();
            for (range, direction) in tree
                .find_all("ParagraphStyleRange")
                .iter()
                .take(15)
                .zip(&expected)
            {
                let native = match direction {
                    schist_text_engine::ParagraphDirection::RightToLeft => "RightToLeftDirection",
                    _ => "LeftToRightDirection",
                };
                assert_eq!(range.attr("ParagraphDirection"), Some(native));
            }
            // An independent consumer can discard every private label and
            // still get the same actual bidi result from the native XML.
            let native = import::read(&without_labels(&written.bytes))
                .unwrap()
                .document;
            assert_eq!(directions(&native), expected);
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(directions(&doc), expected);
            assert_eq!(doc.stories[0], original);
            assert_eq!(doc.styles.paragraphs.len(), count);
            assert_eq!(
                doc.styles.resolve_paragraph("Auto").direction,
                Some(ParagraphDirection::Auto)
            );
        }
        let StoryPoint::Paragraph { text, .. } = &mut doc.stories[0].points[0] else {
            panic!()
        };
        *text = "אבג abc".into();
        assert_eq!(
            directions(&doc)[0],
            schist_text_engine::ParagraphDirection::RightToLeft
        );
    }
}

#[test]
fn changed_native_text_direction_or_style_invalidates_automatic_metadata() {
    for change in ["text", "direction", "style", "malformed"] {
        let mut doc = blank_a4();
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Auto".into(),
            direction: Some(ParagraphDirection::Auto),
            ..Default::default()
        });
        doc.styles.add_paragraph(ParagraphStyle {
            name: "Other".into(),
            ..Default::default()
        });
        doc.stories.push(Story::from_text("abc", "Auto"));
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let path = package
            .names()
            .into_iter()
            .find(|n| n.starts_with("Stories/"))
            .unwrap()
            .to_owned();
        let original = package.text(&path).unwrap();
        let changed = match change {
            "text" => original.replace("<Content>abc</Content>", "<Content>אבג</Content>"),
            "direction" => original.replace(
                "ParagraphDirection=\"LeftToRightDirection\"",
                "ParagraphDirection=\"RightToLeftDirection\"",
            ),
            "style" => original.replace(
                "AppliedParagraphStyle=\"ParagraphStyle/$ID/Auto\"",
                "AppliedParagraphStyle=\"ParagraphStyle/$ID/Other\"",
            ),
            _ => original.replace(
                "Schist.AutomaticParagraphs.v1",
                "Schist.AutomaticParagraphs.future",
            ),
        };
        assert_ne!(changed, original);
        package.insert(path, changed.into_bytes());
        let back = import::read(&container::write(&package.into_parts()))
            .unwrap()
            .document;
        let StoryPoint::Paragraph { style, .. } = &back.stories[0].points[0] else {
            panic!()
        };
        assert_eq!(
            back.styles.resolve_paragraph(style).direction,
            Some(if change == "direction" {
                ParagraphDirection::RightToLeft
            } else {
                ParagraphDirection::LeftToRight
            })
        );
    }
}

#[test]
fn a_changed_native_style_direction_overrides_its_old_auto_label() {
    let mut doc = blank_a4();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Auto".into(),
        direction: Some(ParagraphDirection::Auto),
        ..Default::default()
    });
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let changed = package.text("Resources/Styles.xml").unwrap().replace(
        "ParagraphDirection=\"LeftToRightDirection\"",
        "ParagraphDirection=\"RightToLeftDirection\"",
    );
    package.insert("Resources/Styles.xml", changed.into_bytes());
    let back = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert_eq!(
        back.styles.resolve_paragraph("Auto").direction,
        Some(ParagraphDirection::RightToLeft)
    );
}

#[test]
fn native_omitted_direction_is_ltr_and_children_keep_inheriting() {
    let mut doc = blank_a4();
    doc.stories.push(Story::from_text("אבג abc", "Child"));
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    package.insert("Resources/Styles.xml", br#"<idPkg:Styles><RootParagraphStyleGroup><ParagraphStyle Self="root" Name="Root"/><ParagraphStyle Self="child" Name="Child"><Properties><BasedOn type="object">root</BasedOn></Properties></ParagraphStyle></RootParagraphStyleGroup></idPkg:Styles>"#.to_vec());
    let path = package
        .names()
        .into_iter()
        .find(|n| n.starts_with("Stories/"))
        .unwrap()
        .to_owned();
    package.insert(path, "<idPkg:Story><Story Self=\"Story0\"><ParagraphStyleRange AppliedParagraphStyle=\"child\"><CharacterStyleRange><Content>אבג abc</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>".as_bytes().to_vec());
    let mut back = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert_eq!(
        directions(&back),
        [schist_text_engine::ParagraphDirection::LeftToRight]
    );
    assert_eq!(back.styles.paragraph("Child").unwrap().direction, None);
    back.styles
        .paragraphs
        .iter_mut()
        .find(|s| s.name == "Root")
        .unwrap()
        .direction = Some(ParagraphDirection::RightToLeft);
    assert_eq!(
        directions(&back),
        [schist_text_engine::ParagraphDirection::RightToLeft]
    );
}
