//! Backing up the camera roll to Schist Cloud.
//!
//! A rule — one source of photos, one cloud folder — kept with the
//! preferences, and an engine that brings the folder up to date with the
//! source: every photo the ledger has not seen goes through the same
//! upload pipeline a drag into the cloud gallery uses (`cloud.rs`'s
//! `upload_files`), which hashes it, asks the provider whether the
//! library already holds it, and sends the rest up in compressed batches.
//! The ledger remembers what has been handled so a roll of twenty
//! thousand photos costs one walk per run rather than twenty thousand
//! hashes.
//!
//! The engine knows nothing of the workspace: on Android a `JobService`
//! runs it headless, with no activity and no gpui, to catch up while the
//! app is closed (`camera_sync_android.rs`). The workspace half here
//! drives it while the app is open — after a sign-in, every quarter hour,
//! when the source changes — and asks, once, after the first sign-in on a
//! phone, whether to set it up at all.
//!
//! Sources differ by platform. Android's camera roll is a folder
//! (`DCIM/Camera` under the shared storage) once the media permission is
//! granted, so the source is a path. iOS's is not: photos come from the
//! Photos framework by identifier, and each one is written to a staging
//! folder for the upload and removed after (`camera_sync_ios.rs`).

use super::cloud::{upload_files, Handled, Report};
use super::*;
use anyhow::{anyhow, Result};
use schist_cloud as remote;
use schist_i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime},
};

/// The rule, as preferences.json keeps it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CameraSync {
    /// Whether the backup runs. Off until the prompt is accepted.
    #[serde(default)]
    pub enabled: bool,
    /// Separates checkpoints when the destination or signed-in account changes.
    #[serde(default)]
    pub ledger_key: String,
    /// Whether the sign-in prompt has been shown, accepted or not: it
    /// asks once. Preferences reopens the same dialog.
    #[serde(default)]
    pub asked: bool,
    #[serde(default)]
    pub source: Option<Source>,
    /// The cloud folder photos go into; `None` files them unfiled.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Its name when the rule was made, for the sidebar's caption — the
    /// folder may since have been renamed, which is only cosmetic here.
    #[serde(default)]
    pub folder_name: String,
    /// When the last successful run finished, seconds since the epoch.
    #[serde(default)]
    pub last_run: Option<u64>,
    /// What the last run said when it did not finish.
    #[serde(default)]
    pub last_error: Option<String>,
    /// The extensions the app's codecs decode, lower-case, as they stood
    /// when the rule was made: what counts as a photo. Kept with the rule
    /// so a headless run (Android's job, which has no codec registry)
    /// agrees with the app.
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Where photos come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A folder on disk, walked like a drag into the cloud gallery:
    /// hidden entries and symlinks stay home, sub-folders become cloud
    /// folders.
    Folder { path: PathBuf },
    /// A Photos album on iOS by its local identifier; `library`
    /// is the whole library.
    Album { id: String, title: String },
}
/// The album id that means every photo in the library rather than one
/// album. It survives a change of authorization, which a smart album's
/// real identifier, fetched under the old one, need not.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub const LIBRARY_ALBUM: &str = "library";
#[cfg(any(target_os = "ios", target_os = "android"))]
impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Folder { path } => crate::ui::shown_path(path),
            Source::Album { id, title } if id == LIBRARY_ALBUM || title.is_empty() => {
                t("cloud.sync.all_photos").to_string()
            }
            Source::Album { title, .. } => title.clone(),
        }
    }
}

/// What has been handled: the identity of every photo that went up or
/// was found already in the library. Kept as JSON beside the cloud's
/// other state; a missing or unreadable ledger just means one full
/// pass, which the provider's own duplicate check makes harmless.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub seen: HashSet<String>,
    #[serde(default)]
    pub rule: String,
}
impl Ledger {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// Written whole to a sibling and moved into place, so a run cut
    /// short by the OS leaves the previous ledger rather than half of
    /// this one.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}
/// The ledger's place: with the cloud's recovery checkpoints.
pub fn ledger_path() -> PathBuf {
    super::cloud::state_dir().join("camera-sync.json")
}
/// Where an iOS photo is written for its upload.
pub fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("schist-camera-sync")
}

/// One photo the source offers.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// What the ledger records: the path with its mtime and size, or the
    /// asset's identifier with its modification date — either changes
    /// when the photo does.
    pub identity: String,
    pub name: String,
    /// The path inside the source, so its sub-folders become cloud
    /// folders; `None` for a photo straight from the library.
    pub relative: Option<String>,
    pub locate: Locate,
}
#[derive(Debug, Clone)]
pub enum Locate {
    Path(PathBuf),
    /// A Photos asset, to be written out before upload.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    Asset(String),
}

/// How deep a source folder is walked. The camera roll is flat, or one
/// level of albums; this only stops a runaway.
const MAX_DEPTH: usize = 8;
/// Photos per pass through the pipeline: bounds the staging folder on
/// iOS and how much is lost to an interruption on either platform, since
/// the ledger is written after each chunk.
const CHUNK: usize = 100;

/// The photos under `root` whose extension is one Schist decodes, in a
/// stable order. Same rules as a drag into the cloud: hidden entries and
/// symlinks are left out.
pub fn list_folder(root: &Path, extensions: &HashSet<String>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    walk(root, root, 0, extensions, &mut out)?;
    out.sort_by(|a, b| a.identity.cmp(&b.identity));
    Ok(out)
}
fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    extensions: &HashSet<String>,
    out: &mut Vec<Candidate>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if ty.is_dir() {
            // A sub-folder that cannot be read (another app's private
            // media) is skipped, not fatal.
            if let Err(error) = walk(root, &path, depth + 1, extensions, out) {
                log::warn!("camera sync: skipping {}: {error}", path.display());
            }
            continue;
        }
        if !ty.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !extensions.contains(&extension) {
            continue;
        }
        let metadata = entry.metadata()?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let relative = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .ok();
        out.push(Candidate {
            identity: format!("{}|{mtime}|{}", path.display(), metadata.len()),
            name: entry.file_name().to_string_lossy().into_owned(),
            relative,
            locate: Locate::Path(path),
        });
    }
    Ok(())
}

