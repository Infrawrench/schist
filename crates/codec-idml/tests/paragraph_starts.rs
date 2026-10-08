use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, compose, FrameOverflow, History, Insets, LayoutDocument, LayoutObject, Page,
    ParagraphStyle, Rect, Story, StoryId,
};

const POLICIES: [&str; 6] = [
    "Anywhere",
    "NextColumn",
    "NextFrame",
    "NextPage",
    "NextOddPage",
    "NextEvenPage",
];

fn native(policy: &str, columns: u16) -> LayoutDocument {
    let mut doc = schist_layout::blank_a4();
    doc.add_page(Page::a4());
    doc.add_page(Page::a4());
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Starts".into(),
        based_on: Some("Body".into()),
        ..Default::default()
    });
    let mut ids = Vec::new();
    for page in [0, 0, 1, 1, 2, 2] {
        let created = authoring::text_frame(
            &mut doc,
            &mut History::default(),
            page,
            Rect::new(0.0, 0.0, 500.0, 700.0),
        )
        .unwrap();
        let placed = doc
            .objects
            .iter_mut()
            .find(|o| o.id == created.object)
            .unwrap();
        if let LayoutObject::TextFrame {
            story,
            columns: count,
            insets,
            overflow,
            balance_columns,
            ..
        } = &mut placed.object
        {
            *story = StoryId(0);
            *count = columns;
            *insets = Insets::ZERO;
            *overflow = FrameOverflow::Thread;
            *balance_columns = Some(false);
        }
        ids.push(created.object);
    }
    let mut story = Story::from_text("Before", "Body");
    story.push_paragraph("café chapter", "Starts");
    doc.stories[0] = story;
    doc.thread_order = vec![(StoryId(0), ids)];
    let mut package = container::read(&export::write(&doc).bytes).unwrap();
    let styles = package.text("Resources/Styles.xml").unwrap().replace(
        "Name=\"Starts\"",
        &format!("Name=\"Starts\" StartParagraph=\"{policy}\""),
    );
    package.insert("Resources/Styles.xml", styles.into_bytes());
    let imported = import::read(&container::write(&package.into_parts()))
        .unwrap()
        .document;
    assert_eq!(imported.pages.len(), 3);
    assert_eq!(imported.objects.len(), 6);
    imported
}

#[test]
fn every_native_paragraph_start_survives_repeated_saves_without_changing_text() {
    for policy in POLICIES {
        let mut doc = native(policy, 2);
        let text = doc.stories[0].text();
        for _ in 0..3 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
            let style = root
                .find_all("ParagraphStyle")
                .into_iter()
                .find(|style| style.attr("Name") == Some("Starts"))
                .unwrap();
            assert_eq!(style.attr("StartParagraph"), Some(policy), "{policy}");
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(doc.stories[0].text(), text);
        }
    }
}

#[test]
fn native_paragraph_starts_obey_column_frame_and_numbered_page_boundaries() {
    for columns in [1, 2, 3] {
        for policy in POLICIES {
            let doc = native(policy, columns);
            let original = doc.clone();
            let flow = compose::compose_story(&doc, StoryId(0));
            let expected_frame = match policy {
                "Anywhere" => 0,
                "NextColumn" if columns > 1 => 0,
                "NextColumn" | "NextFrame" => 1,
                "NextPage" | "NextEvenPage" => 2,
                "NextOddPage" => 4,
                _ => unreachable!(),
            };
            let after = flow
                .frames
                .iter()
                .enumerate()
                .flat_map(|(frame, result)| {
                    result
                        .lines
                        .iter()
                        .filter(|line| line.start >= 7)
                        .map(move |line| (frame, line))
                })
                .collect::<Vec<_>>();
            assert_eq!(after.len(), 1, "{policy}, columns={columns}");
            assert_eq!(after[0].0, expected_frame, "{policy}, columns={columns}");
            let line = after[0].1;
            assert_eq!(doc.stories[0].slice(line.start, line.end), "café chapter");
            let first = &flow.frames[0].lines[0];
            if policy == "Anywhere" {
                assert!(line.bounds.y >= first.bounds.bottom());
            } else {
                assert!((line.bounds.y - first.bounds.y).abs() < 0.01, "{policy}");
            }
            let expected_x = if policy == "NextColumn" && columns > 1 {
                500.0 / columns as f32
            } else {
                0.0
            };
            assert!((line.bounds.x - expected_x).abs() < 0.01, "{policy}");
            assert!(!flow.has_overflow());
            assert_eq!(doc, original);
        }
    }
}

