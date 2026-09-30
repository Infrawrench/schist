use schist_codec_idml::{container, export, import};
use schist_layout::{authoring, blank_a4, compose, structure, threading, History, Page, Rect};

#[test]
fn native_thread_order_survives_spread_and_z_order_changes() {
    for count in 2..7 {
        let mut doc = blank_a4();
        let mut history = History::default();
        for page in 1..count {
            assert!(structure::add_page(
                &mut doc,
                &mut history,
                page - 1,
                Page::a4()
            ));
        }
        let frames: Vec<_> = (0..count)
            .map(|page| {
                authoring::text_frame(
                    &mut doc,
                    &mut history,
                    page,
                    Rect::new(20.0, 20.0, 160.0, 42.0),
                )
                .unwrap()
            })
            .collect();
        let text = "A héllo 世界 story with enough words to continue from frame to frame. "
            .repeat(count * 2);
        authoring::set_text(&mut doc, &mut history, frames[0].story, &text);
        // Flow goes last page first, independent of package/spread order.
        for i in (1..count).rev() {
            assert!(threading::link(
                &mut doc,
                &mut history,
                frames[0].object,
                frames[i].object
            ));
        }
        assert!(threading::link(
            &mut doc,
            &mut history,
            frames[count - 1].object,
            frames[0].object
        ));
        let expected: Vec<_> = doc
            .story_frames(frames[0].story)
            .iter()
            .map(|o| o.page)
            .collect();
        doc.objects.reverse();
        for _ in 0..3 {
            let encoded = export::write(&doc);
            let package = container::read(&encoded.bytes).unwrap();
            let xml: String = package
                .names()
                .into_iter()
                .filter(|n| n.starts_with("Spreads/"))
                .map(|n| package.text(n).unwrap())
                .collect();
            assert_eq!(xml.matches("NextTextFrame=").count(), count);
            assert_eq!(xml.matches("PreviousTextFrame=").count(), count);
            doc = import::read(&encoded.bytes).unwrap().document;
            let story = threading::story_of(&doc, doc.objects[0].id).unwrap();
            assert_eq!(
                doc.story_frames(story)
                    .iter()
                    .map(|o| o.page)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(doc.story(story).unwrap().text(), text);
            let composed = compose::compose_story(&doc, story);
            assert_eq!(composed.frames.len(), count);
            let mut consumed = 0;
            for frame in &composed.frames {
                assert!(frame.consumed_to >= consumed);
                for line in &frame.lines {
                    assert!(
                        line.start >= consumed,
                        "text must not restart in each frame"
                    );
                }
                consumed = frame.consumed_to;
            }
            assert!(consumed > 0);
        }
    }
}

fn parent_thread(count: usize) -> schist_layout::LayoutDocument {
    use schist_layout::{
        parents::{ParentPlacement, ParentSheet},
        ParentObject, ParentPage, Point,
    };
    let mut doc = blank_a4();
    doc.pages.push(doc.pages[0].clone());
    doc.facing_pages = true;
    doc.spreads = vec![schist_layout::Spread {
        pages: vec![0, 1],
        binding_location: None,
        gutter: 0.0,
        origin: Point::ZERO,
    }];
    let mut history = History::default();
    let frames: Vec<_> = (0..count)
        .map(|i| {
            authoring::text_frame(
                &mut doc,
                &mut history,
                0,
                Rect::new(20.0, 20.0 + i as f32 * 60.0, 160.0, 40.0),
            )
            .unwrap()
        })
        .collect();
    authoring::set_text(
        &mut doc,
        &mut history,
        frames[0].story,
        "A héllo story flows through the shared parent frames. ".repeat(count * 3),
    );
    for i in 1..count {
        assert!(threading::link(
            &mut doc,
            &mut history,
            frames[i - 1].object,
            frames[i].object
        ));
    }
    for (i, object) in doc.objects.iter_mut().enumerate() {
        object.page = usize::from(i == count - 1);
        object.name = format!("frame {i}");
    }
    let mut objects: Vec<_> = doc
        .objects
        .drain(..)
        .map(|object| ParentObject {
            object,
            overridden_on: Vec::new(),
        })
        .collect();
    objects.reverse();
    doc.parents.push(ParentPage {
        name: "A".into(),
        applied_to: vec![0, 1],
        based_on: None,
        hidden: false,
        objects,
        sheets: (0..2)
            .map(|i| ParentSheet {
                page: doc.pages[i].clone(),
                origin: Point::new((i as f32 - 1.0) * doc.pages[i].width, 0.0),
                source: None,
            })
            .collect(),
        placements: (0..2)
            .map(|page| ParentPlacement {
                page,
                sheet: page,
                transform: Default::default(),
                visible: true,
            })
            .collect(),
    });
    for page in &mut doc.pages {
        page.master = Some(0);
    }
    doc
}

#[test]
fn parent_text_threads_continue_once_per_frame_and_keep_native_order_on_save() {
    for count in 2..6 {
        let mut doc = parent_thread(count);
        let original: Vec<_> = doc
            .frame_thread(&doc.parents[0].objects[0].object)
            .iter()
            .map(|o| o.name.clone())
            .collect();
        for _ in 0..5 {
            let frames = doc.frame_thread(&doc.parents[0].objects[0].object);
            assert_eq!(frames.len(), count);
            assert_eq!(
                frames.iter().map(|o| o.name.clone()).collect::<Vec<_>>(),
                original
            );
            let mut consumed = 0;
            for (index, object) in frames.iter().enumerate() {
                let frame = compose::compose_object(&doc, object).unwrap();
                assert!(!frame.lines.is_empty());
                assert!(
                    frame.lines.iter().all(|line| line.start >= consumed),
                    "a parent frame restarted the story"
                );
                assert!(frame.consumed_to > consumed);
                assert_eq!(frame.passed_on, index + 1 < count);
                consumed = frame.consumed_to;
            }
            let encoded = export::write(&doc);
            let package = container::read(&encoded.bytes).unwrap();
            let master = package
                .names()
                .into_iter()
                .find(|n| n.starts_with("MasterSpreads/"))
                .unwrap();
            let root = schist_codec_idml::xml::parse(package.text(master).unwrap()).unwrap();
            let frames: Vec<_> = root
                .find("MasterSpread")
                .unwrap()
                .children_named("TextFrame")
                .collect();
            assert_eq!(
                frames
                    .iter()
                    .filter(|f| f.attr("NextTextFrame") != Some("n"))
                    .count(),
                count - 1
            );
            assert_eq!(
                frames
                    .iter()
                    .filter(|f| f.attr("PreviousTextFrame") != Some("n"))
                    .count(),
                count - 1
            );
            let imported = import::read(&encoded.bytes).unwrap();
            assert!(!imported
                .report
                .skipped
                .iter()
                .any(|s| s == schist_i18n::t("design.idml_bad_thread")));
            doc = imported.document;
        }
    }
}

#[test]
fn parent_page_breaks_skip_other_frames_on_the_same_template_sheet() {
    let mut doc = parent_thread(3);
    let frame = doc.frame_thread(&doc.parents[0].objects[0].object)[0];
    let schist_layout::LayoutObject::TextFrame { story, .. } = frame.object else {
        panic!()
    };
    *doc.story_mut(story) = schist_layout::Story::from_text("before", "Body");
    doc.story_mut(story)
        .points
        .push(schist_layout::StoryPoint::PageBreak);
    doc.story_mut(story).push_paragraph("after", "Body");
    for _ in 0..4 {
        let frames = doc.frame_thread(&doc.parents[0].objects[0].object);
        let flow: Vec<_> = frames
            .iter()
            .map(|object| compose::compose_object(&doc, object).unwrap())
            .collect();
        assert_eq!(flow[0].lines.len(), 1);
        assert!(flow[1].lines.is_empty());
        assert_eq!(flow[2].lines.len(), 1);
        let story = match frames[0].object {
            schist_layout::LayoutObject::TextFrame { story, .. } => doc.story(story).unwrap(),
            _ => panic!(),
        };
        assert_eq!(
            story.slice(flow[0].lines[0].start, flow[0].lines[0].end),
            "before"
        );
        assert_eq!(
            story.slice(flow[2].lines[0].start, flow[2].lines[0].end),
            "after"
        );
        assert_eq!(flow[2].consumed_to, story.text_len());
        doc = import::read(&export::write(&doc).bytes).unwrap().document;
    }
}