/// One run's tally.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Photos the ledger had not seen when the run began.
    pub total: usize,
    pub uploaded: usize,
    /// Found in the library already.
    pub existing: usize,
    /// Unreadable, too big, or not exportable; tried again next run.
    pub skipped: usize,
    /// Not reached: the run was cancelled.
    pub pending: usize,
}
impl Outcome {
    pub fn message(&self) -> String {
        if self.total == 0 {
            return t("cloud.sync.up_to_date").into();
        }
        let mut message = match self.uploaded {
            0 => t("cloud.sync.summary_none").to_string(),
            n => schist_i18n::tn("cloud.sync.summary_uploaded", n as u64),
        };
        if self.existing > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_existing",
                self.existing as u64,
            ));
        }
        if self.skipped > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_skipped",
                self.skipped as u64,
            ));
        }
        if self.pending > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_pending",
                self.pending as u64,
            ));
        }
        message
    }
}

/// One rule, ready to run against a connection.
pub struct Engine {
    pub source: Source,
    pub folder_id: Option<String>,
    /// Lower-case extensions that count as photos.
    pub extensions: HashSet<String>,
    pub ledger_path: PathBuf,
    pub ledger_key: String,
    pub staging: PathBuf,
    /// Read between photos; set, the run stops after the one in hand.
    pub cancel: Arc<AtomicBool>,
}
static RUNNING: AtomicBool = AtomicBool::new(false);
struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}
impl Engine {
    /// Everything the source offers that the ledger has not seen.
    fn candidates(&self) -> Result<Vec<Candidate>> {
        match &self.source {
            Source::Folder { path } => {
                anyhow::ensure!(
                    path.is_dir(),
                    tf!("cloud.sync.source_unreadable", path = path.display())
                );
                list_folder(path, &self.extensions)
            }
            #[cfg(target_os = "ios")]
            Source::Album { id, .. } => super::camera_sync_ios::list_assets(id),
            #[cfg(not(target_os = "ios"))]
            Source::Album { .. } => Err(anyhow!(t("cloud.sync.albums_ios_only"))),
        }
    }
    /// Bring the cloud folder up to date. `report` hears the progress as
    /// the upload pipeline tells it: done, total, and what it is doing.
    pub async fn run(&self, handle: &remote::Handle, report: Report) -> Result<Outcome> {
        self.run_with(report, |files, report, handled| async move {
            let uploader = upload_files(
                handle,
                self.folder_id.clone(),
                files,
                report,
                Some(self.cancel.clone()),
                Some(handled),
            )
            .await?;
            Ok((
                uploader.uploaded.len(),
                uploader.existing.len(),
                uploader.skipped.len(),
            ))
        })
        .await
    }
    async fn run_with<F, Fut>(&self, report: Report, upload: F) -> Result<Outcome>
    where
        F: Fn(Vec<(PathBuf, Option<String>)>, Report, Handled) -> Fut,
        Fut: std::future::Future<Output = Result<(usize, usize, usize)>>,
    {
        super::cloud::cancellable(Some(&self.cancel), async {
            while RUNNING
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                remote::runtime::sleep(Duration::from_millis(100)).await;
            }
            Ok(())
        })
        .await?;
        let _run = RunGuard;
        // A process killed by the OS cannot run its export cleanup. The
        // staging directory belongs to this engine and runs are serialized.
        if matches!(self.source, Source::Album { .. }) && self.staging.exists() {
            std::fs::remove_dir_all(&self.staging)?;
        }
        let mut ledger = Ledger::load(&self.ledger_path);
        if ledger.rule != self.ledger_key {
            ledger = Ledger {
                rule: self.ledger_key.clone(),
                ..Default::default()
            };
        }
        let todo: Vec<Candidate> = self
            .candidates()?
            .into_iter()
            .filter(|c| !ledger.seen.contains(&c.identity))
            .collect();
        let ledger = Arc::new(Mutex::new(ledger));
        let total = todo.len();
        let mut outcome = Outcome {
            total,
            ..Default::default()
        };
        report(
            0,
            total as u64,
            tf!("cloud.sync.progress", n = 0, m = total),
        );
        let mut base = 0u64;
        for chunk in todo.chunks(CHUNK) {
            if self.cancel.load(Ordering::Relaxed) {
                outcome.pending = total - base as usize;
                break;
            }
            let mut files = Vec::new();
            let mut staged = Vec::new();
            let mut by_path: HashMap<PathBuf, String> = HashMap::new();
            for candidate in chunk {
                if self.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let path = match &candidate.locate {
                    Locate::Path(path) => path.clone(),
                    Locate::Asset(id) => {
                        match materialize(id, &candidate.name, &self.staging, &self.cancel) {
                            Ok(path) => {
                                staged.push(path.clone());
                                path
                            }
                            Err(error) => {
                                log::warn!(
                                    "camera sync: could not export {}: {error}",
                                    candidate.name
                                );
                                outcome.skipped += 1;
                                continue;
                            }
                        }
                    }
                };
                by_path.insert(path.clone(), candidate.identity.clone());
                files.push((path, candidate.relative.clone()));
            }
            let inner: Report = {
                let report = report.clone();
                Arc::new(move |done, _chunk_total, label| report(base + done, total as u64, label))
            };
            let handled: Handled = {
                let ledger = ledger.clone();
                let path = self.ledger_path.clone();
                Arc::new(move |paths| {
                    if paths.is_empty() {
                        return Ok(());
                    }
                    let mut ledger = ledger.lock().unwrap_or_else(|e| e.into_inner());
                    for source in paths {
                        if let Some(identity) = by_path.get(source) {
                            ledger.seen.insert(identity.clone());
                        }
                    }
                    ledger.save(&path)
                })
            };
            let result = upload(files, inner, handled).await;
            for path in &staged {
                let _ = std::fs::remove_file(path);
                // Each staged photo sits in a folder of its own.
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            let (uploaded, existing, skipped) = result?;
            outcome.uploaded += uploaded;
            outcome.existing += existing;
            outcome.skipped += skipped;
            base += chunk.len() as u64;
        }
        report(base, total as u64, outcome.message());
        Ok(outcome)
    }
}

/// Write a Photos asset out for its upload. Only iOS has a Photos
/// library; anywhere else an album source is refused before this.
#[cfg(target_os = "ios")]
fn materialize(id: &str, name: &str, staging: &Path, cancel: &Arc<AtomicBool>) -> Result<PathBuf> {
    super::camera_sync_ios::export_original(id, name, staging, cancel)
}
#[cfg(not(target_os = "ios"))]
fn materialize(
    _id: &str,
    _name: &str,
    _staging: &Path,
    _cancel: &Arc<AtomicBool>,
) -> Result<PathBuf> {
    Err(anyhow!(t("cloud.sync.albums_ios_only")))
}

