//! A document that survives being written and read back.
//!
//! Two directions matter and they are different questions.
//!
//! **Out and back**: a `LayoutDocument` written as IDML and read again
//! should be the same document. That is the round-trip test, and it is
//! the acceptance criterion for this phase.
//!
//! **A real file, rewritten**: a document read from a genuine InDesign
//! export and written out again should read back as the same document. It
//! is a harder test, because it starts from a file this writer never saw,
//! and it is the one that would catch a writer that only round-trips its
//! own output.

use schist_codec_idml::{export, import};
use schist_layout::{
    blank_a4, FrameOverflow, Insets, LayoutDocument, LayoutObject, ObjectId, Orientation, Page,
    ParentPage, PlacedObject, Point, Rect, Spread, Story, StoryId, StoryPoint, StyleRange, SubPath,
};

/// A document with one page, a frame, a story and a style, built so that
/// every field the writer carries is set to something distinguishable.
fn document() -> LayoutDocument {
    let mut document = blank_a4();
    document.pages = vec![Page {
        name: "Cover".into(),
        section: None,
        width: 600.0,
        height: 800.0,
        bleed: 3.0.into(),
        slug: 6.0.into(),
        margins: Insets {
            top: 12.0,
            right: 18.0,
            bottom: 24.0,
            left: 30.0,
        },
        orientation: Orientation::Portrait,
        hidden: false,
        master: None,
        guides: Vec::new(),
    }];
    document.spreads = vec![Spread {
        pages: vec![0],
        binding_location: None,
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    document.parents = vec![ParentPage {
        name: "A-Master".into(),
        sheets: Vec::new(),
        placements: Vec::new(),
        applied_to: vec![0],
        based_on: None,
        objects: vec![schist_layout::ParentObject {
            object: PlacedObject {
                hidden: false,
                appearance: Default::default(),
                id: ObjectId::next(),
                page: 0,
                bounds: Rect::new(10.0, 10.0, 100.0, 20.0),
                object: LayoutObject::Shape {
                    path: schist_layout::ShapePath {
                        subpaths: vec![SubPath {
                            handles: Vec::new(),
                            points: vec![
                                Point::ZERO,
                                Point::new(100.0, 0.0),
                                Point::new(100.0, 20.0),
                            ],
                            closed: true,
                        }],
                        even_odd: false,
                    },
                    fill: None,
                    stroke: None,
                    stroke_width: 1.0,
                    fill_overprint: false,
                    stroke_overprint: false,
                    tints: Default::default(),
                },
                rotation: 0.0,
                transform: Default::default(),
                name: "Master rule".into(),
                locked: true,
                overprint: false,
                transparency: 0.0,
            },
            overridden_on: Vec::new(),
        }],
        hidden: false,
    }];
    document.pages[0].master = Some(0);

    // A story with a paragraph, a break and a character range over part
    // of it -- every shape of story the writer has to survive.
    document.stories = vec![Story {
        prefs: Default::default(),
        structures: Vec::new(),
        points: vec![StoryPoint::Paragraph {
            text: "First line\nsecond line".into(),
            style: "Body".into(),
        }],
        ranges: vec![StyleRange::new(0, 10, "Emphasis")],
    }];

    let id = document.add_object(PlacedObject {
        hidden: false,
        appearance: Default::default(),
        id: ObjectId::next(),
        page: 0,
        bounds: Rect::new(-260.0, 181.97, 720.0, 47.0),
        object: LayoutObject::TextFrame {
            footnotes: Default::default(),
            text_path: None,
            story: StoryId(0),
            columns: 2,
            gutter: 18.0,
            insets: Insets {
                top: 4.0,
                right: 5.0,
                bottom: 6.0,
                left: 7.0,
            },
            overflow: FrameOverflow::Clip,
        },
        rotation: 0.0,
        transform: Default::default(),
        name: "Headline <RT_f> & more".into(),
        locked: false,
        overprint: false,
        transparency: 0.0,
    });
    document.object_layers.push((id, document.layers[0]));

    document.inks = vec![
        schist_layout::Ink::process("Process Cyan", [0.0, 1.0, 1.0]),
        schist_layout::Ink::spot("PANTONE 185 C", [0.9, 0.1, 0.2]),
    ];
    document
}

fn round_trip(document: &LayoutDocument) -> (LayoutDocument, Vec<String>) {
    let written = export::write(document);
    let read = import::read(&written.bytes)
        .unwrap_or_else(|error| panic!("our own output did not read: {error}"));
    (read.document, written.warnings)
}

#[test]
fn a_document_survives_being_written_and_read_back() {
    let original = document();
    let (back, _) = round_trip(&original);

    // The page, including the margins, which are four separate numbers
    // and the easiest thing to lose.
    assert_eq!(back.pages.len(), 1);
    assert_eq!(back.pages[0].width, original.pages[0].width);
    assert_eq!(back.pages[0].height, original.pages[0].height);
    assert_eq!(back.pages[0].name, "Cover");
    assert_eq!(back.pages[0].margins, original.pages[0].margins);
    assert_eq!(back.pages[0].master, Some(0), "the master survived");

    // The frame, with a name that needed escaping in both directions.
    let frame = back
        .objects
        .iter()
        .find(|object| matches!(object.object, LayoutObject::TextFrame { .. }))
        .expect("the frame came back");
    let original_frame = original
        .objects
        .iter()
        .find(|object| matches!(object.object, LayoutObject::TextFrame { .. }))
        .expect("the frame went out");
    assert_eq!(frame.name, original_frame.name, "an escaped name");
    assert_eq!(frame.bounds, original_frame.bounds, "position and size");
    let LayoutObject::TextFrame {
        columns,
        gutter,
        insets,
        ..
    } = &frame.object
    else {
        panic!("expected a text frame");
    };
    assert_eq!(*columns, 2, "the column count");
    assert_eq!(*gutter, 18.0);
    assert_eq!(*insets, insets_of(original_frame), "all four insets");
    assert!(!frame.locked, "an unlocked frame stays unlocked");
}

#[test]
fn a_story_survives_its_text_its_break_and_its_ranges() {
    let original = document();
    let (back, _) = round_trip(&original);
    assert_eq!(back.stories.len(), 1);
    let story = &back.stories[0];
    let StoryPoint::Paragraph { text, style } = &story.points[0] else {
        panic!("expected a paragraph, got {:?}", story.points[0]);
    };
    // The `<Br/>` has to come back as a newline, not as a literal or as
    // nothing.
    assert_eq!(text, "First line\nsecond line");
    assert_eq!(style, "Body");
    // And the character range has to cover the same bytes.
    assert_eq!(story.ranges.len(), 1);
    assert_eq!(story.ranges[0].start, 0);
    assert_eq!(story.ranges[0].end, 10);
    assert_eq!(story.ranges[0].style, "Emphasis");
}

#[test]
fn the_inks_come_back_with_their_kinds() {
    let original = document();
    let (back, _) = round_trip(&original);
    assert_eq!(back.inks.len(), 2);
    // The spot is the one that matters: an ink that needs its own plate
    // and is read back as process is a print error, not a cosmetic one.
    let spot = back
        .inks
        .iter()
        .find(|ink| ink.name == "PANTONE 185 C")
        .expect("the spot came back");
    assert!(spot.spot, "a spot ink must not come back as process");
    let process = back
        .inks
        .iter()
        .find(|ink| ink.name == "Process Cyan")
        .expect("the process ink came back");
    assert!(!process.spot);
}

#[test]
fn a_master_page_contributes_its_item() {
    let original = document();
    let (back, _) = round_trip(&original);
    assert_eq!(back.parents.len(), 1);
    assert_eq!(back.parents[0].name, "A-Master");
    assert_eq!(
        back.parents[0].applied_to,
        vec![0],
        "the page it applies to"
    );
    assert_eq!(
        back.parents[0].objects.len(),
        1,
        "the master's own item came back"
    );
    let item = &back.parents[0].objects[0].object;
    assert_eq!(item.name, "Master rule");
    assert!(item.locked, "a locked master item stays locked");
    assert_eq!(item.bounds, original.parents[0].objects[0].object.bounds);
}

#[test]
fn a_package_we_write_is_shaped_like_one_indesign_wrote() {
    // The OPC rules, which a reader depends on before it has read
    // anything: `mimetype` first and stored.
    let written = export::write(&document());
    let first = container_first(&written.bytes);
    assert_eq!(
        first,
        Some((
            String::from("mimetype"),
            schist_codec_idml::container::METHOD_STORE
        )),
        "{first:?}"
    );
    let package = schist_codec_idml::container::open(&written.bytes).expect("it opens");
    // And the parts a real export has, so a reader looking for them
    // finds them.
    for part in [
        "META-INF/container.xml",
        "META-INF/metadata.xml",
        "designmap.xml",
    ] {
        assert!(package.get(part).is_some(), "missing {part}");
    }
    assert!(
        package
            .names()
            .iter()
            .any(|name| name.starts_with("Spreads/Spread_")),
        "no spread part"
    );
    assert!(
        package
            .names()
            .iter()
            .any(|name| name.starts_with("Stories/Story_")),
        "no story part"
    );
}

#[test]
fn the_system_unzip_accepts_what_we_write() {
    // A file our own reader opens proves self-consistency and nothing
    // else. The system tool is the other kind of evidence.
    if !have("unzip") {
        eprintln!("skipping: no system `unzip`");
        return;
    }
    let dir = std::env::temp_dir().join(format!("schist-idml-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("out.idml");
    let written = export::write(&document());
    std::fs::write(&path, &written.bytes).expect("writing");

    let test = std::process::Command::new("unzip")
        .args(["-t", "-qq"])
        .arg(&path)
        .output()
        .expect("running unzip");
    assert!(
        test.status.success(),
        "unzip rejected our package:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_export_read_and_written_again_reads_as_the_same_document() {
    // The harder direction: a document this writer never saw, taken
    // through a write and a read.
    for (name, fixture) in FIXTURES {
        let first =
            import::read(fixture).unwrap_or_else(|error| panic!("{name} did not read: {error}"));
        let written = export::write(&first.document);
        let second = import::read(&written.bytes)
            .unwrap_or_else(|error| panic!("{name} did not survive our writer: {error}"));
        compare(name, &first.document, &second.document);
    }
}

/// The fixtures, by name.
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "bounded-text.idml",
        include_bytes!("../../../fixtures/idml/bounded-text.idml"),
    ),
    (
        "images.idml",
        include_bytes!("../../../fixtures/idml/images.idml"),
    ),
    (
        "multipage.idml",
        include_bytes!("../../../fixtures/idml/multipage.idml"),
    ),
    (
        "shapes.idml",
        include_bytes!("../../../fixtures/idml/shapes.idml"),
    ),
    (
        "text.idml",
        include_bytes!("../../../fixtures/idml/text.idml"),
    ),
];

/// What has to survive a write and a read, for a document read from a
/// real file.
fn compare(name: &str, before: &LayoutDocument, after: &LayoutDocument) {
    assert_eq!(after.pages.len(), before.pages.len(), "{name}: page count");
    for (index, (was, now)) in before.pages.iter().zip(&after.pages).enumerate() {
        assert_eq!(now.width, was.width, "{name}: page {index} width");
        assert_eq!(now.height, was.height, "{name}: page {index} height");
        assert_eq!(now.name, was.name, "{name}: page {index} name");
        assert_eq!(now.master, was.master, "{name}: page {index} master");
    }
    assert_eq!(
        after.stories.len(),
        before.stories.len(),
        "{name}: story count"
    );
    for (index, (was, now)) in before.stories.iter().zip(&after.stories).enumerate() {
        assert_eq!(
            now.points.len(),
            was.points.len(),
            "{name}: story {index} point count"
        );
        for (point, (was, now)) in was.points.iter().zip(&now.points).enumerate() {
            if let (
                StoryPoint::Paragraph { text: before, .. },
                StoryPoint::Paragraph { text: after, .. },
            ) = (was, now)
            {
                assert_eq!(before, after, "{name}: story {index} point {point} text");
            }
        }
    }
    assert_eq!(
        after.objects.len(),
        before.objects.len(),
        "{name}: object count"
    );
    // Bounds recomputed from cubic extrema can round by a few f32 ULPs.
    // Keep a strict sub-millipoint tolerance, not exact floating equality.
    for (index, (was, now)) in before.objects.iter().zip(&after.objects).enumerate() {
        assert_eq!(now.name, was.name, "{name}: object {index} name");
        assert!(
            (now.bounds.width - was.bounds.width).abs() < 0.001,
            "{name}: object {index} ({}) width",
            was.name
        );
        assert!(
            (now.bounds.height - was.bounds.height).abs() < 0.001,
            "{name}: object {index} ({}) height",
            was.name
        );
    }
    assert_eq!(after.inks.len(), before.inks.len(), "{name}: ink count");
    for (was, now) in before.inks.iter().zip(&after.inks) {
        assert_eq!(now.name, was.name, "{name}: ink name");
        assert_eq!(now.spot, was.spot, "{name}: ink {} is a spot", was.name);
    }
}

/// The first entry's name and method, without decoding the package.
fn container_first(bytes: &[u8]) -> Option<(String, u16)> {
    schist_codec_idml::container::method_of_first(bytes)
}

fn have(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A frame's insets, for the assertions above.
fn insets_of(object: &PlacedObject) -> Insets {
    match &object.object {
        LayoutObject::TextFrame { insets, .. } => *insets,
        _ => Insets::default(),
    }
}

#[test]
fn layer_order_properties_and_membership_survive_idml() {
    use schist_layout::{structure, History, LayerId};
    let mut original = document();
    let mut history = History::default();
    let upper =
        structure::add_layer(&mut original, &mut history, "Upper & special".into()).unwrap();
    let lower = LayerId(0);
    let ids: Vec<_> = original.objects.iter().map(|o| o.id).collect();
    assert!(structure::move_objects_to_layer(
        &mut original,
        &mut history,
        &ids,
        upper
    ));
    for visible in [false, true] {
        for locked in [false, true] {
            structure::change_layer(&mut original, &mut history, upper, |layer| {
                layer.visible = visible;
                layer.locked = locked;
            });
            structure::change_layer(&mut original, &mut history, lower, |layer| {
                layer.name = "Lower".into()
            });
            let (back, _) = round_trip(&original);
            assert_eq!(back.layers, original.layers);
            for layer in &original.layer_properties {
                assert_eq!(
                    back.layer_properties.iter().find(|p| p.id == layer.id),
                    Some(layer)
                );
            }
            for (object, source) in back.objects.iter().zip(&original.objects) {
                assert_eq!(
                    back.object_layer(object.id),
                    original.object_layer(source.id)
                );
            }
        }
    }
}

#[test]
fn named_typography_preserves_values_inheritance_and_xml_characters() {
    use schist_layout::{CharacterStyle, ParagraphStyle};
    let mut original = document();
    let paragraph = ParagraphStyle {
        name: "Heading / 空 & display".into(),
        based_on: Some("Body".into()),
        next: Some("Body".into()),
        point_size: Some(27.0),
        leading: Some(schist_layout::styles::Leading::Points(32.5)),
        left_indent: Some(4.0),
        right_indent: Some(6.0),
        first_line_indent: Some(-2.0),
        space_before: Some(9.0),
        space_after: Some(3.0),
        align: Some(schist_layout::styles::Align::Justify),
        hyphenate: Some(false),
        ..Default::default()
    };
    let character = CharacterStyle {
        name: "Emphasis / 空 & caption".into(),
        based_on: Some("Default".into()),
        family: Some("A & B".into()),
        point_size: Some(10.0),
        leading: Some(schist_layout::styles::Leading::Points(11.0)),
        bold: Some(true),
        italic: Some(false),
        underline: Some(true),
        strikethrough: Some(false),
        ..Default::default()
    };
    original.styles.add_paragraph(paragraph.clone());
    original.styles.add_character(character.clone());
    original.stories[0] = Story::from_text("A & B < C", &paragraph.name);
    original.stories[0].apply_style(0, 5, &character.name);
    let (back, _) = round_trip(&original);
    assert_eq!(back.styles.paragraph(&paragraph.name), Some(&paragraph));
    assert_eq!(back.styles.character(&character.name), Some(&character));
    assert_eq!(back.stories, original.stories);
}
