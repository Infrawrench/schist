use schist_codec_idml::{container, export, import, xml};
use schist_layout::{
    authoring, compose_thread, FrameOverflow, History, Insets, LayoutDocument, ObjectId,
    ParagraphStyle, Rect, Story, StoryId,
};

fn native(attributes: &str) -> LayoutDocument {
    let mut doc = schist_layout::blank_a4();
    let frame = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 100.0, 8000.0),
    )
    .unwrap();
    doc.styles.add_paragraph(ParagraphStyle {
        name: "Keeps".into(),
        point_size: Some(12.0),
        leading: Some(schist_layout::styles::Leading::Points(16.0)),
        ..Default::default()
    });
    doc.stories[frame.story.0 as usize] = Story::from_text(
        "Alpha café beta gamma delta epsilon zeta eta theta. ".repeat(4),
        "Keeps",
    );
    let bytes = export::write(&doc).bytes;
    let package = container::read(&bytes).unwrap();
    let parts = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = if name == "Resources/Styles.xml" {
                package
                    .text(name)
                    .unwrap()
                    .replace("Name=\"Keeps\"", &format!("Name=\"Keeps\" {attributes}"))
                    .into_bytes()
            } else {
                package.get(name).unwrap().to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect::<Vec<_>>();
    import::read(&container::write(&parts)).unwrap().document
}

fn flow(doc: &LayoutDocument, height: f32) -> schist_layout::ComposedThread {
    compose_thread(
        doc,
        StoryId(0),
        &[(
            ObjectId(0),
            Rect::new(0.0, 0.0, 100.0, height),
            FrameOverflow::Clip,
            1,
            0.0,
            Insets::ZERO.into(),
        )],
    )
}

#[test]
fn native_keep_enable_all_and_asymmetric_counts_control_every_possible_split() {
    for enabled in [false, true] {
        for all in [false, true] {
            for (first, last) in [(1, 3), (3, 1), (2, 2)] {
                let doc = native(&format!(
                    "KeepLinesTogether=\"{enabled}\" KeepAllLinesTogether=\"{all}\" \
                     KeepFirstLines=\"{first}\" KeepLastLines=\"{last}\""
                ));
                let original = doc.clone();
                let full = flow(&doc, 8000.0);
                let lines = &full.frames[0].lines;
                assert!(lines.len() > 6 && !full.has_overflow());
                for room in 1..lines.len() {
                    let height = lines[..room]
                        .iter()
                        .map(|line| line.bounds.bottom())
                        .fold(0.0, f32::max)
                        + 0.01;
                    let actual = flow(&doc, height);
                    let mut expected = room;
                    if enabled {
                        if all {
                            expected = 0;
                        } else {
                            if lines.len() - expected < last {
                                expected = lines.len().saturating_sub(last);
                            }
                            if expected < first {
                                expected = 0;
                            }
                        }
                    }
                    assert_eq!(actual.frames[0].lines.len(), expected,
                        "enabled={enabled}, all={all}, first={first}, last={last}, room={room}, total={}", lines.len());
                    for (actual, expected) in actual.frames[0].lines.iter().zip(lines) {
                        assert_eq!((actual.start, actual.end), (expected.start, expected.end));
                    }
                    assert_eq!(doc, original);
                }
            }
        }
    }
}

#[test]
fn native_keep_flags_and_counts_survive_repeated_native_saves() {
    for enabled in [false, true] {
        let attributes = [
            ("KeepLinesTogether", enabled.to_string()),
            ("KeepAllLinesTogether", "true".into()),
            ("KeepFirstLines", "3".into()),
            ("KeepLastLines", "4".into()),
            ("KeepWithNext", "5".into()),
            ("KeepWithPrevious", "true".into()),
        ];
        let mut doc = native(
            &attributes
                .iter()
                .map(|(key, value)| format!("{key}=\"{value}\""))
                .collect::<Vec<_>>()
                .join(" "),
        );
        for _ in 0..3 {
            let written = export::write(&doc);
            let package = container::read(&written.bytes).unwrap();
            let root = xml::parse(package.text("Resources/Styles.xml").unwrap()).unwrap();
            let style = root
                .find_all("ParagraphStyle")
                .into_iter()
                .find(|element| element.attr("Name") == Some("Keeps"))
                .unwrap();
            for (key, value) in &attributes {
                assert_eq!(style.attr(key), Some(value.as_str()), "{key}");
            }
            doc = import::read(&written.bytes).unwrap().document;
        }
    }
}