/// The codecs' extensions, less the document formats: a camera roll
/// holds photos, and a `.psd` or `.afphoto` found beside them is someone's
/// project, not a capture to back up.
pub fn photo_extensions(codec_extensions: Vec<String>) -> HashSet<String> {
    const DOCUMENTS: &[&str] = &["psd", "psb", "afphoto", "afdesign", "afpub", "af"];
    codec_extensions
        .into_iter()
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !DOCUMENTS.contains(&e.as_str()))
        .collect()
}

/// The seconds since the epoch, for `last_run`.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A folder's "something changed" stamp: the newest mtime of the folder
/// and its immediate sub-folders — a new photo in `DCIM/Camera` touches
/// `Camera`, not `DCIM`. One `read_dir`, cheap enough every half minute.
pub fn folder_stamp(root: &Path) -> Option<u64> {
    let mtime = |path: &Path| {
        std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    };
    let mut newest = mtime(root)?;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if let Some(m) = mtime(&entry.path()) {
                    newest = newest.max(m);
                }
            }
        }
    }
    Some(newest)
}

// ---------------------------------------------------------------------
// The workspace half: when to run, the prompt, and what the tray shows.
// ---------------------------------------------------------------------

/// How often a run starts on its own while the app is open, short of
/// the source changing.
const INTERVAL: Duration = Duration::from_secs(15 * 60);
/// How often a folder source is looked at for a change.
const STAMP_INTERVAL: Duration = Duration::from_secs(30);
/// How long the OS permission dialog is waited on before giving up.
const PERMISSION_WAIT: Duration = Duration::from_secs(120);

/// Where the photos go, as the prompt puts it.
#[derive(Debug, Clone)]
pub(crate) enum FolderChoice {
    Existing { id: String, name: String },
    New { name: String },
}
/// An accepted prompt, waiting on a permission or a folder.
#[derive(Debug, Clone)]
pub(crate) struct PendingSetup {
    pub source: Source,
    pub folder: FolderChoice,
}
/// What the engine and the platform bridges send the tick.
pub(crate) enum SyncJob {
    Progress {
        done: u64,
        total: u64,
        label: String,
    },
    Done(std::result::Result<Outcome, String>),
    /// The photo-library authorization was answered (iOS).
    #[cfg(target_os = "ios")]
    Authorized(std::result::Result<(), String>),
    FolderCreated {
        setup: PendingSetup,
        result: std::result::Result<(String, String), String>,
    },
}
/// The workspace's view of the backup.
pub(crate) struct SyncState {
    pub running: bool,
    pub revision: u64,
    pub cancel: Arc<AtomicBool>,
    /// Ask after this sign-in, once the folder list is in.
    pub prompt_pending: bool,
    /// The run in progress: done, total, what it is doing.
    pub progress: Option<(u64, u64, String)>,
    last_run_at: Option<Instant>,
    last_stamp_at: Option<Instant>,
    source_stamp: Option<u64>,
    /// An accepted prompt waiting for the OS permission, and since when.
    pub permission_wait: Option<(Instant, PendingSetup)>,
    /// The albums the iOS prompt offers: identifier, title, count.
    #[cfg(target_os = "ios")]
    pub albums: Vec<(String, String, u64)>,
    /// A run asked for by the OS (iOS's background task) — reported
    /// back to it when done — and how long the cloud is given to come
    /// up before the task is handed back unused.
    background: bool,
    background_deadline: Option<Instant>,
}
/// How long a background launch waits for the cloud connection.
#[cfg(target_os = "ios")]
const BACKGROUND_CONNECT_WAIT: Duration = Duration::from_secs(60);
impl Default for SyncState {
    fn default() -> Self {
        Self {
            running: false,
            revision: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            prompt_pending: false,
            progress: None,
            last_run_at: None,
            last_stamp_at: None,
            source_stamp: None,
            permission_wait: None,
            #[cfg(target_os = "ios")]
            albums: Vec::new(),
            background: false,
            background_deadline: None,
        }
    }
}

/// Whether this build asks about the camera roll at all.
pub(crate) fn offered() -> bool {
    crate::feature_enabled("schist-cloud") && cfg!(any(target_os = "ios", target_os = "android"))
}

