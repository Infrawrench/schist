//! Layout command identity and presentation, independent of raster plugins.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignCommand {
    Undo,
    Redo,
    Delete,
    Duplicate,
    SelectAll,
    Deselect,
}

impl DesignCommand {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "edit.undo" => Self::Undo,
            "edit.redo" => Self::Redo,
            "edit.delete" | "edit.clear" => Self::Delete,
            "edit.duplicate" => Self::Duplicate,
            "select.all" => Self::SelectAll,
            "select.deselect" => Self::Deselect,
            _ => return None,
        })
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Undo => "common.undo",
            Self::Redo => "common.redo",
            Self::Delete => "common.delete",
            Self::Duplicate => "common.duplicate",
            Self::SelectAll => "command.select.all.title",
            Self::Deselect => "common.deselect",
        }
    }

    pub fn keybind(self) -> Option<&'static str> {
        match self {
            Self::Undo => Some("cmd-z"),
            Self::Redo => Some("cmd-shift-z"),
            Self::Duplicate => Some("cmd-d"),
            Self::SelectAll => Some("cmd-a"),
            // Bare Delete/Escape must reach text controls while typing.
            Self::Delete | Self::Deselect => None,
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Undo,
        Self::Redo,
        Self::Delete,
        Self::Duplicate,
        Self::SelectAll,
        Self::Deselect,
    ];
}
