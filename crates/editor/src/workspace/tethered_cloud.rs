//! Cloud-only captures queue their originals without joining the local gallery.
use super::*;
use schist_cloud::Scope;
use schist_cloud_transfer::{cancellable, upload_files, Report, UploadSummary};
use schist_i18n::{t, tf};
use schist_ui::{Button, DropdownButton};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) const DESTINATION_POPUP: Popup = Popup::Field("tethered-cloud-destination");

#[derive(Clone)]
pub(super) struct Target {
    pub epoch: u64,
    pub scope: Scope,
    pub label: String,
}

#[derive(Clone)]
pub(super) struct Pending {
    pub target: Target,
    pub paths: Vec<PathBuf>,
    // Persistent private storage: failure, cancellation and shutdown never
    // delete the only copy of a webcam capture. Only acknowledgement cleans it.
    pub directory: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct State {
    pub target: Option<Target>,
    pending: VecDeque<Pending>,
    active: Option<(u64, Arc<AtomicBool>)>,
    next_id: u64,
    failed: bool,
    message: String,
    recovery: Option<PathBuf>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some((_, cancel)) = &self.active {
            cancel.store(true, Ordering::Release);
        }
    }
}
pub(super) enum Event {
    Progress(String),
    Finished(Result<String, String>),
}

impl State {
    fn invalidate(&mut self, epoch: u64) {
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.epoch != epoch)
        {
            self.target = None;
        }
        if self
            .pending
            .front()
            .is_some_and(|batch| batch.target.epoch != epoch)
        {
            self.stop();
            self.active = None;
            self.pending.clear();
            self.failed = false;
        }
    }
    fn stop(&mut self) {
        if let Some((_, cancel)) = &self.active {
            cancel.store(true, Ordering::Release);
        }
        self.recovery = self
            .pending
            .front()
            .and_then(|batch| batch.directory.as_ref()?.parent().map(PathBuf::from));
        self.failed = !self.pending.is_empty();
        self.message = t("cloud.upload.cancelled").into();
    }
    fn finish(&mut self, id: u64, result: Result<String, String>) -> bool {
        if self.active.as_ref().map(|(active, _)| *active) != Some(id) {
            return false;
        }
        let cancelled = self.active.as_ref().unwrap().1.load(Ordering::Acquire);
        self.active = None;
        match result {
            Ok(message) => {
                self.pending.pop_front();
                self.message = message;
                // A completed upload can race with the cancel button. Do not
                // let its late acknowledgement restart the remaining queue.
                self.failed = cancelled && !self.pending.is_empty();
                self.recovery = self
                    .failed
                    .then(|| {
                        self.pending
                            .front()
                            .and_then(|batch| batch.directory.clone())
                    })
                    .flatten();
            }
            Err(error) => {
                self.failed = true;
                self.recovery = self
                    .pending
                    .front()
                    .and_then(|batch| batch.directory.clone());
                self.message = tf!("cloud.upload.failed_partial", error = error);
            }
        }
        true
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn recover_from(&mut self, root: PathBuf) {
        if std::fs::read_dir(&root).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                std::fs::read_dir(entry.path()).is_ok_and(|mut files| files.next().is_some())
            })
        }) {
            self.recovery = Some(root);
        }
    }
}

