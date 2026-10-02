//! Undo for a layout document.
//!
//! A layout editor cannot be used without undo, and a layout document is
//! large enough that keeping whole copies is not an option: a hundred
//! pages of text is a few megabytes, and a slider drag would make one of
//! those per mouse event.
//!
//! So history stores **operations**, not snapshots. Each edit is
//! recorded as something small and reversible, and applying the inverse
//! restores the previous state exactly. That is the same approach
//! `schist_core` takes for a raster document, and for the same reason.
//!
//! Applied and undone operations live on separate stacks. A new edit
//! discards the undone branch; failed application restores both stacks.

use serde::{Deserialize, Serialize};

/// One reversible change to a layout document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayoutEdit {
    /// Several operations from one gesture, reversed in the opposite order.
    Batch { edits: Vec<LayoutEdit> },
    /// Page membership, spreads and parent applications, without text or pixels.
    TopologyChanged {
        before: Box<crate::structure::Topology>,
        after: Box<crate::structure::Topology>,
    },
    StylesChanged {
        before: Box<crate::StyleSet>,
        after: Box<crate::StyleSet>,
    },
    SwatchesChanged {
        before: Vec<crate::Ink>,
        after: Vec<crate::Ink>,
    },
    LayersChanged {
        before: Box<crate::structure::Layers>,
        after: Box<crate::structure::Layers>,
    },
    ThreadsChanged {
        before: Vec<(crate::StoryId, Vec<crate::ObjectId>)>,
        after: Vec<(crate::StoryId, Vec<crate::ObjectId>)>,
    },
    /// Chronology accompanies the creation transaction, independently of z-order.
    CreationOrderChanged {
        before: Vec<crate::ObjectId>,
        after: Vec<crate::ObjectId>,
    },
    /// A page was added.
    ///
    /// The spread list is recorded *before* the change. A page edit can
    /// disturb any number of spreads -- removing a page renumbers every
    /// spread after it -- so recording one spread is not enough to make
    /// the inverse exact, and an inexact inverse is worse than none.
    AddedPage {
        index: usize,
        page: PageSnapshot,
        spreads: Vec<SpreadSnapshot>,
    },
    /// A page was removed, keeping enough to put it back.
    RemovedPage {
        index: usize,
        page: PageSnapshot,
        spreads: Vec<SpreadSnapshot>,
    },
    /// A page's settings changed.
    PageChanged {
        index: usize,
        before: PageSnapshot,
        after: PageSnapshot,
    },
    /// A page moved within the document.
    PageMoved { from: usize, to: usize },

    /// An object was added.
    AddedObject {
        index: usize,
        object: ObjectSnapshot,
    },
    /// An object was removed.
    RemovedObject {
        index: usize,
        object: ObjectSnapshot,
    },
    /// Several objects went at once, as a delete of a multi-frame
    /// selection does.
    ///
    /// This exists so that one gesture is one undo step. Deleting four
    /// frames as four edits means four presses of ⌘Z, and an undo stack
    /// that does not match the gestures that filled it is worse than no
    /// undo. The indices are positions in the document *before* the
    /// removal, so reversing has to put them back in the order they
    /// appear rather than the order they went.
    RemovedObjects { items: Vec<(usize, ObjectSnapshot)> },
    /// An object's geometry or flags changed.
    ObjectChanged {
        id: u32,
        before: ObjectSnapshot,
        after: ObjectSnapshot,
    },

    /// A story appended with its first frame, undone together with it.
    AddedStory { id: u32, story: StorySnapshot },

    /// A story's points changed.
    StoryChanged {
        id: u32,
        before: StorySnapshot,
        after: StorySnapshot,
    },

    /// A style was added, changed or removed.
    StyleChanged {
        name: String,
        before: Option<StyleSnapshot>,
        after: Option<StyleSnapshot>,
    },

    /// An ink was added, changed or removed.
    InkChanged {
        name: String,
        before: Option<InkSnapshot>,
        after: Option<InkSnapshot>,
    },

    /// The whole document settings changed, as a grid or binding edit.
    ///
    /// Boxed because a settings snapshot is several strings wide and an
    /// undo stack holds a pair of them. Unboxed it made this variant the
    /// largest in the enum by a wide margin, and every edit on the stack
    /// paid for that.
    DocumentChanged {
        before: Box<SettingsSnapshot>,
        after: Box<SettingsSnapshot>,
    },
}

