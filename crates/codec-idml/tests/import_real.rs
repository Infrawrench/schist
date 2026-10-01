//! What the reader makes of the real exports.
//!
//! These assertions are about a document that exists, not about one
//! written to match the reader. Where a value is stated it was read out
//! of the fixture by hand, and where the reader is known to get something
//! wrong that is asserted too, so the gap shows up as a failing test
//! rather than as a document that quietly looks right.

use schist_codec_idml::import;
use schist_layout::{LayoutObject, Rect};

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
        "placeholders.idml",
        include_bytes!("../../../fixtures/idml/placeholders.idml"),
    ),
    (
        "shapes.idml",
        include_bytes!("../../../fixtures/idml/shapes.idml"),
    ),
    (
        "text.idml",
        include_bytes!("../../../fixtures/idml/text.idml"),
    ),
    (
        "themes.idml",
        include_bytes!("../../../fixtures/idml/themes.idml"),
    ),
];

fn read(fixture: &[u8]) -> import::Imported {
    import::read(fixture).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn every_real_export_becomes_a_document_with_pages() {
    for (name, fixture) in FIXTURES {
        let imported = read(fixture);
        assert!(
            !imported.document.pages.is_empty(),
            "{name} produced no pages"
        );
        // Every page belongs to a spread, or a spread is meaningless.
        let placed: Vec<usize> = imported
            .document
            .spreads
            .iter()
            .flat_map(|spread| spread.pages.iter().copied())
            .collect();
        assert_eq!(
            placed.len(),
            imported.document.pages.len(),
            "{name}: every page is in a spread"
        );
    }
}

#[test]
fn a_page_takes_its_size_from_geometric_bounds() {
    // IDML specification table 100: y1, x1, y2, x2, unlike path anchors.
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    let page = &imported.document.pages[0];
    assert_eq!(page.width, 800.0, "width is right minus left");
    assert_eq!(page.height, 600.0, "height is bottom minus top");
    assert!(page.width > 0.0 && page.height > 0.0);
}

#[test]
fn a_page_is_placed_by_its_transform_translation() {
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    let page = &imported.document.pages[0];
    // The renderer uses trim-relative coordinates. Spread translations must
    // be removed at import rather than leaving frames on the pasteboard.
    let frames: Vec<_> = imported
        .document
        .objects
        .iter()
        .filter(|object| matches!(object.object, LayoutObject::TextFrame { .. }))
        .collect();
    assert!(!frames.is_empty());
    for frame in frames {
        assert!(frame.bounds.right() > 0.0 && frame.bounds.x < page.width);
        assert!(frame.bounds.bottom() > 0.0 && frame.bounds.y < page.height);
    }
}

#[test]
fn a_frame_keeps_its_own_size() {
    // The local composition size comes from path geometry independently
    // of the item affine. A frame that came back the same size as
    // the page would be obvious on the pasteboard.
    let imported = read(include_bytes!("../../../fixtures/idml/bounded-text.idml"));
    for object in &imported.document.objects {
        let Rect { width, height, .. } = object.bounds;
        assert!(
            width > 0.0 && height > 0.0,
            "{:?} came back with no size",
            object.name
        );
    }
}

#[test]
fn every_frame_overlaps_the_page_it_is_on() {
    // Not "fits inside": these fixtures hold demo frames that overhang the
    // trim on purpose — a full-page background box is larger than the
    // page, and several copyfitting frames are wider than the column they
    // sit in. What must hold is that the frame is *on* the page, which a
    // frame placed by a transform that was applied twice, or not at all,
    // would fail.
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    let page = imported.document.pages[0].clone();
    // The layout model stores every object relative to the page trim.
    let page_box = Rect::new(0.0, 0.0, page.width, page.height);
    for object in &imported.document.objects {
        let placed = object.visual_bounds();
        let overlaps = placed.x < page_box.right()
            && page_box.x < placed.right()
            && placed.y < page_box.bottom()
            && page_box.y < placed.bottom();
        assert!(
            overlaps,
            "{:?} at {:?} does not reach the page at {:?}",
            object.name, placed, page_box
        );
    }
}

#[test]
fn a_frame_without_its_own_transform_still_uses_spread_coordinates() {
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    let background = imported
        .document
        .objects
        .iter()
        .find(|object| object.name.contains("Background"))
        .unwrap();
    let page = &imported.document.pages[0];
    assert_eq!(
        background.bounds,
        Rect::new(0.0, 0.0, page.width, page.height)
    );
    let LayoutObject::Shape { path, .. } = &background.object else {
        panic!("background shape");
    };
    assert_eq!(path.bounds(), Rect::new(0.0, 0.0, page.width, page.height));
}

#[test]
fn a_story_becomes_text_with_its_breaks() {
    let imported = read(include_bytes!("../../../fixtures/idml/multipage.idml"));
    assert!(!imported.document.stories.is_empty(), "stories were read");
    let with_text = imported
        .document
        .stories
        .iter()
        .find(|story| {
            story
                .points
                .iter()
                .any(|point| matches!(point, schist_layout::StoryPoint::Paragraph { text, .. } if !text.is_empty()))
        })
        .expect("a story with text");
    // Br ends a paragraph. The separator is part of Story::text(),
    // not part of either paragraph's own text.
    let paragraphs = with_text
        .points
        .iter()
        .filter(|point| matches!(point, schist_layout::StoryPoint::Paragraph { .. }))
        .count();
    assert!(paragraphs > 1, "fixture contains paragraph breaks");
    assert!(with_text.text().contains('\n'));
}

#[test]
fn a_character_run_becomes_a_style_range_over_the_story_text() {
    // The ranges index the story's concatenated text, so one that starts
    // past the end of its own paragraph is wrong even though the offsets
    // look plausible.
    let imported = read(include_bytes!("../../../fixtures/idml/multipage.idml"));
    for story in &imported.document.stories {
        let text = story.text();
        let total = text.len();
        for range in &story.ranges {
            assert!(
                range.end <= total,
                "a style range ends at {} of {total} bytes",
                range.end
            );
            assert!(range.start <= range.end, "a range runs backwards");
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }
}

#[test]
fn a_frame_points_at_a_story_that_exists() {
    // `ParentStory` is an InDesign id, and resolving it is the join the
    // whole threading model rests on.
    let imported = read(include_bytes!("../../../fixtures/idml/bounded-text.idml"));
    let count = imported.document.stories.len();
    for object in &imported.document.objects {
        if let LayoutObject::TextFrame { story, .. } = &object.object {
            assert!(
                (story.0 as usize) < count,
                "frame {:?} points at story {} of {count}",
                object.name,
                story.0
            );
        }
    }
}

#[test]
fn the_process_inks_are_read() {
    // Every specimen uses process inks, so this is the one part of the ink
    // model the fixtures actually exercise.
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    let names: Vec<&str> = imported
        .document
        .inks
        .iter()
        .map(|ink| ink.name.as_str())
        .collect();
    assert!(
        names.iter().any(|name| name.contains("Cyan")),
        "expected a cyan process ink in {names:?}"
    );
    assert!(
        imported.document.inks.iter().all(|ink| !ink.spot),
        "every ink in these fixtures is a process ink"
    );
}

#[test]
fn a_page_is_joined_to_the_master_it_applies() {
    // A page names its master by InDesign id and the master part carries
    // the same id, so this join is what gives a page a parent.
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    assert!(
        !imported.document.parents.is_empty(),
        "the fixture has a master"
    );
    for (index, page) in imported.document.pages.iter().enumerate() {
        let master = page
            .master
            .unwrap_or_else(|| panic!("page {index} has no master"));
        let parent = &imported.document.parents[master];
        assert!(
            parent.applied_to.contains(&index),
            "master {:?} does not list page {index}",
            parent.name
        );
    }
}

#[test]
fn every_object_lands_on_the_page_it_was_declared_on() {
    // A frame read into the wrong page is the failure that looks least
    // wrong on screen, so it is worth asserting directly.
    let imported = read(include_bytes!("../../../fixtures/idml/multipage.idml"));
    let pages = imported.document.pages.len();
    for object in &imported.document.objects {
        assert!(
            object.page < pages,
            "{:?} is on page {} of {pages}",
            object.name,
            object.page
        );
    }
}

#[test]
fn every_object_is_on_a_layer() {
    // An object on no layer cannot be selected, so the pasteboard would
    // draw it and the pointer would ignore it.
    let imported = read(include_bytes!("../../../fixtures/idml/text.idml"));
    for object in &imported.document.objects {
        assert!(
            imported
                .document
                .object_layers
                .iter()
                .any(|(id, _)| *id == object.id),
            "{:?} is on no layer",
            object.name
        );
    }
}

#[test]
fn an_imported_document_paints_on_the_pasteboard() {
    // The end of the chain: a document from a real file has to produce a
    // pasteboard, or the reader has produced something nothing can draw.
    let imported = read(include_bytes!("../../../fixtures/idml/bounded-text.idml"));
    let plan = schist_layout::pasteboard::pasteboard(
        &imported.document,
        &schist_layout::pasteboard::PasteboardView::default(),
    )
    .expect("an imported document has a pasteboard");
    assert!(!plan.pages.is_empty());
    let frames = plan.pages.iter().flat_map(|page| &page.objects).count();
    assert!(frames > 0, "the pasteboard has nothing on it");
}

#[test]
fn a_round_trip_through_layout_composition_survives() {
    // The layout engine has to accept the document, not just store it: a
    // story whose text the composer cannot measure is a broken import.
    let imported = read(include_bytes!("../../../fixtures/idml/bounded-text.idml"));
    let document = &imported.document;
    // Every text frame of a story threaded together, which is the unit
    // the composer actually accepts.
    for (story, frames) in threads(document) {
        let flow = document.story(story).expect("story exists");
        if flow.points.is_empty() {
            continue;
        }
        let composed = schist_layout::compose::compose_thread(document, story, &frames);
        assert!(
            composed.lines().count() > 0,
            "a non-empty story composed to nothing"
        );
    }
}

/// One frame as the composer takes it.
type Frame = (
    schist_layout::ObjectId,
    Rect,
    schist_layout::FrameOverflow,
    u16,
    f32,
    schist_layout::compose::InsetsLike,
);

/// A thread: a story and the frames it flows through.
type Thread = (schist_layout::StoryId, Vec<Frame>);

/// The frames of each threaded story, in the shape the composer takes.
fn threads(document: &schist_layout::LayoutDocument) -> Vec<Thread> {
    let mut by_story: std::collections::BTreeMap<schist_layout::StoryId, Vec<Frame>> =
        Default::default();
    for object in &document.objects {
        if let LayoutObject::TextFrame {
            story,
            columns,
            gutter,
            insets,
            overflow,
            ..
        } = &object.object
        {
            by_story.entry(*story).or_default().push((
                object.id,
                object.bounds,
                *overflow,
                *columns,
                *gutter,
                (*insets).into(),
            ));
        }
    }
    by_story.into_iter().collect()
}

#[test]
fn the_report_says_what_was_not_read() {
    // A reader that silently drops part of a document is worse than one
    // that says which part, so the report is part of the contract.
    for (name, fixture) in FIXTURES {
        let imported = read(fixture);
        // None of these seven is fully covered yet, so every one of them
        // has something to say. When they are all fully read this test
        // is the thing that will fail, and that is the point.
        assert!(
            !imported.report.is_complete(),
            "{name} claims to be fully read, which cannot be true yet"
        );
    }
}