impl Workspace {
    /// Reconcile OS scheduling after startup, a preference change, or sign-out.
    pub(crate) fn camera_sync_update_background(&mut self) {
        if !crate::feature_enabled("schist-cloud") {
            #[cfg(target_os = "android")]
            super::camera_sync_android::cancel_job();
            #[cfg(target_os = "ios")]
            super::camera_sync_ios::set_enabled(false);
            return;
        }
        let enabled = self.view.camera_sync.enabled && self.cloud.account.is_some();
        #[cfg(target_os = "android")]
        if enabled {
            super::camera_sync_android::schedule_job();
        } else {
            super::camera_sync_android::cancel_job();
        }
        #[cfg(target_os = "ios")]
        {
            // Account loading may still be in flight on a background launch;
            // the saved preference determines the token's accessibility.
            if let Err(error) =
                super::camera_sync_ios::background_credentials(self.view.camera_sync.enabled)
            {
                self.cloud_error(tf!("cloud.sync.keychain_failed", error = error));
            }
            super::camera_sync_ios::set_enabled(enabled);
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let _ = enabled;
    }
    pub(crate) fn camera_sync_sign_out(&mut self) {
        self.camera_sync_cancel();
        if self.view.camera_sync.enabled || self.view.camera_sync.source.is_some() {
            self.view.camera_sync = CameraSync::default();
            self.save_view_options();
        }
        if let Some(snapshot) = self.preferences_snapshot.as_mut() {
            snapshot.0.camera_sync = self.view.camera_sync.clone();
        }
        self.camera_sync_update_background();
    }
    /// The engine for the current rule, or why there is none.
    fn camera_sync_engine(&self) -> Result<Engine> {
        let rule = &self.view.camera_sync;
        let source = rule
            .source
            .clone()
            .ok_or_else(|| anyhow!(t("cloud.sync.no_source")))?;
        Ok(Engine {
            source,
            folder_id: rule.folder_id.clone(),
            extensions: photo_extensions(self.codec_extensions()),
            ledger_path: ledger_path(),
            ledger_key: rule.ledger_key.clone(),
            staging: staging_dir(),
            cancel: self.cloud.sync.cancel.clone(),
        })
    }
    /// Called every tick from the cloud loop: finishes a setup that was
    /// waiting on a permission, shows the prompt when its moment comes,
    /// and starts a run when one is due.
    pub(crate) fn camera_sync_tick(&mut self, cx: &mut Context<Self>) {
        if !offered() {
            return;
        }
        if let Some((since, _)) = &self.cloud.sync.permission_wait {
            if platform_permission_granted() {
                let (_, setup) = self.cloud.sync.permission_wait.take().unwrap();
                self.camera_sync_finish_setup(setup, cx);
            } else if since.elapsed() > PERMISSION_WAIT {
                self.cloud.sync.permission_wait = None;
                self.camera_sync_error(t("cloud.sync.permission_denied"), cx);
            }
        }
        if self.cloud.sync.prompt_pending
            && self.cloud.library_total.is_some()
            && self.modal.is_none()
        {
            self.cloud.sync.prompt_pending = false;
            self.camera_sync_prompt(cx);
        }
        // A launch iOS made for the backup: take it up as soon as the
        // cloud is connected, or hand it back if that takes too long or
        // there is no backup to run.
        #[cfg(target_os = "ios")]
        if let Some(cancel) = super::camera_sync_ios::take_background_request() {
            let sync = &mut self.cloud.sync;
            if sync.running {
                // Already at it; that run reports for this launch too.
                sync.background = true;
            } else {
                sync.cancel = cancel;
                sync.background = true;
                sync.background_deadline = Some(Instant::now() + BACKGROUND_CONNECT_WAIT);
            }
        }
        let rule = &self.view.camera_sync;
        let runnable = rule.enabled && rule.source.is_some();
        #[cfg(target_os = "ios")]
        if self.cloud.sync.background && !self.cloud.sync.running {
            let timed_out = self
                .cloud
                .sync
                .background_deadline
                .is_some_and(|deadline| Instant::now() > deadline);
            if !runnable || timed_out {
                self.cloud.sync.background = false;
                self.cloud.sync.background_deadline = None;
                super::camera_sync_ios::finish_background(false);
            }
        }
        if !runnable {
            return;
        }
        if self.cloud.sync.running || !self.cloud.connected || self.cloud.progress.is_some() {
            return;
        }
        let sync = &mut self.cloud.sync;
        let mut due = sync.last_run_at.is_none_or(|at| at.elapsed() > INTERVAL);
        #[cfg(target_os = "ios")]
        {
            if super::camera_sync_ios::take_change() {
                due = true;
            }
            if sync.background {
                due = true;
                sync.background_deadline = None;
            }
        }
        if let Some(Source::Folder { path }) = &rule.source {
            if sync
                .last_stamp_at
                .is_none_or(|at| at.elapsed() > STAMP_INTERVAL)
            {
                sync.last_stamp_at = Some(Instant::now());
                let stamp = folder_stamp(path);
                if sync.source_stamp.is_some() && stamp != sync.source_stamp {
                    due = true;
                }
                sync.source_stamp = stamp;
            }
        }
        if due {
            self.camera_sync_start(cx);
        }
    }
    /// Start a run now, if one can.
    pub(crate) fn camera_sync_start(&mut self, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") || self.cloud.sync.running {
            return;
        }
        let Some(client) = &self.cloud.client else {
            return;
        };
        let handle = client.handle.clone();
        self.cloud.sync.last_run_at = Some(Instant::now());
        if !platform_permission_granted() {
            self.camera_sync_error(t("cloud.sync.permission_denied"), cx);
            #[cfg(target_os = "ios")]
            super::camera_sync_ios::run_finished(false);
            self.cloud.sync.background = false;
            self.cloud.sync.background_deadline = None;
            return;
        }
        // A background launch brought its own flag, which its expiration
        // sets; any other run gets a fresh one.
        if !self.cloud.sync.background {
            self.cloud.sync.cancel = Arc::new(AtomicBool::new(false));
        }
        let engine = match self.camera_sync_engine() {
            Ok(engine) => engine,
            Err(error) => {
                self.camera_sync_error(error.to_string(), cx);
                return;
            }
        };
        #[cfg(target_os = "ios")]
        super::camera_sync_ios::run_started(self.cloud.sync.cancel.clone());
        self.cloud.sync.running = true;
        self.cloud.sync.last_run_at = Some(Instant::now());
        self.cloud.sync.progress = Some((0, 0, t("cloud.sync.starting").into()));
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        let revision = self.cloud.sync.revision;
        remote::runtime::spawn(async move {
            let report: Report = {
                let sender = sender.clone();
                Arc::new(move |done, total, label| {
                    let _ = sender.send(super::cloud::Job::Sync {
                        epoch,
                        revision,
                        job: SyncJob::Progress { done, total, label },
                    });
                })
            };
            let result = engine.run(&handle, report).await.map_err(|e| e.to_string());
            let _ = sender.send(super::cloud::Job::Sync {
                epoch,
                revision,
                job: SyncJob::Done(result),
            });
        });
        cx.notify();
    }
    /// Stop the run in progress, if any; the ledger keeps what landed.
    pub(crate) fn camera_sync_cancel(&mut self) {
        self.cloud.sync.cancel.store(true, Ordering::Relaxed);
        self.cloud.sync.revision += 1;
        self.cloud.sync.running = false;
        self.cloud.sync.progress = None;
        #[cfg(target_os = "ios")]
        super::camera_sync_ios::run_finished(false);
        self.cloud.sync.background = false;
        self.cloud.sync.background_deadline = None;
        self.cloud.sync.permission_wait = None;
        self.cloud.sync.prompt_pending = false;
    }
    /// A message from the engine or a platform bridge.
    pub(crate) fn camera_sync_job(&mut self, job: SyncJob, cx: &mut Context<Self>) {
        match job {
            SyncJob::Progress { done, total, label } => {
                self.cloud.sync.progress = Some((done, total, label));
            }
            SyncJob::Done(result) => {
                self.cloud.sync.running = false;
                self.cloud.sync.progress = None;
                match &result {
                    Ok(outcome) => {
                        if outcome.pending == 0 && outcome.skipped == 0 {
                            self.view.camera_sync.last_run = Some(now_secs());
                            self.view.camera_sync.last_error = None;
                        } else {
                            self.view.camera_sync.last_error = Some(outcome.message());
                        }
                        if outcome.total > 0 {
                            self.status = outcome.message().into();
                        }
                        if let Some(Source::Folder { path }) = &self.view.camera_sync.source {
                            self.cloud.sync.source_stamp = folder_stamp(path);
                        }
                    }
                    Err(error) => {
                        self.view.camera_sync.last_error = Some(error.clone());
                        self.status = tf!("cloud.sync.failed", error = error).into();
                    }
                }
                self.save_view_options();
                #[cfg(target_os = "ios")]
                {
                    self.cloud.sync.background = false;
                    super::camera_sync_ios::run_finished(
                        result
                            .as_ref()
                            .is_ok_and(|o| o.pending == 0 && o.skipped == 0),
                    );
                }
            }
            #[cfg(target_os = "ios")]
            SyncJob::Authorized(result) => match result {
                Ok(()) => {
                    #[cfg(target_os = "ios")]
                    {
                        self.cloud.sync.albums = super::camera_sync_ios::albums();
                    }
                    if let Some((_, setup)) = self.cloud.sync.permission_wait.take() {
                        self.camera_sync_finish_setup(setup, cx);
                    }
                }
                Err(error) => {
                    self.cloud.sync.permission_wait = None;
                    self.camera_sync_error(error, cx);
                }
            },
            SyncJob::FolderCreated { setup, result } => match result {
                Ok((id, name)) => self.camera_sync_apply(setup.source, Some(id), name, cx),
                Err(error) => {
                    self.camera_sync_error(tf!("cloud.sync.folder_failed", error = error), cx);
                    self.cloud_refresh_catalogue();
                }
            },
        }
        cx.notify();
    }
    fn camera_sync_error(&mut self, error: impl Into<String>, cx: &mut Context<Self>) {
        let error = error.into();
        self.view.camera_sync.last_error = Some(error.clone());
        self.save_view_options();
        self.status = error.clone().into();
        self.cloud.message = error;
        cx.notify();
    }
    /// The prompt: which photos, into which cloud folder. Also the way
    /// to change the rule later, from Preferences or the cloud row's menu.
    pub(crate) fn camera_sync_prompt(&mut self, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
        if self.cloud.account.is_none() {
            self.cloud_sign_in(cx);
            return;
        }
        self.view.camera_sync.asked = true;
        self.save_view_options();
        let rule = &self.view.camera_sync;
        let source = rule.source.clone().unwrap_or_else(default_source);
        let folder = match &rule.folder_id {
            Some(id) if self.cloud.folders.iter().any(|f| &f.id == id) => id.clone(),
            _ => "new".to_string(),
        };
        let name = if rule.folder_name.is_empty() {
            t("cloud.sync.default_folder").to_string()
        } else {
            rule.folder_name.clone()
        };
        self.open_modal(
            Modal::Cloud {
                kind: "camera-sync",
                fields: vec![
                    (
                        "cloud-sync-source",
                        String::new(),
                        serde_json::to_string(&source).unwrap_or_default(),
                    ),
                    ("cloud-folder", String::new(), folder),
                    ("cloud-name", t("cloud.dialog.folder_name").into(), name),
                ],
            },
            cx,
        );
        // The albums need the library's permission to list; asking here,
        // where the choice is being made, is the natural moment, and the
        // list fills in when it is answered.
        #[cfg(target_os = "ios")]
        {
            self.cloud.sync.albums = super::camera_sync_ios::albums();
            if self.cloud.sync.albums.is_empty() {
                let sender = self.cloud.sender.clone();
                let epoch = self.cloud.epoch;
                let revision = self.cloud.sync.revision;
                super::camera_sync_ios::request_authorization(move |result| {
                    let _ = sender.send(super::cloud::Job::Sync {
                        epoch,
                        revision,
                        job: SyncJob::Authorized(result),
                    });
                });
            }
        }
    }
    /// "Not now": remembered so the prompt does not come back on its own.
    pub(crate) fn camera_sync_decline(&mut self, cx: &mut Context<Self>) {
        self.view.camera_sync.asked = true;
        self.save_view_options();
        self.close_modal(cx);
    }
    /// The prompt's "Back up": read the fields, get the permission, then
    /// the folder, then the rule.
    pub(crate) fn camera_sync_submit(
        &mut self,
        fields: &[(&'static str, String, String)],
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let get = |key: &str| {
            fields
                .iter()
                .find(|(k, _, _)| *k == key)
                .map(|(_, _, v)| v.as_str())
                .unwrap_or("")
        };
        let source: Source = serde_json::from_str(get("cloud-sync-source"))
            .map_err(|_| anyhow!(t("cloud.sync.no_source")))?;
        let folder = match get("cloud-folder") {
            "new" | "" => {
                let name = get("cloud-name").trim().to_string();
                anyhow::ensure!(!name.is_empty(), t("cloud.sync.name_the_folder"));
                FolderChoice::New { name }
            }
            id => {
                let name = self
                    .cloud
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.name.clone())
                    .ok_or_else(|| anyhow!(t("cloud.error.folder_gone")))?;
                FolderChoice::Existing {
                    id: id.to_string(),
                    name,
                }
            }
        };
        let setup = PendingSetup { source, folder };
        self.view.camera_sync.asked = true;
        self.save_view_options();
        if platform_permission_granted() {
            self.camera_sync_finish_setup(setup, cx);
            return Ok(());
        }
        self.cloud.sync.permission_wait = Some((Instant::now(), setup));
        self.status = t("cloud.sync.waiting_for_permission").into();
        #[cfg(target_os = "android")]
        super::camera_sync_android::request_media_permission();
        #[cfg(target_os = "ios")]
        {
            let sender = self.cloud.sender.clone();
            let epoch = self.cloud.epoch;
            let revision = self.cloud.sync.revision;
            super::camera_sync_ios::request_authorization(move |result| {
                let _ = sender.send(super::cloud::Job::Sync {
                    epoch,
                    revision,
                    job: SyncJob::Authorized(result),
                });
            });
        }
        Ok(())
    }
    /// The permission is in hand: make the folder if asked, then apply.
    fn camera_sync_finish_setup(&mut self, setup: PendingSetup, cx: &mut Context<Self>) {
        match setup.folder.clone() {
            FolderChoice::Existing { id, name } => {
                self.camera_sync_apply(setup.source, Some(id), name, cx)
            }
            FolderChoice::New { name } => {
                let Some(client) = &self.cloud.client else {
                    self.camera_sync_error(t("cloud.error.sign_in_first"), cx);
                    return;
                };
                // A folder of that name may be there already — from an
                // earlier phone, or the last attempt — and is the one meant.
                if let Some(folder) = self
                    .cloud
                    .folders
                    .iter()
                    .find(|f| f.parent_id.is_none() && f.name == name)
                {
                    let (id, name) = (folder.id.clone(), folder.name.clone());
                    self.camera_sync_apply(setup.source, Some(id), name, cx);
                    return;
                }
                let handle = client.handle.clone();
                let sender = self.cloud.sender.clone();
                let epoch = self.cloud.epoch;
                let revision = self.cloud.sync.revision;
                self.status = tf!("cloud.sync.creating_folder", name = name).into();
                remote::runtime::spawn(async move {
                    let result = create_folder(&handle, &name)
                        .await
                        .map(|id| (id, name.clone()))
                        .map_err(|e| e.to_string());
                    let _ = sender.send(super::cloud::Job::Sync {
                        epoch,
                        revision,
                        job: SyncJob::FolderCreated { setup, result },
                    });
                });
            }
        }
    }
    /// Save the rule and let the tick start the first run.
    fn camera_sync_apply(
        &mut self,
        source: Source,
        folder_id: Option<String>,
        folder_name: String,
        cx: &mut Context<Self>,
    ) {
        self.camera_sync_cancel();
        let rule = &mut self.view.camera_sync;
        let changed = rule.source.as_ref() != Some(&source) || rule.folder_id != folder_id;
        if changed || rule.ledger_key.is_empty() {
            rule.last_run = None;
            rule.ledger_key = remote::Uuid::new_v4().to_string();
        }
        rule.enabled = true;
        rule.asked = true;
        rule.source = Some(source);
        rule.folder_id = folder_id;
        rule.folder_name = folder_name;
        rule.last_error = None;
        let mut extensions: Vec<String> = photo_extensions(self.codec_extensions())
            .into_iter()
            .collect();
        extensions.sort();
        self.view.camera_sync.extensions = extensions;
        self.save_view_options();
        self.cloud.sync.last_run_at = None;
        self.cloud.sync.source_stamp = None;
        self.cloud_refresh_catalogue();
        self.camera_sync_update_background();
        self.status = t("cloud.sync.enabled").into();
        cx.notify();
    }
    /// Preferences' switch.
    pub(crate) fn camera_sync_set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
        if enabled && self.view.camera_sync.source.is_none() {
            self.keep_preferences();
            self.camera_sync_prompt(cx);
            return;
        }
        self.view.camera_sync.enabled = enabled;
        self.save_view_options();
        self.camera_sync_update_background();
        if !enabled {
            self.camera_sync_cancel();
            #[cfg(target_os = "android")]
            super::camera_sync_android::cancel_job();
        } else {
            self.cloud.sync.last_run_at = None;
        }
        cx.notify();
    }
    /// The sidebar's line under the cloud root: what the backup is doing,
    /// or when it last ran.
    pub(crate) fn camera_sync_status(&self) -> Option<String> {
        let rule = &self.view.camera_sync;
        if !offered() || !rule.enabled {
            return None;
        }
        if let Some((done, total, _)) = &self.cloud.sync.progress {
            return Some(if *total == 0 {
                t("cloud.sync.starting").into()
            } else {
                tf!("cloud.sync.progress", n = done, m = total)
            });
        }
        if let Some(error) = &rule.last_error {
            return Some(tf!("cloud.sync.failed", error = error));
        }
        Some(match rule.last_run {
            Some(at) => tf!("cloud.sync.last_backed_up", when = ago(at)),
            None => t("cloud.sync.waiting").into(),
        })
    }
}