fn rewrite(
    doc: &LayoutDocument,
    change: impl Fn(&str) -> String,
) -> schist_codec_idml::import::Imported {
    let written = export::write(doc);
    let package = container::read(&written.bytes).unwrap();
    let parts: Vec<_> = package
        .names()
        .into_iter()
        .map(|name| {
            let bytes = if name == "Resources/Styles.xml" {
                change(package.text(name).unwrap()).into_bytes()
            } else {
                package.get(name).unwrap().to_vec()
            };
            (name.to_owned(), bytes)
        })
        .collect();
    import::read(&container::write(&parts)).unwrap()
}

#[test]
fn legacy_fields_and_native_overrides_are_lossless_until_native_attributes_change() {
    use schist_layout::paragraph_keeps::ParagraphKeeps;
    for count in [0, 1, 3, 50, 51] {
        let mut doc = native("");
        let style = doc
            .styles
            .paragraphs
            .iter_mut()
            .find(|s| s.name == "Keeps")
            .unwrap();
        style.keep_lines = Some(count);
        style.keep_with_next = Some(true);
        style.keeps = ParagraphKeeps {
            enabled: Some(false),
            last: Some(4),
            ..Default::default()
        };
        let authored = style.clone();
        for _ in 0..3 {
            let written = export::write(&doc);
            assert_eq!(
                written
                    .warnings
                    .iter()
                    .any(|w| w.contains("KeepFirstLines")),
                count > 50
            );
            doc = import::read(&written.bytes).unwrap().document;
            assert_eq!(
                doc.styles
                    .paragraphs
                    .iter()
                    .find(|s| s.name == "Keeps")
                    .unwrap(),
                &authored
            );
        }
        for (from, to) in [
            ("KeepLinesTogether=\"false\"", "KeepLinesTogether=\"true\""),
            ("KeepWithNext=\"1\"", "KeepWithNext=\"0\""),
            ("KeepLastLines=\"4\"", "KeepLastLines=\"3\""),
            ("KeepLastLines=\"4\"", "KeepLastLines=\"broken\""),
        ] {
            let changed = rewrite(&doc, |xml| xml.replace(from, to));
            let style = changed
                .document
                .styles
                .paragraphs
                .iter()
                .find(|s| s.name == "Keeps")
                .unwrap();
            assert_eq!(style.keep_lines, None);
            assert_eq!(style.keep_with_next, None);
            assert_ne!(style.keeps, authored.keeps);
        }
    }
}

#[test]
fn invalid_native_counts_are_reported_without_reactivating_authored_metadata() {
    use schist_layout::paragraph_keeps::ParagraphKeeps;
    for (key, values) in [
        ("KeepFirstLines", ["0", "51", "-1", "1.5"]),
        ("KeepLastLines", ["0", "51", "-1", "NaN"]),
        ("KeepWithNext", ["6", "-1", "1.5", "bad"]),
        ("KeepLinesTogether", ["yes", "2", "-1", ""]),
    ] {
        for value in values {
            let mut doc = native("");
            let style = doc
                .styles
                .paragraphs
                .iter_mut()
                .find(|s| s.name == "Keeps")
                .unwrap();
            style.keep_with_next = Some(false);
            let changed = rewrite(&doc, |xml| {
                let xml = xml.replace("KeepWithNext=\"0\"", "");
                xml.replace(
                    "Name=\"Keeps\"",
                    &format!("Name=\"Keeps\" {key}=\"{value}\""),
                )
            });
            assert!(
                changed.report.skipped.iter().any(|w| w.contains(key)),
                "{key}: {value}"
            );
            let style = changed
                .document
                .styles
                .paragraphs
                .iter()
                .find(|s| s.name == "Keeps")
                .unwrap();
            assert_eq!(style.keeps, ParagraphKeeps::default());
            assert_eq!(style.keep_with_next, None);
        }
    }
}