impl Workspace {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn tethered_cloud_target(&self) -> Option<Target> {
        self.tethered_save
            .target
            .clone()
            .filter(|target| target.epoch == self.cloud.epoch)
    }
    pub(super) fn tethered_cloud_queue(&mut self, pending: Pending, cx: &mut Context<Self>) {
        if pending.target.epoch != self.cloud.epoch {
            self.tethered_save.recovery = pending.directory;
            self.tethered_save.message = t("cloud.upload.cancelled").into();
            cx.notify();
            return;
        }
        self.tethered_save.pending.push_back(pending);
        self.tethered_cloud_start(cx);
    }
    fn tethered_cloud_start(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.tethered_save;
        state.invalidate(self.cloud.epoch);
        if state.active.is_some() || state.failed {
            return;
        }
        let Some(batch) = state.pending.front().cloned() else {
            return;
        };
        let Some(client) = &self.cloud.client else {
            state.failed = true;
            state.recovery = batch.directory;
            state.message = t("cloud.error.sign_in_first").into();
            cx.notify();
            return;
        };
        state.next_id += 1;
        let id = state.next_id;
        let cancel = Arc::new(AtomicBool::new(false));
        state.active = Some((id, cancel.clone()));
        state.message = t("cloud.upload.uploading_files").into();
        let account_cancel = self.cloud.cancel.clone();
        let handle = client.handle.clone();
        let jobs = self.cloud.sender.clone();
        let epoch = batch.target.epoch;
        schist_cloud::runtime::spawn(async move {
            let progress_jobs = jobs.clone();
            let report: Report = Arc::new(move |_, _, label| {
                let _ = progress_jobs.send(cloud::Job::Tethered {
                    epoch,
                    id,
                    event: Event::Progress(label),
                });
            });
            let (bucket, folder) = match &batch.target.scope {
                Scope::Bucket { id } => (Some(id.clone()), None),
                Scope::Folder { id, .. } => (None, Some(id.clone())),
                Scope::Library => (None, None),
            };
            let result = cancellable(
                Some(&account_cancel),
                cancellable(Some(&cancel), async {
                    let files = batch
                        .paths
                        .iter()
                        .cloned()
                        .map(|path| (path, None))
                        .collect();
                    let uploader =
                        upload_files(&handle, folder, files, report, Some(cancel.clone()), None)
                            .await?;
                    if let Some(bucket) = bucket {
                        uploader.add_to_bucket(&bucket).await?;
                    }
                    let summary = UploadSummary {
                        uploaded: uploader.uploaded.len(),
                        existing: uploader.existing.len(),
                        skipped: uploader.skipped,
                    };
                    anyhow::ensure!(
                        !cancel.load(Ordering::Acquire) && !account_cancel.load(Ordering::Acquire),
                        t("cloud.upload.cancelled")
                    );
                    acknowledge(&batch, summary)
                }),
            )
            .await
            .map_err(|error: anyhow::Error| error.to_string());
            let _ = jobs.send(cloud::Job::Tethered {
                epoch,
                id,
                event: Event::Finished(result),
            });
        });
        cx.notify();
    }
    pub(super) fn tethered_cloud_event(&mut self, id: u64, event: Event, cx: &mut Context<Self>) {
        if self
            .tethered_save
            .active
            .as_ref()
            .map(|(active, _)| *active)
            != Some(id)
        {
            return;
        }
        match event {
            Event::Progress(message) => self.tethered_save.message = message,
            Event::Finished(result) => {
                self.tethered_save.finish(id, result);
                self.tethered_cloud_start(cx);
            }
        }
        self.status = self.tethered_save.message.clone().into();
        cx.notify();
    }
    pub(super) fn tethered_cloud_tick(&mut self) {
        self.tethered_save.invalidate(self.cloud.epoch);
    }
}