/// "just now", "5 min ago", "3 h ago", "2 d ago".
fn ago(secs: u64) -> String {
    let elapsed = now_secs().saturating_sub(secs);
    if elapsed < 60 {
        t("cloud.sync.just_now").into()
    } else if elapsed < 3600 {
        tf!("cloud.sync.minutes_ago", n = elapsed / 60)
    } else if elapsed < 86_400 {
        tf!("cloud.sync.hours_ago", n = elapsed / 3600)
    } else {
        tf!("cloud.sync.days_ago", n = elapsed / 86_400)
    }
}

/// `folder.create`, answered with the new folder's id. The reply is the
/// folder as the catalogue lists it; a provider that answers with just
/// the id is read too.
async fn create_folder(handle: &remote::Handle, name: &str) -> Result<String> {
    use schist_cloud::protocol::map;
    let reply = handle
        .request_async(
            "folder.create",
            map([
                ("name", name.into()),
                ("parent_id", remote::Value::Nil),
                ("mutation_id", remote::Uuid::new_v4().to_string().into()),
            ]),
        )
        .await?;
    parse_created_folder(reply)
}
fn parse_created_folder(reply: remote::Value) -> Result<String> {
    if let Some(id) = reply
        .as_map()
        .and_then(|entries| entries.iter().find(|(k, _)| k.as_str() == Some("id")))
        .and_then(|(_, v)| v.as_str())
        .filter(|id| !id.trim().is_empty())
    {
        return Ok(id.to_string());
    }
    Err(anyhow!(t("cloud.sync.folder_id_missing")))
}

