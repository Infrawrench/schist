//! Parent-sheet geometry and its placement on document pages. Master artwork
//! stays shared; rendering applies a page's overlay to a temporary object.
use crate::{affine::Affine, LayoutDocument, ObjectId, Page, ParentPage, PlacedObject, Point};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentSheet {
    pub page: Page,
    pub origin: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ParentSource>,
}

/// The base template of a parent sheet. Overrides are local to this sheet,
/// distinct from the document-page overrides on ParentObject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentSource {
    pub parent: usize,
    pub sheet: usize,
    pub transform: Affine,
    pub visible: bool,
    pub overrides: Vec<ObjectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParentPlacement {
    pub page: usize,
    pub sheet: usize,
    /// Parent page-local coordinates to document page-local coordinates.
    pub transform: Affine,
    pub visible: bool,
}

impl ParentPage {
    /// Choose the template on the requested side of the spread spine. A
    /// single-sheet parent applies to either side.
    pub fn sheet_for_side(&self, left: bool) -> usize {
        self.sheets
            .iter()
            .enumerate()
            .filter(|(_, s)| (s.origin.x + s.page.width / 2.0 < 0.0) == left)
            .min_by(|(_, a), (_, b)| {
                (a.origin.x + a.page.width / 2.0)
                    .abs()
                    .total_cmp(&(b.origin.x + b.page.width / 2.0).abs())
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    pub fn source(&self, doc: &LayoutDocument, sheet: usize) -> Option<ParentSource> {
        self.sheets
            .get(sheet)
            .and_then(|s| s.source.clone())
            .or_else(|| {
                let parent = self.based_on?;
                let base = doc.parents.get(parent)?;
                Some(ParentSource {
                    parent,
                    sheet: sheet.min(base.sheets.len().saturating_sub(1)),
                    transform: Affine::IDENTITY,
                    visible: true,
                    overrides: Vec::new(),
                })
            })
    }

    pub fn placement(&self, doc: &LayoutDocument, page: usize) -> ParentPlacement {
        self.placements
            .iter()
            .find(|p| p.page == page)
            .copied()
            .unwrap_or_else(|| {
                let sheet = self.sheet_for_side(doc.page_is_left(page));
                ParentPlacement {
                    page,
                    sheet,
                    transform: Affine::IDENTITY,
                    visible: true,
                }
            })
    }

    pub(crate) fn remap_placements(&mut self, mut map: impl FnMut(usize) -> Option<usize>) {
        self.placements.retain_mut(|placement| {
            if let Some(page) = map(placement.page) {
                placement.page = page;
                true
            } else {
                false
            }
        });
    }
}

impl LayoutDocument {
    /// Physical page count to the left of a spread's spine. Explicit binding
    /// locations survive numbering changes and page moves between fixed slots.
    pub fn spread_binding(&self, spread: &crate::Spread) -> usize {
        spread
            .binding_location
            .unwrap_or_else(|| {
                if !self.facing_pages {
                    0
                } else if spread.pages.len() > 1 {
                    spread.pages.len() / 2
                } else {
                    spread.pages.first().map_or(0, |page| {
                        let even = self.page_number_value(*page).is_multiple_of(2);
                        usize::from(even != (self.page_binding == crate::PageBinding::RightToLeft))
                    })
                }
            })
            .min(spread.pages.len())
    }

    pub fn page_is_left(&self, page: usize) -> bool {
        self.spreads
            .iter()
            .find_map(|spread| {
                spread
                    .pages
                    .iter()
                    .position(|p| *p == page)
                    .map(|index| index < self.spread_binding(spread))
            })
            .unwrap_or(false)
    }
}

pub(crate) fn placed(object: &PlacedObject, placement: ParentPlacement) -> PlacedObject {
    let mut placed = object.clone();
    placed.page = placement.page;
    // Keep the composition box unchanged. The overlay moves final artwork,
    // including oriented glyphs, native image channels and frame clipping.
    placed.transform = Affine::translate(-object.bounds.x, -object.bounds.y)
        .then(&placement.transform)
        .then(&object.content_transform())
        .then(&Affine::translate(object.bounds.x, object.bounds.y));
    placed.rotation = 0.0;
    placed
}

/// Resolve a hierarchy base-first. Invalid cycles terminate before revisiting
/// a template; native import reports them as invalid parent references.
pub(crate) fn inherited(
    doc: &LayoutDocument,
    parent: usize,
    placement: ParentPlacement,
) -> Vec<PlacedObject> {
    // A sheet has one base. Walk the chain iteratively so a deeply nested
    // imported hierarchy cannot exhaust the thread stack.
    let mut chain = Vec::new();
    let mut counts = std::collections::BTreeMap::<ObjectId, usize>::new();
    let mut visited = std::collections::BTreeSet::new();
    let (mut index, mut placement) = (parent, placement);
    while let Some(template) = doc.parents.get(index) {
        if template.hidden || !placement.visible || !visited.insert((index, placement.sheet)) {
            break;
        }
        let source = template.source(doc, placement.sheet);
        let overrides = source
            .as_ref()
            .map(|s| s.overrides.clone())
            .unwrap_or_default();
        for id in &overrides {
            *counts.entry(*id).or_default() += 1;
        }
        chain.push((index, placement, overrides));
        let Some(source) = source else {
            break;
        };
        index = source.parent;
        placement = ParentPlacement {
            page: placement.page,
            sheet: source.sheet,
            transform: placement.transform.then(&source.transform),
            visible: source.visible,
        };
    }
    let mut out = Vec::new();
    for (index, placement, overrides) in chain.into_iter().rev() {
        // A sheet's own override list suppresses its ancestors only.
        for id in overrides {
            if let Some(count) = counts.get_mut(&id) {
                *count -= 1;
                if *count == 0 {
                    counts.remove(&id);
                }
            }
        }
        let template = &doc.parents[index];
        for entry in &template.objects {
            if entry.tracks_parent(placement.page)
                && !counts.contains_key(&entry.object.id)
                && (template.sheets.is_empty() || entry.object.page == placement.sheet)
            {
                out.push(placed(&entry.object, placement));
            }
        }
    }
    out
}
