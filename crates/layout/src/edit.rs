//! Applying and reversing [`LayoutEdit`]s against a real document.
//!
//! The history stack knows nothing about the document; this is the half
//! that does. Every operation has a forward and an inverse, and they are
//! written as one function each rather than as "undo" and "redo" so the
//! two cannot drift apart -- an undo that is not the exact inverse of
//! its edit is how a document ends up subtly wrong after a few
//! undo/redo cycles and nobody can say why.

use crate::geometry::Page;
use crate::history::{
    History, InkSnapshot, LayoutEdit, ObjectSnapshot, PageSnapshot, SettingsSnapshot,
    SpreadSnapshot, StorySnapshot,
};
use crate::model::{LayoutDocument, LayoutObject, ObjectId, PlacedObject, StoryId};

impl History {
    /// Apply an edit to a document, recording it.
    ///
    /// Returns `false` and leaves the document alone when the edit is
    /// not reversible, so a caller can decide what to do rather than
    /// ending up with a document and a history that disagree.
    pub fn apply(&mut self, doc: &mut LayoutDocument, edit: LayoutEdit) -> bool {
        if !History::is_reversible(&edit) {
            return false;
        }
        if !forward(doc, &edit) {
            return false;
        }
        self.record(edit);
        true
    }

    /// Undo the last operation, restoring the previous state.
    pub fn undo(&mut self, doc: &mut LayoutDocument) -> bool {
        let Some(edit) = self.pop_undo() else {
            return false;
        };
        if !reverse(doc, &edit) {
            // Put it back so the history still matches the document.
            self.restore_undo(edit);
            return false;
        }
        true
    }

    /// Redo the last undone operation.
    pub fn redo(&mut self, doc: &mut LayoutDocument) -> bool {
        let Some(edit) = self.pop_redo() else {
            return false;
        };
        if !forward(doc, &edit) {
            self.restore_redo(edit);
            return false;
        }
        true
    }
}