fn acknowledge(batch: &Pending, summary: UploadSummary) -> anyhow::Result<String> {
    // Skipped originals and failed bucket additions must remain available for
    // retry. This runs only after the uploader and bucket operation both finish.
    anyhow::ensure!(summary.skipped.is_empty(), summary.message());
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(directory) = &batch.directory {
        for path in &batch.paths {
            // Delete only this batch's private copies, never unrelated files.
            if let Err(error) = std::fs::remove_file(path) {
                log::warn!(
                    "Could not remove acknowledged capture {}: {error}",
                    path.display()
                );
            }
        }
        let _ = std::fs::remove_dir(directory);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = batch;
    Ok(summary.message())
}

pub(super) fn render(ws: &Workspace, busy: bool, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let state = &ws.tethered_save;
    let cloud = state.target.is_some();
    let mut controls = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            Button::new("tethered-save-local", t("tethered.save_local"))
                .active(!cloud)
                .disabled(busy)
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.tethered_save.target = None;
                    if ws.open_popup == Some(DESTINATION_POPUP) {
                        ws.close_popup(cx);
                    }
                    cx.notify();
                })),
        )
        .children(crate::feature_enabled("schist-cloud").then(|| {
            Button::new("tethered-save-cloud", t("tethered.save_cloud"))
                .active(cloud)
                .disabled(busy)
                .on_click(cx.listener(|ws, _, _, cx| {
                    if ws.tethered_save.target.is_some() {
                        return;
                    }
                    if ws.cloud.client.is_none() {
                        ws.cloud_sign_in(cx);
                        return;
                    }
                    let scope = match &ws.cloud.query.scope {
                        Scope::Folder { id, .. } => Scope::Folder {
                            id: id.clone(),
                            recursive: false,
                        },
                        scope => scope.clone(),
                    };
                    let label = destination_label(ws, &scope);
                    ws.tethered_save.target = Some(Target {
                        epoch: ws.cloud.epoch,
                        scope,
                        label,
                    });
                    cx.notify();
                }))
        }));
    if let Some(target) = &state.target {
        controls = controls.child(destination_picker(ws, target, busy, cx));
    }
    if state.failed {
        controls = controls.child(
            Button::new("tethered-upload-retry", t("common.retry"))
                .disabled(state.active.is_some())
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.tethered_save.failed = false;
                    ws.tethered_cloud_start(cx);
                })),
        );
    }
    if !state.pending.is_empty() && !state.failed {
        controls = controls.child(
            Button::new("tethered-upload-cancel", t("common.cancel")).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.tethered_save.stop();
                    cx.notify();
                },
            )),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(controls)
        .child(state.message.clone())
        .children(
            state
                .recovery
                .as_ref()
                .map(|path| tf!("tethered.cloud_pending", path = path.display())),
        )
        .into_any_element()
}

fn destination_label(ws: &Workspace, scope: &Scope) -> String {
    match scope {
        Scope::Folder { id, .. } => ws
            .cloud
            .folders
            .iter()
            .find(|f| &f.id == id)
            .map(|f| f.name.clone()),
        Scope::Bucket { id } => ws
            .cloud
            .buckets
            .iter()
            .find(|b| &b.id == id)
            .map(|b| tf!("cloud.gallery.bucket_title", name = b.name)),
        Scope::Library => None,
    }
    .unwrap_or_else(|| t("cloud.gallery.unfiled").into())
}