// The snapshots are the parts of the document an edit can touch. They
// are plain data with no behaviour, so a history file stays readable and
// does not pin the whole document type.

/// A page's settings, detached from the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub name: String,
    #[serde(default)]
    pub section: Option<crate::Section>,
    pub width: f32,
    pub height: f32,
    pub bleed: crate::Insets,
    pub slug: crate::Insets,
    pub margins: [f32; 4],
    pub landscape: bool,
    pub hidden: bool,
    pub master: Option<usize>,
    #[serde(default)]
    pub guides: Vec<crate::geometry::RulerGuide>,
}

/// Which pages a spread holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpreadSnapshot {
    pub pages: Vec<usize>,
    #[serde(default)]
    pub binding_location: Option<usize>,
    pub gutter: f32,
    pub origin: [f32; 2],
}

/// An object, detached from the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectSnapshot {
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub appearance: Box<crate::object_styles::ObjectAppearance>,
    pub id: u32,
    pub page: usize,
    pub bounds: [f32; 4],
    pub name: String,
    pub locked: bool,
    pub overprint: bool,
    pub transparency: f32,
    pub rotation: f32,
    #[serde(default)]
    pub transform: schist_core::Affine,
    /// The object itself, as an opaque value. Composition and rendering
    /// interpret it; history only needs to put it back.
    pub payload: serde_json::Value,
}

/// A story, detached from the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorySnapshot {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structures: Vec<crate::story::StoryStructure>,
    pub points: Vec<StoryPointSnapshot>,
    pub ranges: Vec<(usize, usize, String)>,
    #[serde(default)]
    pub prefs: crate::story::StoryPreferences,
}

/// One point of a story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StoryPointSnapshot {
    Paragraph { text: String, style: String },
    LineBreak,
    ColumnBreak,
    PageBreak,
    FrameBreak,
    Other { kind: String, payload: String },
}

/// A style, detached from the document.
///
/// The style itself travels as an opaque value rather than as a fixed
/// set of fields, so adding a property to `ParagraphStyle` does not
/// invalidate a history file written before it existed. The flag says
/// which of the two style kinds it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleSnapshot {
    pub paragraph: bool,
    pub name: String,
    pub payload: serde_json::Value,
}

/// An ink, detached from the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InkSnapshot {
    pub name: String,
    pub lab: [f32; 3],
    pub preview_rgb: [f32; 3],
    #[serde(default)]
    pub source_cmyk: Option<[f32; 4]>,
    pub spot: bool,
    #[serde(default)]
    pub tint: Option<crate::ink::InkTint>,
}

/// The document-wide settings an edit can change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    #[serde(default)]
    pub footnotes: crate::footnotes::FootnoteOptions,
    #[serde(default)]
    pub frame_footnote_defaults: crate::footnotes::FrameFootnotes,
    #[serde(default)]
    pub balance_columns_default: bool,
    pub facing_pages: bool,
    #[serde(default)]
    pub page_binding: crate::PageBinding,
    pub default_paragraph_style: String,
    pub default_character_style: String,
    pub grids: serde_json::Value,
    pub ink_manager: serde_json::Value,
}

/// How many operations a default history keeps.
pub const DEFAULT_LIMIT: usize = 200;

/// A layout document's undo history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct History {
    undo: Vec<LayoutEdit>,
    redo: Vec<LayoutEdit>,
    /// The furthest point reached, for a history panel to show.
    cursor: usize,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        History::with_limit(DEFAULT_LIMIT)
    }
}

