//! Upload completed camera downloads while the producer keeps copying.
use crate::{ProgressRange, UploadSummary};
use anyhow::Result;
use futures::{channel::mpsc, StreamExt};
use schist_i18n::t;
use std::{
    future::Future,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

/// Shared between the camera producer and uploader, including files still
/// queued behind the active batch. Only accepted, fully copied files are ready.
#[derive(Clone, Default)]
pub struct ImportTotals {
    counts: Arc<Mutex<ImportCounts>>,
    total: Arc<AtomicU64>,
}
#[derive(Default)]
struct ImportCounts {
    ready: u64,
    expected: Option<u64>,
}
impl ImportTotals {
    pub fn ready(&self) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.ready += 1;
        self.total.store(
            counts.expected.unwrap_or(0).max(counts.ready),
            Ordering::Relaxed,
        );
    }
    pub fn expect(&self, total: usize) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.expected = Some(total as u64);
        self.total
            .store((total as u64).max(counts.ready), Ordering::Relaxed);
    }
    pub fn finish(&self) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.expected = Some(counts.ready);
        self.total.store(counts.ready, Ordering::Relaxed);
    }
    pub fn range(&self, offset: u64) -> ProgressRange {
        ProgressRange {
            offset,
            total: self.total.clone(),
        }
    }
}

pub enum ImportEvent {
    Ready(PathBuf),
    Finished { failed: usize },
    Failed(String),
}

