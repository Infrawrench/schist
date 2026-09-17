//! Cancellable, resumable cloud upload orchestration and batch preparation.
#[cfg(not(target_arch = "wasm32"))]
pub mod incoming;
use anyhow::{anyhow, Result};
#[cfg(target_arch = "wasm32")]
use schist_app_platform::web;
use schist_cloud::{
    self as remote,
    protocol::{map, parse},
    Value,
};
use schist_i18n::{t, tf, tn};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Where an upload's progress goes: done, total, and what it is doing.
/// The drag-and-drop path turns it into the tray's bar; the camera-roll
/// backup keeps its own tally, and a headless run just logs it.
pub type Report = Arc<dyn Fn(u64, u64, String) + Send + Sync>;
/// Acknowledged sources, including duplicates; checkpointed before the next request.
pub type Handled = Arc<dyn Fn(&[PathBuf]) -> Result<()> + Send + Sync>;
pub async fn cancellable<T>(
    cancel: Option<&Arc<AtomicBool>>,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let Some(cancel) = cancel else {
        return work.await;
    };
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), t("cloud.upload.cancelled"));
    let stopped = async {
        while !cancel.load(Ordering::Relaxed) {
            remote::runtime::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    futures::pin_mut!(work, stopped);
    match futures::future::select(work, stopped).await {
        futures::future::Either::Left((result, _)) => result,
        futures::future::Either::Right(_) => anyhow::bail!(t("cloud.upload.cancelled")),
    }
}
/// The upload pipeline shared by every local-file upload: the storage
/// check, then the files read, hashed, checked against the library and
/// sent up in compressed batches. `files` are (path, relative path
/// inside the drop). `cancel`, when given, is read between items and
/// stops the run with an error. Returns the uploader with its tallies
/// so the caller can go on with what it collected (a bucket to add to,
/// a ledger to write).
pub async fn upload_files(
    handle: &remote::Handle,
    folder: Option<String>,
    files: Vec<(PathBuf, Option<String>)>,
    report: Report,
    cancel: Option<Arc<AtomicBool>>,
    handled: Option<Handled>,
) -> Result<Uploader> {
    let progress = |done: u64, total: u64, label: String| report(done, total, label);
    progress(
        0,
        files.len() as u64,
        t("cloud.upload.checking_storage").into(),
    );
    if cancel.is_none() {
        let mut selection_bytes = 0u64;
        let mut candidates = Vec::new();
        for (path, relative) in &files {
            #[cfg(not(target_arch = "wasm32"))]
            let size = std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            #[cfg(target_arch = "wasm32")]
            let size = crate::web::read_file(path)
                .map(|bytes| bytes.len() as u64)
                .unwrap_or(0);
            if remote::validate_upload_size(size).is_err() {
                continue;
            }
            selection_bytes = selection_bytes
                .checked_add(size)
                .ok_or_else(|| anyhow!(t("cloud.upload.selection_too_large")))?;
            if size > remote::MAX_SINGLE_UPLOAD_BYTES {
                candidates.push((path, relative, size));
            }
        }
        let initial: remote::multipart::UploadCapacity = parse(
            handle
                .request_async(
                    "asset.check_upload",
                    map([("bytes", selection_bytes.into())]),
                )
                .await?,
        )?;
        if !initial.fits {
            let mut resumes = Vec::new();
            for (path, relative, size) in candidates {
                #[cfg(not(target_arch = "wasm32"))]
                let resume_key = remote::multipart::resume_key_for_path(
                    path,
                    mime(path),
                    folder.as_deref(),
                    relative.as_deref(),
                )?;
                #[cfg(target_arch = "wasm32")]
                let resume_key = remote::multipart::resume_key_for_bytes(
                    &crate::web::read_file(path)?,
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                    mime(path),
                    folder.as_deref(),
                    relative.as_deref(),
                );
                resumes.push(map([
                    ("resume_key", resume_key.into()),
                    ("size", size.into()),
                ]));
            }
            let capacity: remote::multipart::UploadCapacity = parse(
                handle
                    .request_async(
                        "asset.check_upload",
                        map([
                            ("bytes", selection_bytes.into()),
                            ("resumes", Value::Array(resumes)),
                        ]),
                    )
                    .await?,
            )?;
            capacity.require_space()?;
        }
    }
    let total = files.len() as u64;
    progress(0, total, tf!("cloud.upload.progress", n = 0, m = total));
    // A pipeline: files are read and packed into compressed
    // batches ahead of the network, a few at a time, while
    // one batch at a time goes up. The provider's batch
    // support is learned from the first reply and shared
    // back to the packer, so raw files stop being kept once
    // payloads are known to be enough.
    let support = Arc::new(std::sync::atomic::AtomicU8::new(SUPPORT_UNKNOWN));
    let mut uploader = Uploader {
        handle: handle.clone(),
        folder,
        support: support.clone(),
        report,
        cancel,
        handled,
        total,
        done: 0,
        uploaded: Vec::new(),
        existing: Vec::new(),
        skipped: Vec::new(),
        skipped_sources: Vec::new(),
        dedupe: SUPPORT_UNKNOWN,
    };
    let preparer = Preparer::new(files, support);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use futures::{SinkExt, StreamExt};
        let (mut tx, mut rx) = futures::channel::mpsc::channel::<Prepared>(PREPARE_AHEAD);
        std::thread::spawn(move || {
            let mut preparer = preparer;
            while let Some(item) = preparer.next() {
                if futures::executor::block_on(tx.send(item)).is_err() {
                    break;
                }
            }
        });
        while let Some(item) =
            cancellable(uploader.cancel.as_ref(), async { Ok(rx.next().await) }).await?
        {
            uploader.take(item).await?;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let mut preparer = preparer;
        while let Some(item) = preparer.next() {
            uploader.take(item).await?;
        }
    }
    Ok(uploader)
}
/// One batch of a drop: at most this many bytes of files, or this many
/// files, per compressed payload — several payloads for a big drop,
/// each small enough to retry on its own.
const BATCH_BYTES: usize = 48 * 1024 * 1024;
const BATCH_FILES: usize = 250;
fn read_cloud_upload(path: &std::path::Path, relative: Option<String>) -> Result<BatchFile> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read as _;
        let metadata = std::fs::metadata(path)?;
        remote::validate_upload_size(metadata.len())?;
        anyhow::ensure!(
            metadata.len() <= remote::MAX_SINGLE_UPLOAD_BYTES,
            t("cloud.upload.over_100mib")
        );
        // Bound the read too, in case the source grows after checking its size.
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(remote::MAX_SINGLE_UPLOAD_BYTES + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = {
        let bytes = crate::web::read_file(path)?;
        remote::validate_upload_size(bytes.len() as u64)?;
        bytes.to_vec()
    };
    remote::validate_upload_size(bytes.len() as u64)?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    Ok(BatchFile {
        source: path.to_path_buf(),
        path: relative.unwrap_or_else(|| name.clone()),
        name,
        mime: mime(path),
        digest: remote::transfer::sha256_hex(&bytes),
        bytes,
    })
}

struct BatchFile {
    /// Where the bytes came from, to read them again for a repack or a
    /// single upload.
    source: PathBuf,
    /// The relative path inside the drop (sub-folders become cloud
    /// folders), or just the name.
    path: String,
    name: String,
    mime: &'static str,
    /// SHA-256, hex: what the provider deduplicates by.
    digest: String,
    bytes: Vec<u8>,
}
/// A batch member without its bytes: enough to ask the provider whether
/// it already has the file, and to read it again if it must go up on
/// its own.
struct BatchEntry {
    size: u64,
    source: PathBuf,
    path: String,
    digest: String,
}
impl From<&BatchFile> for BatchEntry {
    fn from(file: &BatchFile) -> Self {
        Self {
            size: file.bytes.len() as u64,
            source: file.source.clone(),
            path: file.path.clone(),
            digest: file.digest.clone(),
        }
    }
}
/// Read a batch's members again, for a repack after deduplication or
/// the single-file fallback.
fn reread(entries: &[BatchEntry]) -> Result<Vec<BatchFile>> {
    entries
        .iter()
        .map(|e| read_cloud_upload(&e.source, Some(e.path.clone())))
        .collect()
}
/// The batch payload: magic, a MessagePack manifest, then the files
/// back to back, gzip-compressed. Returns the payload and the size of
/// the files inside it.
fn pack_batch(files: &[BatchFile]) -> Result<(Vec<u8>, u64)> {
    use std::io::Write as _;
    let manifest = remote::protocol::encode(&map([(
        "files",
        Value::Array(
            files
                .iter()
                .map(|f| {
                    map([
                        ("path", f.path.clone().into()),
                        ("mime_type", f.mime.into()),
                        ("size", (f.bytes.len() as u64).into()),
                    ])
                })
                .collect(),
        ),
    )]))?;
    let total: usize = files.iter().map(|f| f.bytes.len()).sum();
    let mut raw = Vec::with_capacity(12 + manifest.len() + total);
    raw.extend_from_slice(b"SCHISTB1");
    raw.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
    raw.extend_from_slice(&manifest);
    for file in files {
        raw.extend_from_slice(&file.bytes);
    }
    // Photos hardly compress; the fast level keeps the CPU out of the
    // way of the network without pretending otherwise.
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw)?;
    Ok((encoder.finish()?, total as u64))
}
#[derive(serde::Deserialize)]
struct BatchTicket {
    batch_id: String,
    put_url: String,
}
#[derive(serde::Deserialize)]
struct BatchCommitted {
    assets: Vec<String>,
}
/// How many packed batches wait ahead of the upload: with the one
/// being packed and the one going up, at most four in memory.
#[cfg(not(target_arch = "wasm32"))]
const PREPARE_AHEAD: usize = 2;
/// The provider's batch support, as the uploader learns it and tells
/// the packer: unknown at first, then batches or singles.
const SUPPORT_UNKNOWN: u8 = 0;
const SUPPORT_BATCH: u8 = 1;
const SUPPORT_SINGLES: u8 = 2;
/// What the packer hands the uploader, in drop order.
enum Prepared {
    /// A batch: its members, and the compressed payload unless the
    /// provider is known to take singles only (an oversized single
    /// file also comes without one). The bytes are not kept: a repack
    /// or a single upload reads them from disk again.
    Batch {
        payload: Option<(Vec<u8>, u64)>,
        entries: Vec<BatchEntry>,
    },
    /// Too big for a batch but small enough for one plain upload: it
    /// goes on its own, never packed.
    Single(BatchEntry),
    /// Too big for a batch: the resumable chunked path reads it itself.
    #[cfg(not(target_arch = "wasm32"))]
    Large {
        path: PathBuf,
        relative: Option<String>,
        size: u64,
    },
    /// Left out, with the reason for the summary.
    Skipped { source: PathBuf, reason: String },
    /// Packing failed; the upload stops here.
    Failed(anyhow::Error),
}
/// Reads and packs the drop's files into batches, ahead of the
/// network. Runs on its own thread on desktop, inline in the browser.
struct Preparer {
    files: std::vec::IntoIter<(PathBuf, Option<String>)>,
    batch: Vec<BatchFile>,
    batch_bytes: usize,
    ready: std::collections::VecDeque<Prepared>,
    support: Arc<std::sync::atomic::AtomicU8>,
}
impl Preparer {
    fn new(
        files: Vec<(PathBuf, Option<String>)>,
        support: Arc<std::sync::atomic::AtomicU8>,
    ) -> Self {
        Self {
            files: files.into_iter(),
            batch: Vec::new(),
            batch_bytes: 0,
            ready: std::collections::VecDeque::new(),
            support,
        }
    }
    /// The batch so far as one item, packed unless the provider is
    /// known to take singles only; the raw files stay unless batches
    /// are known to work.
    fn flush(&mut self) -> Option<Prepared> {
        if self.batch.is_empty() {
            return None;
        }
        let files = std::mem::take(&mut self.batch);
        self.batch_bytes = 0;
        let support = self.support.load(std::sync::atomic::Ordering::Relaxed);
        let payload = if support == SUPPORT_SINGLES {
            None
        } else {
            match pack_batch(&files) {
                Ok(payload) => Some(payload),
                Err(e) => return Some(Prepared::Failed(e)),
            }
        };
        let entries = files.iter().map(BatchEntry::from).collect();
        Some(Prepared::Batch { payload, entries })
    }
    fn next(&mut self) -> Option<Prepared> {
        loop {
            if let Some(item) = self.ready.pop_front() {
                return Some(item);
            }
            let Some((path, relative)) = self.files.next() else {
                return self.flush();
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() > remote::MAX_SINGLE_UPLOAD_BYTES
                    && metadata.len() <= remote::MAX_UPLOAD_BYTES
                {
                    // Keep the drop's order: the batch so far goes first.
                    if let Some(batch) = self.flush() {
                        self.ready.push_back(batch);
                    }
                    self.ready.push_back(Prepared::Large {
                        path,
                        relative,
                        size: metadata.len(),
                    });
                    continue;
                }
            }
            let file = match read_cloud_upload(&path, relative) {
                Ok(file) => file,
                Err(error) => {
                    let reason = format!(
                        "{}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.ready.push_back(Prepared::Skipped {
                        source: path,
                        reason,
                    });
                    continue;
                }
            };
            if file.bytes.len() > BATCH_BYTES {
                if let Some(batch) = self.flush() {
                    self.ready.push_back(batch);
                }
                self.ready
                    .push_back(Prepared::Single(BatchEntry::from(&file)));
                continue;
            }
            if self.batch_bytes + file.bytes.len() > BATCH_BYTES || self.batch.len() >= BATCH_FILES
            {
                if let Some(batch) = self.flush() {
                    self.ready.push_back(batch);
                }
            }
            self.batch_bytes += file.bytes.len();
            self.batch.push(file);
        }
    }
}
/// How long a transfer keeps waiting for the connection to come back:
/// this many rounds of at most half a minute each — about four hours.
const OFFLINE_RETRIES: u32 = 480;
/// Sends prepared items up one at a time and keeps the count.
pub struct Uploader {
    handle: remote::Handle,
    folder: Option<String>,
    support: Arc<std::sync::atomic::AtomicU8>,
    report: Report,
    /// Read between items; set, the run stops with an error.
    cancel: Option<Arc<AtomicBool>>,
    handled: Option<Handled>,
    total: u64,
    done: u64,
    pub uploaded: Vec<String>,
    /// Assets the library already had, found by digest: left out of the
    /// upload, still added to a bucket drop.
    pub existing: Vec<String>,
    pub skipped: Vec<String>,
    /// The files behind `skipped`, for a caller that keeps a record of
    /// what went up and must not count these.
    pub skipped_sources: Vec<PathBuf>,
    /// Whether the provider answers `assets.exists`, learned from the
    /// first reply.
    dedupe: u8,
}
impl Uploader {
    /// Include both new assets and deduplicated originals in the destination.
    pub async fn add_to_bucket(&self, bucket: &str) -> Result<()> {
        self.report(t("cloud.upload.adding_to_bucket").into());
        let members: Vec<String> = self
            .uploaded
            .iter()
            .chain(self.existing.iter())
            .cloned()
            .collect();
        for chunk in members.chunks(1000) {
            let mutation = remote::Uuid::new_v4().to_string();
            self.retrying(t("cloud.upload.step.adding_to_bucket"), || {
                let items = chunk
                    .iter()
                    .map(|id| map([("kind", "asset".into()), ("id", id.clone().into())]))
                    .collect();
                self.handle.request_async(
                    "bucket.add",
                    map([
                        ("id", bucket.into()),
                        ("items", Value::Array(items)),
                        ("mutation_id", mutation.clone().into()),
                    ]),
                )
            })
            .await?;
        }
        Ok(())
    }