fn destination_picker(
    ws: &Workspace,
    target: &Target,
    busy: bool,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    if busy {
        return DropdownButton::new("tethered-destination-disabled", target.label.clone())
            .w(px(280.))
            .opacity(0.5)
            .cursor(gpui::CursorStyle::Arrow)
            .into_any_element();
    }
    let mut options = vec![(t("cloud.gallery.unfiled").into(), Scope::Library)];
    options.extend(ws.cloud.folders.iter().map(|folder| {
        (
            folder.name.clone().into(),
            Scope::Folder {
                id: folder.id.clone(),
                recursive: false,
            },
        )
    }));
    options.extend(ws.cloud.buckets.iter().map(|bucket| {
        (
            tf!("cloud.gallery.bucket_title", name = bucket.name).into(),
            Scope::Bucket {
                id: bucket.id.clone(),
            },
        )
    }));
    crate::ui::dropdown(
        &ws.dropdown,
        crate::ui::Dropdown {
            popup: DESTINATION_POPUP,
            is_open: ws.open_popup == Some(DESTINATION_POPUP),
            current: target.scope.clone(),
            label: target.label.clone().into(),
            width: 280.,
            options,
        },
        |ws, scope, cx| {
            let label = destination_label(ws, &scope);
            if let Some(target) = &mut ws.tethered_save.target {
                target.scope = scope;
                target.label = label;
            }
            cx.notify();
        },
        cx,
    )
    .into_any_element()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn capture(root: &std::path::Path) -> Pending {
        let directory = root.join("capture-one");
        std::fs::create_dir(&directory).unwrap();
        let paths: Vec<_> = ["shot.jpg", "shot.raw"]
            .into_iter()
            .map(|name| directory.join(name))
            .collect();
        for path in &paths {
            std::fs::write(path, b"captured original").unwrap();
        }
        Pending {
            target: Target {
                epoch: 7,
                scope: Scope::Folder {
                    id: "studio".into(),
                    recursive: false,
                },
                label: "Studio".into(),
            },
            paths,
            directory: Some(directory),
        }
    }

    fn queued(batch: Pending) -> (State, Arc<AtomicBool>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let state = State {
            target: Some(batch.target.clone()),
            pending: VecDeque::from([batch]),
            active: Some((1, cancel.clone())),
            next_id: 1,
            failed: false,
            message: String::new(),
            recovery: None,
        };
        (state, cancel)
    }

    #[test]
    fn cancelled_upload_keeps_originals_and_waits_before_retry() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        let (mut state, cancel) = queued(batch.clone());
        state.stop();
        assert!(cancel.load(Ordering::Acquire));
        assert!(
            state.active.is_some(),
            "retry must wait for cancellation acknowledgement"
        );
        assert_eq!(state.recovery.as_deref(), Some(root.path()));
        assert!(state.finish(1, Err("cancelled".into())));
        assert!(state.active.is_none());
        assert!(state.failed);
        assert_eq!(state.pending.len(), 1);
        assert!(batch.paths.iter().all(|path| path.exists()));
        state.active = Some((2, Arc::new(AtomicBool::new(false))));
        assert!(!state.finish(1, Ok("stale completion".into())));
        assert_eq!(state.pending.len(), 1);
        assert!(state.finish(2, Ok("uploaded".into())));
        assert!(state.pending.is_empty());
    }

    #[test]
    fn changing_accounts_cancels_without_retargeting_or_deleting() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        let (mut state, cancel) = queued(batch.clone());
        state.invalidate(8);
        assert!(cancel.load(Ordering::Acquire));
        assert!(state.target.is_none());
        assert!(state.active.is_none());
        assert!(state.pending.is_empty());
        assert!(!state.finish(1, Ok("old account".into())));
        assert!(batch.paths.iter().all(|path| path.exists()));
        assert_eq!(state.recovery.as_deref(), Some(root.path()));
        let mut reopened = State::default();
        reopened.recover_from(root.path().to_path_buf());
        assert_eq!(reopened.recovery, state.recovery);
    }

    #[test]
    fn late_acknowledgement_does_not_restart_cancelled_queue() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        let (mut state, _) = queued(batch.clone());
        state.pending.push_back(batch);
        state.stop();
        assert!(state.finish(1, Ok("already uploaded".into())));
        assert_eq!(state.pending.len(), 1);
        assert!(
            state.failed,
            "remaining uploads still need an explicit retry"
        );
        assert!(state.active.is_none());
    }

    #[test]
    fn skipped_upload_preserves_the_complete_capture_pair() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        assert!(acknowledge(
            &batch,
            UploadSummary {
                uploaded: 1,
                existing: 0,
                skipped: vec!["RAW unavailable".into()]
            }
        )
        .is_err());
        assert!(batch.paths.iter().all(|path| path.exists()));
    }

    #[test]
    fn acknowledged_upload_cleans_only_its_own_private_files() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        let unrelated = batch
            .directory
            .as_ref()
            .unwrap()
            .join("another-capture.jpg");
        std::fs::write(&unrelated, b"keep").unwrap();
        acknowledge(
            &batch,
            UploadSummary {
                uploaded: 1,
                existing: 1,
                skipped: vec![],
            },
        )
        .unwrap();
        assert!(batch.paths.iter().all(|path| !path.exists()));
        assert!(unrelated.exists());
    }

    #[test]
    fn acknowledged_pair_removes_empty_staging_directory() {
        let root = tempfile::tempdir().unwrap();
        let batch = capture(root.path());
        acknowledge(
            &batch,
            UploadSummary {
                uploaded: 2,
                existing: 0,
                skipped: vec![],
            },
        )
        .unwrap();
        assert!(!batch.directory.unwrap().exists());
    }

    #[test]
    fn non_private_sources_are_never_removed() {
        let root = tempfile::tempdir().unwrap();
        let mut batch = capture(root.path());
        batch.directory = None;
        acknowledge(
            &batch,
            UploadSummary {
                uploaded: 2,
                existing: 0,
                skipped: vec![],
            },
        )
        .unwrap();
        assert!(batch.paths.iter().all(|path| path.exists()));
    }
}