impl History {
    /// A history that keeps at most `limit` operations.
    ///
    /// The default is generous for a layout document: edits are small,
    /// and a user who has to redo fifty steps is having a bad day. The
    /// cap exists so a pathological session cannot grow without bound.
    pub fn with_limit(limit: usize) -> History {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: 0,
            limit: limit.max(1),
        }
    }

    /// Record an operation.
    ///
    /// Recording after the document has been changed is the only order
    /// available to a UI, so the caller passes both sides and this
    /// records them in that order. Recording anything after a branch
    /// discards the redo stack, because the future has changed.
    pub fn record(&mut self, edit: LayoutEdit) {
        // A new edit invalidates the redo branch.
        self.redo.clear();
        self.undo.push(edit);
        while self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.cursor = self.undo.len();
    }

    /// The operation to undo, if there is one.
    pub fn pop_undo(&mut self) -> Option<LayoutEdit> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        let edit = self.undo.pop()?;
        self.redo.push(edit.clone());
        Some(edit)
    }

    /// The operation to redo, if there is one.
    pub fn pop_redo(&mut self) -> Option<LayoutEdit> {
        let edit = self.redo.pop()?;
        self.cursor += 1;
        self.undo.push(edit.clone());
        Some(edit)
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.cursor
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// The operations still to be undone, oldest first. A history panel
    /// shows these.
    pub fn pending(&self) -> &[LayoutEdit] {
        &self.undo[..self.cursor]
    }

    /// Whether `edit` can be inverted, so a caller can bail out before
    /// changing the document rather than after.
    pub fn is_reversible(edit: &LayoutEdit) -> bool {
        match edit {
            LayoutEdit::Batch { edits } => {
                !edits.is_empty() && edits.iter().all(Self::is_reversible)
            }
            LayoutEdit::ThreadsChanged { .. }
            | LayoutEdit::CreationOrderChanged { .. }
            | LayoutEdit::TopologyChanged { .. }
            | LayoutEdit::LayersChanged { .. }
            | LayoutEdit::SwatchesChanged { .. }
            | LayoutEdit::StylesChanged { .. } => true,
            LayoutEdit::AddedPage { .. }
            | LayoutEdit::RemovedPage { .. }
            | LayoutEdit::PageChanged { .. }
            | LayoutEdit::PageMoved { .. }
            | LayoutEdit::AddedObject { .. }
            | LayoutEdit::RemovedObject { .. }
            | LayoutEdit::RemovedObjects { .. }
            | LayoutEdit::ObjectChanged { .. }
            | LayoutEdit::StoryChanged { .. }
            | LayoutEdit::AddedStory { .. }
            | LayoutEdit::StyleChanged { .. }
            | LayoutEdit::InkChanged { .. }
            | LayoutEdit::DocumentChanged { .. } => true,
        }
    }

    /// Forget everything. A document that is about to be discarded, or
    /// one that has just been reloaded, should not offer to undo back
    /// into a document that no longer exists.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.cursor = 0;
    }

    /// Put a failed operation back where it came from, so the history
    /// still describes the document after an apply that could not be
    /// completed.
    pub(crate) fn restore_undo(&mut self, edit: LayoutEdit) {
        self.redo.pop();
        self.undo.push(edit);
        self.cursor = self.undo.len();
    }

    /// Put a failed redo back on the redo stack.
    pub(crate) fn restore_redo(&mut self, edit: LayoutEdit) {
        self.undo.pop();
        self.cursor = self.undo.len();
        self.redo.push(edit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(id: u32, name: &str) -> ObjectSnapshot {
        ObjectSnapshot {
            hidden: false,
            appearance: Default::default(),
            id,
            page: 0,
            bounds: [0.0, 0.0, 10.0, 10.0],
            name: name.into(),
            locked: false,
            overprint: false,
            transparency: 1.0,
            rotation: 0.0,
            transform: Default::default(),
            payload: serde_json::Value::Null,
        }
    }

    #[test]
    fn a_new_history_can_do_nothing() {
        let history = History::default();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.undo_depth(), 0);
    }

    #[test]
    fn undo_and_redo_walk_the_stack() {
        let mut history = History::default();
        history.record(LayoutEdit::AddedObject {
            index: 0,
            object: object(1, "One"),
        });
        history.record(LayoutEdit::AddedObject {
            index: 1,
            object: object(2, "Two"),
        });
        assert_eq!(history.undo_depth(), 2);
        assert!(history.can_undo());
        assert!(!history.can_redo());

        assert!(history.pop_undo().is_some());
        assert!(history.can_redo());
        assert_eq!(history.undo_depth(), 1);
        assert!(history.pop_redo().is_some());
        assert_eq!(history.undo_depth(), 2);
    }

    #[test]
    fn undo_stops_at_the_beginning() {
        let mut history = History::default();
        history.record(LayoutEdit::PageMoved { from: 0, to: 1 });
        assert!(history.pop_undo().is_some());
        assert!(history.pop_undo().is_none());
        assert_eq!(history.undo_depth(), 0);
    }

    #[test]
    fn redo_stops_at_the_end() {
        let mut history = History::default();
        history.record(LayoutEdit::PageMoved { from: 0, to: 1 });
        assert!(history.pop_redo().is_none());
    }

    #[test]
    fn a_new_edit_after_undo_discards_the_redo_branch() {
        let mut history = History::default();
        history.record(LayoutEdit::PageMoved { from: 0, to: 1 });
        history.record(LayoutEdit::PageMoved { from: 1, to: 2 });
        history.pop_undo();
        assert!(history.can_redo());
        // A new edit means the undone future never happened.
        history.record(LayoutEdit::PageMoved { from: 5, to: 6 });
        assert!(!history.can_redo());
        assert_eq!(history.redo_depth(), 0);
    }

    #[test]
    fn the_limit_drops_the_oldest_operations() {
        let mut history = History::with_limit(3);
        for i in 0..5 {
            history.record(LayoutEdit::PageMoved { from: i, to: i + 1 });
        }
        // The two oldest are gone, and the cursor still points at the end
        // rather than past it.
        assert_eq!(history.undo_depth(), 3);
        assert_eq!(history.pending().len(), 3);
        assert!(history.can_undo());
    }

    #[test]
    fn a_limit_of_zero_is_clamped_to_one() {
        // Zero would drop every operation as it arrived, so undo would
        // silently do nothing.
        let mut history = History::with_limit(0);
        history.record(LayoutEdit::PageMoved { from: 0, to: 1 });
        assert_eq!(history.undo_depth(), 1);
    }

    #[test]
    fn pending_shows_the_operations_left_to_undo() {
        let mut history = History::default();
        for i in 0..3 {
            history.record(LayoutEdit::PageMoved { from: i, to: i + 1 });
        }
        history.pop_undo();
        let pending = history.pending();
        assert_eq!(pending.len(), 2);
        assert!(matches!(pending[0], LayoutEdit::PageMoved { from: 0, .. }));
    }

    #[test]
    fn clearing_drops_both_directions() {
        let mut history = History::default();
        history.record(LayoutEdit::PageMoved { from: 0, to: 1 });
        history.pop_undo();
        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn every_operation_is_reversible() {
        // An operation that could not be inverted would let undo corrupt
        // the document, so the set is checked rather than trusted.
        for edit in [
            LayoutEdit::AddedObject {
                index: 0,
                object: object(1, "One"),
            },
            LayoutEdit::RemovedObject {
                index: 0,
                object: object(1, "One"),
            },
            LayoutEdit::PageMoved { from: 0, to: 1 },
        ] {
            assert!(History::is_reversible(&edit));
        }
    }

    #[test]
    fn a_history_survives_a_save_and_load() {
        let mut history = History::default();
        history.record(LayoutEdit::AddedObject {
            index: 0,
            object: object(1, "One"),
        });
        let json = serde_json::to_string(&history).unwrap();
        let back: History = serde_json::from_str(&json).unwrap();
        assert_eq!(back, history);
        assert!(back.can_undo());
    }
}