#[test]
fn local_starts_override_inherited_policies_without_growing_styles_or_changing_source() {
    use schist_layout::{styles::ParagraphStart, StoryPoint};
    for base in POLICIES {
        for local in POLICIES {
            let doc = native(base, 2);
            let mut package = container::read(&export::write(&doc).bytes).unwrap();
            let path = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("Stories/"))
                .unwrap()
                .to_owned();
            // Both ranges use a child with no local start setting. Only the
            // second has a native local override, including explicit Anywhere.
            let styles = package.text("Resources/Styles.xml").unwrap();
            let root = xml::parse(styles).unwrap();
            let parent = root
                .find_all("ParagraphStyle")
                .into_iter()
                .find(|s| s.attr("Name") == Some("Starts"))
                .unwrap()
                .attr("Self")
                .unwrap();
            let story = format!(
                r#"<idPkg:Story><Story Self="SchistStory0"><ParagraphStyleRange AppliedParagraphStyle="child"><CharacterStyleRange><Content>é</Content><Br/></CharacterStyleRange></ParagraphStyleRange><ParagraphStyleRange AppliedParagraphStyle="child" StartParagraph="{local}"><CharacterStyleRange><Content>中😀</Content></CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"#
            );
            let styles = format!(
                r#"<idPkg:Styles><ParagraphStyle Self="{parent}" Name="Starts" StartParagraph="{base}"/><ParagraphStyle Self="child" Name="Child"><Properties><BasedOn type="object">{parent}</BasedOn></Properties></ParagraphStyle></idPkg:Styles>"#
            );
            package.insert("Resources/Styles.xml", styles.into_bytes());
            package.insert(path, story.into_bytes());
            let mut doc = import::read(&container::write(&package.into_parts()))
                .unwrap()
                .document;
            let count = doc.styles.paragraphs.len();
            for _ in 0..3 {
                assert_eq!(doc.stories[0].text(), "é\n中😀");
                let policies: Vec<_> = doc.stories[0]
                    .points
                    .iter()
                    .filter_map(|point| match point {
                        StoryPoint::Paragraph { style, .. } => {
                            Some(doc.styles.resolve_paragraph(style).start_paragraph)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    policies,
                    [
                        ParagraphStart::from_native(base),
                        ParagraphStart::from_native(local)
                    ]
                );
                assert_eq!(doc.styles.paragraph("Child").unwrap().start_paragraph, None);
                assert_eq!(doc.styles.paragraphs.len(), count);
                doc = import::read(&export::write(&doc).bytes).unwrap().document;
            }
        }
    }
}

#[test]
fn malformed_native_starts_are_reported_instead_of_becoming_valid_choices() {
    for value in ["nextPage", "NextUnknownPage", "", "0"] {
        let doc = native("Anywhere", 1);
        let mut package = container::read(&export::write(&doc).bytes).unwrap();
        let styles = package.text("Resources/Styles.xml").unwrap().replace(
            "StartParagraph=\"Anywhere\"",
            &format!("StartParagraph=\"{value}\""),
        );
        package.insert("Resources/Styles.xml", styles.into_bytes());
        let imported = import::read(&container::write(&package.into_parts())).unwrap();
        assert_eq!(
            imported
                .document
                .styles
                .paragraph("Starts")
                .unwrap()
                .start_paragraph,
            None
        );
        let expected = schist_i18n::tf!(
            "design.idml_text_preference_invalid",
            property = "StartParagraph",
            value = value
        );
        assert!(imported
            .report
            .skipped
            .iter()
            .any(|warning| warning == &expected));
    }
}
