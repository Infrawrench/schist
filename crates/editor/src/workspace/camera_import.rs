//! Camera import destinations. Cloud imports stage originals until the upload
//! finishes; they never register the staging directory with the local gallery.
use super::*;
use anyhow::{ensure, Result};
use futures::channel::mpsc;
use schist_cloud::Scope;
use schist_cloud_transfer::{
    cancellable,
    incoming::{upload_incoming, ImportEvent, ImportTotals},
    upload_files_in_range, Report, UploadSummary,
};
use schist_i18n::{t, tf, tn};
use std::path::Path;

/// Captured when Import opens, so browsing elsewhere or signing into another
/// account while the camera downloads cannot redirect the upload.
#[derive(Clone)]
pub(super) struct CloudImportTarget {
    pub epoch: u64,
    pub scope: Scope,
}

impl CloudImportTarget {
    fn check(&self, cloud: &cloud::CloudState) -> Result<()> {
        ensure!(self.epoch == cloud.epoch, t("cloud.upload.cancelled"));
        ensure!(cloud.client.is_some(), t("cloud.error.sign_in_first"));
        Ok(())
    }

    pub fn destination(&self, cloud: &cloud::CloudState) -> Result<ImportDestination> {
        self.check(cloud)?;
        let staging = Arc::new(
            tempfile::Builder::new()
                .prefix("schist-import-")
                .tempdir()?,
        );
        let (sender, receiver) = mpsc::unbounded();
        let totals = ImportTotals::default();
        let destination = ImportDestination::Cloud {
            staging: staging.clone(),
            sender,
            totals: totals.clone(),
        };
        let handle = cloud.client.as_ref().unwrap().handle.clone();
        let epoch = self.epoch;
        let jobs = cloud.sender.clone();
        let cancel = cloud.cancel.clone();
        let (bucket, folder) = match &self.scope {
            Scope::Bucket { id } => (Some(id.clone()), None),
            Scope::Folder { id, .. } => (None, Some(id.clone())),
            Scope::Library => (None, None),
        };
        schist_cloud::runtime::spawn(async move {
            let progress_jobs = jobs.clone();
            let report: Report = Arc::new(move |done, total, label| {
                let _ = progress_jobs.send(cloud::Job::Progress {
                    epoch,
                    done,
                    total,
                    label,
                });
            });
            let result = cancellable(
                Some(&cancel),
                upload_incoming(receiver, |paths, offset| {
                    let range = totals.range(offset);
                    let handle = handle.clone();
                    let folder = folder.clone();
                    let bucket = bucket.clone();
                    let report = report.clone();
                    let cancel = cancel.clone();
                    async move {
                        let files = paths.into_iter().map(|p| (p, None)).collect();
                        let uploader = upload_files_in_range(
                            &handle,
                            folder,
                            files,
                            report,
                            Some(cancel),
                            None,
                            Some(range),
                        )
                        .await?;
                        if let Some(bucket) = bucket {
                            uploader.add_to_bucket(&bucket).await?;
                        }
                        Ok(UploadSummary {
                            uploaded: uploader.uploaded.len(),
                            existing: uploader.existing.len(),
                            skipped: uploader.skipped,
                        })
                    }
                }),
            )
            .await;
            let job = match result {
                Ok((summary, failed)) => {
                    let mut message = summary.message();
                    if failed > 0 {
                        message.push_str(" — ");
                        message.push_str(&tn("library.import.n_failed", failed as u64));
                    }
                    cloud::Job::Done { epoch, message }
                }
                Err(error) => cloud::Job::Error {
                    epoch,
                    error: if error
                        .to_string()
                        .starts_with(t("cloud.error.not_enough_storage"))
                    {
                        error.to_string()
                    } else {
                        tf!("cloud.upload.failed_partial", error = error)
                    },
                },
            };
            let _ = jobs.send(job);
            // The producer also owns this directory until its last copy ends.
            drop(staging);
        });
        Ok(destination)
    }