    fn handled(&self, paths: &[PathBuf]) -> Result<()> {
        if let Some(handled) = &self.handled {
            handled(paths)?;
        }
        Ok(())
    }
    // The backup checks only bytes left after deduplication. Drag uploads
    // retain their whole-selection preflight above.
    async fn check_space(&self, bytes: u64) -> Result<()> {
        if self.cancel.is_some() && bytes > 0 {
            let capacity: remote::multipart::UploadCapacity = parse(
                self.retrying(t("cloud.upload.checking_storage"), || {
                    self.handle
                        .request_async("asset.check_upload", map([("bytes", bytes.into())]))
                })
                .await?,
            )?;
            capacity.require_space()?;
        }
        Ok(())
    }
    pub fn report(&self, label: String) {
        (self.report)(self.done, self.total, label);
    }
    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
    }
    /// Run one network step, and when the connection is what failed,
    /// wait for it to return and run the step again — with the same
    /// mutation IDs, so the provider answers a repeated commit from its
    /// record rather than doing it twice. A refused request (quota, a
    /// bad file, an expired ticket) is an answer and comes straight back.
    pub async fn retrying<T, Fut>(&self, what: &str, step: impl Fn() -> Fut) -> Result<T>
    where
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;
        loop {
            match cancellable(self.cancel.as_ref(), step()).await {
                Ok(value) => return Ok(value),
                Err(error) if remote::transport::transient(&error) && attempt < OFFLINE_RETRIES => {
                    attempt += 1;
                    self.report(tf!(
                        "cloud.upload.interrupted",
                        what = what,
                        n = self.done,
                        m = self.total
                    ));
                    // The first retry is immediate: the session renews
                    // itself every quarter hour by reconnecting, and that
                    // is over in a moment. Only a repeat failure pauses,
                    // for a gateway that just dropped us and is not ready.
                    if attempt > 1 {
                        let pause = (1u64 << attempt.min(5)).min(30);
                        cancellable(self.cancel.as_ref(), async {
                            remote::runtime::sleep(std::time::Duration::from_secs(pause)).await;
                            Ok(())
                        })
                        .await?;
                    }
                    if !cancellable(self.cancel.as_ref(), async {
                        Ok(self.handle.wait_online().await)
                    })
                    .await?
                    {
                        return Err(error.context(t("cloud.upload.connection_closed")));
                    }
                    self.report(tf!(
                        "cloud.upload.resuming",
                        what = what,
                        n = self.done,
                        m = self.total
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fn step(&mut self, by: usize) {
        self.done += by as u64;
        let (done, total) = (self.done, self.total);
        self.report(if self.skipped.is_empty() {
            tf!("cloud.upload.progress", n = done, m = total)
        } else {
            tf!(
                "cloud.upload.progress_skipped",
                n = done,
                m = total,
                skipped = self.skipped.len()
            )
        });
    }
    async fn take(&mut self, item: Prepared) -> Result<()> {
        if self.cancelled() {
            anyhow::bail!(t("cloud.upload.cancelled"));
        }
        match item {
            Prepared::Batch {
                mut payload,
                mut entries,
            } => {
                let count = entries.len();
                // Ask first what the library already holds: those files
                // stay home, and the payload is packed again without
                // them. A provider without the question uploads all.
                if self.dedupe != SUPPORT_SINGLES && !entries.is_empty() {
                    let digests: Vec<String> = entries.iter().map(|e| e.digest.clone()).collect();
                    match self
                        .retrying(t("cloud.upload.step.checking_duplicates"), || {
                            existing_assets(&self.handle, &digests)
                        })
                        .await?
                    {
                        Some(found) => {
                            self.dedupe = SUPPORT_BATCH;
                            self.handled(
                                &entries
                                    .iter()
                                    .filter(|e| found.contains_key(&e.digest))
                                    .map(|e| e.source.clone())
                                    .collect::<Vec<_>>(),
                            )?;
                            let before = entries.len();
                            let existing = &mut self.existing;
                            entries.retain(|e| match found.get(&e.digest) {
                                Some(id) => {
                                    existing.push(id.clone());
                                    false
                                }
                                None => true,
                            });
                            if entries.len() != before {
                                payload = None;
                            }
                        }
                        None => self.dedupe = SUPPORT_SINGLES,
                    }
                }
                if entries.is_empty() {
                    self.step(count);
                    return Ok(());
                }
                let folder = self.folder.clone();
                self.check_space(entries.iter().map(|e| e.size).sum())
                    .await?;
                let mut ids = None;
                if self.support.load(std::sync::atomic::Ordering::Relaxed) != SUPPORT_SINGLES {
                    let (payload, total) = match payload.take() {
                        Some(packed) => packed,
                        None => pack_batch(&reread(&entries)?)?,
                    };
                    {
                        // One set of IDs for the batch: every retry
                        // re-sends the same mutations, so the provider
                        // can answer a repeat from its record.
                        let batch = BatchIds::new();
                        match self
                            .retrying(t("cloud.upload.step.uploading_batch"), || {
                                // The count the provider checks is the
                                // batch as packed — after duplicates
                                // were left out — not the drop's tally.
                                send_batch(
                                    &self.handle,
                                    folder.as_deref(),
                                    &payload,
                                    total,
                                    entries.len(),
                                    &batch,
                                )
                            })
                            .await?
                        {
                            Some(found) => {
                                self.support
                                    .store(SUPPORT_BATCH, std::sync::atomic::Ordering::Relaxed);
                                ids = Some(found);
                            }
                            None => self
                                .support
                                .store(SUPPORT_SINGLES, std::sync::atomic::Ordering::Relaxed),
                        }
                    }
                }
                match ids {
                    Some(ids) => {
                        anyhow::ensure!(
                            ids.len() == entries.len(),
                            t("cloud.upload.incomplete_batch")
                        );
                        self.handled(
                            &entries.iter().map(|e| e.source.clone()).collect::<Vec<_>>(),
                        )?;
                        self.uploaded.extend(ids);
                    }
                    None => {
                        let files = reread(&entries)?;
                        for file in &files {
                            let mutation = remote::Uuid::new_v4().to_string();
                            let id = self
                                .retrying(t("cloud.upload.step.uploading_photo"), || {
                                    send_single(&self.handle, folder.as_deref(), file, &mutation)
                                })
                                .await?;
                            self.uploaded.push(id);
                            self.handled(std::slice::from_ref(&file.source))?;
                        }
                    }
                }
                self.step(count);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Prepared::Large {
                path,
                relative,
                size,
            } => {
                if self.cancel.is_some() && self.dedupe != SUPPORT_SINGLES {
                    let digest = upload_path_digest(&path, self.cancel.as_ref())?;
                    match self
                        .retrying(t("cloud.upload.step.checking_duplicates"), || {
                            existing_assets(&self.handle, std::slice::from_ref(&digest))
                        })
                        .await?
                    {
                        Some(found) => {
                            self.dedupe = SUPPORT_BATCH;
                            if let Some(id) = found.get(&digest) {
                                self.handled(std::slice::from_ref(&path))?;
                                self.existing.push(id.clone());
                                self.step(1);
                                return Ok(());
                            }
                        }
                        None => self.dedupe = SUPPORT_SINGLES,
                    }
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.report(tf!("cloud.upload.checking_resumable", name = name));
                let (report, done, total) = (self.report.clone(), self.done, self.total);
                // The chunked upload resumes from the parts already
                // stored, so a retry after an outage picks up where it
                // stopped.
                let asset = self
                    .retrying(t("cloud.upload.step.uploading_large_file"), || {
                        let (report, name) = (report.clone(), name.clone());
                        self.handle.upload_path_async(
                            &path,
                            mime(&path),
                            self.folder.as_deref(),
                            relative.as_deref(),
                            move |bytes| {
                                report(
                                    done,
                                    total,
                                    tf!(
                                        "cloud.upload.large_progress",
                                        name = name,
                                        percent = bytes * 100 / size.max(1),
                                        done = bytes / 1024 / 1024,
                                        total = size / 1024 / 1024
                                    ),
                                );
                            },
                        )
                    })
                    .await?;
                self.uploaded.push(asset.id);
                self.handled(std::slice::from_ref(&path))?;
                self.step(1);
            }
            Prepared::Single(entry) => {
                // Still worth asking whether the library has it.
                if self.dedupe != SUPPORT_SINGLES {
                    let digests = vec![entry.digest.clone()];
                    match self
                        .retrying(t("cloud.upload.step.checking_duplicates"), || {
                            existing_assets(&self.handle, &digests)
                        })
                        .await?
                    {
                        Some(found) => {
                            self.dedupe = SUPPORT_BATCH;
                            if let Some(id) = found.get(&entry.digest) {
                                self.handled(std::slice::from_ref(&entry.source))?;
                                self.existing.push(id.clone());
                                self.step(1);
                                return Ok(());
                            }
                        }
                        None => self.dedupe = SUPPORT_SINGLES,
                    }
                }
                let folder = self.folder.clone();
                let file = read_cloud_upload(&entry.source, Some(entry.path.clone()))?;
                self.check_space(file.bytes.len() as u64).await?;
                let mutation = remote::Uuid::new_v4().to_string();
                let id = self
                    .retrying(t("cloud.upload.step.uploading_photo"), || {
                        send_single(&self.handle, folder.as_deref(), &file, &mutation)
                    })
                    .await?;
                self.uploaded.push(id);
                self.handled(std::slice::from_ref(&entry.source))?;
                self.step(1);
            }
            Prepared::Skipped { source, reason } => {
                self.skipped.push(reason);
                self.skipped_sources.push(source);
                self.step(1);
            }
            Prepared::Failed(error) => return Err(error),
        }
        Ok(())
    }
}
#[derive(serde::Deserialize)]
struct ExistingAsset {
    sha256: String,
    id: String,
}
#[derive(serde::Deserialize)]
struct ExistingAssets {
    found: Vec<ExistingAsset>,
}

pub struct UploadSummary {
    pub uploaded: usize,
    /// Left out because the library already held them.
    pub existing: usize,
    pub skipped: Vec<String>,
}
impl UploadSummary {
    pub fn message(&self) -> String {
        // The clauses are each a whole sentence; "; " joins them.
        let mut uploaded = match self.uploaded {
            0 => t("cloud.upload.summary_none").to_string(),
            n => tn("cloud.upload.summary_uploaded", n as u64),
        };
        if self.existing > 0 {
            uploaded.push_str("; ");
            uploaded.push_str(&tn("cloud.upload.summary_existing", self.existing as u64));
        }
        match self.skipped.first() {
            None => uploaded,
            Some(reason) => {
                let reason = if self.skipped.len() > 1 {
                    tf!(
                        "cloud.upload.summary_skipped_more",
                        reason = reason,
                        n = self.skipped.len() - 1
                    )
                } else {
                    reason.clone()
                };
                let skipped = tn!(
                    "cloud.upload.summary_skipped",
                    self.skipped.len() as u64,
                    reason = reason
                );
                format!("{uploaded}; {skipped}")
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn upload_path_digest(path: &std::path::Path, cancel: Option<&Arc<AtomicBool>>) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut read = 0u64;
    loop {
        anyhow::ensure!(
            !cancel.is_some_and(|c| c.load(Ordering::Relaxed)),
            t("cloud.upload.cancelled")
        );
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        anyhow::ensure!(
            read <= metadata.len(),
            t("cloud.transport.source_grew_during_preparation")
        );
        hash.update(&buffer[..count]);
    }
    anyhow::ensure!(
        read == metadata.len() && file.metadata()?.modified()? == metadata.modified()?,
        t("cloud.transport.source_changed_during_preparation")
    );
    Ok(format!("{:x}", hash.finalize()))
}
/// Which of these digests the library already holds, as digest → asset
/// ID. `None` when the provider cannot say, so everything uploads.
async fn existing_assets(
    handle: &remote::Handle,
    digests: &[String],
) -> Result<Option<HashMap<String, String>>> {
    let params = map([(
        "sha256",
        Value::Array(digests.iter().map(|d| d.clone().into()).collect()),
    )]);
    match handle.request_async("assets.exists", params).await {
        Ok(reply) => {
            let found: ExistingAssets = parse(reply)?;
            Ok(Some(
                found.found.into_iter().map(|f| (f.sha256, f.id)).collect(),
            ))
        }
        Err(e) if e.to_string().contains("method_not_found") => Ok(None),
        Err(e) => Err(e),
    }
}
/// The mutation IDs one batch uses, fixed for its lifetime so retries
/// repeat rather than duplicate.
struct BatchIds {
    prepare: String,
    commit: String,
}
impl BatchIds {
    fn new() -> Self {
        Self {
            prepare: remote::Uuid::new_v4().to_string(),
            commit: remote::Uuid::new_v4().to_string(),
        }
    }
}
/// Upload one packed batch: prepare, put the payload, commit. `None`
/// when the provider has no batch method, so the caller falls back to
/// singles. Repeating the call after an outage repeats the same
/// mutations: a prepare already answered returns its ticket, a commit
/// already done returns its assets.
async fn send_batch(
    handle: &remote::Handle,
    folder: Option<&str>,
    payload: &[u8],
    total: u64,
    count: usize,
    ids: &BatchIds,
) -> Result<Option<Vec<String>>> {
    let mut fields = vec![
        ("size", (payload.len() as u64).into()),
        ("total", total.into()),
        ("count", (count as u64).into()),
        ("mutation_id", ids.prepare.clone().into()),
    ];
    if let Some(folder) = folder {
        fields.push(("folder_id", folder.into()));
    }
    let ticket: BatchTicket = match handle
        .request_async("asset.prepare_batch", map(fields))
        .await
    {
        Ok(reply) => parse(reply)?,
        Err(e) if e.to_string().contains("method_not_found") => return Ok(None),
        Err(e) => return Err(e),
    };
    remote::auth::upload_async(&ticket.put_url, "application/gzip", payload).await?;
    let committed: BatchCommitted = parse(
        handle
            .request_async(
                "asset.commit_batch",
                map([
                    ("batch_id", ticket.batch_id.into()),
                    ("mutation_id", ids.commit.clone().into()),
                ]),
            )
            .await?,
    )?;
    Ok(Some(committed.assets))
}
/// The one-file path: a provider without batches, or a file too big for
/// one.
async fn send_single(
    handle: &remote::Handle,
    folder: Option<&str>,
    file: &BatchFile,
    mutation: &str,
) -> Result<String> {
    let relative = (file.path != file.name).then_some(file.path.as_str());
    let asset = handle
        .upload_async(remote::Upload {
            name: &file.name,
            bytes: &file.bytes,
            mime: file.mime,
            folder,
            asset: None,
            relative,
            mutation,
        })
        .await?;
    Ok(asset.id)
}
pub fn mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "psd" | "psb" => "image/vnd.adobe.photoshop",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn oversized_and_empty_files_do_not_discard_the_valid_batch() {
        use std::io::Read as _;
        let root = std::env::temp_dir().join(format!("schist-upload-{}", remote::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("before.jpg"), b"before").unwrap();
        std::fs::File::create(root.join("large.mov"))
            .unwrap()
            .set_len(remote::MAX_UPLOAD_BYTES + 1)
            .unwrap();
        std::fs::write(root.join("empty.jpg"), b"").unwrap();
        std::fs::write(root.join("after.jpg"), b"after").unwrap();
        let mut batch = Vec::new();
        let mut skipped = Vec::new();
        for name in ["before.jpg", "large.mov", "empty.jpg", "after.jpg"] {
            match read_cloud_upload(&root.join(name), Some(format!("trip/{name}"))) {
                Ok(file) => batch.push(file),
                Err(error) => skipped.push(format!("{name}: {error}")),
            }
        }
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].path, "trip/before.jpg");
        assert_eq!(batch[1].path, "trip/after.jpg");
        let (payload, total) = pack_batch(&batch).unwrap();
        assert_eq!(total, 11);
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(payload.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let length = u32::from_be_bytes(raw[8..12].try_into().unwrap()) as usize;
        assert_eq!(&raw[12 + length..], b"beforeafter");
        let message = UploadSummary {
            uploaded: batch.len(),
            existing: 0,
            skipped: skipped.clone(),
        }
        .message();
        assert!(message.contains("Uploaded 2 photos; skipped 2 files"));
        let deduped = UploadSummary {
            uploaded: batch.len(),
            existing: 3,
            skipped,
        }
        .message();
        assert!(deduped.contains("Uploaded 2 photos; 3 were already in Schist Cloud; skipped 2"));
        assert!(message.contains("large.mov"));
        assert!(message.contains("5 GiB"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
