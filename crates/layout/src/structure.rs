//! Reversible page and layer operations. Snapshots contain membership and
//! settings, never a copy of the document's stories or graphic pixels.

use crate::{
    History, LayerId, LayoutDocument, LayoutEdit, LayoutLayer, ObjectId, Page, ParentPage, Spread,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub pages: Vec<Page>,
    pub spreads: Vec<Spread>,
    pub parents: Vec<ParentPage>,
    pub object_pages: Vec<(ObjectId, usize)>,
}

impl Topology {
    pub fn of(doc: &LayoutDocument) -> Self {
        Self {
            pages: doc.pages.clone(),
            spreads: doc.spreads.clone(),
            parents: doc.parents.clone(),
            object_pages: doc.objects.iter().map(|o| (o.id, o.page)).collect(),
        }
    }

    pub(crate) fn restore(&self, doc: &mut LayoutDocument) {
        doc.pages.clone_from(&self.pages);
        doc.spreads.clone_from(&self.spreads);
        doc.parents.clone_from(&self.parents);
        for (id, page) in &self.object_pages {
            if let Some(object) = doc.objects.iter_mut().find(|o| o.id == *id) {
                object.page = *page;
            }
        }
    }

    fn remap(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.object_pages.retain_mut(|(_, page)| match map(*page) {
            Some(next) => {
                *page = next;
                true
            }
            None => false,
        });
        for parent in &mut self.parents {
            parent.remap_placements(&map);
            parent.applied_to = parent.applied_to.iter().filter_map(|i| map(*i)).collect();
            for object in &mut parent.objects {
                object.overridden_on = object
                    .overridden_on
                    .iter()
                    .filter_map(|i| map(*i))
                    .collect();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layers {
    pub order: Vec<LayerId>,
    pub properties: Vec<LayoutLayer>,
    pub objects: Vec<(ObjectId, LayerId)>,
}

impl Layers {
    pub fn of(doc: &LayoutDocument) -> Self {
        Self {
            order: doc.layers.clone(),
            properties: doc.layer_properties.clone(),
            objects: doc.object_layers.clone(),
        }
    }

    pub(crate) fn restore(&self, doc: &mut LayoutDocument) {
        doc.layers.clone_from(&self.order);
        doc.layer_properties.clone_from(&self.properties);
        doc.object_layers.clone_from(&self.objects);
    }
}

fn topology_edit(before: Topology, after: Topology) -> LayoutEdit {
    LayoutEdit::TopologyChanged {
        before: Box::new(before),
        after: Box::new(after),
    }
}

fn apply_topology(
    doc: &mut LayoutDocument,
    history: &mut History,
    before: Topology,
    after: Topology,
) -> bool {
    before != after && history.apply(doc, topology_edit(before, after))
}

pub fn add_page(
    doc: &mut LayoutDocument,
    history: &mut History,
    after_page: usize,
    mut page: Page,
) -> bool {
    let index = (after_page.saturating_add(1)).min(doc.pages.len());
    page.master = None;
    // Inserting a copy of a page must not duplicate its numbering restart.
    page.section = None;
    let before = Topology::of(doc);
    let mut after = before.clone();
    after.pages.insert(index, page);
    after.remap(|i| Some(if i >= index { i + 1 } else { i }));
    for spread in &mut after.spreads {
        for page in &mut spread.pages {
            if *page >= index {
                *page += 1;
            }
        }
    }
    let spread_index = after
        .spreads
        .iter()
        .position(|s| s.pages.contains(&after_page))
        .map(|i| i + 1)
        .unwrap_or(after.spreads.len());
    after.spreads.insert(spread_index, Spread::single(index));
    apply_topology(doc, history, before, after)
}

/// Remove a page and its direct objects in one undo step. Stories remain
/// available to the Story Editor, including those shared by other pages.
/// A section boundary on the removed page goes with it; following pages
/// inherit the preceding section until the next surviving boundary.
pub fn remove_page(doc: &mut LayoutDocument, history: &mut History, index: usize) -> bool {
    if doc.pages.len() <= 1 || index >= doc.pages.len() {
        return false;
    }
    let before = Topology::of(doc);
    let mut after = before.clone();
    after.pages.remove(index);
    let map = |i| {
        if i == index {
            None
        } else {
            Some(if i > index { i - 1 } else { i })
        }
    };
    after.remap(map);
    for spread in &mut after.spreads {
        if let Some(binding) = &mut spread.binding_location {
            *binding = spread
                .pages
                .iter()
                .take(*binding)
                .filter(|i| map(**i).is_some())
                .count();
        }
        spread.pages = spread.pages.iter().filter_map(|i| map(*i)).collect();
    }
    after.spreads.retain(|s| !s.pages.is_empty());
    let items: Vec<_> = doc
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.page == index)
        .map(|(i, o)| (i, crate::snapshot_object(o)))
        .collect();
    let mut edits = vec![topology_edit(before, after)];
    if !items.is_empty() {
        let layers_before = Layers::of(doc);
        let mut layers_after = layers_before.clone();
        layers_after
            .objects
            .retain(|(id, _)| !items.iter().any(|(_, o)| o.id == id.0));
        edits.push(LayoutEdit::RemovedObjects { items });
        edits.push(LayoutEdit::LayersChanged {
            before: Box::new(layers_before),
            after: Box::new(layers_after),
        });
    }
    history.apply(doc, LayoutEdit::Batch { edits })
}

/// Reorder the pages within the existing spread slots. Objects, parent
/// applications and overrides follow their page; parent IDs stay fixed. Facing
/// parent artwork follows the destination slot's side of the binding spine.
pub fn move_page(doc: &mut LayoutDocument, history: &mut History, from: usize, to: usize) -> bool {
    if from >= doc.pages.len() || to >= doc.pages.len() || from == to {
        return false;
    }
    let before = Topology::of(doc);
    let mut after = before.clone();
    let page = after.pages.remove(from);
    after.pages.insert(to, page);
    after.remap(|i| {
        Some(if i == from {
            to
        } else if from < i && i <= to {
            i - 1
        } else if to <= i && i < from {
            i + 1
        } else {
            i
        })
    });
    for parent in &mut after.parents {
        if parent.sheets.len() > 1 {
            let sheets: Vec<_> = parent
                .placements
                .iter()
                .map(|p| parent.sheet_for_side(doc.page_is_left(p.page)))
                .collect();
            for (placement, sheet) in parent.placements.iter_mut().zip(sheets) {
                placement.sheet = sheet;
            }
        }
    }
    apply_topology(doc, history, before, after)
}

pub fn set_parent(
    doc: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    parent: Option<usize>,
) -> bool {
    if page >= doc.pages.len() || parent.is_some_and(|i| i >= doc.parents.len()) {
        return false;
    }
    let before = Topology::of(doc);
    let mut after = before.clone();
    after.pages[page].master = parent;
    for (index, definition) in after.parents.iter_mut().enumerate() {
        definition.applied_to.retain(|p| *p != page);
        definition.placements.retain(|p| p.page != page);
        if Some(index) == parent {
            definition.applied_to.push(page);
        }
    }
    apply_topology(doc, history, before, after)
}

pub fn toggle_page_hidden(doc: &mut LayoutDocument, history: &mut History, index: usize) -> bool {
    let Some(page) = doc.pages.get(index) else {
        return false;
    };
    let before = crate::snapshot_page(page);
    let mut after = before.clone();
    after.hidden = !after.hidden;
    history.apply(
        doc,
        LayoutEdit::PageChanged {
            index,
            before,
            after,
        },
    )
}

pub fn edit_layers(
    doc: &mut LayoutDocument,
    history: &mut History,
    edit: impl FnOnce(&mut Layers),
) -> bool {
    let before = Layers::of(doc);
    let mut after = before.clone();
    edit(&mut after);
    if before.order != after.order {
        // Old documents may omit membership for objects on the first layer.
        // Reordering/adding layers must not silently move those objects.
        for object in doc.objects.iter().chain(
            doc.parents
                .iter()
                .flat_map(|parent| parent.objects.iter().map(|entry| &entry.object)),
        ) {
            if !after.objects.iter().any(|(id, _)| *id == object.id) {
                after.objects.push((object.id, doc.object_layer(object.id)));
            }
        }
    }
    if before == after || after.order.is_empty() {
        return false;
    }
    history.apply(
        doc,
        LayoutEdit::LayersChanged {
            before: Box::new(before),
            after: Box::new(after),
        },
    )
}

pub fn add_layer(doc: &mut LayoutDocument, history: &mut History, name: String) -> Option<LayerId> {
    let id = LayerId(
        doc.layers
            .iter()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)?,
    );
    edit_layers(doc, history, |layers| {
        layers.order.insert(0, id);
        layers.properties.push(LayoutLayer {
            id,
            name,
            visible: true,
            locked: false,
        });
    })
    .then_some(id)
}

pub fn change_layer(
    doc: &mut LayoutDocument,
    history: &mut History,
    id: LayerId,
    edit: impl FnOnce(&mut LayoutLayer),
) -> bool {
    if !doc.layers.contains(&id) {
        return false;
    }
    edit_layers(doc, history, |layers| {
        let index = layers
            .properties
            .iter()
            .position(|p| p.id == id)
            .unwrap_or_else(|| {
                layers.properties.push(LayoutLayer {
                    id,
                    name: String::new(),
                    visible: true,
                    locked: false,
                });
                layers.properties.len() - 1
            });
        edit(&mut layers.properties[index]);
    })
}

pub fn move_layer(
    doc: &mut LayoutDocument,
    history: &mut History,
    id: LayerId,
    delta: isize,
) -> bool {
    let Some(index) = doc.layers.iter().position(|layer| *layer == id) else {
        return false;
    };
    let Some(to) = index
        .checked_add_signed(delta)
        .filter(|to| *to < doc.layers.len())
    else {
        return false;
    };
    edit_layers(doc, history, |layers| {
        let layer = layers.order.remove(index);
        layers.order.insert(to, layer);
    })
}

/// Insert a layer before another layer, or at the bottom for `None`.
/// A drag preserves the relative order of all other layers and undoes once.
pub fn place_layer(
    doc: &mut LayoutDocument,
    history: &mut History,
    id: LayerId,
    before: Option<LayerId>,
) -> bool {
    if !doc.layers.contains(&id)
        || before == Some(id)
        || before.is_some_and(|target| !doc.layers.contains(&target))
    {
        return false;
    }
    edit_layers(doc, history, |layers| {
        layers.order.retain(|layer| *layer != id);
        let index = before
            .and_then(|target| layers.order.iter().position(|layer| *layer == target))
            .unwrap_or(layers.order.len());
        layers.order.insert(index, id);
    })
}

pub fn move_objects_to_layer(
    doc: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    target: LayerId,
) -> bool {
    if ids.is_empty()
        || !doc.layers.contains(&target)
        || doc.layer_locked(target)
        || ids.iter().any(|id| doc.object_locked(*id))
    {
        return false;
    }
    let moved: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| doc.object_layer(*id) != target)
        .collect();
    edit_layers(doc, history, |layers| {
        for id in &moved {
            match layers.objects.iter_mut().find(|(object, _)| object == id) {
                Some((_, layer)) => *layer = target,
                None => layers.objects.push((*id, target)),
            }
        }
    })
}