/// Queue paths, never file contents. Upload only files explicitly marked ready;
/// scanning the staging folder could pick up a download still being written.
/// Flush whatever is ready immediately, with bounded batches when copying wins
/// the race. Completion waits for both the producer and every queued upload.
/// Each upload receives the number already handled across earlier batches.
pub async fn upload_incoming<F, Fut>(
    receiver: mpsc::UnboundedReceiver<ImportEvent>,
    mut upload: F,
) -> Result<(UploadSummary, usize)>
where
    F: FnMut(Vec<PathBuf>, u64) -> Fut,
    Fut: Future<Output = Result<UploadSummary>>,
{
    let mut batches = receiver.ready_chunks(16);
    let mut summary = UploadSummary {
        uploaded: 0,
        existing: 0,
        skipped: Vec::new(),
    };
    while let Some(events) = batches.next().await {
        let mut files = Vec::new();
        let mut finished = None;
        for event in events {
            match event {
                ImportEvent::Ready(path) => files.push(path),
                ImportEvent::Finished { failed } => {
                    finished = Some(Ok(failed));
                    break;
                }
                ImportEvent::Failed(error) => {
                    finished = Some(Err(anyhow::anyhow!(error)));
                    break;
                }
            }
        }
        if !files.is_empty() {
            let offset = (summary.uploaded + summary.existing + summary.skipped.len()) as u64;
            let batch = upload(files, offset).await?;
            summary.uploaded += batch.uploaded;
            summary.existing += batch.existing;
            summary.skipped.extend(batch.skipped);
        }
        if let Some(result) = finished {
            return Ok((summary, result?));
        }
    }
    anyhow::bail!(t("cloud.upload.cancelled"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{executor::block_on, FutureExt};

    fn summary(uploaded: usize) -> UploadSummary {
        UploadSummary {
            uploaded,
            existing: 0,
            skipped: Vec::new(),
        }
    }

    #[test]
    fn uploads_before_downloads_finish_and_waits_for_the_last_upload() {
        let (tx, rx) = mpsc::unbounded();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = futures::channel::oneshot::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut release_rx = Some(release_rx);
            let result = block_on(upload_incoming(rx, |files, _offset| {
                let release = release_rx.take();
                started_tx.send(files.clone()).unwrap();
                async move {
                    if let Some(release) = release {
                        release.await.unwrap();
                    }
                    Ok(summary(files.len()))
                }
            }));
            done_tx.send(result).unwrap();
        });
        tx.unbounded_send(ImportEvent::Ready("first.jpg".into()))
            .unwrap();
        assert_eq!(
            started_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            vec![PathBuf::from("first.jpg")]
        );
        // The camera can finish another download while the first upload waits.
        tx.unbounded_send(ImportEvent::Ready("second.jpg".into()))
            .unwrap();
        tx.unbounded_send(ImportEvent::Finished { failed: 2 })
            .unwrap();
        assert!(done_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        let (result, failed) = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!((result.uploaded, failed), (2, 2));
        worker.join().unwrap();
    }

    #[test]
    fn idle_receiver_yields_and_a_dropped_producer_is_not_success() {
        let (tx, rx) = mpsc::unbounded();
        let mut upload = Box::pin(upload_incoming(rx, |_, _| async {
            panic!("no files ready")
        }));
        assert!(upload.as_mut().now_or_never().is_none());
        drop(tx);
        assert_eq!(
            block_on(upload).err().unwrap().to_string(),
            t("cloud.upload.cancelled")
        );
    }

    #[test]
    fn account_cancellation_interrupts_a_wait_for_the_next_download() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let (tx, rx) = mpsc::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        schist_cloud::runtime::spawn(async move {
            let result = crate::cancellable(
                Some(&worker_cancel),
                upload_incoming(rx, |files, _offset| {
                    started_tx.send(()).unwrap();
                    async move { Ok(summary(files.len())) }
                }),
            )
            .await;
            done_tx.send(result).unwrap();
        });
        tx.unbounded_send(ImportEvent::Ready("first.jpg".into()))
            .unwrap();
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // Keep the producer alive, with no completion event or next photo.
        cancel.store(true, Ordering::Relaxed);
        let error = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(error.to_string(), t("cloud.upload.cancelled"));
        assert!(tx
            .unbounded_send(ImportEvent::Ready("later.jpg".into()))
            .is_err());
    }

    #[test]
    fn batches_are_bounded_and_results_include_duplicates_and_skips() {
        let (tx, rx) = mpsc::unbounded();
        for n in 0..40 {
            tx.unbounded_send(ImportEvent::Ready(format!("{n}.jpg").into()))
                .unwrap();
        }
        tx.unbounded_send(ImportEvent::Finished { failed: 0 })
            .unwrap();
        let totals = ImportTotals::default();
        totals.expect(40);
        for _ in 0..40 {
            totals.ready();
        }
        totals.finish();
        let mut sizes = Vec::new();
        let mut progress = Vec::new();
        let (result, _) = block_on(upload_incoming(rx, |files, offset| {
            sizes.push(files.len());
            let range = totals.range(offset);
            progress.push(ProgressRange::position(Some(&range), 0, files.len() as u64));
            progress.push(ProgressRange::position(
                Some(&range),
                files.len() as u64,
                files.len() as u64,
            ));
            async move {
                Ok(UploadSummary {
                    uploaded: files.len() - 2,
                    existing: 1,
                    skipped: vec!["unsupported".into()],
                })
            }
        }))
        .unwrap();
        assert_eq!(sizes, vec![16, 16, 8]);
        assert_eq!(
            progress,
            vec![(0, 40), (16, 40), (16, 40), (32, 40), (32, 40), (40, 40)]
        );
        assert_eq!(
            (result.uploaded, result.existing, result.skipped.len()),
            (34, 3, 3)
        );
    }

    #[test]
    fn camera_totals_update_an_active_batch_and_exclude_rejected_downloads() {
        let totals = ImportTotals::default();
        let producer = totals.clone();
        producer.expect(50);
        for _ in 0..23 {
            producer.ready();
        }
        let range = totals.range(7);
        // Seven photos finished before this 16-photo batch, exactly the tray regression.
        assert_eq!(ProgressRange::position(Some(&range), 0, 16), (7, 50));
        producer.expect(47); // A failed download and two outside the map area.
        assert_eq!(ProgressRange::position(Some(&range), 4, 16), (11, 47));
        for _ in 23..40 {
            producer.ready();
        }
        producer.finish(); // The final accepted count, even while the batch is in flight.
        assert_eq!(ProgressRange::position(Some(&range), 16, 16), (23, 40));
        assert_eq!(
            ProgressRange::position(Some(&totals.range(23)), 0, 16),
            (23, 40)
        );
        assert_eq!(
            ProgressRange::position(Some(&totals.range(39)), 1, 1),
            (40, 40)
        );
        // A normal file drop still reports its own selection.
        assert_eq!(ProgressRange::position(None, 2, 5), (2, 5));
    }

    #[test]
    fn unknown_camera_catalog_counts_ready_files_until_the_producer_finishes() {
        let totals = ImportTotals::default();
        for _ in 0..7 {
            totals.ready();
        }
        let range = totals.range(0);
        assert_eq!(ProgressRange::position(Some(&range), 0, 7), (0, 7));
        for _ in 0..16 {
            totals.ready();
        }
        assert_eq!(ProgressRange::position(Some(&range), 7, 7), (7, 23));
        totals.finish();
        assert_eq!(
            ProgressRange::position(Some(&totals.range(7)), 16, 16),
            (23, 23)
        );
    }

    #[test]
    fn producer_and_upload_failures_never_report_success() {
        let (tx, rx) = mpsc::unbounded();
        tx.unbounded_send(ImportEvent::Ready("saved.jpg".into()))
            .unwrap();
        tx.unbounded_send(ImportEvent::Failed("camera disconnected".into()))
            .unwrap();
        let mut uploaded = 0;
        let error = block_on(upload_incoming(rx, |files, _offset| {
            uploaded += files.len();
            async move { Ok(summary(files.len())) }
        }))
        .err()
        .unwrap();
        assert_eq!(uploaded, 1);
        assert_eq!(error.to_string(), "camera disconnected");

        let (tx, rx) = mpsc::unbounded();
        tx.unbounded_send(ImportEvent::Ready("saved.jpg".into()))
            .unwrap();
        let error = block_on(upload_incoming(rx, |_, _| async {
            anyhow::bail!("upload failed")
        }))
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "upload failed");
        assert!(tx
            .unbounded_send(ImportEvent::Ready("later.jpg".into()))
            .is_err());
    }
}