    pub fn label(&self, cloud: &cloud::CloudState) -> String {
        let name = match &self.scope {
            Scope::Folder { id, .. } => cloud.folders.iter().find(|f| &f.id == id).map(|f| &f.name),
            Scope::Bucket { id } => cloud.buckets.iter().find(|b| &b.id == id).map(|b| &b.name),
            Scope::Library => None,
        };
        match name {
            Some(name) => format!("{} / {name}", t("cloud.upload.prompt")),
            None => t("cloud.upload.prompt").into(),
        }
    }
}

#[derive(Clone)]
pub(super) enum ImportDestination {
    Local(PathBuf),
    Cloud {
        staging: Arc<tempfile::TempDir>,
        sender: mpsc::UnboundedSender<ImportEvent>,
        totals: ImportTotals,
    },
}

impl ImportDestination {
    pub fn path(&self) -> &Path {
        match self {
            Self::Local(path) => path,
            Self::Cloud { staging, .. } => staging.path(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// Call only after the original has been completely copied and accepted.
    pub fn ready(&self, path: PathBuf) -> Result<()> {
        if let Self::Cloud { sender, totals, .. } = self {
            totals.ready();
            sender
                .unbounded_send(ImportEvent::Ready(path))
                .map_err(|_| anyhow::anyhow!(t("cloud.upload.cancelled")))?;
        }
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "ios", test))]
    pub fn expect(&self, total: usize) {
        if let Self::Cloud { totals, .. } = self {
            totals.expect(total);
        }
    }

    pub fn finish(&self, failed: usize) {
        if let Self::Cloud { sender, totals, .. } = self {
            totals.finish();
            let _ = sender.unbounded_send(ImportEvent::Finished { failed });
        }
    }

    pub fn abort(&self, error: String) {
        if let Self::Cloud { sender, .. } = self {
            let _ = sender.unbounded_send(ImportEvent::Failed(error));
        }
    }
}

/// ImageCaptureCore calls back on the UI thread. Inspect EXIF, delete rejected
/// downloads and hand accepted paths to the uploader on a single worker instead.
#[cfg(any(target_os = "macos", test))]
pub(super) mod downloaded {
    use super::*;
    use std::sync::mpsc;

    pub type KeepFilter = Box<dyn Fn(&Path) -> bool + Send>;
    pub enum Outcome {
        Copied,
        Filtered,
        Failed,
    }

    pub struct Processor {
        pub sender: mpsc::Sender<Option<PathBuf>>,
        pub results: mpsc::Receiver<Outcome>,
    }

    impl Processor {
        pub fn new(dest: ImportDestination, keep: Option<KeepFilter>) -> Self {
            let (sender, input) = mpsc::channel::<Option<PathBuf>>();
            let (output, results) = mpsc::channel();
            std::thread::spawn(move || {
                for path in input {
                    let outcome = match path {
                        Some(path) if keep.as_ref().is_some_and(|keep| !keep(&path)) => {
                            let _ = std::fs::remove_file(path);
                            Outcome::Filtered
                        }
                        Some(path) => match dest.ready(path) {
                            Ok(()) => Outcome::Copied,
                            Err(_) => Outcome::Failed,
                        },
                        None => Outcome::Failed,
                    };
                    if output.send(outcome).is_err() {
                        break;
                    }
                }
            });
            Self { sender, results }
        }
    }
}

impl Workspace {
    pub(super) fn import_destination(
        &self,
        local: impl FnOnce() -> Result<PathBuf>,
    ) -> Result<ImportDestination> {
        if let Some(target) = &self.library.import_cloud {
            target.destination(&self.cloud)
        } else {
            Ok(ImportDestination::Local(local()?))
        }
    }

    /// Keep local indexing entirely on the local import path.
    pub(super) fn watch_import_destination(&mut self, dest: &ImportDestination) {
        if dest.is_local() && !self.library.folders.iter().any(|p| p == dest.path()) {
            self.library.folders.push(dest.path().to_path_buf());
            self.library.folders.sort();
            self.library.save();
        }
        self.library.open = true;
    }
}

#[cfg(test)]
mod cloud_lifecycle_tests {
    use super::*;
    use futures::{executor::block_on, FutureExt, StreamExt};