/// Apply an edit.
pub fn forward(doc: &mut LayoutDocument, edit: &LayoutEdit) -> bool {
    match edit {
        LayoutEdit::SwatchesChanged { after, .. } => {
            doc.inks.clone_from(after);
            true
        }
        LayoutEdit::Batch { edits } => {
            for (index, edit) in edits.iter().enumerate() {
                if !forward(doc, edit) {
                    for applied in edits[..index].iter().rev() {
                        assert!(reverse(doc, applied), "a valid edit must be reversible");
                    }
                    return false;
                }
            }
            true
        }
        LayoutEdit::TopologyChanged { after, .. } => {
            after.restore(doc);
            true
        }
        LayoutEdit::ThreadsChanged { after, .. } => {
            doc.thread_order.clone_from(after);
            true
        }
        LayoutEdit::StylesChanged { after, .. } => {
            doc.styles = *after.clone();
            true
        }
        LayoutEdit::LayersChanged { after, .. } => {
            after.restore(doc);
            true
        }
        LayoutEdit::AddedPage {
            index,
            page,
            spreads,
        } => {
            if !insert_page(doc, *index, page_from(page)) {
                return false;
            }
            // The recorded spreads are shifted for the insertion, and
            // the new page joins the last one.
            doc.spreads = restore_spreads(spreads, |i| Some(if i >= *index { i + 1 } else { i }));
            match doc.spreads.last_mut() {
                Some(last) => last.pages.push(*index),
                None => doc.spreads.push(crate::geometry::Spread::single(*index)),
            }
            true
        }
        LayoutEdit::RemovedPage { index, spreads, .. } => {
            if remove_page(doc, *index).is_none() {
                return false;
            }
            // Restoring the recorded list drops the removed page and
            // shifts the ones after it down, which is exactly the
            // renumbering the page removal already applied elsewhere.
            doc.spreads = restore_spreads(spreads, |i| match i.cmp(index) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(i - 1),
                std::cmp::Ordering::Less => Some(i),
            });
            doc.spreads.retain(|s| !s.pages.is_empty());
            if doc.spreads.is_empty() {
                doc.spreads.push(crate::geometry::Spread::single(
                    doc.pages.len().saturating_sub(1),
                ));
            }
            true
        }
        LayoutEdit::PageChanged { index, after, .. } => {
            if let Some(page) = doc.pages.get_mut(*index) {
                *page = page_from(after);
                true
            } else {
                false
            }
        }
        LayoutEdit::PageMoved { from, to } => move_page(doc, *from, *to),

        LayoutEdit::AddedObject { index, object } => {
            let Some(object) = object_from(object, doc) else {
                return false;
            };
            let index = (*index).min(doc.objects.len());
            doc.objects.insert(index, object);
            true
        }
        LayoutEdit::RemovedObject { index, .. } => {
            if *index < doc.objects.len() {
                doc.objects.remove(*index);
                true
            } else {
                false
            }
        }
        LayoutEdit::RemovedObjects { items } => {
            let mut ordered: Vec<_> = items.iter().collect();
            ordered.sort_by_key(|(index, _)| *index);
            if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0)
                || ordered.iter().any(|(index, object)| {
                    doc.objects
                        .get(*index)
                        .is_none_or(|live| live.id.0 != object.id)
                })
            {
                return false;
            }
            for (index, _) in ordered.into_iter().rev() {
                doc.objects.remove(*index);
            }
            true
        }
        LayoutEdit::ObjectChanged { id, after, .. } => {
            let Some(mut restored) = object_from(after, doc) else {
                return false;
            };
            // The id is the object's identity, not part of the edit:
            // restoring a different one would make the object unfindable.
            restored.id = ObjectId(*id);
            match doc.objects.iter_mut().find(|o| o.id.0 == *id) {
                Some(slot) => {
                    *slot = restored;
                    true
                }
                None => false,
            }
        }

        LayoutEdit::AddedStory { id, story } => {
            if *id as usize != doc.stories.len() {
                return false;
            }
            doc.stories.push(story_from(story));
            true
        }

        LayoutEdit::StoryChanged { id, after, .. } => {
            let Some(story) = doc.stories.get_mut(*id as usize) else {
                return false;
            };
            *story = story_from(after);
            true
        }

        LayoutEdit::StyleChanged { name, after, .. } => {
            // Forward means "the style is now whatever `after` says".
            // A `None` `after` is a removal, not a no-op.
            if after.is_none() {
                remove_style(doc, name);
            }
            apply_style(doc, after.as_ref());
            true
        }

        LayoutEdit::InkChanged { name, after, .. } => {
            if after.is_none() {
                doc.inks.retain(|i| i.name != *name);
                return true;
            }
            let Some(snapshot) = after else {
                return true;
            };
            let ink = ink_from(snapshot);
            match doc.inks.iter_mut().find(|i| i.name == ink.name) {
                Some(slot) => *slot = ink,
                None => doc.inks.push(ink),
            }
            true
        }

        LayoutEdit::DocumentChanged { after, .. } => {
            apply_settings(doc, after);
            true
        }
    }
}

