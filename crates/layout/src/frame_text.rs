//! Rectangular text-frame flow policy, independent of story paragraph styles.
use crate::{History, LayoutDocument, LayoutEdit, LayoutObject, ObjectId};

/// Older Schist snapshots always enabled balancing. Retain that preference;
/// new authoring and native imports use explicit values and native defaults.
pub(crate) fn legacy_balance() -> Option<bool> {
    Some(true)
}

/// Document and parent objects supply their resolved policy. Geometry-only
/// compose_thread callers without a stored object retain legacy balancing.
pub(crate) fn balanced(doc: &LayoutDocument, id: ObjectId) -> bool {
    doc.object(id)
        .or_else(|| {
            doc.parents
                .iter()
                .flat_map(|p| &p.objects)
                .find(|p| p.object.id == id)
                .map(|p| &p.object)
        })
        .is_none_or(|object| doc.styles.frame_balance(object))
}

/// A whole selection changes in one undo step. Invalid, locked or path-text
/// targets reject the entire edit. None restores object-style inheritance.
pub fn set_balance(
    doc: &mut LayoutDocument,
    history: &mut History,
    ids: &[ObjectId],
    value: Option<bool>,
) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        let Some(object) = doc.object(*id).filter(|_| !doc.object_locked(*id)) else {
            return false;
        };
        let mut after = object.clone();
        let LayoutObject::TextFrame {
            balance_columns,
            text_path: None,
            ..
        } = &mut after.object
        else {
            return false;
        };
        *balance_columns = value;
        if after != *object {
            edits.push(LayoutEdit::ObjectChanged {
                id: id.0,
                before: crate::snapshot_object(object),
                after: crate::snapshot_object(&after),
            });
        }
    }
    !edits.is_empty() && history.apply(doc, LayoutEdit::Batch { edits })
}

/// Creation defaults affect future frames only, with a single settings edit.
pub fn set_default(doc: &mut LayoutDocument, history: &mut History, value: bool) -> bool {
    if doc.balance_columns_default == value {
        return false;
    }
    let before = crate::snapshot_settings(doc);
    let mut after = before.clone();
    after.balance_columns_default = value;
    history.apply(
        doc,
        LayoutEdit::DocumentChanged {
            before: Box::new(before),
            after: Box::new(after),
        },
    )
}
