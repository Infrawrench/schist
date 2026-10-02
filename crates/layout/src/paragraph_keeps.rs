//! Independent paragraph keep policies. Unset properties inherit; disabling
//! line keeps retains their first/last counts and whole-paragraph choice.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParagraphKeeps {
    pub enabled: Option<bool>,
    pub all: Option<bool>,
    pub first: Option<usize>,
    pub last: Option<usize>,
    /// Following-paragraph lines kept with this paragraph's last line.
    pub next: Option<usize>,
    /// Keep this paragraph's first line with the preceding paragraph's last.
    pub previous: Option<bool>,
}

impl ParagraphKeeps {
    pub fn over(&self, base: &Self) -> Self {
        Self {
            enabled: self.enabled.or(base.enabled),
            all: self.all.or(base.all),
            first: self.first.or(base.first),
            last: self.last.or(base.last),
            next: self.next.or(base.next),
            previous: self.previous.or(base.previous),
        }
    }

    /// Older Schist styles used one symmetric widow/orphan count and a
    /// keep-with-next toggle. Preserve those saved values as an inherited base.
    pub fn from_legacy(next: Option<bool>, lines: Option<usize>) -> Self {
        Self {
            enabled: lines.map(|_| true),
            all: lines.map(|_| false),
            first: lines.map(|n| n.max(1)),
            last: lines.map(|n| n.max(1)),
            next: next.map(usize::from),
            previous: None,
        }
    }

    /// Choose a complete prefix of already-shaped lines, retaining the
    /// minimum fragments on both sides of a paragraph split.
    pub fn fitting_lines(&self, available: usize, total: usize) -> usize {
        if available >= total || self.enabled != Some(true) {
            return available;
        }
        if self.all == Some(true) {
            return 0;
        }
        let first = self.first.unwrap_or(2).max(1);
        let last = self.last.unwrap_or(2).max(1);
        let count = available.min(total.saturating_sub(last));
        if count < first {
            0
        } else {
            count
        }
    }
}
