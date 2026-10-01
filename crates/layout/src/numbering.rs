//! Numbering boundaries belong to pages, not spread slots. Reordering a page
//! carries its section; removing it removes that boundary. Before the first
//! explicit boundary, numbering starts at 1 in the default Arabic style.
use crate::{snapshot_page, History, LayoutDocument, LayoutEdit, NumberStyle};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub start: u32,
    pub continue_numbering: bool,
    pub style: NumberStyle,
    pub prefix: String,
    pub include_prefix: bool,
    pub name: String,
    pub marker: String,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            start: 1,
            continue_numbering: true,
            style: NumberStyle::Arabic,
            prefix: String::new(),
            include_prefix: false,
            name: String::new(),
            marker: String::new(),
        }
    }
}

impl LayoutDocument {
    /// The active boundary's page and settings. Page zero also has an implicit
    /// default section when it has no explicit settings of its own.
    pub fn section_at(&self, page: usize) -> (usize, Section) {
        self.pages
            .iter()
            .enumerate()
            .take(page.saturating_add(1))
            .rev()
            .find_map(|(i, p)| p.section.as_ref().map(|s| (i, s.clone())))
            .unwrap_or_default()
    }

    /// The numeric sequence advances in document reading order, including
    /// hidden pages. A continuing section changes style/prefix without a reset.
    pub fn page_number_value(&self, page: usize) -> u32 {
        let mut start = 1u32;
        let mut from = 0;
        for (i, p) in self.pages.iter().enumerate().take(page.saturating_add(1)) {
            if let Some(section) = &p.section {
                if !section.continue_numbering {
                    start = section.start.max(1);
                    from = i;
                }
            }
        }
        start.saturating_add(u32::try_from(page.saturating_sub(from)).unwrap_or(u32::MAX))
    }

    pub fn page_number(&self, page: usize) -> String {
        let (_, section) = self.section_at(page);
        format!(
            "{}{}",
            if section.include_prefix {
                &section.prefix
            } else {
                ""
            },
            section.style.format(self.page_number_value(page))
        )
    }
}

/// Set or remove a boundary in one undo step. An explicit default at page zero
/// is redundant and is normalized away, including on IDML import.
pub fn set_section(
    doc: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    section: Option<Section>,
) -> bool {
    let Some(current) = doc.pages.get(page) else {
        return false;
    };
    if section
        .as_ref()
        .is_some_and(|s| !(1..=999999).contains(&s.start))
    {
        return false;
    }
    let before = snapshot_page(current);
    let mut after = before.clone();
    after.section = section.filter(|s| page != 0 || *s != Section::default());
    before != after
        && history.apply(
            doc,
            LayoutEdit::PageChanged {
                index: page,
                before,
                after,
            },
        )
}

/// Edit the section captured by the caller, independently of later selection
/// changes. Callers use `section_at` to address the boundary, not its interior.
pub fn edit_section(
    doc: &mut LayoutDocument,
    history: &mut History,
    page: usize,
    edit: impl FnOnce(&mut Section),
) -> bool {
    if page >= doc.pages.len() {
        return false;
    }
    let (_, mut section) = doc.section_at(page);
    edit(&mut section);
    set_section(doc, history, page, Some(section))
}