/// Reverse an edit.
pub fn reverse(doc: &mut LayoutDocument, edit: &LayoutEdit) -> bool {
    match edit {
        LayoutEdit::SwatchesChanged { before, .. } => {
            doc.inks.clone_from(before);
            true
        }
        LayoutEdit::Batch { edits } => {
            for (index, edit) in edits.iter().enumerate().rev() {
                if !reverse(doc, edit) {
                    for undone in &edits[index + 1..] {
                        assert!(forward(doc, undone), "a valid inverse must be reversible");
                    }
                    return false;
                }
            }
            true
        }
        LayoutEdit::TopologyChanged { before, .. } => {
            before.restore(doc);
            true
        }
        LayoutEdit::ThreadsChanged { before, .. } => {
            doc.thread_order.clone_from(before);
            true
        }
        LayoutEdit::StylesChanged { before, .. } => {
            doc.styles = *before.clone();
            true
        }
        LayoutEdit::LayersChanged { before, .. } => {
            before.restore(doc);
            true
        }
        LayoutEdit::AddedPage { index, spreads, .. } => {
            if remove_page(doc, *index).is_none() {
                return false;
            }
            doc.spreads = restore_spreads(spreads, Some);
            true
        }
        LayoutEdit::RemovedPage {
            index,
            page,
            spreads,
        } => {
            if !insert_page(doc, *index, page_from(page)) {
                return false;
            }
            // The recorded list is from *before* the removal, which is
            // the same page numbering as after the page goes back in, so
            // it is restored as it stands. Shifting it again would be
            // undoing an edit that was never applied.
            doc.spreads = restore_spreads(spreads, Some);
            true
        }
        LayoutEdit::PageChanged { index, before, .. } => {
            if let Some(page) = doc.pages.get_mut(*index) {
                *page = page_from(before);
                true
            } else {
                false
            }
        }
        // Its own inverse: swapping the two indices again.
        LayoutEdit::PageMoved { from, to } => move_page(doc, *to, *from),

        LayoutEdit::AddedObject { index, .. } => {
            if *index < doc.objects.len() {
                doc.objects.remove(*index);
                true
            } else {
                false
            }
        }
        LayoutEdit::RemovedObject { index, object } => {
            let Some(object) = object_from(object, doc) else {
                return false;
            };
            let index = (*index).min(doc.objects.len());
            doc.objects.insert(index, object);
            true
        }
        LayoutEdit::RemovedObjects { items } => {
            let Some(mut ordered) = items
                .iter()
                .map(|(index, snapshot)| object_from(snapshot, doc).map(|object| (*index, object)))
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            ordered.sort_by_key(|(index, _)| *index);
            if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0)
                || ordered.iter().enumerate().any(|(offset, (index, object))| {
                    *index > doc.objects.len() + offset || doc.object(object.id).is_some()
                })
            {
                return false;
            }
            for (index, object) in ordered {
                doc.objects.insert(index, object);
            }
            true
        }
        LayoutEdit::ObjectChanged { id, before, .. } => {
            let Some(mut restored) = object_from(before, doc) else {
                return false;
            };
            restored.id = ObjectId(*id);
            match doc.objects.iter_mut().find(|o| o.id.0 == *id) {
                Some(slot) => {
                    *slot = restored;
                    true
                }
                None => false,
            }
        }

        LayoutEdit::AddedStory { id, .. } => {
            if *id as usize + 1 != doc.stories.len() {
                return false;
            }
            doc.stories.pop();
            true
        }

        LayoutEdit::StoryChanged { id, before, .. } => {
            let Some(story) = doc.stories.get_mut(*id as usize) else {
                return false;
            };
            *story = story_from(before);
            true
        }

        LayoutEdit::StyleChanged { name, before, .. } => {
            remove_style(doc, name);
            apply_style(doc, before.as_ref());
            true
        }

        LayoutEdit::InkChanged { name, before, .. } => {
            doc.inks.retain(|i| i.name != *name);
            if let Some(snapshot) = before {
                doc.inks.push(ink_from(snapshot));
            }
            true
        }

        LayoutEdit::DocumentChanged { before, .. } => {
            apply_settings(doc, before);
            true
        }
    }
}

fn remove_style(doc: &mut LayoutDocument, name: &str) {
    doc.styles.paragraphs.retain(|s| s.name != name);
    doc.styles.characters.retain(|s| s.name != name);
}

fn insert_page(doc: &mut LayoutDocument, index: usize, page: Page) -> bool {
    if index > doc.pages.len() {
        return false;
    }
    doc.pages.insert(index, page);
    // Everything from the insertion point up moves one place later, so
    // every reference to a page has to move with it. A page reference
    // that silently did not move would point at the wrong sheet.
    renumber(doc, |i| Some(if i >= index { i + 1 } else { i }));
    if doc.spreads.is_empty() {
        doc.spreads.push(crate::geometry::Spread::single(index));
    } else if let Some(last) = doc.spreads.last_mut() {
        last.pages.push(index);
    }
    true
}

fn remove_page(doc: &mut LayoutDocument, index: usize) -> Option<Page> {
    if index >= doc.pages.len() {
        return None;
    }
    let page = doc.pages.remove(index);
    // Everything after the hole moves down, and the removed page itself
    // is gone. Spreads drop it; objects cannot lose their page, so they
    // are brought to the last sheet.
    // `None` means "this reference is now dead" and drops it. A page
    // below the hole is unchanged, not dead, and conflating the two
    // silently empties spreads.
    renumber(doc, |i| match i {
        i if i == index => None,
        i if i > index => Some(i - 1),
        i => Some(i),
    });
    Some(page)
}

