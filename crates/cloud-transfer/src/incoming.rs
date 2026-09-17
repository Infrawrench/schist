//! Upload completed camera downloads while the producer keeps copying.
use crate::UploadSummary;
use anyhow::Result;
use futures::{channel::mpsc, StreamExt};
use schist_i18n::t;
use std::{future::Future, path::PathBuf};

pub enum ImportEvent {
    Ready(PathBuf),
    Finished { failed: usize },
    Failed(String),
}

/// Queue paths, never file contents. Upload only files explicitly marked ready;
/// scanning the staging folder could pick up a download still being written.
/// Flush whatever is ready immediately, with bounded batches when copying wins
/// the race. Completion waits for both the producer and every queued upload.
pub async fn upload_incoming<F, Fut>(
    receiver: mpsc::UnboundedReceiver<ImportEvent>,
    mut upload: F,
) -> Result<(UploadSummary, usize)>
where
    F: FnMut(Vec<PathBuf>) -> Fut,
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
            let batch = upload(files).await?;
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
            let result = block_on(upload_incoming(rx, |files| {
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
        let mut upload = Box::pin(upload_incoming(rx, |_| async { panic!("no files ready") }));
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
                upload_incoming(rx, |files| {
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
        let mut sizes = Vec::new();
        let (result, _) = block_on(upload_incoming(rx, |files| {
            sizes.push(files.len());
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
            (result.uploaded, result.existing, result.skipped.len()),
            (34, 3, 3)
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
        let error = block_on(upload_incoming(rx, |files| {
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
        let error = block_on(upload_incoming(rx, |_| async {
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
