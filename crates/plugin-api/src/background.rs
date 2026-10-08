//! Tool edits slow enough to run off the UI thread.
//!
//! A tool that hands the host a [`BackgroundEdit`] splits its work in
//! three: `prepare` reads what it needs from the document on the UI
//! thread, the closure it returns does the slow part on a worker, and
//! the closure *that* returns writes the result back on the UI thread as
//! one history entry. The document never crosses threads; only owned
//! copies of its pixels do.
//!
//! `prepare` can be called again. A host that finds the document changed
//! underneath a running edit (an undo, another tool) drops the stale
//! result and prepares the same edit afresh from the document as it now
//! is, so a removal is never written over pixels it did not see.
//!
//! Hosts that cannot run work elsewhere -- tests, the MCP server, the
//! browser -- simply never enable background edits, and the tool runs
//! the three stages in a row with [`BackgroundEdit::run_now`].

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use schist_core::Document;

use crate::Overlay;

/// Progress, cancellation and notices shared between a host and the
/// worker running a [`BackgroundEdit`]. Cloning shares the same state.
#[derive(Clone, Default)]
pub struct JobControl {
    cancelled: Arc<AtomicBool>,
    /// `f32` bits, 0..=1.
    progress: Arc<AtomicU32>,
    missing_model: Arc<Mutex<Option<&'static str>>>,
}

impl JobControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the worker to stop. Checked between stages; a stage that is
    /// already running (a network's forward pass) finishes first.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Report how far the work has got, 0..=1. Never moves backwards.
    pub fn set_progress(&self, fraction: f32) {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut current = self.progress.load(Ordering::Relaxed);
        while fraction > f32::from_bits(current) {
            match self.progress.compare_exchange_weak(
                current,
                fraction.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    /// The work fell back to a classical path because the model with
    /// this catalogue id is not installed. Hosts offer the download.
    pub fn note_missing_model(&self, id: &'static str) {
        if let Ok(mut slot) = self.missing_model.lock() {
            *slot = Some(id);
        }
    }

    pub fn missing_model(&self) -> Option<&'static str> {
        self.missing_model.lock().ok().and_then(|slot| *slot)
    }
}

/// Writes a finished result into the document as one history entry.
/// Returns whether anything was written.
pub type BackgroundApply = Box<dyn FnOnce(&mut Document) -> bool + Send>;
/// The slow part, run on a worker. `None` means cancelled or nothing to
/// do.
pub type BackgroundRun = Box<dyn FnOnce(&JobControl) -> Option<BackgroundApply> + Send>;
/// Reads what the slow part needs from the document, on the UI thread.
pub type BackgroundPrepare = Box<dyn Fn(&Document) -> Option<BackgroundRun> + Send>;

/// An edit a tool wants run off the UI thread. See the module docs.
pub struct BackgroundEdit {
    /// What the host shows while it runs, already translated.
    pub name: &'static str,
    pub prepare: BackgroundPrepare,
    /// Drawn by the host while the edit is queued or running, so what
    /// the user painted stays on screen until its result lands.
    pub overlay: Vec<Overlay>,
}

impl BackgroundEdit {
    /// Prepare, run and apply in a row, on this thread. What a host that
    /// has no worker to give does, and what a tool does when its host
    /// has not enabled background edits.
    pub fn run_now(&self, doc: &mut Document, control: &JobControl) -> bool {
        let Some(run) = (self.prepare)(doc) else {
            return false;
        };
        let Some(apply) = run(control) else {
            return false;
        };
        apply(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_only_moves_forwards_and_stays_in_range() {
        let control = JobControl::new();
        control.set_progress(0.4);
        control.set_progress(0.2);
        assert_eq!(control.progress(), 0.4);
        control.set_progress(7.0);
        assert_eq!(control.progress(), 1.0);
        control.set_progress(f32::NAN);
        assert_eq!(control.progress(), 1.0);
    }

    #[test]
    fn clones_share_cancellation_and_notices() {
        let control = JobControl::new();
        let worker = control.clone();
        assert!(!worker.is_cancelled());
        control.cancel();
        assert!(worker.is_cancelled());
        worker.note_missing_model("inpaint");
        assert_eq!(control.missing_model(), Some("inpaint"));
    }
}