/// Rebuild a spread list from snapshots, renumbering each page reference.
fn restore_spreads(
    snapshots: &[SpreadSnapshot],
    map: impl Fn(usize) -> Option<usize> + Copy,
) -> Vec<crate::geometry::Spread> {
    snapshots
        .iter()
        .map(|snapshot| {
            let mut spread = spread_from(snapshot);
            if let Some(binding) = &mut spread.binding_location {
                *binding = spread
                    .pages
                    .iter()
                    .take(*binding)
                    .filter(|i| map(**i).is_some())
                    .count();
            }
            spread.pages = spread.pages.iter().filter_map(|i| map(*i)).collect();
            spread
        })
        .collect()
}

/// Move a page, renumbering everything that refers to it.
fn move_page(doc: &mut LayoutDocument, from: usize, to: usize) -> bool {
    if from >= doc.pages.len() || to >= doc.pages.len() || from == to {
        return false;
    }
    let page = doc.pages.remove(from);
    doc.pages.insert(to, page);
    renumber(doc, |index| match index {
        i if i == from => Some(to),
        i if from < i && i <= to => Some(i - 1),
        i if to <= i && i < from => Some(i + 1),
        i => Some(i),
    });
    true
}

/// Renumber every page reference in the document.
///
/// `map` returns the new index, or `None` when the reference is now dead
/// and should be dropped.
fn renumber(doc: &mut LayoutDocument, mut map: impl FnMut(usize) -> Option<usize>) {
    let last = doc.pages.len().saturating_sub(1);
    for spread in &mut doc.spreads {
        let mut pages = Vec::with_capacity(spread.pages.len());
        let mut left = 0;
        for (position, index) in spread.pages.iter().enumerate() {
            match map(*index) {
                Some(next) => {
                    pages.push(next);
                    if spread
                        .binding_location
                        .is_some_and(|binding| position < binding)
                    {
                        left += 1;
                    }
                }
                // A spread that referred to a page which is gone drops
                // it. Leaving it in would put a spread on a sheet it has
                // nothing to do with.
                None => continue,
            }
        }
        spread.pages = pages;
        if spread.binding_location.is_some() {
            spread.binding_location = Some(left);
        }
    }
    for object in &mut doc.objects {
        // An object cannot simply lose its page, so a removed one is
        // brought to the last sheet rather than dropped.
        object.page = match map(object.page) {
            Some(next) => next,
            None => object.page.min(last),
        };
    }
    for parent in &mut doc.parents {
        parent.remap_placements(&mut map);
        parent.applied_to = parent
            .applied_to
            .iter()
            .filter_map(|index| map(*index))
            .collect();
    }
    // Page.master indexes parents, not pages. Parent override indices do
    // refer to pages and must follow the same permutation as applications.
    for parent in &mut doc.parents {
        for object in &mut parent.objects {
            object.overridden_on = object
                .overridden_on
                .iter()
                .filter_map(|i| map(*i))
                .collect();
        }
    }
}

fn page_from(snapshot: &PageSnapshot) -> Page {
    Page {
        name: snapshot.name.clone(),
        section: snapshot.section.clone(),
        width: snapshot.width,
        height: snapshot.height,
        bleed: snapshot.bleed,
        slug: snapshot.slug,
        margins: crate::geometry::Insets::new(
            snapshot.margins[0],
            snapshot.margins[1],
            snapshot.margins[2],
            snapshot.margins[3],
        ),
        orientation: if snapshot.landscape {
            crate::geometry::Orientation::Landscape
        } else {
            crate::geometry::Orientation::Portrait
        },
        hidden: snapshot.hidden,
        master: snapshot.master,
        guides: snapshot.guides.clone(),
    }
}

