//! Text frames anchored in text: their own story composes in the item's box,
//! and they draw as part of the frame that holds them.
use schist_layout::anchored::{self, AnchoredItem, AnchoredPosition, Placement};
use schist_layout::{
    authoring, blank_a4, compose::compose_object, compose::compose_story, Display, History,
    LayoutDocument, LayoutObject, ObjectId, PasteboardView, Rect, Story, StoryId, StoryStructure,
};

const BEFORE: &str = "A boxed note ";

/// A host frame whose story anchors a text frame holding `inner` after
/// BEFORE. With `own_story`, the anchored frame shows the host's own story.
fn document(
    inner: &str,
    position: AnchoredPosition,
    own_story: bool,
) -> (LayoutDocument, ObjectId) {
    let mut doc = blank_a4();
    let host = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(40.0, 40.0, 420.0, 400.0),
    )
    .unwrap();
    let boxed = authoring::text_frame(
        &mut doc,
        &mut History::default(),
        0,
        Rect::new(0.0, 0.0, 90.0, 40.0),
    )
    .unwrap();
    // The anchored frame lives in the story, not on the page.
    let mut object = doc.object(boxed.object).unwrap().clone();
    doc.objects.retain(|o| o.id != boxed.object);
    if own_story {
        if let LayoutObject::TextFrame { story, .. } = &mut object.object {
            *story = host.story;
        }
    }
    doc.stories[boxed.story.0 as usize] = Story::from_text(inner, "Body");
    let mut story = Story::from_text(format!("{BEFORE} sits in this line."), "Body");
    story.structures.push(StoryStructure {
        at: Some(BEFORE.len()),
        kind: "TextFrame".into(),
        payload: "<TextFrame />".into(),
        control: None,
        footnote: None,
        anchored: Some(Box::new(AnchoredItem {
            position,
            y_offset: 0.0,
            placement: Placement::default(),
            object,
            members: Vec::new(),
        })),
    });
    doc.stories[host.story.0 as usize] = story;
    (doc, host.object)
}

fn placed(doc: &LayoutDocument, host: ObjectId) -> Vec<schist_layout::PlacedObject> {
    let lines: Vec<_> = compose_story(doc, StoryId(0)).lines().cloned().collect();
    anchored::placements(doc, &doc.stories[0], doc.object(host).unwrap(), &lines)
}

#[test]
fn an_anchored_text_frame_composes_its_story_in_its_own_box() {
    for position in [
        AnchoredPosition::Inline,
        AnchoredPosition::AboveLine,
        AnchoredPosition::Anchored,
    ] {
        let (doc, host) = document("Inner words", position, false);
        let flow = compose_story(&doc, StoryId(0));
        assert_eq!(flow.frames[0].unrendered_structures, 0, "{position:?}");
        let [item] = &placed(&doc, host)[..] else {
            panic!("{position:?}")
        };
        assert!(matches!(item.object, LayoutObject::TextFrame { .. }));
        let composed = compose_object(&doc, item).expect("standalone composition");
        let text: String = composed
            .all_lines()
            .map(|l| doc.stories[1].slice(l.start, l.end))
            .collect();
        assert_eq!(text.trim(), "Inner words", "{position:?}");
        for line in composed.all_lines() {
            assert!(
                line.bounds.x >= item.bounds.x - 0.01
                    && line.bounds.right() <= item.bounds.right() + 0.01
                    && line.bounds.y >= item.bounds.y - 0.01,
                "{position:?}: {:?} in {:?}",
                line.bounds,
                item.bounds
            );
        }
        assert!(!composed.lost);
    }
}

#[test]
fn its_text_draws_as_generated_text_of_the_holding_frame() {
    let (doc, host) = document("Inner words", AnchoredPosition::Inline, false);
    let plan = schist_layout::pasteboard(
        &doc,
        &PasteboardView {
            scale: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let inner: Vec<_> = plan
        .pages
        .iter()
        .flat_map(|p| &p.objects)
        .filter_map(|d| match d {
            Display::Text {
                generated,
                object,
                story,
                text,
                ..
            } if *story == StoryId(1) => Some((*generated, *object, text.clone())),
            _ => None,
        })
        .collect();
    assert!(!inner.is_empty());
    assert!(inner
        .iter()
        .all(|(generated, object, _)| *generated && *object == host));
    assert!(inner.iter().any(|(_, _, text)| text.contains("Inner")));
}

#[test]
fn a_frame_that_leads_back_to_its_own_story_is_not_composed() {
    let (doc, host) = document("unused", AnchoredPosition::Inline, true);
    let flow = compose_story(&doc, StoryId(0));
    assert_eq!(flow.frames[0].unrendered_structures, 1);
    assert!(placed(&doc, host).is_empty());
    let item = doc.stories[0].structures[0].anchored.as_deref().unwrap();
    assert!(anchored::reaches(&doc, item, StoryId(0)));
    assert!(!anchored::reaches(&doc, item, StoryId(1)));
}

#[test]
fn an_overfull_anchored_frame_reports_lost_text() {
    let long = "words ".repeat(80);
    let (doc, host) = document(&long, AnchoredPosition::Inline, false);
    let [item] = &placed(&doc, host)[..] else {
        panic!()
    };
    assert!(compose_object(&doc, item).unwrap().lost);
}