    #[test]
    fn downloaded_photos_are_filtered_off_thread_before_entering_the_upload_queue() {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let accepted = staging.path().join("accepted.jpg");
        let rejected = staging.path().join("rejected.jpg");
        std::fs::write(&accepted, b"complete original").unwrap();
        std::fs::write(&rejected, b"outside boundary").unwrap();
        let (sender, mut receiver) = mpsc::unbounded();
        let totals = ImportTotals::default();
        let dest = ImportDestination::Cloud {
            staging,
            sender,
            totals: totals.clone(),
        };
        dest.expect(3);
        assert_eq!(
            totals
                .range(0)
                .total
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let ui_thread = std::thread::current().id();
        let processor = downloaded::Processor::new(
            dest.clone(),
            Some(Box::new(move |path| {
                assert_ne!(std::thread::current().id(), ui_thread);
                if path.file_name().unwrap() == "accepted.jpg" {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    true
                } else {
                    false
                }
            })),
        );
        processor.sender.send(Some(accepted.clone())).unwrap();
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(receiver.next().now_or_never().is_none());
        // Callback/polling can continue while EXIF work is blocked.
        processor.sender.send(Some(rejected.clone())).unwrap();
        processor.sender.send(None).unwrap();
        assert!(processor.results.try_recv().is_err());
        release_tx.send(()).unwrap();
        let timeout = std::time::Duration::from_secs(5);
        assert!(matches!(
            processor.results.recv_timeout(timeout).unwrap(),
            downloaded::Outcome::Copied
        ));
        assert!(matches!(
            processor.results.recv_timeout(timeout).unwrap(),
            downloaded::Outcome::Filtered
        ));
        assert!(matches!(
            processor.results.recv_timeout(timeout).unwrap(),
            downloaded::Outcome::Failed
        ));
        match block_on(receiver.next()).unwrap() {
            ImportEvent::Ready(path) => {
                assert_eq!(path, accepted);
                assert_eq!(std::fs::read(path).unwrap(), b"complete original");
            }
            _ => panic!("expected the accepted photo"),
        }
        assert!(!rejected.exists());
        assert!(receiver.next().now_or_never().is_none());
        dest.finish(1);
        assert_eq!(
            totals
                .range(0)
                .total
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert!(matches!(
            block_on(receiver.next()),
            Some(ImportEvent::Finished { failed: 1 })
        ));
    }

    #[test]
    fn staging_survives_until_both_downloading_and_uploading_release_it() {
        let original = tempfile::tempdir().unwrap();
        let photo = original.path().join("photo.jpg");
        std::fs::write(&photo, b"camera original").unwrap();
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let path = staging.path().to_path_buf();
        std::fs::copy(&photo, path.join("photo.jpg")).unwrap();
        let (sender, _receiver) = mpsc::unbounded();
        let producer = ImportDestination::Cloud {
            staging: staging.clone(),
            sender,
            totals: ImportTotals::default(),
        };
        // This is the separate guard captured by the async upload task.
        let upload = staging.clone();
        drop(staging);
        drop(producer);
        assert!(path.exists());
        assert_eq!(
            std::fs::read(path.join("photo.jpg")).unwrap(),
            b"camera original"
        );
        drop(upload);
        assert!(!path.exists());
        assert_eq!(std::fs::read(photo).unwrap(), b"camera original");
    }

    #[test]
    fn camera_import_rejects_an_account_change_before_staging_or_upload() {
        let cloud = cloud::CloudState::default();
        let target = CloudImportTarget {
            epoch: cloud.epoch,
            scope: Scope::Library,
        };
        assert_eq!(
            target.check(&cloud).unwrap_err().to_string(),
            t("cloud.error.sign_in_first")
        );
        let mut changed_account = cloud;
        changed_account.epoch += 1;
        assert_eq!(
            target.check(&changed_account).unwrap_err().to_string(),
            t("cloud.upload.cancelled")
        );
        assert!(target.destination(&changed_account).is_err());
    }
}