/// Snapshot a page.
pub fn snapshot_page(page: &Page) -> PageSnapshot {
    PageSnapshot {
        name: page.name.clone(),
        section: page.section.clone(),
        width: page.width,
        height: page.height,
        bleed: page.bleed,
        slug: page.slug,
        margins: [
            page.margins.top,
            page.margins.right,
            page.margins.bottom,
            page.margins.left,
        ],
        landscape: page.orientation == crate::geometry::Orientation::Landscape,
        hidden: page.hidden,
        master: page.master,
        guides: page.guides.clone(),
    }
}

fn spread_from(snapshot: &SpreadSnapshot) -> crate::geometry::Spread {
    crate::geometry::Spread {
        pages: snapshot.pages.clone(),
        binding_location: snapshot.binding_location,
        gutter: snapshot.gutter,
        origin: crate::geometry::Point::new(snapshot.origin[0], snapshot.origin[1]),
    }
}

/// Snapshot a spread.
pub fn snapshot_spread(spread: &crate::geometry::Spread) -> SpreadSnapshot {
    SpreadSnapshot {
        pages: spread.pages.clone(),
        binding_location: spread.binding_location,
        gutter: spread.gutter,
        origin: [spread.origin.x, spread.origin.y],
    }
}

fn object_from(snapshot: &ObjectSnapshot, doc: &LayoutDocument) -> Option<PlacedObject> {
    let object: LayoutObject = serde_json::from_value(snapshot.payload.clone()).ok()?;
    let _ = doc;
    Some(PlacedObject {
        id: ObjectId(snapshot.id),
        page: snapshot.page,
        bounds: crate::geometry::Rect::new(
            snapshot.bounds[0],
            snapshot.bounds[1],
            snapshot.bounds[2],
            snapshot.bounds[3],
        ),
        object,
        rotation: snapshot.rotation,
        transform: snapshot.transform,
        name: snapshot.name.clone(),
        locked: snapshot.locked,
        overprint: snapshot.overprint,
        transparency: snapshot.transparency,
    })
}

/// Snapshot an object.
pub fn snapshot_object(object: &PlacedObject) -> ObjectSnapshot {
    ObjectSnapshot {
        id: object.id.0,
        page: object.page,
        bounds: [
            object.bounds.x,
            object.bounds.y,
            object.bounds.width,
            object.bounds.height,
        ],
        name: object.name.clone(),
        locked: object.locked,
        overprint: object.overprint,
        transparency: object.transparency,
        rotation: object.rotation,
        transform: object.transform,
        payload: serde_json::to_value(&object.object).unwrap_or(serde_json::Value::Null),
    }
}

fn story_from(snapshot: &StorySnapshot) -> crate::story::Story {
    crate::story::Story {
        prefs: snapshot.prefs,
        points: snapshot
            .points
            .iter()
            .map(|point| match point {
                crate::history::StoryPointSnapshot::Paragraph { text, style } => {
                    crate::story::Point::Paragraph {
                        text: text.clone(),
                        style: style.clone(),
                    }
                }
                crate::history::StoryPointSnapshot::LineBreak => crate::story::Point::LineBreak,
                crate::history::StoryPointSnapshot::ColumnBreak => crate::story::Point::ColumnBreak,
                crate::history::StoryPointSnapshot::PageBreak => crate::story::Point::PageBreak,
                crate::history::StoryPointSnapshot::FrameBreak => crate::story::Point::FrameBreak,
                crate::history::StoryPointSnapshot::Other { kind, payload } => {
                    crate::story::Point::Other {
                        kind: kind.clone(),
                        payload: payload.clone(),
                    }
                }
            })
            .collect(),
        ranges: snapshot
            .ranges
            .iter()
            .map(|(start, end, style)| crate::story::StyleRange::new(*start, *end, style.clone()))
            .collect(),
    }
}

