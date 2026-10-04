//! Tool edits that run off the UI thread: the Remove tool's fills.
//!
//! A tool hands over [`BackgroundEdit`]s; they run one at a time, in the
//! order they were made, each prepared from the document as the previous
//! one left it. Painting carries on meanwhile -- a new stroke queues
//! behind the running one -- and the status bar shows progress with a
//! Cancel. A result whose document changed underneath it (an undo,
//! another tool) is thrown away and the edit prepared again from the
//! document as it now is.

use std::collections::VecDeque;

use schist_i18n::{t, tf};
use schist_plugin_api::{BackgroundApply, BackgroundEdit, JobControl, Overlay};

use super::*;

/// How many times an edit is prepared again because the document kept
/// changing under it before the result is written anyway. Something
/// that changes the document every second would otherwise starve it.
const MAX_RETRIES: u32 = 3;

#[derive(Default)]
pub struct ToolJobs {
    queue: VecDeque<BackgroundEdit>,
    running: Option<Running>,
    /// Identifies the running job, so a result arriving after a cancel
    /// is recognised as stale.
    next_id: u64,
    /// The last job fell back because a model was missing; the status
    /// bar offers Manage Models until the next one starts.
    pub(crate) missing_model: bool,
}

struct Running {
    id: u64,
    edit: BackgroundEdit,
    control: JobControl,
    document: schist_core::DocumentId,
    revision: u64,
    retries: u32,
}

impl ToolJobs {
    /// The running job's name and progress, 0..=1.
    pub(crate) fn progress(&self) -> Option<(&'static str, f32)> {
        self.running
            .as_ref()
            .map(|r| (r.edit.name, r.control.progress()))
    }

    /// What queued and running edits want drawn until they land.
    pub(crate) fn overlays(&self) -> impl Iterator<Item = &Overlay> {
        self.running
            .iter()
            .map(|r| &r.edit)
            .chain(self.queue.iter())
            .flat_map(|e| e.overlay.iter())
    }
}

impl Workspace {
    /// Collect the edits `tool_id` handed over during the last event and
    /// start the first if nothing is running.
    pub(super) fn drain_tool_jobs(&mut self, tool_id: &'static str, cx: &mut Context<Self>) {
        let Some(tool) = self.registry.tool_mut(tool_id) else {
            return;
        };
        let mut added = false;
        while let Some(edit) = tool.take_background_edit() {
            self.tool_jobs.queue.push_back(edit);
            added = true;
        }
        if added {
            self.start_tool_job(0, cx);
        }
    }

    fn start_tool_job(&mut self, retries: u32, cx: &mut Context<Self>) {
        if self.tool_jobs.running.is_some() {
            return;
        }
        while let Some(edit) = self.tool_jobs.queue.pop_front() {
            let Some(doc) = self.doc.as_ref() else {
                self.tool_jobs.queue.clear();
                return;
            };
            let Some(run) = (edit.prepare)(doc) else {
                continue;
            };
            let control = JobControl::new();
            let id = self.tool_jobs.next_id;
            self.tool_jobs.next_id += 1;
            self.tool_jobs.missing_model = false;
            self.tool_jobs.running = Some(Running {
                id,
                edit,
                control: control.clone(),
                document: doc.id,
                revision: doc.revision,
                retries,
            });
            cx.spawn(async move |this, cx| {
                let apply = cx
                    .background_executor()
                    .spawn(async move { run(&control) })
                    .await;
                this.update(cx, |ws, cx| ws.finish_tool_job(id, apply, cx))
                    .ok();
            })
            .detach();
            cx.notify();
            return;
        }
    }

    fn finish_tool_job(&mut self, id: u64, apply: Option<BackgroundApply>, cx: &mut Context<Self>) {
        if self.tool_jobs.running.as_ref().map(|r| r.id) != Some(id) {
            return; // cancelled while it ran
        }
        let Some(running) = self.tool_jobs.running.take() else {
            return;
        };
        let name = running.edit.name;
        match (apply, self.doc.as_mut()) {
            (Some(apply), Some(doc)) if doc.id == running.document => {
                if doc.revision != running.revision && running.retries < MAX_RETRIES {
                    self.tool_jobs.queue.push_front(running.edit);
                    self.start_tool_job(running.retries + 1, cx);
                    return;
                }
                if apply(doc) {
                    self.status = name.into();
                }
                if running.control.missing_model().is_some() {
                    self.tool_jobs.missing_model = true;
                    self.status = t("workspace.tool_jobs.missing_model").into();
                }
            }
            (Some(_), _) => {
                self.status = tf!("workspace.tool_jobs.document_changed", name = name).into();
            }
            (None, _) => {}
        }
        self.start_tool_job(0, cx);
        self.after_change(cx);
    }

    /// Stop the running edit and drop the queued ones. False if there
    /// were none, so Escape can fall through to whatever else it does.
    pub(crate) fn cancel_tool_jobs(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(running) = self.tool_jobs.running.take() else {
            return false;
        };
        running.control.cancel();
        self.tool_jobs.queue.clear();
        self.status = t("common.cancelled").into();
        cx.notify();
        true
    }
}
