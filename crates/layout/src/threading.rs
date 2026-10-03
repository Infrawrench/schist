//! Text flow order is independent of page order and layer z-order.
use crate::{
    edit::{snapshot_object, snapshot_story},
    FrameOverflow, History, LayoutDocument, LayoutEdit, LayoutObject, ObjectId, PlacedObject,
    Story, StoryId,
};

impl LayoutDocument {
    /// All ordinary frames for a story. Stale IDs are ignored and frames
    /// absent from an explicit order are appended once, in insertion order.
    pub fn story_frames(&self, story: StoryId) -> Vec<&PlacedObject> {
        self.ordered_frames(story, self.objects.iter())
    }

    /// A frame's flow scope. Parent stories flow through their shared template
    /// frames; ordinary document instances retain their own independent flow.
    pub fn frame_thread(&self, object: &PlacedObject) -> Vec<&PlacedObject> {
        let LayoutObject::TextFrame { story, .. } = object.object else {
            return Vec::new();
        };
        if self.object(object.id).is_some() {
            return self.story_frames(story);
        }
        self.ordered_frames(
            story,
            self.parents
                .iter()
                .flat_map(|parent| parent.objects.iter().map(|entry| &entry.object)),
        )
    }

    fn ordered_frames<'a>(
        &self,
        story: StoryId,
        objects: impl Iterator<Item = &'a PlacedObject>,
    ) -> Vec<&'a PlacedObject> {
        let mut seen = std::collections::HashSet::new();
        let mut frames: Vec<_> = objects
            .filter(|o| matches!(o.object, LayoutObject::TextFrame {story:s,..} if s == story))
            .filter(|o| seen.insert(o.id))
            .collect();
        if let Some((_, order)) = self.thread_order.iter().find(|(s, _)| *s == story) {
            let mut ranks = std::collections::HashMap::new();
            for (index, id) in order.iter().enumerate() {
                ranks.entry(*id).or_insert(index);
            }
            // Stable order retains insertion order for frames missing from the
            // explicit chain, and duplicate references never duplicate text.
            frames.sort_by_key(|o| ranks.get(&o.id).copied().unwrap_or(order.len()));
        }
        frames
    }
}

pub fn story_of(doc: &LayoutDocument, id: ObjectId) -> Option<StoryId> {
    match doc.object(id)?.object {
        LayoutObject::TextFrame { story, .. } => Some(story),
        _ => None,
    }
}