/// Snapshot a story.
pub fn snapshot_story(story: &crate::story::Story) -> StorySnapshot {
    StorySnapshot {
        prefs: story.prefs,
        points: story
            .points
            .iter()
            .map(|point| match point {
                crate::story::Point::Paragraph { text, style } => {
                    crate::history::StoryPointSnapshot::Paragraph {
                        text: text.clone(),
                        style: style.clone(),
                    }
                }
                crate::story::Point::LineBreak => crate::history::StoryPointSnapshot::LineBreak,
                crate::story::Point::ColumnBreak => crate::history::StoryPointSnapshot::ColumnBreak,
                crate::story::Point::PageBreak => crate::history::StoryPointSnapshot::PageBreak,
                crate::story::Point::FrameBreak => crate::history::StoryPointSnapshot::FrameBreak,
                crate::story::Point::Other { kind, payload } => {
                    crate::history::StoryPointSnapshot::Other {
                        kind: kind.clone(),
                        payload: payload.clone(),
                    }
                }
            })
            .collect(),
        ranges: story
            .ranges
            .iter()
            .map(|r| (r.start, r.end, r.style.clone()))
            .collect(),
    }
}

/// Put a style back, or take one away when the snapshot is `None`.
fn apply_style(doc: &mut LayoutDocument, snapshot: Option<&crate::history::StyleSnapshot>) {
    let Some(snapshot) = snapshot else {
        return;
    };
    if snapshot.paragraph {
        if let Ok(style) = serde_json::from_value(snapshot.payload.clone()) {
            doc.styles.add_paragraph(style);
        }
    } else if let Ok(style) = serde_json::from_value(snapshot.payload.clone()) {
        doc.styles.add_character(style);
    }
}

/// Snapshot a paragraph style.
pub fn snapshot_paragraph_style(
    style: &crate::styles::ParagraphStyle,
) -> crate::history::StyleSnapshot {
    crate::history::StyleSnapshot {
        paragraph: true,
        name: style.name.clone(),
        payload: serde_json::to_value(style).unwrap_or(serde_json::Value::Null),
    }
}

/// Snapshot a character style.
pub fn snapshot_character_style(
    style: &crate::styles::CharacterStyle,
) -> crate::history::StyleSnapshot {
    crate::history::StyleSnapshot {
        paragraph: false,
        name: style.name.clone(),
        payload: serde_json::to_value(style).unwrap_or(serde_json::Value::Null),
    }
}

/// Snapshot an ink.
pub fn snapshot_ink(ink: &crate::ink::Ink) -> InkSnapshot {
    InkSnapshot {
        name: ink.name.clone(),
        lab: ink.lab,
        preview_rgb: ink.preview_rgb,
        source_cmyk: ink.source_cmyk,
        spot: ink.spot,
        tint: ink.tint.clone(),
    }
}

fn ink_from(snapshot: &InkSnapshot) -> crate::ink::Ink {
    crate::ink::Ink {
        name: snapshot.name.clone(),
        lab: snapshot.lab,
        preview_rgb: snapshot.preview_rgb,
        source_cmyk: snapshot.source_cmyk,
        spot: snapshot.spot,
        tint: snapshot.tint.clone(),
    }
}

fn apply_settings(doc: &mut LayoutDocument, snapshot: &SettingsSnapshot) {
    doc.facing_pages = snapshot.facing_pages;
    doc.page_binding = snapshot.page_binding;
    doc.default_paragraph_style = snapshot.default_paragraph_style.clone();
    doc.default_character_style = snapshot.default_character_style.clone();
    if let Ok(grids) = serde_json::from_value(snapshot.grids.clone()) {
        doc.grids = grids;
    }
    if let Ok(manager) = serde_json::from_value(snapshot.ink_manager.clone()) {
        doc.ink_manager = manager;
    }
}

/// Snapshot the document-wide settings.
pub fn snapshot_settings(doc: &LayoutDocument) -> SettingsSnapshot {
    SettingsSnapshot {
        facing_pages: doc.facing_pages,
        page_binding: doc.page_binding,
        default_paragraph_style: doc.default_paragraph_style.clone(),
        default_character_style: doc.default_character_style.clone(),
        grids: serde_json::to_value(&doc.grids).unwrap_or(serde_json::Value::Null),
        ink_manager: serde_json::to_value(&doc.ink_manager).unwrap_or(serde_json::Value::Null),
    }
}

/// The story a text frame points at, for an editor that needs to build a
/// story edit.
pub fn story_id_of(object: &PlacedObject) -> Option<StoryId> {
    match &object.object {
        LayoutObject::TextFrame { story, .. } => Some(*story),
        _ => None,
    }
}