/// The source the prompt starts on.
fn default_source() -> Source {
    #[cfg(target_os = "android")]
    {
        if let Some((_, path)) = super::camera_sync_android::sources().into_iter().next() {
            return Source::Folder { path };
        }
    }
    #[cfg(target_os = "ios")]
    {
        return Source::Album {
            id: LIBRARY_ALBUM.into(),
            title: String::new(),
        };
    }
    #[allow(unreachable_code)]
    Source::Folder {
        path: PathBuf::from("."),
    }
}

/// Whether the OS lets the app read the source. Only the phones have a
/// permission to ask for.
fn platform_permission_granted() -> bool {
    #[cfg(target_os = "android")]
    {
        super::camera_sync_android::has_media_permission()
    }
    #[cfg(target_os = "ios")]
    {
        super::camera_sync_ios::authorized()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        true
    }
}

// ---------------------------------------------------------------------
// The prompt.
// ---------------------------------------------------------------------

/// The "Back up your camera roll?" dialog: the source, the folder, the
/// name for a new one.
pub(crate) fn form(
    ws: &mut Workspace,
    fields: &[(&'static str, String, String)],
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    use schist_ui::{Radio, TextInput};
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, v)| v.clone())
            .unwrap_or_default()
    };
    let set = |ws: &mut Workspace, key: &'static str, value: String| {
        ws.update_modal(|modal| {
            if let Modal::Cloud { fields, .. } = modal {
                if let Some((_, _, v)) = fields.iter_mut().find(|(k, _, _)| *k == key) {
                    *v = value;
                }
            }
        });
    };
    let current_source: Option<Source> = serde_json::from_str(&get("cloud-sync-source")).ok();
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(chrome_caption(t("cloud.sync.prompt_note")));

    // The source.
    let mut sources = div().flex().flex_col().gap_1();
    for (label, source) in source_choices(ws) {
        let selected = current_source.as_ref() == Some(&source);
        let value = serde_json::to_string(&source).unwrap_or_default();
        sources = sources.child(
            Radio::new(
                SharedString::from(format!("radio-source-{value}")),
                label,
                selected,
            )
            .on_select(cx.listener(move |ws, _e, _w, cx| {
                set(ws, "cloud-sync-source", value.clone());
                cx.notify();
            })),
        );
    }
    #[cfg(target_os = "android")]
    {
        sources = sources.child(super::gallery_chrome::gallery_button(
            t("cloud.sync.choose_folder"),
            false,
            move |ws, _w, cx| {
                let picker = ws.prompt_for_paths(
                    gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some(t("cloud.sync.choose_folder").into()),
                    },
                    cx,
                );
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = picker.await {
                        if let Some(path) = paths.into_iter().next() {
                            let _ = this.update(cx, |ws, cx| {
                                let value = serde_json::to_string(&Source::Folder { path })
                                    .unwrap_or_default();
                                set(ws, "cloud-sync-source", value);
                                cx.notify();
                            });
                        }
                    }
                })
                .detach();
            },
            cx,
        ));
    }
    body = body.child(crate::ui::field_row(t("cloud.sync.photos_from"), sources));

    // The folder: the library's top-level folders, or a new one.
    let committed = get("cloud-folder");
    let mut folders = div().flex().flex_col().gap_1();
    for folder in ws.cloud.folders.iter().filter(|f| f.parent_id.is_none()) {
        let id = folder.id.clone();
        folders = folders.child(
            Radio::new(
                SharedString::from(format!("radio-folder-{id}")),
                folder.name.clone(),
                id == committed,
            )
            .on_select(cx.listener(move |ws, _e, _w, cx| {
                set(ws, "cloud-folder", id.clone());
                cx.notify();
            })),
        );
    }
    folders = folders.child(
        Radio::new(
            SharedString::from("radio-folder-new"),
            t("cloud.sync.new_folder"),
            committed == "new" || committed.is_empty(),
        )
        .on_select(cx.listener(move |ws, _e, _w, cx| {
            set(ws, "cloud-folder", "new".into());
            cx.notify();
        })),
    );
    body = body.child(crate::ui::field_row(t("common.folder"), folders));
    if committed == "new" || committed.is_empty() {
        let key = "cloud-name";
        let committed = get(key);
        let active = ws.focused_field == Some(key);
        let shown = if active {
            ws.field_buffer.clone()
        } else {
            committed.clone()
        };
        body = body.child(crate::ui::field_row(
            t("cloud.dialog.folder_name"),
            TextInput::new(key, shown)
                .cursor(ws.field_cursor)
                .selection(ws.field_selection())
                .active(active)
                .caret_on(ws.caret_on())
                .w(px(270.0))
                .on_focus(cx.listener(move |ws, press: &crate::ui::TextPress, _, cx| {
                    ws.press_field(key, committed.clone(), press);
                    cx.notify();
                }))
                .on_select_to(cx.listener(move |ws, offset: &usize, _, cx| {
                    ws.drag_field(key, *offset);
                    cx.notify();
                })),
        ));
    }
    #[cfg(target_os = "ios")]
    if super::camera_sync_ios::limited() {
        body = body.child(chrome_caption(t("cloud.sync.limited_note")));
    }
    body = body.child(chrome_caption(t("cloud.sync.background_note")));

    let actions = div()
        .flex()
        .gap_2()
        .child(crate::ui::button(
            t("cloud.sync.not_now"),
            false,
            |ws, _, cx| ws.camera_sync_decline(cx),
            cx,
        ))
        .child(crate::ui::button(
            t("cloud.sync.back_up"),
            true,
            |ws, _, cx| {
                ws.commit_focused_field();
                let Some(Modal::Cloud { fields, .. }) = ws.modal.clone() else {
                    return;
                };
                match ws.camera_sync_submit(&fields, cx) {
                    Ok(()) => ws.close_modal(cx),
                    Err(e) => {
                        ws.status = e.to_string().into();
                        ws.cloud.message = e.to_string();
                        cx.notify();
                    }
                }
            },
            cx,
        ));
    crate::ui::modal_frame(t("cloud.sync.prompt_title"), 560.0, body, actions).into_any_element()
}
fn chrome_caption(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .text_color(gpui::rgb(crate::ui::palette().text_dim))
        .child(text.into())
}
/// The sources the prompt lists, labelled.
fn source_choices(ws: &Workspace) -> Vec<(String, Source)> {
    #[cfg(target_os = "android")]
    {
        let mut out: Vec<(String, Source)> = super::camera_sync_android::sources()
            .into_iter()
            .map(|(label, path)| (label, Source::Folder { path }))
            .collect();
        // A folder chosen by hand stays on the list while it is chosen.
        if let Some(Modal::Cloud { fields, .. }) = &ws.modal {
            if let Some(Source::Folder { path }) = fields
                .iter()
                .find(|(k, _, _)| *k == "cloud-sync-source")
                .and_then(|(_, _, v)| serde_json::from_str::<Source>(v).ok())
            {
                if !out
                    .iter()
                    .any(|(_, s)| matches!(s, Source::Folder { path: p } if *p == path))
                {
                    out.push((crate::ui::shown_path(&path), Source::Folder { path }));
                }
            }
        }
        out
    }
    #[cfg(target_os = "ios")]
    {
        let mut out = vec![(
            t("cloud.sync.all_photos").to_string(),
            Source::Album {
                id: LIBRARY_ALBUM.into(),
                title: String::new(),
            },
        )];
        for (id, title, count) in &ws.cloud.sync.albums {
            if id == LIBRARY_ALBUM {
                continue;
            }
            out.push((
                tf!("cloud.sync.album_with_count", name = title, n = count),
                Source::Album {
                    id: id.clone(),
                    title: title.clone(),
                },
            ));
        }
        out
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = ws;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts() -> HashSet<String> {
        ["jpg", "heic", "png"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn folder_listing_skips_hidden_symlinks_and_other_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Camera/.thumbnails")).unwrap();
        std::fs::write(root.join("Camera/IMG_0001.JPG"), b"jpeg").unwrap();
        std::fs::write(root.join("Camera/VID_0001.mp4"), b"video").unwrap();
        std::fs::write(root.join("Camera/.nomedia"), b"").unwrap();
        std::fs::write(root.join("Camera/.thumbnails/x.jpg"), b"thumb").unwrap();
        std::fs::write(root.join("Screenshots.png"), b"png").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Screenshots.png"), root.join("link.png")).unwrap();
        let listed = list_folder(root, &exts()).unwrap();
        let mut names: Vec<_> = listed.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["IMG_0001.JPG", "Screenshots.png"]);
        let camera = listed.iter().find(|c| c.name == "IMG_0001.JPG").unwrap();
        assert_eq!(camera.relative.as_deref(), Some("Camera/IMG_0001.JPG"));
        assert!(
            camera.identity.contains("|4"),
            "size is part of the identity"
        );
    }

    #[test]
    fn ledger_round_trips_and_survives_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/camera-sync.json");
        assert!(Ledger::load(&path).seen.is_empty());
        let mut ledger = Ledger::default();
        ledger.seen.insert("a|1|2".into());
        ledger.save(&path).unwrap();
        assert!(Ledger::load(&path).seen.contains("a|1|2"));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn rule_serialises_with_defaults() {
        let rule: CameraSync = serde_json::from_str("{}").unwrap();
        assert!(!rule.enabled && !rule.asked && rule.source.is_none());
        let source = Source::Folder {
            path: PathBuf::from("/storage/emulated/0/DCIM/Camera"),
        };
        let text = serde_json::to_string(&source).unwrap();
        assert!(text.contains("\"kind\":\"folder\""));
        assert_eq!(serde_json::from_str::<Source>(&text).unwrap(), source);
    }

    #[test]
    fn folder_stamp_moves_with_a_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("Camera")).unwrap();
        let before = folder_stamp(root).unwrap();
        // mtimes are whole seconds on some filesystems: push the
        // sub-folder well past the root.
        let later = filetime::FileTime::from_unix_time(before as i64 + 60, 0);
        filetime::set_file_mtime(root.join("Camera"), later).unwrap();
        assert!(folder_stamp(root).unwrap() > before);
    }

    #[test]
    fn outcome_message_reads_well() {
        assert_eq!(
            Outcome::default().message(),
            t("cloud.sync.up_to_date").to_string()
        );
        let outcome = Outcome {
            total: 3,
            uploaded: 2,
            existing: 1,
            ..Default::default()
        };
        assert!(outcome.message().contains("; "));
    }

    fn engine(root: &Path, count: usize) -> Engine {
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        for n in 0..count {
            std::fs::write(source.join(format!("{n:04}.jpg")), [n as u8]).unwrap();
        }
        Engine {
            source: Source::Folder { path: source },
            folder_id: Some("destination".into()),
            extensions: exts(),
            ledger_path: root.join("ledger.json"),
            ledger_key: "rule-one".into(),
            staging: root.join("staging"),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    fn run_test(work: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(work);
    }
    fn report() -> Report {
        Arc::new(|_, _, _| {})
    }

    #[test]
    fn camera_sync_chunks_cancel_and_resume_without_reuploading() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let engine = engine(dir.path(), CHUNK * 2 + 3);
            let cancel = engine.cancel.clone();
            let first = engine
                .run_with(report(), |files, _, handled| {
                    let cancel = cancel.clone();
                    async move {
                        assert_eq!(files.len(), CHUNK);
                        handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                        cancel.store(true, Ordering::Relaxed);
                        Ok((files.len(), 0, 0))
                    }
                })
                .await
                .unwrap();
            assert_eq!(first.uploaded, CHUNK);
            assert_eq!(first.pending, CHUNK + 3);
            engine.cancel.store(false, Ordering::Relaxed);
            let sizes = Mutex::new(Vec::new());
            let second = engine
                .run_with(report(), |files, _, handled| {
                    sizes.lock().unwrap().push(files.len());
                    async move {
                        handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                        Ok((files.len(), 0, 0))
                    }
                })
                .await
                .unwrap();
            assert_eq!(second.uploaded, CHUNK + 3);
            assert_eq!(*sizes.lock().unwrap(), [CHUNK, 3]);
            let third = engine
                .run_with(report(), |_, _, _| async {
                    panic!("known photos uploaded")
                })
                .await
                .unwrap();
            assert_eq!(third.total, 0);
        });
    }

    #[test]
    fn camera_sync_checkpoints_partial_failure_and_retries_unhandled_photos() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let engine = engine(dir.path(), 3);
            let failed = engine
                .run_with(report(), |files, _, handled| async move {
                    handled(&[files[0].0.clone()])?; // duplicate or completed upload
                    anyhow::bail!("connection lost after acknowledgement")
                })
                .await;
            assert!(failed.is_err());
            assert_eq!(Ledger::load(&engine.ledger_path).seen.len(), 1);
            let retry = engine
                .run_with(report(), |files, _, handled| async move {
                    assert_eq!(files.len(), 2);
                    handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                    Ok((1, 1, 0))
                })
                .await
                .unwrap();
            assert_eq!((retry.uploaded, retry.existing), (1, 1));
        });
    }

    #[test]
    fn camera_sync_new_rule_does_not_reuse_another_destinations_ledger() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let mut engine = engine(dir.path(), 1);
            for rule in ["first-account", "other-account"] {
                engine.ledger_key = rule.into();
                let result = engine
                    .run_with(report(), |files, _, handled| async move {
                        assert_eq!(files.len(), 1);
                        handled(&[files[0].0.clone()])?;
                        Ok((1, 0, 0))
                    })
                    .await
                    .unwrap();
                assert_eq!(result.total, 1);
            }
        });
    }

    #[test]
    fn camera_sync_folder_reply_requires_a_nonempty_id() {
        use remote::protocol::map;
        assert_eq!(
            parse_created_folder(map([("id", "folder-1".into())])).unwrap(),
            "folder-1"
        );
        assert!(parse_created_folder(map([("id", "".into())])).is_err());
        assert!(parse_created_folder(remote::Value::Nil).is_err());
    }

    #[test]
    fn camera_sync_cancellation_interrupts_a_pending_request() {
        run_test(async {
            let cancel = Arc::new(AtomicBool::new(false));
            let trigger = cancel.clone();
            let task = async move {
                remote::runtime::sleep(Duration::from_millis(20)).await;
                trigger.store(true, Ordering::Relaxed);
            };
            let waiting = super::super::cloud::cancellable(
                Some(&cancel),
                futures::future::pending::<Result<()>>(),
            );
            let (_, result) = futures::join!(task, waiting);
            assert!(result
                .unwrap_err()
                .to_string()
                .contains(t("cloud.upload.cancelled")));
        });
    }
}