/// Insert an empty frame after a source, or reorder a frame already in
/// the same story. A different nonempty story is never overwritten.
pub fn link(
    doc: &mut LayoutDocument,
    history: &mut History,
    source: ObjectId,
    target: ObjectId,
) -> bool {
    if source == target || doc.object_locked(source) || doc.object_locked(target) {
        return false;
    }
    let (Some(story), Some(old_story)) = (story_of(doc, source), story_of(doc, target)) else {
        return false;
    };
    if story != old_story
        && (doc
            .story(old_story)
            .is_none_or(|s| s.text_len() != 0 || s.retained_structures() > 0)
            || doc.story_frames(old_story).len() != 1)
    {
        return false;
    }
    let mut order: Vec<_> = doc
        .story_frames(story)
        .into_iter()
        .map(|o| o.id)
        .filter(|id| *id != target)
        .collect();
    let Some(index) = order.iter().position(|id| *id == source) else {
        return false;
    };
    order.insert(index + 1, target);
    if order.iter().any(|id| doc.object_locked(*id)) {
        return false;
    }
    let mut edits = Vec::new();
    for id in &order {
        let object = doc.object(*id).unwrap();
        let mut changed = object.clone();
        let LayoutObject::TextFrame {
            story: s, overflow, ..
        } = &mut changed.object
        else {
            unreachable!()
        };
        *s = story;
        *overflow = FrameOverflow::Thread;
        if changed != *object {
            edits.push(LayoutEdit::ObjectChanged {
                id: id.0,
                before: snapshot_object(object),
                after: snapshot_object(&changed),
            });
        }
    }
    let mut after = doc.thread_order.clone();
    after.retain(|(s, _)| *s != story && (old_story == story || *s != old_story));
    after.push((story, order));
    if after != doc.thread_order {
        edits.push(LayoutEdit::ThreadsChanged {
            before: doc.thread_order.clone(),
            after,
        });
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

/// Detach one frame with a new empty story. The complete original text
/// stays in its original story and reflows through the remaining frames.
pub fn detach(doc: &mut LayoutDocument, history: &mut History, id: ObjectId) -> bool {
    if doc.object_locked(id) {
        return false;
    }
    let Some(story) = story_of(doc, id) else {
        return false;
    };
    let frames = doc.story_frames(story);
    if frames.len() < 2 || frames.iter().any(|o| doc.object_locked(o.id)) {
        return false;
    }
    let order = frames.iter().filter(|o| o.id != id).map(|o| o.id).collect();
    let object = doc.object(id).unwrap();
    let mut changed = object.clone();
    let new = StoryId(doc.stories.len() as u32);
    let LayoutObject::TextFrame {
        story: s, overflow, ..
    } = &mut changed.object
    else {
        unreachable!()
    };
    *s = new;
    *overflow = FrameOverflow::Clip;
    let mut after = doc.thread_order.clone();
    after.retain(|(s, _)| *s != story);
    after.push((story, order));
    let mut detached = Story::from_text("", &doc.default_paragraph_style);
    detached.prefs = doc.story(story).map_or_default(|story| story.prefs);
    history.apply(
        doc,
        LayoutEdit::Batch {
            edits: vec![
                LayoutEdit::AddedStory {
                    id: new.0,
                    story: snapshot_story(&detached),
                },
                LayoutEdit::ObjectChanged {
                    id: id.0,
                    before: snapshot_object(object),
                    after: snapshot_object(&changed),
                },
                LayoutEdit::ThreadsChanged {
                    before: doc.thread_order.clone(),
                    after,
                },
            ],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{authoring, blank_a4, Rect};

    #[test]
    fn linking_and_detaching_are_one_undo_and_never_discard_either_story() {
        for count in 2..9 {
            let mut doc = blank_a4();
            let mut history = History::default();
            let frames: Vec<_> = (0..count)
                .map(|_| {
                    authoring::text_frame(
                        &mut doc,
                        &mut history,
                        0,
                        Rect::new(0.0, 0.0, 100.0, 40.0),
                    )
                    .unwrap()
                })
                .collect();
            authoring::set_text(
                &mut doc,
                &mut history,
                frames[0].story,
                "A story that must never disappear",
            );
            for i in 1..count {
                let before = doc.clone();
                let depth = history.undo_depth();
                assert!(link(
                    &mut doc,
                    &mut history,
                    frames[i - 1].object,
                    frames[i].object
                ));
                let after = doc.clone();
                assert_eq!(history.undo_depth(), depth + 1);
                history.undo(&mut doc);
                assert_eq!(doc, before);
                history.redo(&mut doc);
                assert_eq!(doc, after);
            }
            let before = doc.clone();
            let depth = history.undo_depth();
            assert!(detach(&mut doc, &mut history, frames[1].object));
            assert_eq!(doc.stories[0], before.stories[0]);
            assert_eq!(history.undo_depth(), depth + 1);
            assert_eq!(doc.story_frames(frames[0].story).len(), count - 1);
            history.undo(&mut doc);
            assert_eq!(doc, before);
            assert!(!link(
                &mut doc,
                &mut history,
                frames[0].object,
                frames[0].object
            ));
        }
    }

    #[test]
    fn nonempty_target_and_locked_chain_refuse_without_side_effects() {
        let mut doc = blank_a4();
        let mut history = History::default();
        let a = authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0))
            .unwrap();
        let b = authoring::text_frame(&mut doc, &mut history, 0, Rect::new(0.0, 0.0, 100.0, 40.0))
            .unwrap();
        authoring::set_text(&mut doc, &mut history, b.story, "Target words");
        let before = doc.clone();
        let old_history = history.clone();
        assert!(!link(&mut doc, &mut history, a.object, b.object));
        assert_eq!(doc, before);
        assert_eq!(history, old_history);
        authoring::set_text(&mut doc, &mut history, b.story, "");
        assert!(link(&mut doc, &mut history, a.object, b.object));
        for locked in [a.object, b.object] {
            doc.objects
                .iter_mut()
                .find(|o| o.id == locked)
                .unwrap()
                .locked = true;
            let before = doc.clone();
            let old_history = history.clone();
            assert!(!detach(&mut doc, &mut history, a.object));
            assert!(!link(&mut doc, &mut history, b.object, a.object));
            assert_eq!(doc, before);
            assert_eq!(history, old_history);
            doc.objects
                .iter_mut()
                .find(|o| o.id == locked)
                .unwrap()
                .locked = false;
        }
    }
}
